use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use facecam::modules::render::ImageProcessor;
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

    let shared = Arc::new(Mutex::new(ImageProcessor::default()));
    let running = Arc::new(AtomicBool::new(true));
    let running_cam = running.clone();
    let shared_img_proc = shared.clone();

    let camera_thread = thread::spawn(move || {
        let camera = RefCell::new(
            Camera::new(
                nokhwa::utils::CameraIndex::Index(0),
                RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate),
            )
            .unwrap(),
        );
        let res = camera.borrow().resolution();
        let (w, h) = (res.width(), res.height());

        {
            let mut process_lock = shared_img_proc.lock().unwrap();
            process_lock.set_resolution(res);
            process_lock.start_camera();
        }

        let virtual_camera = RefCell::new(
            virtualcam_rs::Camera::new(w as i32, h as i32, "Unity Video Capture").unwrap(),
        );

        while running_cam.load(Ordering::Relaxed) {
            let buffer = match camera.borrow_mut().frame() {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("frame error: {e}");
                    continue;
                }
            };
            let pixels = shared_img_proc.lock().unwrap().get_edited_camera_image(buffer);
            let _ = virtual_camera.borrow_mut().send(pixels);
        }
        let default_img = shared_img_proc.lock().unwrap().get_default_img();
        let _ = virtual_camera.borrow_mut().send(default_img);
    });

    let mut options = eframe::NativeOptions::default();
    options.viewport = eframe::egui::ViewportBuilder::default()
        .with_always_on_top()
        .with_resizable(false)
        .with_inner_size(eframe::egui::Vec2::new(300.0, 250.0));

    let app = Box::new(ViewApp::new(shared));
    let result = eframe::run_native("Racoon Camera", options, Box::new(|_cc| Ok(app)));

    running.store(false, Ordering::Relaxed);
    let _ = camera_thread.join();

    result.unwrap();
}
