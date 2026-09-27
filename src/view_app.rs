use eframe::{self, egui::Color32};
use facecam::modules::render_camera::{EffectsActivationConfig, RenderCamera};

const MAX_ZOOM_FACTOR: f32 = 10f32;

pub struct ViewApp {
    camera: RenderCamera,
    rotate_delta: f32,
}

impl Default for ViewApp {
    fn default() -> Self {
        Self {
            camera: RenderCamera::default(),
            rotate_delta: 0.0,
        }
    }
}

impl eframe::App for ViewApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        ui.request_repaint();

        eframe::egui::CentralPanel::default().show(ui, |ui: &mut eframe::egui::Ui| {
            self.camera.set_camera_image();
            let mut effects_config = self.camera.effects_config.borrow_mut();

            let current_effect_color = effects_config.color();
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
                effects_config.set_red(r);
            }
            if ui
                .add(
                    eframe::egui::Slider::new(&mut g, 0..=255)
                        .text("g")
                        .text_color(Color32::GREEN),
                )
                .changed()
            {
                effects_config.set_green(g);
            }
            if ui
                .add(
                    eframe::egui::Slider::new(&mut b, 0..=255)
                        .text("b")
                        .text_color(Color32::BLUE),
                )
                .changed()
            {
                effects_config.set_blue(b);
            }

            let mut is_zoom = effects_config.is_zoom();
            let mut is_racoon = effects_config.is_racoon();
            let mut is_disco = effects_config.is_disco();

            if ui
                .add(eframe::egui::Checkbox::new(&mut is_racoon, "On Racoon"))
                .changed()
            {
                effects_config
                    .update_activations(EffectsActivationConfig::new(is_zoom, is_racoon, is_disco));
            }

            if effects_config.is_racoon() {
                ui.add(eframe::egui::Slider::new(&mut self.rotate_delta, -1.0..=1.0).text("speed"));
                let current_rot = effects_config.rotation();
                effects_config.set_rotation(current_rot + self.rotate_delta);
            } else {
                effects_config.set_rotation(0f32);
                self.rotate_delta = 0f32;
            }

            if ui
                .add(eframe::egui::Checkbox::new(&mut is_disco, "On Disco"))
                .changed()
            {
                effects_config
                    .update_activations(EffectsActivationConfig::new(is_zoom, is_racoon, is_disco));
            }

            if ui
                .add(eframe::egui::Checkbox::new(&mut is_zoom, "On Zoom"))
                .changed()
            {
                effects_config
                    .update_activations(EffectsActivationConfig::new(is_zoom, is_racoon, is_disco));
                if !effects_config.is_disco() {
                    effects_config.set_color(image::Rgb([0, 0, 0]));
                }
            }

            if effects_config.is_zoom() {
                let mut zoom_factor = effects_config.zoom_factor();

                // .step_by(0.05) дає змогу точно змінювати дробові значення (наприклад, 2.65, 2.70)
                let sl_zoom = eframe::egui::Slider::new(&mut zoom_factor, 1.0..=MAX_ZOOM_FACTOR)
                    .step_by(0.05)
                    .text("zoom");

                if ui.add(sl_zoom).changed() {
                    effects_config.set_zoom_factor(zoom_factor);
                }
            }

            if effects_config.is_disco() {
                let time = ui.input(|i| i.time);

                let interval = 0.3;
                let step = (time / interval) as u64;

                let r = ((step.wrapping_mul(1103515245) + 12345) % 100) as u8;
                let g = ((step.wrapping_mul(123456789) + 54321) % 100) as u8;
                let b = ((step.wrapping_mul(987654321) + 67890) % 100) as u8;

                effects_config.set_color(image::Rgb([r, g, b]));
            }
        });
    }
}
