//! Every generation parameter, in one serialisable struct.
//!
//! A `WorldConfig` fully determines a world: the same config always produces
//! the same output. Stages take `&WorldConfig` rather than loose arguments.

use serde::{Deserialize, Serialize};

/// Bump when a change makes existing configs produce different worlds (new
/// RNG derivation, changed algorithm or defaults), so saved configs can be
/// detected as coming from an older generator.
pub const GENERATOR_VERSION: u32 = 3;

pub const MIN_PLATES: usize = 8;
pub const MAX_PLATES: usize = 40;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldConfig {
    /// Which generator version this config was made for.
    pub generator_version: u32,
    pub seed: u64,
    pub width: usize,
    pub height: usize,
    pub tectonics: TectonicConfig,
    pub noise: NoiseConfig,
    pub blend: BlendConfig,
    pub terrain: TerrainConfig,
    pub erosion: ErosionConfig,
    pub climate: ClimateConfig,
    pub hydrology: HydrologyConfig,
    pub magic: MagicConfig,
    pub politics: PoliticsConfig,
    pub disasters: DisasterConfig,
    pub history: HistoryConfig,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            generator_version: GENERATOR_VERSION,
            seed: 42,
            width: 512,
            height: 512,
            tectonics: TectonicConfig::default(),
            noise: NoiseConfig::default(),
            blend: BlendConfig::default(),
            terrain: TerrainConfig::default(),
            erosion: ErosionConfig::default(),
            climate: ClimateConfig::default(),
            hydrology: HydrologyConfig::default(),
            magic: MagicConfig::default(),
            politics: PoliticsConfig::default(),
            disasters: DisasterConfig::default(),
            history: HistoryConfig::default(),
        }
    }
}

// The sections below describe the world's story. No stage reads them yet;
// they are carried in configs, presets and share links so later stages
// (settlements, realms, history) can use them.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MagicLevel {
    None,
    Rare,
    #[default]
    Common,
    Saturated,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MagicConfig {
    pub level: MagicLevel,
}

pub const MAX_REALMS: u32 = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PoliticsConfig {
    /// Roughly how many independent realms the world should settle into.
    pub target_realms: u32,
    /// Tendency of realms to conquer and absorb neighbours, `0.0..=1.0`.
    pub empire_formation: f32,
    /// Tendency of large realms to break apart, `0.0..=1.0`.
    pub fragmentation_rate: f32,
}

impl Default for PoliticsConfig {
    fn default() -> Self {
        Self {
            target_realms: 10,
            empire_formation: 0.4,
            fragmentation_rate: 0.3,
        }
    }
}

/// One kind of disaster: whether it happens and how often.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Disaster {
    pub enabled: bool,
    /// Expected events per century across the world.
    pub per_century: f32,
}

impl Default for Disaster {
    fn default() -> Self {
        Self {
            enabled: true,
            per_century: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisasterConfig {
    pub volcanoes: Disaster,
    pub earthquakes: Disaster,
    pub floods: Disaster,
    pub droughts: Disaster,
    pub plagues: Disaster,
}

impl Default for DisasterConfig {
    fn default() -> Self {
        let every = |per_century| Disaster {
            enabled: true,
            per_century,
        };
        Self {
            volcanoes: every(0.5),
            earthquakes: every(1.0),
            floods: every(2.0),
            droughts: every(1.0),
            plagues: every(0.3),
        }
    }
}

pub const MIN_HISTORY_YEARS: u32 = 100;
pub const MAX_HISTORY_YEARS: u32 = 20_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    /// Years of history to simulate.
    pub years: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self { years: 2000 }
    }
}

/// How normalised heights map onto the real world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerrainConfig {
    /// Normalised height of the sea surface, `0.0..=1.0`.
    pub sea_level: f32,
    /// Height in metres of the highest point (normalised height 1.0).
    pub max_elevation_m: f32,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            sea_level: 0.45,
            max_elevation_m: 6000.0,
        }
    }
}

pub const MAX_DROPLETS: u32 = 1_000_000;
pub const MIN_RIVER_THRESHOLD: f32 = 5.0;
pub const MAX_RIVER_THRESHOLD: f32 = 500.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HydrologyConfig {
    /// Discharge at which a cell becomes a river, in units of one cell's
    /// yearly runoff at 1000 mm of precipitation. Kept in
    /// `MIN_RIVER_THRESHOLD..=MAX_RIVER_THRESHOLD` by the UI.
    pub river_threshold: f32,
    /// Smallest filled depression, in cells, recorded as a lake. Smaller ones
    /// are still filled for drainage.
    pub min_lake_cells: u32,
    /// Minimum drop between a filled cell and the one it drains to, so flat
    /// filled areas still drain (normalised height units).
    pub fill_epsilon: f32,
}

impl Default for HydrologyConfig {
    fn default() -> Self {
        Self {
            river_threshold: 60.0,
            min_lake_cells: 6,
            fill_epsilon: 1e-5,
        }
    }
}

