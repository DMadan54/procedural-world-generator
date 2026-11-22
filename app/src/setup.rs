//! The world setup window: edits a draft of the full `WorldConfig` and only
//! applies it when the user presses Generate.

use eframe::egui;
use worldgen_core::config::{
    Disaster, MagicLevel, MAX_HISTORY_YEARS, MAX_REALMS, MIN_HISTORY_YEARS,
};
use worldgen_core::share::from_partial_json;
use worldgen_core::{
    WorldConfig, MAX_DROPLETS, MAX_PLATES, MAX_RIVER_THRESHOLD, MAX_TEMPERATURE_OFFSET, MIN_PLATES,
    MIN_RIVER_THRESHOLD, MIN_TEMPERATURE_OFFSET,
};

/// Saved configs shipped with the app. Each names only the settings it
/// changes; loading one keeps the current seed and world size.
pub const PRESETS: [(&str, &str); 4] = [
    (
        "Classic High Fantasy",
        include_str!("../presets/classic_high_fantasy.json"),
    ),
    (
        "Grounded Historical",
        include_str!("../presets/grounded_historical.json"),
    ),
    ("Mythic Age", include_str!("../presets/mythic_age.json")),
    ("Archipelago", include_str!("../presets/archipelago.json")),
];

/// Loads a preset on top of the defaults, keeping `current`'s seed and size.
pub fn load_preset(json: &str, current: &WorldConfig) -> Result<WorldConfig, String> {
    let mut config = from_partial_json(json).map_err(|e| e.to_string())?;
    config.seed = current.seed;
    config.width = current.width;
    config.height = current.height;
    Ok(config)
}

/// Overall land layout. Choosing one sets the underlying plate, coastline
/// and sea-level settings; it is not stored separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Continents,
    Archipelago,
    Pangaea,
    Islands,
}

impl Layout {
    pub const ALL: [Layout; 4] = [
        Layout::Continents,
        Layout::Archipelago,
        Layout::Pangaea,
        Layout::Islands,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Continents => "Continents",
            Layout::Archipelago => "Archipelago",
            Layout::Pangaea => "Pangaea",
            Layout::Islands => "Scattered islands",
        }
    }

    /// (plates, continental fraction, clustering, noise weight, sea level)
    fn settings(self) -> (usize, f32, f32, f32, f32) {
        match self {
            Layout::Continents => (16, 0.4, 0.0, 0.9, 0.45),
            Layout::Archipelago => (30, 0.25, 0.0, 1.2, 0.48),
            Layout::Pangaea => (12, 0.45, 1.0, 0.9, 0.45),
            Layout::Islands => (38, 0.12, 0.0, 1.4, 0.5),
        }
    }

    pub fn apply(self, c: &mut WorldConfig) {
        let (plates, fraction, clustering, noise, sea) = self.settings();
        c.tectonics.plate_count = plates;
        c.tectonics.continental_fraction = fraction;
        c.tectonics.continent_clustering = clustering;
        c.blend.noise_weight = noise;
        c.terrain.sea_level = sea;
    }

    /// The layout whose settings `c` currently has, if any.
    pub fn detect(c: &WorldConfig) -> Option<Layout> {
        Layout::ALL.into_iter().find(|l| {
            let (plates, fraction, clustering, noise, sea) = l.settings();
            c.tectonics.plate_count == plates
                && c.tectonics.continental_fraction == fraction
                && c.tectonics.continent_clustering == clustering
                && c.blend.noise_weight == noise
                && c.terrain.sea_level == sea
        })
    }
}

/// Which latitudes the map spans: (name, north edge, south edge).
pub const CLIMATE_BANDS: [(&str, f32, f32); 6] = [
    ("Whole globe", 90.0, -90.0),
    ("Northern hemisphere", 90.0, 0.0),
    ("Southern hemisphere", 0.0, -90.0),
    ("Temperate band", 60.0, 30.0),
    ("Tropics", 25.0, -25.0),
    ("Polar north", 90.0, 55.0),
];

pub const WORLD_SIZES: [(&str, usize); 3] = [
    ("Small (256²)", 256),
    ("Medium (512²)", 512),
    ("Large (1024²)", 1024),
];

#[derive(Default)]
pub struct SetupPanel {
    pub open: bool,
    draft: WorldConfig,
    seed_text: String,
    error: Option<String>,
}

