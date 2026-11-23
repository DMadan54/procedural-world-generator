//! The generation pipeline: runs each stage in order from one [`WorldConfig`].
//!
//! Stages: tectonics and noise (independent) → blended heightmap → erosion →
//! climate → hydrology → biomes → habitability and travel cost → peoples →
//! settlements → realms. Erosion can take seconds, so besides the blocking
//! [`World::generate`] and [`World::update`] there is an incremental path:
//! [`World::set_config`] runs the cheap stages straight away and queues
//! erosion and everything after it, and [`World::advance`] does the queued
//! work a little at a time. Both paths give identical worlds.

use crate::biome::{generate_biomes, Biomes};
use crate::climate::{generate_climate, Climate};
use crate::config::WorldConfig;
use crate::erosion::{erode, ErosionJob};
use crate::fractal::fbm_field;
use crate::habitability::{generate_habitability, Habitability};
use crate::heightmap::{blend_heightmap, Heightmap};
use crate::hydrology::{generate_hydrology, Hydrology};
use crate::peoples::{generate_peoples, Peoples};
use crate::realms::{generate_realms, Realms};
use crate::settlements::{generate_settlements, SettlementInputs, Settlements};
use crate::tectonics::{generate_tectonics, TectonicMap};
use crate::travel::travel_cost_grid;

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
    /// Stage: biome per cell.
    pub biomes: Biomes,
    /// Stage: how good each cell is to live on.
    pub habitability: Habitability,
    /// Stage: cost of entering each cell overland (see `travel`).
    pub travel_cost: Vec<f32>,
    /// Stage: peoples, their homelands and languages.
    pub peoples: Peoples,
    /// Stage: villages, towns, cities and capitals.
    pub settlements: Settlements,
    /// Stage: the political map at year 0.
    pub realms: Realms,
    /// Work queued by `set_config` and not yet done by `advance`.
    pending: Pending,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Pending {
    erosion: Option<ErosionJob>,
    climate: bool,
    hydrology: bool,
    biomes: bool,
    habitability: bool,
    travel: bool,
    peoples: bool,
    settlements: bool,
    realms: bool,
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
    pub biomes: bool,
    pub habitability: bool,
    pub travel: bool,
    pub peoples: bool,
    pub settlements: bool,
    pub realms: bool,
}

