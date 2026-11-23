use eframe::egui::{self, Color32, ColorImage, Rect, Stroke, TextureHandle, TextureOptions};
use worldgen_core::{
    BoundaryKind, Heightmap, PlateKind, World, WorldConfig, MAX_DROPLETS, MAX_PLATES,
    MAX_RIVER_THRESHOLD, MAX_TEMPERATURE_OFFSET, MIN_PLATES, MIN_RIVER_THRESHOLD,
    MIN_TEMPERATURE_OFFSET,
};

/// Erosion rounds (4096 droplets each) to run per frame while erosion is in
/// progress: enough to finish quickly, few enough to keep the UI responsive.
#[cfg(not(target_arch = "wasm32"))]
const EROSION_ROUNDS_PER_FRAME: usize = 6;
#[cfg(target_arch = "wasm32")]
const EROSION_ROUNDS_PER_FRAME: usize = 1;

/// Height change at which the erosion-change view saturates.
const EROSION_CHANGE_SCALE: f32 = 0.04;

use crate::link;
use crate::render::{self, Rgb};
use crate::setup::SetupPanel;
use crate::style::{all_styles, MapStyle};

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
    /// Erosion stage: where material was removed and deposited.
    ErosionChange,
    /// Climate stage: mean temperature.
    Temperature,
    /// Climate stage: annual precipitation.
    Precipitation,
    /// Climate stage: prevailing winds.
    Wind,
    /// Hydrology stage: flow accumulation, lakes and rivers.
    Drainage,
    /// Biome stage: biome per cell.
    Biomes,
    /// Habitability stage: how good each cell is to live on.
    Habitability,
    /// Peoples stage: homelands, origins and names.
    Peoples,
    /// Settlements stage: sites by rank over the homelands.
    Settlements,
    /// Realms stage: the political map at year 0.
    Realms,
}

impl DebugView {
    const ALL: [DebugView; 15] = [
        DebugView::Terrain,
        DebugView::Plates,
        DebugView::TectonicRelief,
        DebugView::Noise,
        DebugView::Heightmap,
        DebugView::ErosionChange,
        DebugView::Temperature,
        DebugView::Precipitation,
        DebugView::Wind,
        DebugView::Drainage,
        DebugView::Biomes,
        DebugView::Habitability,
        DebugView::Peoples,
        DebugView::Settlements,
        DebugView::Realms,
    ];

    fn label(self) -> &'static str {
        match self {
            DebugView::Terrain => "Terrain",
            DebugView::Plates => "Tectonic plates",
            DebugView::TectonicRelief => "Tectonic relief",
            DebugView::Noise => "Fractal noise",
            DebugView::Heightmap => "Heightmap (greyscale)",
            DebugView::ErosionChange => "Erosion change",
            DebugView::Temperature => "Temperature",
            DebugView::Precipitation => "Precipitation",
            DebugView::Wind => "Prevailing winds",
            DebugView::Drainage => "Drainage (flow accumulation)",
            DebugView::Biomes => "Biomes",
            DebugView::Habitability => "Habitability",
            DebugView::Peoples => "Peoples",
            DebugView::Settlements => "Settlements",
            DebugView::Realms => "Realms",
        }
    }
}

pub struct WorldGenApp {
    /// What the controls are editing; applied to `world` via `World::update`.
    config: WorldConfig,
    world: World,
    seed_text: String,
    seed_error: Option<String>,
    setup: SetupPanel,
    /// A message for the user (a bad share link, a copied link).
    notice: Option<String>,
    /// The config last written to the browser address bar.
    linked_config: Option<WorldConfig>,
    /// Finished-map styles; `style` picks one, or `None` for a debug view.
    styles: Vec<Box<dyn MapStyle>>,
    style: Option<usize>,
    view: DebugView,
    /// Show the terrain as it was before erosion (Terrain and Heightmap views).
    before_erosion: bool,
    texture: Option<TextureHandle>,
    /// Set when the image needs redrawing (new world, view or colouring).
    texture_dirty: bool,
}

