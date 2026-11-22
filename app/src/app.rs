use eframe::egui::{self, Color32, ColorImage, Rect, Stroke, TextureHandle, TextureOptions};
use worldgen_core::{
    BoundaryKind, PlateKind, World, WorldConfig, MAX_PLATES, MAX_TEMPERATURE_OFFSET, MIN_PLATES,
    MIN_TEMPERATURE_OFFSET,
};

use crate::render::{self, Rgb};

/// What the central panel shows. Every pipeline stage gets an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DebugView {
    /// Final heightmap coloured by elevation relative to sea level.
    Terrain,
    /// Tectonics stage: plate regions, boundary types and drift.
    Plates,
    /// Tectonics stage: the relief it contributes.
    TectonicRelief,
    /// Noise stage: raw fractal noise.
    Noise,
    /// Final heightmap in greyscale.
    Heightmap,
    /// Climate stage: mean temperature.
    Temperature,
    /// Climate stage: annual precipitation.
    Precipitation,
    /// Climate stage: prevailing winds.
    Wind,
}

impl DebugView {
    const ALL: [DebugView; 8] = [
        DebugView::Terrain,
        DebugView::Plates,
        DebugView::TectonicRelief,
        DebugView::Noise,
        DebugView::Heightmap,
        DebugView::Temperature,
        DebugView::Precipitation,
        DebugView::Wind,
    ];

    fn label(self) -> &'static str {
        match self {
            DebugView::Terrain => "Terrain",
            DebugView::Plates => "Tectonic plates",
            DebugView::TectonicRelief => "Tectonic relief",
            DebugView::Noise => "Fractal noise",
            DebugView::Heightmap => "Heightmap (greyscale)",
            DebugView::Temperature => "Temperature",
            DebugView::Precipitation => "Precipitation",
            DebugView::Wind => "Prevailing winds",
        }
    }
}

pub struct WorldGenApp {
    /// What the controls are editing; applied to `world` via `World::update`.
    config: WorldConfig,
    world: World,
    seed_text: String,
    seed_error: Option<String>,
    view: DebugView,
    texture: Option<TextureHandle>,
    /// Set when the image needs redrawing (new world, view or colouring).
    texture_dirty: bool,
}

