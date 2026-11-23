//! Stage: resources. Deposits of ore, timber, grain, salt and gems, placed
//! from geology and biome, and what each settlement produces from the
//! deposits and farmland around it.

use rand_chacha::rand_core::RngCore;
use serde::{Deserialize, Serialize};

use crate::biome::{Biome, Biomes};
use crate::climate::{elevation_m, Climate};
use crate::config::WorldConfig;
use crate::habitability::Habitability;
use crate::heightmap::Heightmap;
use crate::hydrology::{Hydrology, NO_LAKE, NO_SEGMENT};
use crate::rng::{stage, stage_rng, unit_f32, unit_f64};
use crate::settlements::Settlements;
use crate::tectonics::{BoundaryKind, TectonicMap, NO_BOUNDARY};
use crate::travel::spread;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Resource {
    Ore,
    Timber,
    Grain,
    Salt,
    Gems,
}

impl Resource {
    pub const COUNT: usize = 5;
    pub const ALL: [Resource; 5] = [
        Resource::Ore,
        Resource::Timber,
        Resource::Grain,
        Resource::Salt,
        Resource::Gems,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Resource::Ore => "ore",
            Resource::Timber => "timber",
            Resource::Grain => "grain",
            Resource::Salt => "salt",
            Resource::Gems => "gems",
        }
    }

    /// Yearly use per 1000 people.
    pub fn consumption_per_thousand(self) -> f32 {
        match self {
            Resource::Grain => 1.0,
            Resource::Timber => 0.3,
            Resource::Ore => 0.15,
            Resource::Salt => 0.1,
            Resource::Gems => 0.02,
        }
    }
}

/// Amounts of each resource, indexed by [`Resource::index`].
pub type Goods = [f32; Resource::COUNT];

#[derive(Clone, Debug, PartialEq)]
pub struct Deposit {
    pub cell: u32,
    pub resource: Resource,
    /// `0.0..=1.0`.
    pub richness: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resources {
    pub width: usize,
    pub height: usize,
    pub deposits: Vec<Deposit>,
    /// Per year-0 settlement: yearly output from nearby deposits.
    pub output: Vec<Goods>,
    /// Per year-0 settlement: grain grown per 1000 people (from the land).
    pub farming: Vec<f32>,
}

/// Runs the resources stage.
#[allow(clippy::too_many_arguments)]
pub fn generate_resources(
    config: &WorldConfig,
    tectonics: &TectonicMap,
    terrain: &Heightmap,
    climate: &Climate,
    hydrology: &Hydrology,
    biomes: &Biomes,
    habitability: &Habitability,
    settlements: &Settlements,
) -> Resources {
    let (w, h) = (terrain.width, terrain.height);
    let p = &config.resources;
    let reference = w as f32 / 512.0;

    // Distance (in cells) to the nearest converging plate boundary.
    let convergent: Vec<(usize, u16)> = (0..w * h)
        .filter(|&i| {
            let b = tectonics.boundary_id[i];
            b != NO_BOUNDARY && tectonics.boundaries[b as usize].kind == BoundaryKind::Convergent
        })
        .map(|i| (i, 0))
        .collect();
    let to_boundary = spread(&vec![1.0; w * h], w, h, &convergent, 60.0 * reference).cost;

    let input = Inputs {
        config,
        terrain,
        climate,
        hydrology,
        biomes,
        habitability,
        to_boundary: &to_boundary,
        reference,
    };
    let mut deposits = Vec::new();
    let rng = &mut stage_rng(config.seed, stage::RESOURCES);
    for resource in Resource::ALL {
        let scores: Vec<f32> = (0..w * h).map(|i| score(resource, i, &input)).collect();
        let target =
            (p.deposits_per_map[resource.index()] as f32 * reference * reference).round() as usize;
        let spacing = p.spacing[resource.index()] * reference;
        place(&mut deposits, resource, &scores, target, spacing, w, rng);
    }

    let catchment = p.catchment_radius * reference;
    let output = settlements
        .settlements
        .iter()
        .map(|s| deposit_output(s.cell, &deposits, w, catchment))
        .collect();
    let farming = settlements
        .settlements
        .iter()
        .map(|s| farming_yield(s.cell, &habitability.value, w, h, catchment))
        .collect();
    Resources {
        width: w,
        height: h,
        deposits,
        output,
        farming,
    }
}

struct Inputs<'a> {
    config: &'a WorldConfig,
    terrain: &'a Heightmap,
    climate: &'a Climate,
    hydrology: &'a Hydrology,
    biomes: &'a Biomes,
    habitability: &'a Habitability,
    to_boundary: &'a [f32],
    reference: f32,
}

