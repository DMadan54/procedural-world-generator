use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use worldgen_core::{generate_heightmap, Heightmap, HeightmapParams, WorldSeed};

const DEFAULT_SEED: u64 = 42;

/// Which pipeline stage the central panel shows. Every stage gets an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DebugView {
    Heightmap,
}

impl DebugView {
    const ALL: [DebugView; 1] = [DebugView::Heightmap];

    fn label(self) -> &'static str {
        match self {
            DebugView::Heightmap => "Heightmap",
        }
    }
}

pub struct WorldGenApp {
    seed_text: String,
    seed_error: Option<String>,
    seed: WorldSeed,
    params: HeightmapParams,
    heightmap: Heightmap,
    view: DebugView,
    /// Cleared whenever the world or view changes; rebuilt on the next frame.
    texture: Option<TextureHandle>,
}

impl WorldGenApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let seed = WorldSeed(DEFAULT_SEED);
        let params = HeightmapParams::default();
        let heightmap = generate_heightmap(seed, &params);
        Self {
            seed_text: DEFAULT_SEED.to_string(),
            seed_error: None,
            seed,
            params,
            heightmap,
            view: DebugView::Heightmap,
            texture: None,
        }
    }

    fn regenerate(&mut self) {
        match self.seed_text.trim().parse::<u64>() {
            Ok(value) => {
                self.seed_error = None;
                self.seed = WorldSeed(value);
                self.heightmap = generate_heightmap(self.seed, &self.params);
                self.texture = None;
            }
            Err(_) => {
                self.seed_error = Some(format!("Seed must be a whole number from 0 to {}.", u64::MAX));
            }
        }
    }

    fn render_view(&self) -> ColorImage {
        match self.view {
            DebugView::Heightmap => heightmap_image(&self.heightmap),
        }
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("controls")
            .resizable(false)
            .exact_width(220.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.heading("World");
                ui.add_space(8.0);

                ui.label("Seed");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.seed_text)
                        .desired_width(f32::INFINITY)
                        .hint_text("e.g. 42"),
                );
                let pressed_enter =
                    field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if let Some(error) = &self.seed_error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }

                ui.add_space(4.0);
                let clicked = ui
                    .add_sized([ui.available_width(), 28.0], egui::Button::new("Regenerate"))
                    .clicked();
                if clicked || pressed_enter {
                    self.regenerate();
                }

                ui.add_space(16.0);
                ui.separator();
                ui.label("Debug view");
                let before = self.view;
                egui::ComboBox::from_id_salt("debug_view")
                    .width(ui.available_width())
                    .selected_text(self.view.label())
                    .show_ui(ui, |ui| {
                        for view in DebugView::ALL {
                            ui.selectable_value(&mut self.view, view, view.label());
                        }
                    });
                if self.view != before {
                    self.texture = None;
                }

                ui.add_space(16.0);
                ui.separator();
                ui.weak(format!(
                    "Showing seed {} at {}×{}",
                    self.seed.0, self.heightmap.width, self.heightmap.height
                ));
            });
    }

    fn central_panel(&mut self, ctx: &egui::Context) {
        if self.texture.is_none() {
            let image = self.render_view();
            self.texture = Some(ctx.load_texture("debug_view", image, TextureOptions::NEAREST));
        }
        let texture = self.texture.as_ref().expect("texture was just created");

        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_size();
            let [w, h] = texture.size().map(|v| v as f32);
            // Fit the whole image in the panel, keeping its aspect ratio.
            let scale = (available.x / w).min(available.y / h);
            let size = egui::vec2(w * scale, h * scale);
            ui.centered_and_justified(|ui| {
                ui.add(egui::Image::new(texture).fit_to_exact_size(size));
            });
        });
    }
}

impl eframe::App for WorldGenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.side_panel(ctx);
        self.central_panel(ctx);
    }
}

fn heightmap_image(map: &Heightmap) -> ColorImage {
    let grey: Vec<u8> = map
        .data
        .iter()
        .map(|&h| (h.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    ColorImage::from_gray([map.width, map.height], &grey)
}