impl WorldGenApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (config, notice) = link::startup_config();
        // Show a map straight away and erode in the background with progress,
        // rather than blocking start-up on erosion.
        let mut world = World::generate(&WorldConfig {
            erosion: worldgen_core::config::ErosionConfig {
                droplets: 0,
                ..config.erosion.clone()
            },
            ..config.clone()
        });
        world.set_config(&config);
        Self {
            seed_text: config.seed.to_string(),
            config,
            world,
            seed_error: None,
            setup: SetupPanel::default(),
            notice,
            linked_config: None,
            styles: all_styles(),
            style: Some(0),
            view: DebugView::Terrain,
            before_erosion: false,
            texture: None,
            texture_dirty: true,
        }
    }

    /// Brings the world up to date with `config`, re-running only what changed.
    /// Runs cheap stages at once; erosion and later stages continue over the
    /// next frames (see `advance_pipeline`).
    fn apply_config(&mut self) {
        let run = self.world.set_config(&self.config);
        if run != Default::default() {
            self.texture_dirty = true;
        }
    }

    /// Continues queued erosion a few rounds per frame.
    fn advance_pipeline(&mut self, ctx: &egui::Context) {
        if self.world.is_ready() {
            return;
        }
        if self.world.advance(EROSION_ROUNDS_PER_FRAME) {
            self.texture_dirty = true;
        }
        ctx.request_repaint();
    }

    /// The terrain to show: eroded, or the pre-erosion heightmap.
    fn shown_terrain(&self) -> &Heightmap {
        if self.before_erosion {
            &self.world.heightmap
        } else {
            &self.world.terrain
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
        if let Some(style) = self.style.and_then(|i| self.styles.get(i)) {
            let canvas = style.render(&self.world);
            return ColorImage::from_rgb([canvas.width, canvas.height], &canvas.to_rgb8());
        }
        let (w, h) = (self.world.config.width, self.world.config.height);
        let sea = self.world.config.terrain.sea_level;
        let (map, climate) = (&self.world.terrain, &self.world.climate);
        match self.view {
            DebugView::Terrain if self.before_erosion => {
                render::hypsometric(&self.world.heightmap, sea)
            }
            DebugView::Terrain => render::terrain_with_lakes(map, &self.world.hydrology, sea),
            DebugView::Plates => render::plates(&self.world.tectonics),
            DebugView::TectonicRelief => {
                render::stretched_greyscale(w, h, &self.world.tectonics.elevation)
            }
            DebugView::Noise => render::stretched_greyscale(w, h, &self.world.noise),
            DebugView::Heightmap => render::greyscale(self.shown_terrain()),
            DebugView::ErosionChange => render::erosion_change(
                &self.world.heightmap,
                &self.world.terrain,
                EROSION_CHANGE_SCALE,
            ),
            DebugView::Temperature => render::temperature(climate, map, sea),
            DebugView::Precipitation => render::precipitation(climate, map, sea),
            DebugView::Wind => render::wind_background(map, sea),
            DebugView::Drainage => render::flow_accumulation(map, &self.world.hydrology, sea),
            DebugView::Biomes => render::biomes(&self.world.biomes),
            DebugView::Habitability => render::habitability(&self.world),
            DebugView::Peoples | DebugView::Settlements => render::peoples(&self.world),
            DebugView::Realms => render::realms(&self.world),
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
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("World setup…").clicked() {
                self.setup.open_with(&self.config);
            }
            if ui.button("Copy share link").clicked() {
                ui.ctx().copy_text(link::link_for(&self.config));
                self.notice = Some("Link copied to the clipboard.".into());
            }
        });
        if let Some(notice) = &self.notice {
            ui.weak(notice);
        }
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
            .add_sized(
                [ui.available_width(), 28.0],
                egui::Button::new("Regenerate"),
            )
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

        ui.add_space(6.0);
        self.erosion_controls(ui);

        ui.add_space(6.0);
        ui.label("River threshold");
        let rivers = ui.add(
            egui::Slider::new(
                &mut self.config.hydrology.river_threshold,
                MIN_RIVER_THRESHOLD..=MAX_RIVER_THRESHOLD,
            )
            .logarithmic(true)
            .fixed_decimals(0),
        );
        if rivers.changed() {
            self.apply_config();
        }

        ui.add_space(12.0);
        ui.separator();
        ui.label("View");
        let before = (self.style, self.view);
        let selected = match self.style.and_then(|i| self.styles.get(i)) {
            Some(style) => style.name(),
            None => self.view.label(),
        };
        egui::ComboBox::from_id_salt("view")
            .width(ui.available_width())
            .height(600.0)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (i, style) in self.styles.iter().enumerate() {
                    if ui
                        .selectable_label(self.style == Some(i), style.name())
                        .clicked()
                    {
                        self.style = Some(i);
                    }
                }
                ui.separator();
                ui.weak("Debug views");
                for view in DebugView::ALL {
                    let on = self.style.is_none() && self.view == view;
                    if ui.selectable_label(on, view.label()).clicked() {
                        self.style = None;
                        self.view = view;
                    }
                }
            });
        if (self.style, self.view) != before {
            self.texture_dirty = true;
        }

        if self.style.is_none() && matches!(self.view, DebugView::Terrain | DebugView::Heightmap) {
            let toggle = ui
                .checkbox(&mut self.before_erosion, "Show before erosion (B)")
                .changed();
            let key = ui.input(|i| i.key_pressed(egui::Key::B)) && !ui.ctx().wants_keyboard_input();
            if key {
                self.before_erosion = !self.before_erosion;
            }
            if toggle || key {
                self.texture_dirty = true;
            }
        }

        ui.add_space(8.0);
        let debug_view = if self.style.is_none() {
            Some(self.view)
        } else {
            None
        };
        match debug_view {
            None => {}
            Some(DebugView::Habitability) => {
                for (value, colour) in render::HABITABILITY_SCALE {
                    legend_row(ui, colour, &format!("{value:.2}"));
                }
            }
            Some(DebugView::Peoples) => {
                let total: u32 = self
                    .world
                    .peoples
                    .peoples
                    .iter()
                    .map(|p| p.cells)
                    .sum::<u32>()
                    .max(1);
                for people in &self.world.peoples.peoples {
                    let label = format!(
                        "{}, {} ({:.0}%)",
                        people.name,
                        people.trait_.name(),
                        100.0 * people.cells as f32 / total as f32
                    );
                    legend_row(ui, render::people_colour(people.id), &label);
                }
                ui.weak("Grey land belongs to no people.");
            }
            Some(DebugView::Settlements) => {
                use worldgen_core::settlements::Rank;
                let list = &self.world.settlements.settlements;
                for rank in [Rank::Capital, Rank::City, Rank::Town, Rank::Village] {
                    let n = list.iter().filter(|s| s.rank == rank).count();
                    ui.label(format!("{}: {n}", rank.name()));
                }
                let flags = |f: fn(&worldgen_core::settlements::SettlementFlags) -> bool| {
                    list.iter().filter(|s| f(&s.flags)).count()
                };
                ui.weak(format!(
                    "Ports {}, river crossings {}, passes {}, oases {}",
                    flags(|f| f.port),
                    flags(|f| f.river_crossing),
                    flags(|f| f.mountain_pass),
                    flags(|f| f.oasis)
                ));
            }
            Some(DebugView::Realms) => {
                let r = &self.world.realms;
                let total: u32 = r.realms.iter().map(|x| x.cells).sum::<u32>().max(1);
                for realm in &r.realms {
                    let people = &self.world.peoples.peoples[realm.people as usize];
                    let label = format!(
                        "{} ({}, House {}, {:.0}%)",
                        realm.name,
                        people.name,
                        realm.dynasty.house,
                        100.0 * realm.cells as f32 / total as f32
                    );
                    legend_row(ui, render::realm_rgb(realm.colour), &label);
                }
                ui.weak("Grey land is unclaimed wilderness.");
            }
            Some(DebugView::Biomes) => {
                ui.weak("Share of land");
                for (biome, share) in self.world.biomes.coverage() {
                    let label = format!("{} {:.1}%", biome.name(), share * 100.0);
                    legend_row(ui, render::biome_colour(biome), &label);
                }
                ui.add_space(8.0);
                ui.weak("Whittaker diagram");
                whittaker_diagram(ui, &self.world);
            }
            Some(view) => match view {
                DebugView::Plates => self.plate_legend(ui),
                DebugView::ErosionChange => {
                    legend_row(ui, [220, 50, 40], "Eroded");
                    legend_row(ui, [40, 110, 230], "Deposited");
                    ui.weak(format!(
                        "Full colour at a change of {:.0}% of the height range.",
                        EROSION_CHANGE_SCALE * 100.0
                    ));
                }
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
                DebugView::Terrain | DebugView::Drainage if !self.before_erosion => {
                    let hy = &self.world.hydrology;
                    legend_row(
                        ui,
                        render::RIVER,
                        &format!(
                            "Rivers: {} ({} segments)",
                            hy.rivers.len(),
                            hy.segments.len()
                        ),
                    );
                    legend_row(ui, render::LAKE, &format!("Lakes: {}", hy.lakes.len()));
                    if let Some(longest) = hy
                        .rivers
                        .iter()
                        .max_by(|a, b| a.discharge.total_cmp(&b.discharge))
                    {
                        ui.weak(format!("Largest river discharge: {:.0}", longest.discharge));
                    }
                }
                _ => {}
            },
        }

        ui.add_space(12.0);
        ui.separator();
        ui.weak(format!(
            "Seed {} at {}×{}",
            self.world.config.seed, self.world.config.width, self.world.config.height
        ));
    }

    fn erosion_controls(&mut self, ui: &mut egui::Ui) {
        let e = &mut self.config.erosion;
        ui.label("Erosion droplets");
        let mut changed = ui
            .add(egui::Slider::new(&mut e.droplets, 0..=MAX_DROPLETS).logarithmic(true))
            .changed();
        egui::CollapsingHeader::new("Erosion settings").show(ui, |ui| {
            let slider = |ui: &mut egui::Ui, label: &str, value: &mut f32, max: f32| {
                ui.label(label);
                ui.add(egui::Slider::new(value, 0.0..=max).fixed_decimals(3))
                    .changed()
            };
            changed |= slider(ui, "Erosion rate", &mut e.erosion_rate, 1.0);
            changed |= slider(ui, "Deposition rate", &mut e.deposition_rate, 1.0);
            changed |= slider(ui, "Evaporation", &mut e.evaporation, 0.1);
            changed |= slider(ui, "Inertia", &mut e.inertia, 0.99);
        });
        if changed {
            self.apply_config();
        }
        if let Some(progress) = self.world.erosion_progress() {
            ui.add(
                egui::ProgressBar::new(progress)
                    .text(format!("Eroding… {:.0}%", progress * 100.0))
                    .animate(true),
            );
        }
    }

    fn plate_legend(&self, ui: &mut egui::Ui) {
        let t = &self.world.tectonics;
        let count = |kind| t.boundaries.iter().filter(|b| b.kind == kind).count();
        let continental = t
            .plates
            .iter()
            .filter(|p| p.kind == PlateKind::Continental)
            .count();

        legend_row(
            ui,
            render::plate_colour(1, PlateKind::Continental),
            &format!("Continental plates: {continental}"),
        );
        legend_row(
            ui,
            render::plate_colour(1, PlateKind::Oceanic),
            &format!("Oceanic plates: {}", t.plates.len() - continental),
        );
        legend_row(
            ui,
            render::CONVERGENT,
            &format!("Convergent: {}", count(BoundaryKind::Convergent)),
        );
        legend_row(
            ui,
            render::DIVERGENT,
            &format!("Divergent: {}", count(BoundaryKind::Divergent)),
        );
        legend_row(
            ui,
            render::TRANSFORM,
            &format!("Transform: {}", count(BoundaryKind::Transform)),
        );
        ui.weak("Arrows show each plate's drift.");
    }

    /// Redraws the image if needed, reusing the GPU texture when it exists.
    fn update_texture(&mut self, ctx: &egui::Context) {
        if !self.texture_dirty && self.texture.is_some() {
            return;
        }
        let image = self.render_view();
        // Styled maps are drawn above world resolution: smooth them when
        // scaling. Debug views show exact cells.
        let options = self.texture_options();
        match &mut self.texture {
            Some(texture) => texture.set(image, options),
            None => self.texture = Some(ctx.load_texture("view", image, options)),
        }
        self.texture_dirty = false;
    }

    fn texture_options(&self) -> TextureOptions {
        if self.style.is_some() {
            TextureOptions::LINEAR
        } else {
            TextureOptions::NEAREST
        }
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
            let response = ui.allocate_rect(rect, egui::Sense::hover());
            if let Some(pointer) = response.hover_pos() {
                self.settlement_tooltip(&response, rect, pointer);
            }

            let debug_view = if self.style.is_none() {
                Some(self.view)
            } else {
                None
            };
            // Styled maps draw everything themselves; debug views get overlays.
            match debug_view {
                Some(DebugView::Plates) => self.draw_drift_arrows(ui.painter(), rect),
                Some(DebugView::Peoples) => self.draw_people_names(ui.painter(), rect),
                Some(DebugView::Settlements) => self.draw_settlement_markers(ui.painter(), rect),
                Some(DebugView::Realms) => self.draw_capitals(ui.painter(), rect),
                Some(DebugView::Wind) => self.draw_wind_arrows(ui.painter(), rect),
                Some(DebugView::Terrain | DebugView::Drainage) if !self.before_erosion => {
                    self.draw_rivers(ui.painter(), rect)
                }
                _ => {}
            }
            if self.style.is_none()
                && self.before_erosion
                && matches!(self.view, DebugView::Terrain | DebugView::Heightmap)
            {
                badge(ui.painter(), rect, "Before erosion");
            } else if let Some(progress) = self.world.erosion_progress() {
                badge(
                    ui.painter(),
                    rect,
                    &format!("Eroding… {:.0}%", progress * 100.0),
                );
            }
        });
    }

    fn draw_drift_arrows(&self, painter: &egui::Painter, rect: Rect) {
        let t = &self.world.tectonics;
        let to_screen = |p: [f32; 2]| {
            rect.min
                + egui::vec2(
                    p[0] / t.width as f32 * rect.width(),
                    p[1] / t.height as f32 * rect.height(),
                )
        };
        let length = rect.width() * 0.07;
        for plate in &t.plates {
            let origin = to_screen(plate.centre);
            let arrow = egui::vec2(plate.drift[0], plate.drift[1]) * length;
            painter.arrow(
                origin,
                arrow,
                Stroke::new(4.0_f32, Color32::from_black_alpha(170)),
            );
            painter.arrow(origin, arrow, Stroke::new(2.0_f32, Color32::WHITE));
            painter.circle_filled(origin, 3.5_f32, Color32::WHITE);
            painter.circle_stroke(origin, 3.5_f32, Stroke::new(1.0_f32, Color32::BLACK));
        }
    }
}

