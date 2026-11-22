//! The generation pipeline: runs each stage in order from one [`WorldConfig`].

use crate::climate::{generate_climate, Climate};
use crate::config::WorldConfig;
use crate::fractal::fbm_field;
use crate::heightmap::{blend_heightmap, Heightmap};
use crate::tectonics::{generate_tectonics, TectonicMap};

/// A generated world: the config it came from plus every stage's output.
#[derive(Clone, Debug, PartialEq)]
pub struct World {
    pub config: WorldConfig,
    /// Stage: tectonic plates and boundary relief.
    pub tectonics: TectonicMap,
    /// Stage: raw fractal noise.
    pub noise: Vec<f32>,
    /// Stage: tectonics and noise blended and normalised.
    pub heightmap: Heightmap,
    /// Stage: temperature, wind and precipitation.
    pub climate: Climate,
}

/// Which stages an [`World::update`] re-ran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StagesRun {
    pub tectonics: bool,
    pub noise: bool,
    pub heightmap: bool,
    pub climate: bool,
}

impl World {
    pub fn generate(config: &WorldConfig) -> Self {
        let tectonics = generate_tectonics(config);
        let noise = fbm_field(config);
        let heightmap = blend_heightmap(config, &tectonics, &noise);
        let climate = generate_climate(config, &heightmap);
        Self {
            config: config.clone(),
            tectonics,
            noise,
            heightmap,
            climate,
        }
    }

    /// Moves to a new config, re-running only the stages whose inputs changed.
    /// The result is always identical to `World::generate(config)`.
    pub fn update(&mut self, config: &WorldConfig) -> StagesRun {
        let old = &self.config;
        let shared = config.generator_version != old.generator_version
            || config.seed != old.seed
            || config.width != old.width
            || config.height != old.height;
        let tectonics = shared || config.tectonics != old.tectonics;
        let noise = shared || config.noise != old.noise;
        let heightmap = tectonics || noise || config.blend != old.blend;
        let climate = heightmap || config.terrain != old.terrain || config.climate != old.climate;
        let run = StagesRun {
            tectonics,
            noise,
            heightmap,
            climate,
        };

        if run.tectonics {
            self.tectonics = generate_tectonics(config);
        }
        if run.noise {
            self.noise = fbm_field(config);
        }
        if run.heightmap {
            self.heightmap = blend_heightmap(config, &self.tectonics, &self.noise);
        }
        if run.climate {
            self.climate = generate_climate(config, &self.heightmap);
        }
        self.config = config.clone();
        run
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small(seed: u64) -> WorldConfig {
        WorldConfig {
            seed,
            width: 80,
            height: 64,
            ..Default::default()
        }
    }

    fn only(f: impl FnOnce(&mut StagesRun)) -> StagesRun {
        let mut run = StagesRun::default();
        f(&mut run);
        run
    }

    #[test]
    fn same_config_same_world() {
        assert_eq!(World::generate(&small(42)), World::generate(&small(42)));
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(
            World::generate(&small(42)).heightmap,
            World::generate(&small(43)).heightmap
        );
    }

    #[test]
    fn heightmap_is_normalised() {
        let world = World::generate(&small(9));
        let data = &world.heightmap.data;
        assert!(data.iter().all(|v| (0.0..=1.0).contains(v)));
        assert_eq!(data.iter().cloned().fold(f32::INFINITY, f32::min), 0.0);
        assert_eq!(data.iter().cloned().fold(f32::NEG_INFINITY, f32::max), 1.0);
    }

    #[test]
    fn changing_plates_skips_noise() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.tectonics.plate_count = 30;
        let run = world.update(&config);
        assert_eq!(
            run,
            only(|r| {
                r.tectonics = true;
                r.heightmap = true;
                r.climate = true;
            })
        );
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_blend_reruns_heightmap_and_climate() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.blend.noise_weight = 0.2;
        let run = world.update(&config);
        assert_eq!(
            run,
            only(|r| {
                r.heightmap = true;
                r.climate = true;
            })
        );
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_sea_level_or_temperature_reruns_only_climate() {
        for change in [
            (|c: &mut WorldConfig| c.terrain.sea_level = 0.6) as fn(&mut WorldConfig),
            |c| c.climate.global_temperature_offset = -8.0,
        ] {
            let mut world = World::generate(&small(42));
            let mut config = small(42);
            change(&mut config);
            assert_eq!(world.update(&config), only(|r| r.climate = true));
            assert_eq!(world, World::generate(&config));
        }
    }

    #[test]
    fn climate_settings_never_change_the_terrain() {
        let base = World::generate(&small(42));
        let mut config = small(42);
        config.climate.global_temperature_offset = 12.0;
        config.climate.evaporation = 0.3;
        let changed = World::generate(&config);
        assert_eq!(base.tectonics, changed.tectonics);
        assert_eq!(base.heightmap, changed.heightmap);
    }

    #[test]
    fn unchanged_config_reruns_nothing() {
        let mut world = World::generate(&small(42));
        assert_eq!(world.update(&small(42)), StagesRun::default());
    }

    #[test]
    fn new_seed_matches_fresh_generation() {
        let mut world = World::generate(&small(42));
        world.update(&small(7));
        assert_eq!(world, World::generate(&small(7)));
    }
}
