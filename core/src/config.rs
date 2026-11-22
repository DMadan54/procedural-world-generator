//! Every generation parameter, in one serialisable struct.
//!
//! A `WorldConfig` fully determines a world: the same config always produces
//! the same output. Stages take `&WorldConfig` rather than loose arguments.

use serde::{Deserialize, Serialize};

/// Bump when a change makes existing configs produce different worlds (new
/// RNG derivation, changed algorithm or defaults), so saved configs can be
/// detected as coming from an older generator.
pub const GENERATOR_VERSION: u32 = 1;

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
