//! Every generation parameter, in one serialisable struct.
//!
//! A `WorldConfig` fully determines a world: the same config always produces
//! the same output. Stages take `&WorldConfig` rather than loose arguments.

use serde::{Deserialize, Serialize};

/// Bump when a change makes existing configs produce different worlds (new
/// RNG derivation, changed algorithm or defaults), so saved configs can be
/// detected as coming from an older generator.
pub const GENERATOR_VERSION: u32 = 6;

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
    pub biomes: BiomeConfig,
    pub habitability: HabitabilityConfig,
    pub travel: TravelConfig,
    pub peoples: PeoplesConfig,
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
            biomes: BiomeConfig::default(),
            habitability: HabitabilityConfig::default(),
            travel: TravelConfig::default(),
            peoples: PeoplesConfig::default(),
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

/// Weights for how attractive land is to live on. Radii are in cells of a
/// 512-wide map and scale with world size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HabitabilityConfig {
    /// Bonus next to the biggest rivers (smaller rivers get less).
    pub river_weight: f32,
    pub river_radius: f32,
    /// Bonus along lake shores.
    pub lake_weight: f32,
    /// Bonus for sheltered coast (bays score highest).
    pub coast_weight: f32,
    pub coast_radius: f32,
    /// Penalty per unit of slope (normalised height per 512-map cell).
    pub slope_penalty: f32,
    /// Penalty per km of altitude.
    pub elevation_penalty: f32,
}

impl Default for HabitabilityConfig {
    fn default() -> Self {
        Self {
            river_weight: 0.35,
            river_radius: 4.0,
            lake_weight: 0.2,
            coast_weight: 0.25,
            coast_radius: 5.0,
            slope_penalty: 40.0,
            elevation_penalty: 0.15,
        }
    }
}

/// Travel cost of terrain, used wherever things move overland: peoples
/// spreading, realms claiming land, roads and armies. Costs are per cell
/// entered (1 is open lowland).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TravelConfig {
    /// Crossing open sea.
    pub sea_cost: f32,
    pub lake_cost: f32,
    /// Extra cost per km of altitude above 1500 m.
    pub mountain_cost: f32,
    /// Extra cost per unit of slope (normalised height per 512-map cell).
    pub slope_cost: f32,
    /// Extra cost in forest.
    pub forest_cost: f32,
    /// Extra cost in swamp and jungle.
    pub swamp_cost: f32,
    /// Extra cost in desert, tundra, ice and bare rock.
    pub barren_cost: f32,
    /// Multiplier along rivers, below 1 so rivers are corridors.
    pub river_factor: f32,
}

impl Default for TravelConfig {
    fn default() -> Self {
        Self {
            sea_cost: 12.0,
            lake_cost: 6.0,
            mountain_cost: 3.0,
            slope_cost: 60.0,
            forest_cost: 0.6,
            swamp_cost: 1.5,
            barren_cost: 2.0,
            river_factor: 0.5,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PeoplesConfig {
    /// How many peoples to place (fewer if the land runs out).
    pub count: u32,
    /// Minimum distance between origins, as a fraction of map width.
    pub min_origin_spacing: f32,
    /// Land below this habitability belongs to no people.
    pub min_habitability: f32,
    /// How far (in travel cost) a people can spread from its origin.
    pub max_expansion_cost: f32,
}

impl Default for PeoplesConfig {
    fn default() -> Self {
        Self {
            count: 8,
            min_origin_spacing: 0.12,
            min_habitability: 0.1,
            max_expansion_cost: 160.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BiomeConfig {
    /// Amplitude (°C) of the noise added to temperature before classifying,
    /// so biome borders come out ragged rather than following contours.
    pub edge_jitter_c: f32,
    /// Amplitude of the same noise on precipitation, as a fraction of it.
    pub edge_jitter_frac: f32,
    /// Flow accumulation (see hydrology) from which flat land is wetland,
    /// for a 512-wide map (scaled for other sizes).
    pub wetland_accumulation: f32,
    /// Steepest slope that can be wetland, in normalised height per cell of
    /// a 512-wide map (scaled for other sizes).
    pub wetland_max_slope: f32,
    /// Passes of the 3x3 majority filter that removes speckle.
    pub smoothing_passes: u32,
}

impl Default for BiomeConfig {
    fn default() -> Self {
        Self {
            edge_jitter_c: 2.5,
            edge_jitter_frac: 0.15,
            wetland_accumulation: 40.0,
            wetland_max_slope: 0.004,
            smoothing_passes: 2,
        }
    }
}
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
    /// Height of a blended value of 0 (before the soft limits).
    pub height_offset: f32,
    /// Height change per unit of blended value. Fixed rather than fitted to
    /// each world, so sea level means the same in every world.
    pub height_scale: f32,
}

impl Default for BlendConfig {
    fn default() -> Self {
        Self {
            tectonic_weight: 1.0,
            noise_weight: 0.9,
            height_offset: 0.437,
            height_scale: 0.258,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let mut config = WorldConfig {
            seed: 123,
            ..Default::default()
        };
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