impl SetupPanel {
    /// Opens the window with a copy of `current` to edit.
    pub fn open_with(&mut self, current: &WorldConfig) {
        self.draft = current.clone();
        self.seed_text = current.seed.to_string();
        self.error = None;
        self.open = true;
    }

    /// Shows the window. Returns the new config when the user presses
    /// Generate.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<WorldConfig> {
        if !self.open {
            return None;
        }
        let mut result = None;
        let mut open = self.open;
        egui::Window::new("World setup")
            .open(&mut open)
            .default_width(360.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(ctx.screen_rect().height() * 0.75)
                    .show(ui, |ui| {
                        self.contents(ui);
                    });
                ui.separator();
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    if ui.button("Generate").clicked() {
                        match self.seed_text.trim().parse::<u64>() {
                            Ok(seed) => {
                                self.draft.seed = seed;
                                result = Some(self.draft.clone());
                                self.open = false;
                            }
                            Err(_) => self.error = Some("Seed must be a whole number.".into()),
                        }
                    }
                    if ui.button("Reset to defaults").clicked() {
                        let seed = self.draft.seed;
                        self.draft = WorldConfig {
                            seed,
                            ..Default::default()
                        };
                    }
                    if ui.button("Cancel").clicked() {
                        self.open = false;
                    }
                });
            });
        self.open &= open;
        result
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        ui.label("Presets");
        ui.horizontal_wrapped(|ui| {
            for (name, json) in PRESETS {
                if ui.button(name).clicked() {
                    match load_preset(json, &self.draft) {
                        Ok(config) => {
                            self.draft = config;
                            self.error = None;
                        }
                        Err(e) => self.error = Some(format!("Preset {name}: {e}")),
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Seed");
            ui.text_edit_singleline(&mut self.seed_text);
        });

        let c = &mut self.draft;
        section(ui, "Geography", true, |ui| {
            let current = Layout::detect(c);
            egui::ComboBox::from_label("Layout")
                .selected_text(current.map_or("Custom", Layout::name))
                .show_ui(ui, |ui| {
                    for layout in Layout::ALL {
                        if ui
                            .selectable_label(current == Some(layout), layout.name())
                            .clicked()
                        {
                            layout.apply(c);
                        }
                    }
                });
            ui.add(egui::Slider::new(&mut c.terrain.sea_level, 0.0..=1.0).text("Sea level"));
            ui.add(
                egui::Slider::new(&mut c.tectonics.mountain_scale, 0.25..=2.5)
                    .text("Mountain density"),
            );
            ui.add(
                egui::Slider::new(&mut c.tectonics.plate_count, MIN_PLATES..=MAX_PLATES)
                    .text("Tectonic plates"),
            );
            ui.add(
                egui::Slider::new(&mut c.tectonics.continental_fraction, 0.05..=0.8)
                    .text("Land share of plates"),
            );
            ui.add(
                egui::Slider::new(&mut c.tectonics.continent_clustering, 0.0..=1.0)
                    .text("Continent clustering"),
            );

            let band = CLIMATE_BANDS
                .iter()
                .find(|b| b.1 == c.climate.latitude_north && b.2 == c.climate.latitude_south);
            egui::ComboBox::from_label("Climate band")
                .selected_text(band.map_or("Custom", |b| b.0))
                .show_ui(ui, |ui| {
                    for (name, north, south) in CLIMATE_BANDS {
                        let on = band.is_some_and(|b| b.0 == name);
                        if ui.selectable_label(on, name).clicked() {
                            c.climate.latitude_north = north;
                            c.climate.latitude_south = south;
                        }
                    }
                });

            let size = WORLD_SIZES
                .iter()
                .find(|s| s.1 == c.width && s.1 == c.height);
            egui::ComboBox::from_label("World size")
                .selected_text(
                    size.map_or_else(|| format!("{}×{}", c.width, c.height), |s| s.0.to_owned()),
                )
                .show_ui(ui, |ui| {
                    for (name, px) in WORLD_SIZES {
                        if ui
                            .selectable_label(c.width == px && c.height == px, name)
                            .clicked()
                        {
                            c.width = px;
                            c.height = px;
                        }
                    }
                });
        });

        section(ui, "Climate", false, |ui| {
            ui.add(
                egui::Slider::new(
                    &mut c.climate.global_temperature_offset,
                    MIN_TEMPERATURE_OFFSET..=MAX_TEMPERATURE_OFFSET,
                )
                .suffix(" °C")
                .text("Global temperature"),
            );
        });

        section(ui, "Magic", false, |ui| {
            ui.horizontal(|ui| {
                for (level, name) in [
                    (MagicLevel::None, "None"),
                    (MagicLevel::Rare, "Rare"),
                    (MagicLevel::Common, "Common"),
                    (MagicLevel::Saturated, "Saturated"),
                ] {
                    ui.radio_value(&mut c.magic.level, level, name);
                }
            });
        });

        section(ui, "Politics", false, |ui| {
            ui.add(
                egui::Slider::new(&mut c.politics.target_realms, 1..=MAX_REALMS)
                    .text("Target realms"),
            );
            ui.add(
                egui::Slider::new(&mut c.politics.empire_formation, 0.0..=1.0)
                    .text("Empire formation"),
            );
            ui.add(
                egui::Slider::new(&mut c.politics.fragmentation_rate, 0.0..=1.0)
                    .text("Fragmentation rate"),
            );
        });

        section(ui, "Disasters", false, |ui| {
            let d = &mut c.disasters;
            for (name, disaster) in [
                ("Volcanoes", &mut d.volcanoes),
                ("Earthquakes", &mut d.earthquakes),
                ("Floods", &mut d.floods),
                ("Droughts", &mut d.droughts),
                ("Plagues", &mut d.plagues),
            ] {
                disaster_row(ui, name, disaster);
            }
        });

        section(ui, "History", false, |ui| {
            ui.add(
                egui::Slider::new(&mut c.history.years, MIN_HISTORY_YEARS..=MAX_HISTORY_YEARS)
                    .logarithmic(true)
                    .suffix(" years")
                    .text("History length"),
            );
        });

        section(ui, "Advanced", false, |ui| {
            ui.add(
                egui::Slider::new(&mut c.erosion.droplets, 0..=MAX_DROPLETS)
                    .logarithmic(true)
                    .text("Erosion droplets"),
            );
            ui.add(
                egui::Slider::new(
                    &mut c.hydrology.river_threshold,
                    MIN_RIVER_THRESHOLD..=MAX_RIVER_THRESHOLD,
                )
                .logarithmic(true)
                .text("River threshold"),
            );
        });
        ui.add_space(4.0);
        ui.weak("Magic, politics, disasters and history are saved with the world for stages still to come; they don't change the map yet.");
    }
}

fn section(ui: &mut egui::Ui, title: &str, open: bool, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(title)
        .default_open(open)
        .show(ui, body);
}

fn disaster_row(ui: &mut egui::Ui, name: &str, disaster: &mut Disaster) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut disaster.enabled, name);
        ui.add_enabled(
            disaster.enabled,
            egui::Slider::new(&mut disaster.per_century, 0.0..=10.0).suffix(" / century"),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_loads_and_keeps_seed_and_size() {
        let current = WorldConfig {
            seed: 77,
            width: 256,
            height: 256,
            ..Default::default()
        };
        for (name, json) in PRESETS {
            let config = load_preset(json, &current).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                (config.seed, config.width, config.height),
                (77, 256, 256),
                "{name}"
            );
            assert_ne!(config, current, "{name} should change something");
        }
    }

    #[test]
    fn presets_use_named_layouts() {
        let current = WorldConfig::default();
        let layout = |json| Layout::detect(&load_preset(json, &current).unwrap());
        assert_eq!(layout(PRESETS[0].1), Some(Layout::Continents));
        assert_eq!(layout(PRESETS[2].1), Some(Layout::Pangaea));
        assert_eq!(layout(PRESETS[3].1), Some(Layout::Archipelago));
    }

    #[test]
    fn layouts_round_trip_through_detect() {
        for layout in Layout::ALL {
            let mut c = WorldConfig::default();
            layout.apply(&mut c);
            assert_eq!(Layout::detect(&c), Some(layout));
        }
    }

    #[test]
    fn default_config_is_the_continents_layout() {
        assert_eq!(
            Layout::detect(&WorldConfig::default()),
            Some(Layout::Continents)
        );
    }
}
