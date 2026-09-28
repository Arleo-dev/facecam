use std::sync::{Arc, Mutex};

use fast_image_resize::images::Image;
use image::ImageBuffer;
use imageproc::geometric_transformations::Border;
use ndarray::{ArrayD, Axis};
use nokhwa::{pixel_format::RgbAFormat, utils::Resolution, Buffer};
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::TensorRef,
};

const CONFIDENCE_THRESHOLD: f32 = 0.55;

pub struct ImageProcessor {
    pub resolution: Resolution,
    ort_session: Session,
    frame_count: u8,
    pub effects_config: Arc<Mutex<EffectsConfig>>,
    latest_raw_box: DetectionRect<f32>,
    is_work: bool,
}

impl Default for ImageProcessor {
    fn default() -> Self {
        let path = std::env::current_dir().unwrap();
        let path = format!("{}/resources/version-RFB-640.onnx", path.display());

        let ort_session = Session::builder()
            .unwrap()
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .unwrap()
            .with_intra_threads(4)
            .unwrap()
            .commit_from_file(path)
            .expect("Failed to load ONNX model");

        Self {
            resolution: Resolution::new(1920, 1080),
            ort_session,
            frame_count: 0,
            latest_raw_box: DetectionRect::default(),
            effects_config: Arc::new(Mutex::new(EffectsConfig::default())),
            is_work: false,
        }
    }
}

impl Drop for ImageProcessor {
    fn drop(&mut self) {
        self.get_default_img();
    }
}

impl ImageProcessor {
    pub fn set_resolution(&mut self, res: Resolution) {
        self.resolution = res;
    }

    pub fn get_edited_camera_image(&mut self, frame: Buffer) -> Vec<u8> {
        if !self.is_work {
            return self.get_default_img();
        }

        let mut image: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            frame.decode_image::<RgbAFormat>().unwrap();
        
        let effects = self.effects_config.lock().unwrap();

        if effects.activations.is_zoom {
            drop(effects);
            image = self.get_zoomed_face(image);
        } else {
            drop(effects);
        };

        let effects = self.effects_config.lock().unwrap();
        image = if effects.activations.is_racoon {
            drop(effects);
            self.get_rotated_img(image)
        } else {
            drop(effects);
            self.get_color_effected_image(image)
        };

        let pixels = get_pixels_from_img(image);
        pixels
    }

    pub fn start_camera(&mut self) {
        self.is_work = true;
    }

    pub fn stop_camera(&mut self) {
        self.is_work = false;
    }

    pub fn is_work(&self) -> bool {
        self.is_work
    }

    pub fn get_default_img(&mut self) -> Vec<u8> {
        let path = std::env::current_dir().unwrap();
        let path = format!("{}/resources/on_exit_img.jpg", path.display());
        let mut img = image::open(path).unwrap().into_rgba8();
        let res = self.resolution;
        img = get_resized_image(&img, res.width(), res.height());
        let pixels = get_pixels_from_img(img);
        pixels
    }

