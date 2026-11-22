//! The generation pipeline: runs each stage in order from one [`WorldConfig`].
//!
//! Stages: tectonics and noise (independent) → blended heightmap → erosion →
//! climate → hydrology. Erosion can take seconds, so besides the blocking
//! [`World::generate`] and [`World::update`] there is an incremental path:
//! [`World::set_config`] runs the cheap stages straight away and queues
//! erosion and everything after it, and [`World::advance`] does the queued
//! work a little at a time. Both paths give identical worlds.

use crate::climate::{generate_climate, Climate};
use crate::config::WorldConfig;
use crate::erosion::{erode, ErosionJob};
use crate::fractal::fbm_field;
use crate::heightmap::{blend_heightmap, Heightmap};
use crate::hydrology::{generate_hydrology, Hydrology};
use crate::tectonics::{generate_tectonics, TectonicMap};

/// A generated world: the config it came from plus every stage's output.
#[derive(Clone, Debug, PartialEq)]
pub struct World {
    pub config: WorldConfig,
    /// Stage: tectonic plates and boundary relief.
    pub tectonics: TectonicMap,
    /// Stage: raw fractal noise.
    pub noise: Vec<f32>,
    /// Stage: tectonics and noise blended and normalised, before erosion.
    pub heightmap: Heightmap,
    /// Stage: the heightmap after hydraulic erosion. Later stages build on
    /// this.
    pub terrain: Heightmap,
    /// Stage: temperature, wind and precipitation.
    pub climate: Climate,
    /// Stage: lakes, drainage and river networks.
    pub hydrology: Hydrology,
    /// Work queued by `set_config` and not yet done by `advance`.
    pending: Pending,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Pending {
    erosion: Option<ErosionJob>,
    climate: bool,
    hydrology: bool,
}

/// Which stages a config change re-ran or queued.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StagesRun {
    pub tectonics: bool,
    pub noise: bool,
    pub heightmap: bool,
    pub erosion: bool,
    pub climate: bool,
    pub hydrology: bool,
}

impl World {
    pub fn generate(config: &WorldConfig) -> Self {
        let tectonics = generate_tectonics(config);
        let noise = fbm_field(config);
        let heightmap = blend_heightmap(config, &tectonics, &noise);
        let terrain = erode(config, &heightmap);
        let climate = generate_climate(config, &terrain);
        let hydrology = generate_hydrology(config, &terrain, &climate);
        Self {
            config: config.clone(),
            tectonics,
            noise,
            heightmap,
            terrain,
            climate,
            hydrology,
            pending: Pending::default(),
        }
    }

    /// Moves to a new config and finishes all the work. The result is always
    /// identical to `World::generate(config)`.
    pub fn update(&mut self, config: &WorldConfig) -> StagesRun {
        let run = self.set_config(config);
        self.finish();
        run
    }

    /// Moves to a new config: re-runs the cheap stages whose inputs changed
    /// now and queues erosion and later stages for [`World::advance`]. Safe to
    /// call again before the queued work is done; it restarts only what the
    /// new change affects.
    pub fn set_config(&mut self, config: &WorldConfig) -> StagesRun {
        let old = &self.config;
        let shared = config.generator_version != old.generator_version
            || config.seed != old.seed
            || config.width != old.width
            || config.height != old.height;
        let tectonics = shared || config.tectonics != old.tectonics;
        let noise = shared || config.noise != old.noise;
        let heightmap = tectonics || noise || config.blend != old.blend;
        let erosion = heightmap || config.erosion != old.erosion;
        let climate = erosion
            || self.pending.erosion.is_some()
            || config.terrain != old.terrain
            || config.climate != old.climate;
        let hydrology = climate || config.hydrology != old.hydrology;
        let run = StagesRun {
            tectonics,
            noise,
            heightmap,
            erosion,
            climate,
            hydrology,
        };

        if tectonics {
            self.tectonics = generate_tectonics(config);
        }
        if noise {
            self.noise = fbm_field(config);
        }
        if heightmap {
            self.heightmap = blend_heightmap(config, &self.tectonics, &self.noise);
        }
        if erosion {
            self.pending.erosion = Some(ErosionJob::new(config, &self.heightmap));
        }
        self.pending.climate |= climate;
        self.pending.hydrology |= hydrology;
        self.config = config.clone();
        // Without erosion to wait for, the rest is quick: do it now.
        if self.pending.erosion.is_none() {
            self.advance(0);
        }
        run
    }