impl WorldGenApp {
    /// When the pointer is over (or right next to) a settlement, shows its
    /// name, rank, people, population and features.
    fn settlement_tooltip(&self, response: &egui::Response, rect: Rect, pointer: egui::Pos2) {
        let s = &self.world.settlements;
        if s.width == 0 {
            return;
        }
        let cell = rect.width() / s.width as f32;
        let (wx, wy) = (
            (pointer.x - rect.left()) / cell,
            (pointer.y - rect.top()) / cell,
        );
        // Within about 4 screen pixels or 1.5 cells, whichever is larger.
        let reach = (4.0 / cell).max(1.5);
        let nearest = s
            .settlements
            .iter()
            .map(|st| {
                let [x, y] = st.position(s.width);
                let (dx, dy) = (x as f32 + 0.5 - wx, y as f32 + 0.5 - wy);
                (dx * dx + dy * dy, st)
            })
            .filter(|(d2, _)| *d2 <= reach * reach)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        let Some((_, settlement)) = nearest else {
            return;
        };
        let people = &self.world.peoples.peoples[settlement.people as usize];
        response.clone().on_hover_ui_at_pointer(|ui| {
            ui.strong(&settlement.name);
            ui.label(format!(
                "{} of the {} ({})",
                capitalised(settlement.rank.name()),
                people.name,
                people.trait_.name()
            ));
            ui.label(format!("Population {}", settlement.population));
            let flags = settlement.flags.describe();
            if !flags.is_empty() {
                ui.weak(capitalised(&flags));
            }
        });
    }