impl World {
    pub fn generate(config: &WorldConfig) -> Self {
        let tectonics = generate_tectonics(config);
        let noise = fbm_field(config);
        let heightmap = blend_heightmap(config, &tectonics, &noise);
        let terrain = erode(config, &heightmap);
        let climate = generate_climate(config, &terrain);
        let hydrology = generate_hydrology(config, &terrain, &climate);
        let biomes = generate_biomes(config, &terrain, &climate, &hydrology);
        let habitability = generate_habitability(config, &terrain, &hydrology, &biomes);
        let travel_cost = travel_cost_grid(config, &terrain, &hydrology, &biomes);
        let peoples = generate_peoples(
            config,
            &terrain,
            &hydrology,
            &biomes,
            &habitability,
            &travel_cost,
        );
        let settlements = generate_settlements(&SettlementInputs {
            config,
            terrain: &terrain,
            climate: &climate,
            hydrology: &hydrology,
            biomes: &biomes,
            habitability: &habitability,
            peoples: &peoples,
        });
        let realms = generate_realms(config, &biomes, &peoples, &settlements, &travel_cost);
        Self {
            config: config.clone(),
            tectonics,
            noise,
            heightmap,
            terrain,
            climate,
            hydrology,
            biomes,
            habitability,
            travel_cost,
            peoples,
            settlements,
            realms,
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
        // Biomes read the climate, terrain and lakes, so they follow hydrology.
        let biomes = hydrology || config.biomes != old.biomes;
        let habitability = biomes || config.habitability != old.habitability;
        let travel = biomes || config.travel != old.travel;
        let peoples = habitability || travel || config.peoples != old.peoples;
        // Settlements also read the realm target to decide how many capitals.
        let settlements = peoples
            || config.settlements != old.settlements
            || config.politics.target_realms != old.politics.target_realms;
        let realms = settlements || travel || config.realms != old.realms;
        let run = StagesRun {
            tectonics,
            noise,
            heightmap,
            erosion,
            climate,
            hydrology,
            biomes,
            habitability,
            travel,
            peoples,
            settlements,
            realms,
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
        self.pending.biomes |= biomes;
        self.pending.habitability |= habitability;
        self.pending.travel |= travel;
        self.pending.peoples |= peoples;
        self.pending.settlements |= settlements;
        self.pending.realms |= realms;
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
            self.pending.biomes = true;
        }
        if self.pending.biomes {
            self.biomes =
                generate_biomes(&self.config, &self.terrain, &self.climate, &self.hydrology);
            self.pending.biomes = false;
            self.pending.habitability = true;
            self.pending.travel = true;
        }
        if self.pending.habitability {
            self.habitability =
                generate_habitability(&self.config, &self.terrain, &self.hydrology, &self.biomes);
            self.pending.habitability = false;
            self.pending.peoples = true;
        }
        if self.pending.travel {
            self.travel_cost =
                travel_cost_grid(&self.config, &self.terrain, &self.hydrology, &self.biomes);
            self.pending.travel = false;
            self.pending.peoples = true;
        }
        if self.pending.peoples {
            self.peoples = generate_peoples(
                &self.config,
                &self.terrain,
                &self.hydrology,
                &self.biomes,
                &self.habitability,
                &self.travel_cost,
            );
            self.pending.peoples = false;
            self.pending.settlements = true;
        }
        if self.pending.settlements {
            self.settlements = generate_settlements(&SettlementInputs {
                config: &self.config,
                terrain: &self.terrain,
                climate: &self.climate,
                hydrology: &self.hydrology,
                biomes: &self.biomes,
                habitability: &self.habitability,
                peoples: &self.peoples,
            });
            self.pending.settlements = false;
            self.pending.realms = true;
        }
        if self.pending.realms {
            self.realms = generate_realms(
                &self.config,
                &self.biomes,
                &self.peoples,
                &self.settlements,
                &self.travel_cost,
            );
            self.pending.realms = false;
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

    /// The expected report: the stages `f` sets, plus every later stage that
    /// necessarily follows them.
    fn only(f: impl FnOnce(&mut StagesRun)) -> StagesRun {
        let mut run = StagesRun::default();
        f(&mut run);
        if run.biomes {
            run.habitability = true;
            run.travel = true;
        }
        if run.habitability || run.travel {
            run.peoples = true;
        }
        if run.peoples {
            run.settlements = true;
        }
        if run.settlements || run.travel {
            run.realms = true;
        }
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
    fn heights_are_in_range() {
        let world = World::generate(&small(9));
        assert!(world.heightmap.data.iter().all(|v| (0.0..1.0).contains(v)));
        assert!(world.terrain.data.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn mountain_density_does_not_drown_the_land() {
        // The bug this guards against: with heights stretched to each world's
        // own range, taller mountains pushed everything else under the sea.
        let land_share = |mountains: f32| {
            let mut c = small(42);
            c.erosion.droplets = 0;
            c.tectonics.mountain_scale = mountains;
            let w = World::generate(&c);
            let sea = c.terrain.sea_level;
            w.heightmap.data.iter().filter(|&&h| h >= sea).count() as f32
                / w.heightmap.data.len() as f32
        };
        let (low, high) = (land_share(1.0), land_share(2.0));
        assert!(high >= low * 0.85, "land {low} at 1.0 vs {high} at 2.0");
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
                r.biomes = true;
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
                r.biomes = true;
            })
        );
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_biome_settings_reruns_only_biomes() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.biomes.edge_jitter_c = 5.0;
        assert_eq!(world.set_config(&config), only(|r| r.biomes = true));
        assert!(world.is_ready());
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_people_settings_reruns_only_peoples() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.peoples.count = 4;
        assert_eq!(world.set_config(&config), only(|r| r.peoples = true));
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_settlement_settings_or_realm_target_reruns_only_settlements() {
        for change in [
            (|c: &mut WorldConfig| c.settlements.target_count = 60) as fn(&mut WorldConfig),
            |c| c.politics.target_realms = 4,
        ] {
            let mut world = World::generate(&small(42));
            let mut config = small(42);
            change(&mut config);
            assert_eq!(world.set_config(&config), only(|r| r.settlements = true));
            assert_eq!(world, World::generate(&config));
        }
    }

    #[test]
    fn changing_realm_settings_reruns_only_realms() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.realms.max_reach = 60.0;
        assert_eq!(world.set_config(&config), only(|r| r.realms = true));
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn story_settings_rerun_nothing() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.magic.level = crate::config::MagicLevel::Saturated;
        config.history.years = 9000;
        config.politics.empire_formation = 0.9;
        assert_eq!(world.set_config(&config), StagesRun::default());
    }

    #[test]
    fn changing_travel_costs_reruns_travel_and_peoples() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.travel.sea_cost = 30.0;
        assert_eq!(world.set_config(&config), only(|r| r.travel = true));
        assert_eq!(world, World::generate(&config));
    }

    #[test]
    fn changing_river_threshold_reruns_only_hydrology() {
        let mut world = World::generate(&small(42));
        let mut config = small(42);
        config.hydrology.river_threshold = 20.0;
        assert_eq!(
            world.set_config(&config),
            only(|r| {
                r.hydrology = true;
                r.biomes = true;
            })
        );
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
                    r.biomes = true;
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
