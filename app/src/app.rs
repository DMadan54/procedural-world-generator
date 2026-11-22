use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use worldgen_core::{generate_heightmap, Heightmap, HeightmapParams, WorldSeed};

use crate::render;

const DEFAULT_SEED: u64 = 42;
const DEFAULT_SEA_LEVEL: f32 = 0.45;

/// What the central panel shows. Every pipeline stage gets an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DebugView {
    /// Heightmap coloured by elevation relative to sea level.
    Terrain,
    /// Raw heightmap stage output in greyscale.
    Heightmap,
}

impl DebugView {
    const ALL: [DebugView; 2] = [DebugView::Terrain, DebugView::Heightmap];

    fn label(self) -> &'static str {
        match self {
            DebugView::Terrain => "Terrain",
            DebugView::Heightmap => "Heightmap (greyscale)",
        }
    }
}

pub struct WorldGenApp {
    seed_text: String,
    seed_error: Option<String>,
    seed: WorldSeed,
    params: HeightmapParams,
    heightmap: Heightmap,
    /// Display only: changing it recolours the existing heightmap.
    sea_level: f32,
    view: DebugView,
    texture: Option<TextureHandle>,
    /// Set when the image needs redrawing (new world, view or colouring).
    texture_dirty: bool,
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
            sea_level: DEFAULT_SEA_LEVEL,
            view: DebugView::Terrain,
            texture: None,
            texture_dirty: true,
        }
    }

    fn regenerate(&mut self) {
        match self.seed_text.trim().parse::<u64>() {
            Ok(value) => {
                self.seed_error = None;
                self.seed = WorldSeed(value);
                self.heightmap = generate_heightmap(self.seed, &self.params);
                self.texture_dirty = true;
            }
            Err(_) => {
                self.seed_error = Some(format!(
                    "Seed must be a whole number from 0 to {}.",
                    u64::MAX
                ));
            }
        }
    }

    fn render_view(&self) -> ColorImage {
        match self.view {
            DebugView::Terrain => render::hypsometric(&self.heightmap, self.sea_level),
            DebugView::Heightmap => render::greyscale(&self.heightmap),
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
                ui.label("Sea level");
                ui.spacing_mut().slider_width = ui.available_width() - 56.0;
                let sea = ui.add(
                    egui::Slider::new(&mut self.sea_level, 0.0..=1.0)
                        .fixed_decimals(2)
                        .step_by(0.01),
                );
                if sea.changed() && self.view == DebugView::Terrain {
                    self.texture_dirty = true;
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
                    self.texture_dirty = true;
                }

                ui.add_space(16.0);
                ui.separator();
                ui.weak(format!(
                    "Showing seed {} at {}×{}",
                    self.seed.0, self.heightmap.width, self.heightmap.height
                ));
            });
    }

    /// Redraws the image if needed, reusing the GPU texture when it exists.
    fn update_texture(&mut self, ctx: &egui::Context) {
        if !self.texture_dirty && self.texture.is_some() {
            return;
        }
        let image = self.render_view();
        match &mut self.texture {
            Some(texture) => texture.set(image, TextureOptions::NEAREST),
            None => {
                self.texture = Some(ctx.load_texture("debug_view", image, TextureOptions::NEAREST))
            }
        }
        self.texture_dirty = false;
    }

    fn central_panel(&mut self, ctx: &egui::Context) {
        self.update_texture(ctx);
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