fn near(i: usize, w: usize, h: usize, r: usize, test: impl Fn(usize) -> bool) -> bool {
    let (x, y) = (i % w, i / w);
    (y.saturating_sub(r)..(y + r + 1).min(h))
        .any(|ny| (x.saturating_sub(r)..(x + r + 1).min(w)).any(|nx| test(ny * w + nx)))
}

/// How suitable a cell is for a deposit of `resource`, 0 meaning impossible.
fn score(resource: Resource, i: usize, input: &Inputs) -> f32 {
    let (w, h) = (input.terrain.width, input.terrain.height);
    let biome = input.biomes.biome[i];
    if biome.is_water() {
        return 0.0;
    }
    let elevation = elevation_m(input.terrain.data[i], &input.config.terrain);
    let boundary = input.to_boundary[i] / input.reference;
    match resource {
        // Ore: folded and faulted rock near colliding plates, or high ground.
        Resource::Ore => {
            if elevation < 600.0 || (boundary > 40.0 && elevation < 1500.0) {
                return 0.0;
            }
            0.7 * (1.0 - boundary / 40.0).max(0.0) + 0.3 * (elevation / 3000.0).min(1.0)
        }
        // Gems: rarer, higher and closer to the collision zone.
        Resource::Gems => {
            if elevation < 1200.0 || boundary > 20.0 {
                return 0.0;
            }
            (1.0 - boundary / 20.0) * (elevation / 4000.0).min(1.0)
        }
        Resource::Timber => match biome {
            Biome::TemperateForest | Biome::TemperateRainforest => 0.9,
            Biome::BorealForest => 0.8,
            Biome::TropicalForest => 0.75,
            Biome::TropicalRainforest => 0.7,
            _ => 0.0,
        },
        Resource::Grain => {
            let fertile = matches!(
                biome,
                Biome::Grassland | Biome::TemperateForest | Biome::Savanna | Biome::Shrubland
            );
            let hab = input.habitability.value[i];
            if !fertile || hab < 0.5 {
                return 0.0;
            }
            let river = near(i, w, h, 3, |j| input.hydrology.segment_id[j] != NO_SEGMENT);
            hab * if river { 1.0 } else { 0.4 }
        }
        Resource::Salt => {
            let dry = input.climate.precipitation[i] < 600.0;
            let ocean_edge = near(i, w, h, 1, |j| input.biomes.biome[j] == Biome::Ocean);
            let arid_coast = dry
                && ocean_edge
                && matches!(biome, Biome::Desert | Biome::Shrubland | Biome::Savanna);
            let salt_marsh = ocean_edge && biome.is_wetland();
            // Salt flats ring lakes with no outlet in dry country.
            let flat = dry
                && near(i, w, h, 1, |j| {
                    let lake = input.hydrology.lake_id[j];
                    lake != NO_LAKE && input.hydrology.lakes[lake as usize].outlet.is_none()
                });
            if flat {
                1.0
            } else if arid_coast {
                0.8
            } else if salt_marsh {
                0.6
            } else {
                0.0
            }
        }
    }
}

/// Weighted sampling (by score squared) with a minimum spacing between
/// deposits of the same kind.
fn place(
    deposits: &mut Vec<Deposit>,
    resource: Resource,
    scores: &[f32],
    target: usize,
    spacing: f32,
    w: usize,
    rng: &mut impl RngCore,
) {
    let mut prefix = Vec::with_capacity(scores.len());
    let mut total = 0.0f64;
    for &s in scores {
        total += (s * s) as f64;
        prefix.push(total);
    }
    if total <= 0.0 {
        return;
    }
    let mut placed: Vec<u32> = Vec::new();
    for _ in 0..target * 60 {
        if placed.len() >= target {
            break;
        }
        let pick = unit_f64(rng) * total;
        let cell = prefix.partition_point(|&s| s <= pick).min(prefix.len() - 1);
        let clear = placed.iter().all(|&c| {
            let (dx, dy) = (
                (c as usize % w) as f32 - (cell % w) as f32,
                (c as usize / w) as f32 - (cell / w) as f32,
            );
            dx * dx + dy * dy >= spacing * spacing
        });
        if clear && scores[cell] > 0.0 {
            placed.push(cell as u32);
            deposits.push(Deposit {
                cell: cell as u32,
                resource,
                richness: (scores[cell] * (0.6 + 0.8 * unit_f32(rng))).min(1.0),
            });
        }
    }
}

