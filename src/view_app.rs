use std::sync::mpsc::Sender;

use eframe::{self, egui::Color32};
use facecam::modules::render::{EffectsActivationConfig, ImageEditConfig};

const MAX_ZOOM_FACTOR: f32 = 10f32;

pub struct ViewApp {
    effect_config_sender: Sender<(ImageEditConfig, bool)>,
    current_config: (ImageEditConfig, bool),
    rotate_delta: f32,
}

impl ViewApp {
    pub fn new(sender: Sender<(ImageEditConfig, bool)>) -> Self {
        Self {
            current_config: (ImageEditConfig::default(), true),
            effect_config_sender: sender,
            rotate_delta: 0.0,
        }
    }

    fn set_config(&mut self, config: (ImageEditConfig, bool)) {
        self.current_config = config;
    }
}

impl eframe::App for ViewApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        ui.request_repaint();

        eframe::egui::CentralPanel::default().show(ui, |ui: &mut eframe::egui::Ui| {
            let mut is_camera_work = self.current_config.1;
            let mut edit_config = self.current_config.0;

            ui.add(eframe::egui::Checkbox::new(
                &mut is_camera_work,
                "Camera on/off",
            ));

            let current_effect_color = edit_config.color();
            let mut r = current_effect_color.0[0];
            let mut g = current_effect_color.0[1];
            let mut b = current_effect_color.0[2];

            if ui
                .add(
                    eframe::egui::Slider::new(&mut r, 0..=255)
                        .text("r")
                        .text_color(Color32::RED),
                )
                .changed()
            {
                edit_config.set_red(r);
            }
            if ui
                .add(
                    eframe::egui::Slider::new(&mut g, 0..=255)
                        .text("g")
                        .text_color(Color32::GREEN),
                )
                .changed()
            {
                edit_config.set_green(g);
            }
            if ui
                .add(
                    eframe::egui::Slider::new(&mut b, 0..=255)
                        .text("b")
                        .text_color(Color32::BLUE),
                )
                .changed()
            {
                edit_config.set_blue(b);
            }

            let mut is_zoom = edit_config.is_zoom();
            let mut is_racoon = edit_config.is_racoon();
            let mut is_disco = edit_config.is_disco();

            if ui
                .add(eframe::egui::Checkbox::new(&mut is_racoon, "On Racoon"))
                .changed()
            {
                edit_config
                    .update_activations(EffectsActivationConfig::new(is_zoom, is_racoon, is_disco));
            }

            if edit_config.is_racoon() {
                ui.add(eframe::egui::Slider::new(&mut self.rotate_delta, -1.0..=1.0).text("speed"));
                let current_rot = edit_config.rotation();
                edit_config.set_rotation(current_rot + self.rotate_delta);
            } else {
                edit_config.set_rotation(0f32);
                self.rotate_delta = 0f32;
            }

            if ui
                .add(eframe::egui::Checkbox::new(&mut is_disco, "On Disco"))
                .changed()
            {
                edit_config
                    .update_activations(EffectsActivationConfig::new(is_zoom, is_racoon, is_disco));
                if !is_disco {
                    edit_config.set_color(image::Rgb([0, 0, 0]));
                }
            }

            if ui
                .add(eframe::egui::Checkbox::new(&mut is_zoom, "On Zoom"))
                .changed()
            {
                edit_config
                    .update_activations(EffectsActivationConfig::new(is_zoom, is_racoon, is_disco));
            }

            if edit_config.is_zoom() {
                let mut zoom_factor = edit_config.zoom_factor();
                let sl_zoom = eframe::egui::Slider::new(&mut zoom_factor, 1.0..=MAX_ZOOM_FACTOR)
                    .step_by(0.05)
                    .text("zoom");

                if ui.add(sl_zoom).changed() {
                    edit_config.set_zoom_factor(zoom_factor);
                }
            }

            if edit_config.is_disco() {
                let time = ui.input(|i| i.time);

                let interval = 0.3;
                let step = (time / interval) as u64;

                let r = ((step.wrapping_mul(1103515245) + 12345) % 100) as u8;
                let g = ((step.wrapping_mul(123456789) + 54321) % 100) as u8;
                let b = ((step.wrapping_mul(987654321) + 67890) % 100) as u8;

                edit_config.set_color(image::Rgb([r, g, b]));
            }
            let updated_config = (edit_config, is_camera_work);
            self.set_config(updated_config);

            if let Err(e) = self.effect_config_sender.send(updated_config) {
                println!("Failed to send data: Channel is closed! Error: {:?}", e);
            }
        });
    }
}
