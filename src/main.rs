use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::TryRecvError;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use facecam::modules::render::{ImageEditConfig, ImageProcessor};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{RequestedFormat, RequestedFormatType};
use nokhwa::*;
use view_app::ViewApp;
mod view_app;

fn main() {
    let backend = native_api_backend().unwrap();
    let devices = query(backend).unwrap();
    println!("There are {} available cameras.", devices.len());
    for device in devices {
        println!("{device}");
    }

    let running = Arc::new(AtomicBool::new(true));
    let running_cam = running.clone();
    let (sender, receiver) = mpsc::channel::<(ImageEditConfig, bool)>();

    let camera_thread = thread::spawn(move || {
        let mut image_processor = ImageProcessor::default();
        let mut camera = Camera::new(
            nokhwa::utils::CameraIndex::Index(0),
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate),
        )
        .unwrap();
        let res = camera.resolution();
        let (w, h) = (res.width(), res.height());

        image_processor.set_resolution(res);
        image_processor.start_camera();

        let mut virtual_camera =
            virtualcam_rs::Camera::new(w as i32, h as i32, "Unity Video Capture").unwrap();

        'outer: while running_cam.load(Ordering::Relaxed) {
            let mut newest = None;
            loop {
                match receiver.try_recv() {
                    Ok(cfg) => newest = Some(cfg),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break 'outer,
                }
            }
            if let Some((config, camera_on)) = newest {
                image_processor.set_config(config);
                if camera_on {
                    image_processor.start_camera();
                } else {
                    image_processor.stop_camera();
                }
            }

            let buffer = match camera.frame() {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("frame error: {e}");
                    continue;
                }
            };
            let pixels = image_processor.get_edited_camera_image(buffer);
            let _ = virtual_camera.send(pixels);
        }
        let default_img = image_processor.get_default_img();
        let _ = virtual_camera.send(default_img);
    });

    let mut options = eframe::NativeOptions::default();
    options.viewport = eframe::egui::ViewportBuilder::default()
        .with_always_on_top()
        .with_resizable(false)
        .with_inner_size(eframe::egui::Vec2::new(300.0, 250.0));

    let app = Box::new(ViewApp::new(sender));
    let result = eframe::run_native("Racoon Camera", options, Box::new(|_cc| Ok(app)));

    running.store(false, Ordering::Relaxed);
    let _ = camera_thread.join();

    result.unwrap();
}
