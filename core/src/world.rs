//! The generation pipeline: runs each stage in order from one [`WorldConfig`].

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
}

/// Which stages an [`World::update`] re-ran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StagesRun {
    pub tectonics: bool,
    pub noise: bool,
    pub heightmap: bool,
}

impl World {
    pub fn generate(config: &WorldConfig) -> Self {
        let tectonics = generate_tectonics(config);
        let noise = fbm_field(config);
        let heightmap = blend_heightmap(config, &tectonics, &noise);
        Self {
            config: config.clone(),
            tectonics,
            noise,
            heightmap,
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
        let run = StagesRun {
            tectonics: shared || config.tectonics != old.tectonics,
            noise: shared || config.noise != old.noise,
            heightmap: false,
        };
        let run = StagesRun {
            heightmap: run.tectonics || run.noise || config.blend != old.blend,
            ..run
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
    fn changing_plates_reruns_only_tectonics_and_blend() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.tectonics.plate_count = 30;
        let run = world.update(&config);
        assert_eq!(
            run,
            StagesRun {
                tectonics: true,
                noise: false,
                heightmap: true
            }
        );
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_blend_reruns_only_blend() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.blend.noise_weight = 0.2;
        let run = world.update(&config);
        assert_eq!(
            run,
            StagesRun {
                tectonics: false,
                noise: false,
                heightmap: true
            }
        );
        assert_eq!(world, World::generate(&config));
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
