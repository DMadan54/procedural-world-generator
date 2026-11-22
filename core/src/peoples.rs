//! Stage: peoples. The cultures that first settle the world.
//!
//! Origins are drawn from habitable land (weighted towards the best of it,
//! kept apart from each other). Each people then spreads over the travel-cost
//! grid; the cheapest claim wins, so homelands end at mountains, deserts and
//! seas. Every people gets traits from its homeland and its own naming
//! language.

use crate::biome::{Biome, Biomes};
use crate::climate::elevation_m;
use crate::config::WorldConfig;
use crate::habitability::Habitability;
use crate::heightmap::Heightmap;
use crate::hydrology::{Hydrology, NO_SEGMENT};
use crate::names::Language;
use crate::rng::{entity_rng, stage, stage_rng, unit_f32, unit_f64};
use crate::travel::{spread, UNOWNED};

/// Marks a cell outside every homeland.
pub const NO_PEOPLE: u16 = UNOWNED;

/// What a people's homeland made of it. Later stages read the numeric
/// tendencies; the trait is mostly for flavour and naming.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PeopleTrait {
    RiverFolk,
    HillClans,
    SteppeRiders,
    SeaRaiders,
    ForestDwellers,
    DesertNomads,
    MarshFolk,
}

impl PeopleTrait {
    pub fn name(self) -> &'static str {
        match self {
            PeopleTrait::RiverFolk => "river folk",
            PeopleTrait::HillClans => "hill clans",
            PeopleTrait::SteppeRiders => "steppe riders",
            PeopleTrait::SeaRaiders => "sea raiders",
            PeopleTrait::ForestDwellers => "forest dwellers",
            PeopleTrait::DesertNomads => "desert nomads",
            PeopleTrait::MarshFolk => "marsh folk",
        }
    }

    /// (expansionism, seafaring, insularity) before per-people variation.
    fn tendencies(self) -> (f32, f32, f32) {
        match self {
            PeopleTrait::SteppeRiders => (0.8, 0.1, 0.2),
            PeopleTrait::SeaRaiders => (0.6, 0.9, 0.3),
            PeopleTrait::DesertNomads => (0.5, 0.15, 0.5),
            PeopleTrait::RiverFolk => (0.5, 0.4, 0.3),
            PeopleTrait::HillClans => (0.4, 0.1, 0.7),
            PeopleTrait::ForestDwellers => (0.3, 0.15, 0.6),
            PeopleTrait::MarshFolk => (0.25, 0.3, 0.6),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct People {
    pub id: u16,
    /// What they call themselves.
    pub name: String,
    /// Cell index the people spread from.
    pub origin: usize,
    pub trait_: PeopleTrait,
    /// Appetite for new land, `0.0..=1.0`.
    pub expansionism: f32,
    /// Ease on the sea, `0.0..=1.0`.
    pub seafaring: f32,
    /// Wariness of outsiders, `0.0..=1.0`.
    pub insularity: f32,
    pub language: Language,
    /// Homeland size in cells.
    pub cells: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peoples {
    pub width: usize,
    pub height: usize,
    pub peoples: Vec<People>,
    /// Per cell: the people whose homeland it is, or [`NO_PEOPLE`].
    pub homeland: Vec<u16>,
}

impl Peoples {
    pub fn people_at(&self, x: usize, y: usize) -> Option<&People> {
        self.peoples.get(self.homeland[y * self.width + x] as usize)
    }
}

/// Runs the peoples stage.
pub fn generate_peoples(
    config: &WorldConfig,
    terrain: &Heightmap,
    hydrology: &Hydrology,
    biomes: &Biomes,
    habitability: &Habitability,
    travel_cost: &[f32],
) -> Peoples {
    let (w, h) = (terrain.width, terrain.height);
    let p = &config.peoples;
    let origins = choose_origins(config, habitability);

    let sources: Vec<(usize, u16)> = origins
        .iter()
        .enumerate()
        .map(|(id, &cell)| (cell, id as u16))
        .collect();
    let reach = spread(travel_cost, w, h, &sources, p.max_expansion_cost);
    // Nobody makes a home on water or land nobody could live on.
    let homeland: Vec<u16> = (0..w * h)
        .map(|i| {
            if biomes.biome[i].is_water() || habitability.value[i] < p.min_habitability {
                NO_PEOPLE
            } else {
                reach.owner[i]
            }
        })
        .collect();

    let peoples = origins
        .iter()
        .enumerate()
        .map(|(id, &origin)| {
            let id = id as u16;
            let stats =
                HomelandStats::measure(id, &homeland, w, h, config, terrain, hydrology, biomes);
            let trait_ = stats.trait_();
            let (expansionism, seafaring, insularity) = trait_.tendencies();
            let rng = &mut entity_rng(config.seed, stage::PEOPLE_TRAITS, id as u64);
            let mut vary = |v: f32| (v + (unit_f32(rng) - 0.5) * 0.3).clamp(0.0, 1.0);
            let language = Language::generate(config.seed, id as u32);
            People {
                id,
                name: language.people_name(),
                origin,
                trait_,
                expansionism: vary(expansionism),
                seafaring: vary(seafaring),
                insularity: vary(insularity),
                language,
                cells: stats.cells,
            }
        })
        .collect();

    Peoples {
        width: w,
        height: h,
        peoples,
        homeland,
    }
}

/// Origins by weighted sampling of habitability (squared, so the best land is
/// strongly preferred), at least `min_origin_spacing` of the map width apart.
fn choose_origins(config: &WorldConfig, habitability: &Habitability) -> Vec<usize> {
    let p = &config.peoples;
    let w = habitability.width;
    let rng = &mut stage_rng(config.seed, stage::PEOPLES);
    let mut prefix = Vec::with_capacity(habitability.value.len());
    let mut total = 0.0f64;
    for &v in &habitability.value {
        total += (v * v) as f64;
        prefix.push(total);
    }
    let mut origins: Vec<usize> = Vec::new();
    if total <= 0.0 {
        return origins;
    }
    let spacing = p.min_origin_spacing * w as f32;
    for _ in 0..p.count * 200 {
        if origins.len() >= p.count as usize {
            break;
        }
        let target = unit_f64(rng) * total;
        let cell = prefix
            .partition_point(|&s| s <= target)
            .min(prefix.len() - 1);
        let far_enough = origins.iter().all(|&o| {
            let (dx, dy) = (
                (o % w) as f32 - (cell % w) as f32,
                (o / w) as f32 - (cell / w) as f32,
            );
            (dx * dx + dy * dy).sqrt() >= spacing
        });
        if far_enough && habitability.value[cell] >= p.min_habitability {
            origins.push(cell);
        }
    }
    origins
}

/// What a homeland is like, for choosing a people's trait.
struct HomelandStats {
    cells: u32,
    coast: f32,
    mean_elevation_m: f32,
    wetland: f32,
    desert: f32,
    open: f32,
    forest: f32,
    river: f32,
}

impl HomelandStats {
    #[allow(clippy::too_many_arguments)]
    fn measure(
        id: u16,
        homeland: &[u16],
        w: usize,
        h: usize,
        config: &WorldConfig,
        terrain: &Heightmap,
        hydrology: &Hydrology,
        biomes: &Biomes,
    ) -> Self {
        let mut s = HomelandStats {
            cells: 0,
            coast: 0.0,
            mean_elevation_m: 0.0,
            wetland: 0.0,
            desert: 0.0,
            open: 0.0,
            forest: 0.0,
            river: 0.0,
        };
        for i in (0..w * h).filter(|&i| homeland[i] == id) {
            s.cells += 1;
            let (x, y) = (i % w, i / w);
            let coastal = (y.saturating_sub(1)..(y + 2).min(h)).any(|ny| {
                (x.saturating_sub(1)..(x + 2).min(w))
                    .any(|nx| biomes.biome[ny * w + nx] == Biome::Ocean)
            });
            s.coast += coastal as u32 as f32;
            s.mean_elevation_m += elevation_m(terrain.data[i], &config.terrain);
            s.river += (hydrology.segment_id[i] != NO_SEGMENT) as u32 as f32;
            match biomes.biome[i] {
                Biome::Wetland | Biome::Mangrove => s.wetland += 1.0,
                Biome::Desert => s.desert += 1.0,
                Biome::Grassland | Biome::Savanna | Biome::Shrubland => s.open += 1.0,
                b if b.is_forest() => s.forest += 1.0,
                _ => {}
            }
        }
        let n = s.cells.max(1) as f32;
        for v in [
            &mut s.coast,
            &mut s.mean_elevation_m,
            &mut s.wetland,
            &mut s.desert,
            &mut s.open,
            &mut s.forest,
            &mut s.river,
        ] {
            *v /= n;
        }
        s
    }

    /// The first matching rule wins, from the most distinctive homelands to
    /// the most ordinary.
    fn trait_(&self) -> PeopleTrait {
        if self.wetland > 0.12 {
            PeopleTrait::MarshFolk
        } else if self.mean_elevation_m > 1100.0 {
            PeopleTrait::HillClans
        } else if self.desert > 0.3 {
            PeopleTrait::DesertNomads
        } else if self.coast > 0.18 {
            PeopleTrait::SeaRaiders
        } else if self.river > 0.12 {
            PeopleTrait::RiverFolk
        } else if self.open > 0.4 {
            PeopleTrait::SteppeRiders
        } else if self.forest > 0.5 {
            PeopleTrait::ForestDwellers
        } else {
            PeopleTrait::RiverFolk
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    fn small(seed: u64) -> WorldConfig {
        let mut config = WorldConfig {
            seed,
            width: 128,
            height: 128,
            ..Default::default()
        };
        config.erosion.droplets = 4000;
        config
    }

    #[test]
    fn deterministic() {
        assert_eq!(
            World::generate(&small(5)).peoples,
            World::generate(&small(5)).peoples
        );
    }

    #[test]
    fn origins_are_spaced_habitable_and_in_their_homeland() {
        let world = World::generate(&small(42));
        let p = &world.peoples;
        assert!(!p.peoples.is_empty());
        let w = p.width;
        let spacing = world.config.peoples.min_origin_spacing * w as f32;
        for (i, a) in p.peoples.iter().enumerate() {
            assert_eq!(
                p.homeland[a.origin], a.id,
                "people {} owns its origin",
                a.id
            );
            assert!(world.habitability.value[a.origin] >= world.config.peoples.min_habitability);
            for b in &p.peoples[i + 1..] {
                let (dx, dy) = (
                    (a.origin % w) as f32 - (b.origin % w) as f32,
                    (a.origin / w) as f32 - (b.origin / w) as f32,
                );
                assert!((dx * dx + dy * dy).sqrt() >= spacing);
            }
        }
    }

    #[test]
    fn homelands_never_cover_water() {
        let world = World::generate(&small(42));
        for (i, &owner) in world.peoples.homeland.iter().enumerate() {
            if world.biomes.biome[i].is_water() {
                assert_eq!(owner, NO_PEOPLE);
            }
        }
    }

    #[test]
    fn each_people_has_a_name_and_tendencies_in_range() {
        let world = World::generate(&small(42));
        for people in &world.peoples.peoples {
            assert!(
                crate::names::is_pronounceable(&people.name),
                "{}",
                people.name
            );
            for v in [people.expansionism, people.seafaring, people.insularity] {
                assert!((0.0..=1.0).contains(&v));
            }
            assert!(people.cells > 0);
        }
    }

    #[test]
    fn people_settings_leave_earlier_stages_alone() {
        let base = World::generate(&small(42));
        let mut config = small(42);
        config.peoples.count = 3;
        config.peoples.min_origin_spacing = 0.3;
        let changed = World::generate(&config);
        assert_eq!(base.biomes, changed.biomes);
        assert_eq!(base.habitability, changed.habitability);
        assert_ne!(base.peoples, changed.peoples);
    }
}
