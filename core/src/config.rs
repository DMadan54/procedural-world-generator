//! Every generation parameter, in one serialisable struct.
//!
//! A `WorldConfig` fully determines a world: the same config always produces
//! the same output. Stages take `&WorldConfig` rather than loose arguments.

use serde::{Deserialize, Serialize};

/// Bump when a change makes existing configs produce different worlds (new
/// RNG derivation, changed algorithm or defaults), so saved configs can be
/// detected as coming from an older generator.
pub const GENERATOR_VERSION: u32 = 2;

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
        }
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
}

impl Default for TectonicConfig {
    fn default() -> Self {
        Self {
            plate_count: 16,
            continental_fraction: 0.4,
            boundary_warp: 0.06,
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