    /// Does up to `max_erosion_rounds` rounds of queued erosion, then any
    /// later stages once erosion is done. Returns true when the world is
    /// fully up to date.
    pub fn advance(&mut self, max_erosion_rounds: usize) -> bool {
        if let Some(job) = &mut self.pending.erosion {
            job.step(max_erosion_rounds);
            if !job.is_done() {
                return false;
            }
            let job = self.pending.erosion.take().expect("checked above");
            self.terrain = job.finish();
            self.pending.climate = true;
        }
        if self.pending.climate {
            self.climate = generate_climate(&self.config, &self.terrain);
            self.pending.climate = false;
            self.pending.hydrology = true;
        }
        if self.pending.hydrology {
            self.hydrology = generate_hydrology(&self.config, &self.terrain, &self.climate);
            self.pending.hydrology = false;
        }
        true
    }

    /// Does all queued work.
    pub fn finish(&mut self) {
        while !self.advance(usize::MAX) {}
    }

    /// True when no work is queued.
    pub fn is_ready(&self) -> bool {
        self.pending == Pending::default()
    }

    /// Progress of the queued erosion run, if one is in progress.
    pub fn erosion_progress(&self) -> Option<f32> {
        self.pending.erosion.as_ref().map(ErosionJob::progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small(seed: u64) -> WorldConfig {
        let mut config = WorldConfig {
            seed,
            width: 80,
            height: 64,
            ..Default::default()
        };
        config.erosion.droplets = 6000;
        config
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
            World::generate(&small(42)).terrain,
            World::generate(&small(43)).terrain
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
    fn erosion_changes_terrain_but_not_heightmap() {
        let mut no_erosion = small(42);
        no_erosion.erosion.droplets = 0;
        let a = World::generate(&small(42));
        let b = World::generate(&no_erosion);
        assert_eq!(a.heightmap, b.heightmap);
        assert_ne!(a.terrain, b.terrain);
        assert_eq!(b.terrain, b.heightmap);
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
                r.erosion = true;
                r.climate = true;
                r.hydrology = true;
            })
        );
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_erosion_skips_earlier_stages() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.erosion.inertia = 0.3;
        let run = world.update(&config);
        assert_eq!(
            run,
            only(|r| {
                r.erosion = true;
                r.climate = true;
                r.hydrology = true;
            })
        );
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_river_threshold_reruns_only_hydrology() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.hydrology.river_threshold = 20.0;
        assert_eq!(world.set_config(&config), only(|r| r.hydrology = true));
        assert!(world.is_ready());
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_sea_level_or_temperature_skips_terrain() {
        for change in [
            (|c: &mut WorldConfig| c.terrain.sea_level = 0.6) as fn(&mut WorldConfig),
            |c| c.climate.global_temperature_offset = -8.0,
        ] {
            let mut world = World::generate(&small(42));
            let mut config = small(42);
            change(&mut config);
            assert_eq!(
                world.set_config(&config),
                only(|r| {
                    r.climate = true;
                    r.hydrology = true;
                })
            );
            assert!(world.is_ready(), "climate-only changes finish immediately");
            assert_eq!(world, World::generate(&config));
        }
    }

    #[test]
    fn later_settings_never_change_earlier_output() {
        let base = World::generate(&small(42));
        let mut config = small(42);
        config.climate.global_temperature_offset = 12.0;
        config.erosion.droplets = 100;
        let changed = World::generate(&config);
        assert_eq!(base.tectonics, changed.tectonics);
        assert_eq!(base.noise, changed.noise);
        assert_eq!(base.heightmap, changed.heightmap);
    }

    #[test]
    fn incremental_matches_blocking() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.erosion.droplets = 9000;
        world.set_config(&config);
        assert!(!world.is_ready());
        let mut last = world.erosion_progress().unwrap();
        while !world.advance(1) {
            let now = world.erosion_progress().unwrap();
            assert!(now > last);
            last = now;
        }
        assert!(world.is_ready());
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_config_mid_erosion_still_matches() {
        let mut world = World::generate(&small(42));
        let mut first = small(42);
        first.erosion.droplets = 9000;
        world.set_config(&first);
        world.advance(1);
        // Now a climate-only change: erosion must carry on, not be lost.
        let mut second = first.clone();
        second.climate.global_temperature_offset = 5.0;
        world.set_config(&second);
        assert!(!world.is_ready());
        world.finish();
        assert_eq!(world, World::generate(&second));

        // And an upstream change restarts erosion from the new heightmap.
        world.set_config(&first);
        world.advance(1);
        let mut third = first.clone();
        third.tectonics.plate_count = 25;
        world.set_config(&third);
        world.finish();
        assert_eq!(world, World::generate(&third));
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