impl WorldGenApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = WorldConfig::default();
        let world = World::generate(&config);
        Self {
            seed_text: config.seed.to_string(),
            config,
            world,
            seed_error: None,
            view: DebugView::Terrain,
            texture: None,
            texture_dirty: true,
        }
    }

    /// Brings the world up to date with `config`, re-running only what changed.
    fn apply_config(&mut self) {
        let run = self.world.update(&self.config);
        if run != Default::default() {
            self.texture_dirty = true;
        }
    }

    fn regenerate(&mut self) {
        match self.seed_text.trim().parse::<u64>() {
            Ok(seed) => {
                self.seed_error = None;
                self.config.seed = seed;
                self.apply_config();
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
        let (w, h) = (self.world.config.width, self.world.config.height);
        let sea = self.world.config.terrain.sea_level;
        let (map, climate) = (&self.world.heightmap, &self.world.climate);
        match self.view {
            DebugView::Terrain => render::hypsometric(map, sea),
            DebugView::Plates => render::plates(&self.world.tectonics),
            DebugView::TectonicRelief => {
                render::stretched_greyscale(w, h, &self.world.tectonics.elevation)
            }
            DebugView::Noise => render::stretched_greyscale(w, h, &self.world.noise),
            DebugView::Heightmap => render::greyscale(map),
            DebugView::Temperature => render::temperature(climate, map, sea),
            DebugView::Precipitation => render::precipitation(climate, map, sea),
            DebugView::Wind => render::wind_background(map, sea),
        }
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("controls")
            .resizable(false)
            .exact_width(230.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
            });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("World");
        ui.add_space(8.0);

        ui.label("Seed");
        let field = ui.add(
            egui::TextEdit::singleline(&mut self.seed_text)
                .desired_width(f32::INFINITY)
                .hint_text("e.g. 42"),
        );
        let pressed_enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
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

        ui.add_space(12.0);
        ui.separator();
        ui.spacing_mut().slider_width = ui.available_width() - 56.0;

        ui.label("Tectonic plates");
        let plates = ui.add(egui::Slider::new(
            &mut self.config.tectonics.plate_count,
            MIN_PLATES..=MAX_PLATES,
        ));
        if plates.changed() {
            self.apply_config();
        }

        ui.add_space(6.0);
        ui.label("Sea level");
        let sea = ui.add(
            egui::Slider::new(&mut self.config.terrain.sea_level, 0.0..=1.0)
                .fixed_decimals(2)
                .step_by(0.01),
        );
        if sea.changed() {
            // Re-runs climate only; the heightmap is unchanged.
            self.apply_config();
        }

        ui.add_space(6.0);
        ui.label("Global temperature");
        let temperature = ui.add(
            egui::Slider::new(
                &mut self.config.climate.global_temperature_offset,
                MIN_TEMPERATURE_OFFSET..=MAX_TEMPERATURE_OFFSET,
            )
            .fixed_decimals(1)
            .step_by(0.5)
            .suffix(" °C"),
        );
        ui.horizontal(|ui| {
            ui.weak("Ice age");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(56.0);
                ui.weak("Hothouse");
            });
        });
        if temperature.changed() {
            self.apply_config();
        }

        ui.add_space(12.0);
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

        ui.add_space(8.0);
        match self.view {
            DebugView::Plates => self.plate_legend(ui),
            DebugView::Temperature => {
                for (value, colour) in render::TEMPERATURE_SCALE {
                    legend_row(ui, colour, &format!("{value:.0} °C"));
                }
            }
            DebugView::Precipitation => {
                for (value, colour) in render::PRECIPITATION_SCALE {
                    legend_row(ui, colour, &format!("{value:.0} mm/year"));
                }
            }
            DebugView::Wind => {
                ui.weak("Easterly trades 0-30°, westerlies 30-60°, polar easterlies 60-90°.");
            }
            _ => {}
        }

        ui.add_space(12.0);
        ui.separator();
        ui.weak(format!(
            "Seed {} at {}×{}",
            self.world.config.seed, self.world.config.width, self.world.config.height
        ));
    }

    fn plate_legend(&self, ui: &mut egui::Ui) {
        let t = &self.world.tectonics;
        let count = |kind| t.boundaries.iter().filter(|b| b.kind == kind).count();
        let continental = t.plates.iter().filter(|p| p.kind == PlateKind::Continental).count();

        legend_row(ui, render::plate_colour(1, PlateKind::Continental), &format!("Continental plates: {continental}"));
        legend_row(ui, render::plate_colour(1, PlateKind::Oceanic), &format!("Oceanic plates: {}", t.plates.len() - continental));
        legend_row(ui, render::CONVERGENT, &format!("Convergent: {}", count(BoundaryKind::Convergent)));
        legend_row(ui, render::DIVERGENT, &format!("Divergent: {}", count(BoundaryKind::Divergent)));
        legend_row(ui, render::TRANSFORM, &format!("Transform: {}", count(BoundaryKind::Transform)));
        ui.weak("Arrows show each plate's drift.");
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
            let panel = ui.available_rect_before_wrap();
            let [w, h] = texture.size().map(|v| v as f32);
            // Fit the whole image in the panel, keeping its aspect ratio.
            let scale = (panel.width() / w).min(panel.height() / h);
            let rect = Rect::from_center_size(panel.center(), egui::vec2(w * scale, h * scale));
            egui::Image::new(texture).paint_at(ui, rect);

            match self.view {
                DebugView::Plates => self.draw_drift_arrows(ui.painter(), rect),
                DebugView::Wind => self.draw_wind_arrows(ui.painter(), rect),
                _ => {}
            }
        });
    }

    fn draw_drift_arrows(&self, painter: &egui::Painter, rect: Rect) {
        let t = &self.world.tectonics;
        let to_screen = |p: [f32; 2]| {
            rect.min + egui::vec2(p[0] / t.width as f32 * rect.width(), p[1] / t.height as f32 * rect.height())
        };
        let length = rect.width() * 0.07;
        for plate in &t.plates {
            let origin = to_screen(plate.centre);
            let arrow = egui::vec2(plate.drift[0], plate.drift[1]) * length;
            painter.arrow(origin, arrow, Stroke::new(4.0_f32, Color32::from_black_alpha(170)));
            painter.arrow(origin, arrow, Stroke::new(2.0_f32, Color32::WHITE));
            painter.circle_filled(origin, 3.5_f32, Color32::WHITE);
            painter.circle_stroke(origin, 3.5_f32, Stroke::new(1.0_f32, Color32::BLACK));
        }
    }
}

impl WorldGenApp {
    fn draw_wind_arrows(&self, painter: &egui::Painter, rect: Rect) {
        let c = &self.world.climate;
        if c.width == 0 || c.height == 0 {
            return;
        }
        // About 20 arrows across, whatever the map size.
        let spacing = (c.width / 20).max(1);
        let cell = rect.width() / c.width as f32 * spacing as f32;
        let length = cell * 0.8;
        for y in (spacing / 2..c.height).step_by(spacing) {
            for x in (spacing / 2..c.width).step_by(spacing) {
                let [u, v] = c.wind[y * c.width + x];
                let centre = rect.min
                    + egui::vec2(
                        (x as f32 + 0.5) / c.width as f32 * rect.width(),
                        (y as f32 + 0.5) / c.height as f32 * rect.height(),
                    );
                let dir = egui::vec2(u, v);
                if dir.length() < 0.05 {
                    painter.circle_filled(centre, 1.5_f32, Color32::from_gray(30));
                    continue;
                }
                let arrow = dir.normalized() * length * dir.length().min(1.0);
                let origin = centre - arrow * 0.5;
                painter.arrow(origin, arrow, Stroke::new(1.5_f32, Color32::from_gray(25)));
            }
        }
    }
}

fn legend_row(ui: &mut egui::Ui, colour: Rgb, label: &str) {
    ui.horizontal(|ui| {
        let (swatch, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(swatch, 2.0_f32, Color32::from_rgb(colour[0], colour[1], colour[2]));
        ui.painter()
            .rect_stroke(swatch, 2.0_f32, Stroke::new(1.0_f32, Color32::GRAY), egui::StrokeKind::Inside);
        ui.label(label);
    });
}

impl eframe::App for WorldGenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.side_panel(ctx);
        self.central_panel(ctx);
    }
}