/// Droplet-based hydraulic erosion. Heights are in normalised units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ErosionConfig {
    /// Number of droplets simulated, up to `MAX_DROPLETS`. 0 disables erosion.
    pub droplets: u32,
    /// Share of spare sediment capacity a droplet erodes per step.
    pub erosion_rate: f32,
    /// Share of excess sediment a droplet drops per step.
    pub deposition_rate: f32,
    /// Share of a droplet's water lost per step.
    pub evaporation: f32,
    /// How much a droplet keeps its direction rather than following the
    /// slope, `0.0..1.0`.
    pub inertia: f32,
    /// Sediment carried per unit of slope, speed and water.
    pub sediment_capacity: f32,
    /// Floor on capacity so droplets on gentle slopes still erode a little.
    pub min_sediment_capacity: f32,
    /// Acceleration down slopes.
    pub gravity: f32,
    /// Maximum steps per droplet.
    pub max_lifetime: u32,
    /// Radius in pixels over which a droplet erodes.
    pub brush_radius: u32,
}

impl Default for ErosionConfig {
    fn default() -> Self {
        Self {
            droplets: 100_000,
            erosion_rate: 0.3,
            deposition_rate: 0.3,
            evaporation: 0.01,
            inertia: 0.05,
            sediment_capacity: 4.0,
            min_sediment_capacity: 0.01,
            gravity: 4.0,
            max_lifetime: 30,
            brush_radius: 3,
        }
    }
}

pub const MIN_TEMPERATURE_OFFSET: f32 = -15.0;
pub const MAX_TEMPERATURE_OFFSET: f32 = 15.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClimateConfig {
    /// Shifts every temperature, in °C: negative towards an ice age,
    /// positive towards a hothouse. Kept in
    /// `MIN_TEMPERATURE_OFFSET..=MAX_TEMPERATURE_OFFSET` by the UI.
    pub global_temperature_offset: f32,
    /// Mean sea-level temperature at the equator, °C.
    pub equator_temperature: f32,
    /// Mean sea-level temperature at the poles, °C.
    pub pole_temperature: f32,
    /// Cooling with altitude, °C per km.
    pub lapse_rate: f32,
    /// Random large-scale temperature variation, ± °C.
    pub temperature_variability: f32,
    /// Share of the missing moisture an air parcel picks up per step over
    /// ocean (a fifth of this over land, from plants and lakes).
    pub evaporation: f32,
    /// Share of a parcel's moisture rained out per km it is forced upwards.
    pub orographic_rain: f32,
    /// Share of a parcel's moisture rained out per step regardless of terrain.
    pub base_rain: f32,
    /// Latitude (degrees, north positive) of the top row of the map.
    pub latitude_north: f32,
    /// Latitude of the bottom row. With the defaults the map spans the globe.
    pub latitude_south: f32,
}

impl Default for ClimateConfig {
    fn default() -> Self {
        Self {
            global_temperature_offset: 0.0,
            equator_temperature: 27.0,
            pole_temperature: -25.0,
            lapse_rate: 6.5,
            temperature_variability: 2.0,
            evaporation: 0.08,
            orographic_rain: 0.8,
            base_rain: 0.015,
            latitude_north: 90.0,
            latitude_south: -90.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TectonicConfig {
    /// Number of plates, in `MIN_PLATES..=MAX_PLATES`.
    pub plate_count: usize,
    /// Share of plates that are continental.
    pub continental_fraction: f32,
    /// How far plate edges are pushed around by noise, as a fraction of map
    /// width. Zero gives straight Voronoi edges.
    pub boundary_warp: f64,
    /// 0 scatters continental plates at random; 1 gathers them around one
    /// point into a supercontinent.
    pub continent_clustering: f32,
    /// Multiplies the uplift at converging plate boundaries: more and higher
    /// mountain ranges above 1, fewer and lower below.
    pub mountain_scale: f32,
}

impl Default for TectonicConfig {
    fn default() -> Self {
        Self {
            plate_count: 16,
            continental_fraction: 0.4,
            boundary_warp: 0.06,
            continent_clustering: 0.0,
            mountain_scale: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseConfig {
    /// Number of noise layers summed together.
    pub octaves: usize,
    /// Base frequency, in noise cycles per map width.
    pub frequency: f64,
    /// Frequency multiplier between octaves.
    pub lacunarity: f64,
    /// Amplitude multiplier between octaves.
    pub persistence: f64,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            octaves: 6,
            frequency: 4.0,
            lacunarity: 2.0,
            persistence: 0.5,
        }
    }
}

/// How much each input contributes to the final heightmap.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlendConfig {
    pub tectonic_weight: f32,
    pub noise_weight: f32,
}

impl Default for BlendConfig {
    fn default() -> Self {
        Self {
            tectonic_weight: 1.0,
            noise_weight: 0.9,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let mut config = WorldConfig::default();
        config.seed = 123;
        config.tectonics.plate_count = 30;
        let json = serde_json::to_string(&config).unwrap();
        let back: WorldConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, back);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let config: WorldConfig = serde_json::from_str(r#"{ "seed": 9 }"#).unwrap();
        assert_eq!(config.seed, 9);
        assert_eq!(config.tectonics, TectonicConfig::default());
        assert_eq!(config.generator_version, GENERATOR_VERSION);
    }

    #[test]
    fn default_plate_count_is_in_range() {
        let n = WorldConfig::default().tectonics.plate_count;
        assert!((MIN_PLATES..=MAX_PLATES).contains(&n));
    }
}