    /// Each realm's capital with the realm's name.
    fn draw_capitals(&self, painter: &egui::Painter, rect: Rect) {
        let s = &self.world.settlements;
        let scale = rect.width() / s.width.max(1) as f32;
        for realm in &self.world.realms.realms {
            let capital = &s.settlements[realm.capital as usize];
            let [x, y] = capital.position(s.width);
            let at = rect.min + egui::vec2(x as f32 + 0.5, y as f32 + 0.5) * scale;
            painter.circle_filled(at, 5.0_f32, Color32::BLACK);
            painter.circle_filled(at, 3.0_f32, Color32::WHITE);
            painter.text(
                at + egui::vec2(0.0, -8.0),
                egui::Align2::CENTER_BOTTOM,
                &realm.name,
                egui::FontId::proportional(13.0),
                Color32::BLACK,
            );
        }
    }

    /// Settlements as rank-sized dots; capitals labelled.
    fn draw_settlement_markers(&self, painter: &egui::Painter, rect: Rect) {
        use worldgen_core::settlements::Rank;
        let s = &self.world.settlements;
        let scale = rect.width() / s.width.max(1) as f32;
        for settlement in &s.settlements {
            let [x, y] = settlement.position(s.width);
            let at = rect.min + egui::vec2(x as f32 + 0.5, y as f32 + 0.5) * scale;
            let radius = match settlement.rank {
                Rank::Capital => 5.0,
                Rank::City => 3.5,
                Rank::Town => 2.5,
                Rank::Village => 1.6,
            };
            painter.circle_filled(at, radius + 1.0, Color32::BLACK);
            painter.circle_filled(at, radius, Color32::WHITE);
            if settlement.rank == Rank::Capital {
                painter.text(
                    at + egui::vec2(0.0, -8.0),
                    egui::Align2::CENTER_BOTTOM,
                    &settlement.name,
                    egui::FontId::proportional(13.0),
                    Color32::BLACK,
                );
            }
        }
    }