    fn get_zoomed_face(
        &mut self,
        frame: ImageBuffer<image::Rgba<u8>, Vec<u8>>,
    ) -> ImageBuffer<image::Rgba<u8>, Vec<u8>> {
        let orig_w = frame.width();
        let orig_h = frame.height();
        let model_h = 480;
        let model_w = 640;

        self.frame_count += 1;
        if self.frame_count % 5 == 0 || self.latest_raw_box.is_default() {
            let resized_frame = get_resized_image(&frame, model_w, model_h);
            let (scores_array, boxes_array) = {
                let mut input =
                    ndarray::Array4::<f32>::zeros((1, 3, model_h as usize, model_w as usize));
                for (x, y, pixel) in resized_frame.enumerate_pixels() {
                    input[[0, 0, y as usize, x as usize]] = pixel[0] as f32 / 255.0;
                    input[[0, 1, y as usize, x as usize]] = pixel[1] as f32 / 255.0;
                    input[[0, 2, y as usize, x as usize]] = pixel[2] as f32 / 255.0;
                }

                let outputs = match self
                    .ort_session
                    .run(ort::inputs![TensorRef::from_array_view(&input).unwrap()])
                {
                    Ok(o) => o,
                    Err(e) => {
                        eprintln!("❌ ONNX inference failed: {:?}", e);
                        return frame;
                    }
                };

                let scores_array = outputs[0].try_extract_array::<f32>().unwrap().to_owned();
                let boxes_array = outputs[1].try_extract_array::<f32>().unwrap().to_owned();
                (scores_array, boxes_array)
            };

            let scores_slice = scores_array.index_axis(Axis(0), 0);

            let (i, score) = scores_slice
                .axis_iter(Axis(0))
                .enumerate()
                .max_by(|(_, x), (_, y)| {
                    x[1].partial_cmp(&y[1]).unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap();

            let face_score = score[1];

            if face_score > CONFIDENCE_THRESHOLD {
                self.try_update_latest_box(boxes_array.into_dyn(), i);
            }
            self.frame_count = 0;
        }

        let current_zoom = self.effects_config.lock().unwrap().zoom_factor;
        let (x, y, w, h) =
            get_box_size_with_scale(current_zoom, self.latest_raw_box.clone(), orig_w, orig_h);

        if w == 0 || h == 0 {
            return frame;
        }

        let cropped_face = image::imageops::crop_imm(&frame, x, y, w, h).to_image();
        get_resized_image(&cropped_face, orig_w, orig_h)
    }

    fn try_update_latest_box(&mut self, boxes_array: ArrayD<f32>, index: usize) {
        let boxes_batch = boxes_array.index_axis(Axis(0), 0);
        let box_coords = boxes_batch.index_axis(Axis(0), index);

        let new_x_min = box_coords[0];
        let new_y_min = box_coords[1];
        let new_x_max = box_coords[2];
        let new_y_max = box_coords[3];

        let prev_w = self.latest_raw_box.x_max - self.latest_raw_box.x_min;
        let prev_h = self.latest_raw_box.y_max - self.latest_raw_box.y_min;

        let new_w = new_x_max - new_x_min;
        let new_h = new_y_max - new_y_min;

        let prev_cx = self.latest_raw_box.x_min + prev_w / 2.0;
        let prev_cy = self.latest_raw_box.y_min + prev_h / 2.0;
        let new_cx = new_x_min + new_w / 2.0;
        let new_cy = new_y_min + new_h / 2.0;

        let shift_x = (new_cx - prev_cx).abs();
        let shift_y = (new_cy - prev_cy).abs();
        let delta_w = (new_w - prev_w).abs();
        let delta_h = (new_h - prev_h).abs();

        let is_first_init = prev_w <= 0.0 || prev_h <= 0.0;
        let moved_significantly = shift_x > (prev_w / 4.0) || shift_y > (prev_h / 4.0);
        let resized_significantly = delta_w > (prev_w / 4.0) || delta_h > (prev_h / 4.0);

        if is_first_init || moved_significantly || resized_significantly {
            self.latest_raw_box
                .set_min(new_x_min, new_y_min)
                .set_max(new_x_max, new_y_max);
        }
    }

    fn get_color_effected_pixel(&mut self, pixel: &image::Rgba<u8>) -> image::Rgba<u8> {
        let rgb = self.effects_config.lock().unwrap().rgb;
        let r = pixel.0[0].max(rgb.0[0]);
        let g = pixel.0[1].max(rgb.0[1]);
        let b = pixel.0[2].max(rgb.0[2]);
        image::Rgba([r, g, b, pixel.0[3]])
    }

    fn get_rotated_img(
        &mut self,
        image: ImageBuffer<image::Rgba<u8>, Vec<u8>>,
    ) -> ImageBuffer<image::Rgba<u8>, Vec<u8>> {
        let radius: i32 = image.height() as i32 / 2;
        let (cx, cy) = (image.width() as i32 / 2, image.height() as i32 / 2);
        let mut image = imageproc::geometric_transformations::rotate_about_center(
            &image,
            self.effects_config.lock().unwrap().rotation,
            imageproc::geometric_transformations::Interpolation::Nearest,
            Border::Constant(image::Rgba([0, 0, 0, 255])),
        );

        for x in 0..image.width() {
            for y in 0..image.height() {
                let dx = x as i32 - cx;
                let dy = y as i32 - cy;
                if dx * dx + dy * dy >= radius * radius {
                    image.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                } else {
                    let pixel = self.get_color_effected_pixel(image.get_pixel(x, y));
                    image.put_pixel(x, y, pixel);
                }
            }
        }
        image
    }

    fn get_color_effected_image(
        &mut self,
        mut image: ImageBuffer<image::Rgba<u8>, Vec<u8>>,
    ) -> ImageBuffer<image::Rgba<u8>, Vec<u8>> {
        for x in 0..image.width() {
            for y in 0..image.height() {
                let pixel = self.get_color_effected_pixel(image.get_pixel(x, y));
                image.put_pixel(x, y, pixel);
            }
        }
        image
    }
}

#[derive(Default, Clone, Copy)]
pub struct EffectsActivationConfig {
    is_zoom: bool,
    is_racoon: bool,
    is_disco: bool,
}

impl EffectsActivationConfig {
    pub fn new(is_zoom: bool, is_racoon: bool, is_disco: bool) -> Self {
        Self {
            is_zoom,
            is_racoon,
            is_disco,
        }
    }
}

#[derive(Clone, Copy)]
pub struct EffectsConfig {
    activations: EffectsActivationConfig,
    zoom_factor: f32,
    rgb: image::Rgb<u8>,
    rotation: f32,
}

impl EffectsConfig {
    pub fn color(&self) -> image::Rgb<u8> {
        self.rgb
    }

    pub fn set_color(&mut self, rgb: image::Rgb<u8>) {
        self.rgb = rgb;
    }

    pub fn set_red(&mut self, r: u8) {
        self.rgb.0[0] = r;
    }

    pub fn set_green(&mut self, g: u8) {
        self.rgb.0[1] = g;
    }

    pub fn set_blue(&mut self, b: u8) {
        self.rgb.0[2] = b;
    }

    pub fn is_zoom(&self) -> bool {
        self.activations.is_zoom
    }
    pub fn is_disco(&self) -> bool {
        self.activations.is_disco
    }
    pub fn is_racoon(&self) -> bool {
        self.activations.is_racoon
    }

    pub fn update_activations(&mut self, activations: EffectsActivationConfig) {
        self.activations.is_disco = activations.is_disco;
        self.activations.is_racoon = activations.is_racoon;
        self.activations.is_zoom = activations.is_zoom;
    }

    pub fn zoom_factor(&self) -> f32 {
        self.zoom_factor
    }
    pub fn set_zoom_factor(&mut self, zoom_factor: f32) {
        self.zoom_factor = zoom_factor;
    }

    pub fn rotation(&self) -> f32 {
        self.rotation
    }

    pub fn set_rotation(&mut self, rotation: f32) {
        self.rotation = rotation;
    }
}

impl Default for EffectsConfig {
    fn default() -> Self {
        Self {
            activations: EffectsActivationConfig::default(),
            zoom_factor: 0f32,
            rgb: image::Rgb([0, 0, 0]),
            rotation: 0f32,
        }
    }
}

#[derive(Default, Clone)]
struct DetectionRect<T> {
    x_min: T,
    y_min: T,
    x_max: T,
    y_max: T,
}

impl<T: Default + PartialEq + Clone> DetectionRect<T> {
    fn set_min(&mut self, x: T, y: T) -> &mut Self {
        self.x_min = x;
        self.y_min = y;
        self
    }
    fn set_max(&mut self, x: T, y: T) -> &mut Self {
        self.x_max = x;
        self.y_max = y;
        self
    }

    fn is_default(&self) -> bool {
        self.x_min == T::default()
            && self.y_min == T::default()
            && self.x_max == T::default()
            && self.y_max == T::default()
    }
}

fn get_pixels_from_img(img: ImageBuffer<image::Rgba<u8>, Vec<u8>>) -> Vec<u8> {
    let mut pixels = Vec::new();
    for pixel in img.pixels().clone() {
        let p = *pixel;
        pixels.push(p.0[3]);
        pixels.push(p.0[2]);
        pixels.push(p.0[1]);
        pixels.push(p.0[0]);
    }
    pixels.reverse();
    pixels
}

fn get_box_size_with_scale(
    zoom_factor: f32,
    box_size: DetectionRect<f32>,
    orig_w: u32,
    orig_h: u32,
) -> (u32, u32, u32, u32) {
    let mut x = box_size.x_min * orig_w as f32;
    let mut y = box_size.y_min * orig_h as f32;
    let mut w = (box_size.x_max - box_size.x_min) * orig_w as f32;
    let mut h = (box_size.y_max - box_size.y_min) * orig_h as f32;

    let cx = x as f32 + w as f32 / 2.0;
    let cy = y as f32 + h as f32 / 2.0;

    w = orig_w as f32 / zoom_factor;
    h = orig_h as f32 / zoom_factor;

    x = (cx - w / 2.0).round();
    y = (cy - h / 2.0).round();

    let x = (x.round() as u32).max(0) as u32;
    let y = (y.round() as u32).max(0) as u32;
    let w = (w.round() as u32).min(orig_w - x);
    let h = (h.round() as u32).min(orig_h - y);

    (x, y, w, h)
}

fn get_resized_image(
    image: &ImageBuffer<image::Rgba<u8>, Vec<u8>>,
    resize_w: u32,
    resize_h: u32,
) -> ImageBuffer<image::Rgba<u8>, Vec<u8>> {
    let orig_w = image.width();
    let orig_h = image.height();
    let mut resizer = fast_image_resize::Resizer::new();
    let image_raw = image.as_raw();
    let image = Image::from_vec_u8(
        orig_w,
        orig_h,
        image_raw.clone(),
        fast_image_resize::PixelType::U8x4,
    )
    .expect("Failed to create crop view");

    let mut resized = Image::new(resize_w, resize_h, fast_image_resize::PixelType::U8x4);
    resizer.resize(&image, &mut resized, None).unwrap();

    let resized_frame: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_vec(resize_w, resize_h, resized.into_vec()).unwrap();
    resized_frame
}