/// Yearly output a settlement at `cell` draws from deposits within `radius`.
pub fn deposit_output(cell: usize, deposits: &[Deposit], w: usize, radius: f32) -> Goods {
    let mut goods = [0.0; Resource::COUNT];
    let (x, y) = ((cell % w) as f32, (cell / w) as f32);
    for d in deposits {
        let (dx, dy) = (
            (d.cell as usize % w) as f32 - x,
            (d.cell as usize / w) as f32 - y,
        );
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < radius {
            goods[d.resource.index()] += 10.0 * d.richness * (1.0 - dist / radius);
        }
    }
    goods
}

/// Grain grown per 1000 people: better land feeds more than its own.
pub fn farming_yield(cell: usize, habitability: &[f32], w: usize, h: usize, radius: f32) -> f32 {
    let r = radius.round().max(1.0) as usize;
    let (x, y) = (cell % w, cell / w);
    let (mut sum, mut n) = (0.0, 0.0);
    for ny in y.saturating_sub(r)..(y + r + 1).min(h) {
        for nx in x.saturating_sub(r)..(x + r + 1).min(w) {
            sum += habitability[ny * w + nx];
            n += 1.0;
        }
    }
    // Mean habitability 0.5 feeds itself exactly; richer land has surplus.
    2.0 * sum / n
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
        config.history.years = 100;
        config
    }

    #[test]
    fn deterministic() {
        assert_eq!(
            World::generate(&small(5)).resources,
            World::generate(&small(5)).resources
        );
    }

    #[test]
    fn no_resource_in_an_impossible_place() {
        for seed in [1, 42] {
            let world = World::generate(&small(seed));
            let w = world.resources.width;
            assert!(!world.resources.deposits.is_empty());
            for d in &world.resources.deposits {
                let i = d.cell as usize;
                let biome = world.biomes.biome[i];
                assert!(!biome.is_water(), "{:?} on water", d.resource);
                match d.resource {
                    Resource::Timber => assert!(biome.is_forest(), "timber in {biome:?}"),
                    Resource::Grain => assert!(
                        !matches!(
                            biome,
                            Biome::Desert | Biome::Ice | Biome::Alpine | Biome::Tundra
                        ),
                        "grain in {biome:?}"
                    ),
                    Resource::Ore | Resource::Gems => {
                        let e = elevation_m(world.terrain.data[i], &world.config.terrain);
                        assert!(e >= 600.0, "{:?} at {e} m", d.resource);
                    }
                    Resource::Salt => {
                        let h = world.resources.height;
                        let ocean = near(i, w, h, 1, |j| world.biomes.biome[j] == Biome::Ocean);
                        let lake = near(i, w, h, 1, |j| world.hydrology.lake_id[j] != NO_LAKE);
                        assert!(ocean || lake, "salt away from any water");
                    }
                }
                assert!((0.0..=1.0).contains(&d.richness));
            }
        }
    }

    #[test]
    fn every_settlement_has_an_output_row() {
        let world = World::generate(&small(42));
        assert_eq!(
            world.resources.output.len(),
            world.settlements.settlements.len()
        );
        assert_eq!(
            world.resources.farming.len(),
            world.settlements.settlements.len()
        );
        assert!(world
            .resources
            .farming
            .iter()
            .all(|f| f.is_finite() && *f >= 0.0));
    }

    #[test]
    fn resource_settings_leave_earlier_stages_alone() {
        let base = World::generate(&small(42));
        let mut config = small(42);
        config.resources.deposits_per_map = [5, 5, 5, 5, 5];
        let changed = World::generate(&config);
        assert_eq!(base.settlements, changed.settlements);
        assert_eq!(base.biomes, changed.biomes);
        assert_ne!(base.resources, changed.resources);
    }
}