    /// Each people's name at its origin, with a marker.
    fn draw_people_names(&self, painter: &egui::Painter, rect: Rect) {
        let p = &self.world.peoples;
        let scale = rect.width() / p.width.max(1) as f32;
        for people in &p.peoples {
            let (x, y) = (people.origin % p.width, people.origin / p.width);
            let at = rect.min + egui::vec2(x as f32 + 0.5, y as f32 + 0.5) * scale;
            painter.circle_filled(at, 4.0_f32, Color32::BLACK);
            painter.circle_filled(at, 2.5_f32, Color32::WHITE);
            painter.text(
                at + egui::vec2(0.0, -8.0),
                egui::Align2::CENTER_BOTTOM,
                &people.name,
                egui::FontId::proportional(14.0),
                Color32::BLACK,
            );
        }
    }

    /// Rivers from the hydrology polylines, wider with more discharge.
    fn draw_rivers(&self, painter: &egui::Painter, rect: Rect) {
        let hy = &self.world.hydrology;
        if hy.width == 0 {
            return;
        }
        let scale = rect.width() / hy.width as f32;
        let threshold = self.world.config.hydrology.river_threshold.max(1e-3);
        let colour = Color32::from_rgb(render::RIVER[0], render::RIVER[1], render::RIVER[2]);
        let to_screen = |p: [f32; 2]| rect.min + egui::vec2(p[0], p[1]) * scale;
        for segment in &hy.segments {
            for (pair, q) in segment.points.windows(2).zip(&segment.discharge[1..]) {
                let width = (0.35 + 0.45 * (q / threshold).sqrt()).min(3.5) * scale;
                painter.line_segment(
                    [to_screen(pair[0]), to_screen(pair[1])],
                    Stroke::new(width.max(0.8), colour),
                );
            }
        }
    }

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

/// Temperature (x) against precipitation (y), shaded by the classification
/// regions, with a fixed sample of land cells as dots in their biome colour.
/// Shows at a glance whether a world's climate spread is believable.
fn whittaker_diagram(ui: &mut egui::Ui, world: &World) {
    const T_RANGE: (f32, f32) = (-20.0, 32.0);
    const P_MAX: f32 = 4000.0;
    const SAMPLES: usize = 2000;

    let width = ui.available_width().min(220.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, width * 0.75), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let to_screen = |t: f32, p: f32| {
        let u = (t - T_RANGE.0) / (T_RANGE.1 - T_RANGE.0);
        let v = (p / P_MAX).clamp(0.0, 1.0);
        egui::pos2(
            rect.left() + u * rect.width(),
            rect.bottom() - v * rect.height(),
        )
    };

    // Regions: classify a grid of (temperature, precipitation) at sea level.
    const STEPS: usize = 40;
    for i in 0..STEPS {
        for j in 0..STEPS {
            let t0 = T_RANGE.0 + (T_RANGE.1 - T_RANGE.0) * i as f32 / STEPS as f32;
            let t1 = T_RANGE.0 + (T_RANGE.1 - T_RANGE.0) * (i + 1) as f32 / STEPS as f32;
            let p0 = P_MAX * j as f32 / STEPS as f32;
            let p1 = P_MAX * (j + 1) as f32 / STEPS as f32;
            let biome = worldgen_core::biome::classify((t0 + t1) / 2.0, (p0 + p1) / 2.0, 0.0);
            let [r, g, b] = render::biome_colour(biome);
            let cell = Rect::from_two_pos(to_screen(t0, p0), to_screen(t1, p1));
            painter.rect_filled(cell, 0.0, Color32::from_rgb(r, g, b).gamma_multiply(0.45));
        }
    }

    // A deterministic sample of land cells: every k-th one in index order.
    let land: Vec<usize> = (0..world.biomes.biome.len())
        .filter(|&i| !world.biomes.biome[i].is_water())
        .collect();
    let stride = (land.len() / SAMPLES).max(1);
    for &i in land.iter().step_by(stride) {
        let [r, g, b] = render::biome_colour(world.biomes.biome[i]);
        let pos = to_screen(world.climate.temperature[i], world.climate.precipitation[i]);
        painter.circle_filled(pos, 1.6_f32, Color32::from_rgb(r, g, b));
    }

    painter.rect_stroke(
        rect,
        0.0_f32,
        Stroke::new(1.0_f32, Color32::GRAY),
        egui::StrokeKind::Inside,
    );
    let small = egui::FontId::proportional(10.0);
    let text = ui.visuals().weak_text_color();
    painter.text(
        rect.left_bottom() + egui::vec2(2.0, -2.0),
        egui::Align2::LEFT_BOTTOM,
        "-20 °C",
        small.clone(),
        text,
    );
    painter.text(
        rect.right_bottom() + egui::vec2(-2.0, -2.0),
        egui::Align2::RIGHT_BOTTOM,
        "32 °C",
        small.clone(),
        text,
    );
    painter.text(
        rect.left_top() + egui::vec2(2.0, 2.0),
        egui::Align2::LEFT_TOP,
        "4000 mm",
        small,
        text,
    );
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// A small label in the top-left corner of the map.
fn badge(painter: &egui::Painter, rect: Rect, text: &str) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(14.0),
        Color32::WHITE,
    );
    let pos = rect.min + egui::vec2(10.0, 10.0);
    let bg = Rect::from_min_size(pos, galley.size()).expand(6.0);
    painter.rect_filled(bg, 4.0_f32, Color32::from_black_alpha(170));
    painter.galley(pos, galley, Color32::WHITE);
}

fn legend_row(ui: &mut egui::Ui, colour: Rgb, label: &str) {
    ui.horizontal(|ui| {
        let (swatch, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().rect_filled(
            swatch,
            2.0_f32,
            Color32::from_rgb(colour[0], colour[1], colour[2]),
        );
        ui.painter().rect_stroke(
            swatch,
            2.0_f32,
            Stroke::new(1.0_f32, Color32::GRAY),
            egui::StrokeKind::Inside,
        );
        ui.label(label);
    });
}

impl eframe::App for WorldGenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(config) = self.setup.show(ctx) {
            self.seed_text = config.seed.to_string();
            self.seed_error = None;
            self.notice = None;
            self.config = config;
            self.apply_config();
        }
        // Keep the web address bar pointing at this world, but not on every
        // frame of a slider drag.
        let dragging = ctx.input(|i| i.pointer.any_down());
        if !dragging && self.linked_config.as_ref() != Some(&self.config) {
            link::show_in_address_bar(&self.config);
            self.linked_config = Some(self.config.clone());
        }
        self.advance_pipeline(ctx);
        self.side_panel(ctx);
        self.central_panel(ctx);
    }
}
