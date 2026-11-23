//! Stage: settlements. Villages, towns, cities and capitals.
//!
//! Sites are chosen greedily from the best land: repeatedly take the
//! highest-scoring cell far enough from every settlement so far, where "far
//! enough" shrinks on fertile land so rich valleys fill up densely. Each
//! settlement belongs to the people whose homeland it is in and is named in
//! their language. Ranks come from estimated population, with about
//! `politics.target_realms` capitals.

use crate::dmath;
use std::collections::BTreeSet;

use crate::biome::{Biome, Biomes};
use crate::climate::{elevation_m, Climate};
use crate::config::WorldConfig;
use crate::habitability::Habitability;
use crate::heightmap::Heightmap;
use crate::hydrology::{Hydrology, NO_LAKE, NO_SEGMENT};
use crate::peoples::{Peoples, NO_PEOPLE};
use crate::rng::{entity_rng, stage, unit_f32};

pub const NO_SETTLEMENT: u32 = u32::MAX;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Rank {
    Village,
    Town,
    City,
    Capital,
}

impl Rank {
    pub fn name(self) -> &'static str {
        match self {
            Rank::Village => "village",
            Rank::Town => "town",
            Rank::City => "city",
            Rank::Capital => "capital",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettlementFlags {
    /// On the sea coast.
    pub port: bool,
    /// Beside a major river (several times the river threshold), where a ford
    /// or bridge matters.
    pub river_crossing: bool,
    /// On a saddle between higher ground.
    pub mountain_pass: bool,
    /// Fresh water in dry country.
    pub oasis: bool,
}

impl SettlementFlags {
    /// Human-readable list, e.g. "port, river crossing".
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.port {
            parts.push("port");
        }
        if self.river_crossing {
            parts.push("river crossing");
        }
        if self.mountain_pass {
            parts.push("mountain pass");
        }
        if self.oasis {
            parts.push("oasis");
        }
        parts.join(", ")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settlement {
    pub id: u32,
    /// Cell index.
    pub cell: usize,
    pub name: String,
    pub people: u16,
    pub rank: Rank,
    pub population: u32,
    pub flags: SettlementFlags,
}

impl Settlement {
    pub fn position(&self, width: usize) -> [usize; 2] {
        [self.cell % width, self.cell / width]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settlements {
    pub width: usize,
    pub height: usize,
    /// In placement order (best site first); ids are indices.
    pub settlements: Vec<Settlement>,
    /// Per cell: the settlement on it, or [`NO_SETTLEMENT`].
    pub at_cell: Vec<u32>,
}

impl Settlements {
    pub fn at(&self, x: usize, y: usize) -> Option<&Settlement> {
        self.settlements
            .get(self.at_cell[y * self.width + x] as usize)
    }
}

/// Inputs every settlement rule reads.
pub struct SettlementInputs<'a> {
    pub config: &'a WorldConfig,
    pub terrain: &'a Heightmap,
    pub climate: &'a Climate,
    pub hydrology: &'a Hydrology,
    pub biomes: &'a Biomes,
    pub habitability: &'a Habitability,
    pub peoples: &'a Peoples,
}

/// Runs the settlement stage.
pub fn generate_settlements(input: &SettlementInputs) -> Settlements {
    let (w, h) = (input.terrain.width, input.terrain.height);
    let p = &input.config.settlements;
    let reference = w as f32 / 512.0;
    let area_scale = (w * h) as f32 / (512.0 * 512.0);
    // Small worlds still need enough settlements to found every realm.
    let target = ((p.target_count as f32 * area_scale).round() as usize)
        .max(2 * input.config.politics.target_realms as usize)
        .max(1);

    // Candidate sites, best first; ties by cell index.
    let score = |i: usize| -> f32 {
        let flags = site_flags(i, input);
        input.habitability.value[i]
            + p.river_bonus * flags.river_crossing as u32 as f32
            + p.port_bonus * flags.port as u32 as f32
    };
    let mut candidates: Vec<(f32, usize)> = (0..w * h)
        .filter(|&i| !input.biomes.biome[i].is_water() && input.peoples.homeland[i] != NO_PEOPLE)
        .map(|i| (score(i), i))
        .filter(|&(s, _)| s >= p.score_floor)
        .collect();
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    let spacing_of = |i: usize| spacing(p, input.habitability.value[i]) * reference;
    let mut chosen: Vec<usize> = Vec::new();
    // Spatial buckets so spacing checks only look nearby.
    let bucket = (p.min_spacing * reference).ceil().max(1.0) as usize;
    let (bw, bh) = (w.div_ceil(bucket), h.div_ceil(bucket));
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); bw * bh];
    for &(_, cell) in &candidates {
        if chosen.len() >= target {
            break;
        }
        let (x, y) = (cell % w, cell / w);
        let need = spacing_of(cell);
        let (bx, by) = (x / bucket, y / bucket);
        let reach = (need / bucket as f32).ceil() as usize;
        let mut clear = true;
        'scan: for ny in by.saturating_sub(reach)..(by + reach + 1).min(bh) {
            for nx in bx.saturating_sub(reach)..(bx + reach + 1).min(bw) {
                for &other in &buckets[ny * bw + nx] {
                    let (dx, dy) = ((other % w) as f32 - x as f32, (other / w) as f32 - y as f32);
                    if (dx * dx + dy * dy).sqrt() < need {
                        clear = false;
                        break 'scan;
                    }
                }
            }
        }
        if clear {
            buckets[by * bw + bx].push(cell);
            chosen.push(cell);
        }
    }

    // Population from the land around each site.
    let catchment = ((p.catchment_radius * reference).round() as usize).max(1);
    let threshold = input.config.hydrology.river_threshold.max(1e-3);
    let mut settlements: Vec<Settlement> = chosen
        .iter()
        .enumerate()
        .map(|(id, &cell)| {
            let flags = site_flags(cell, input);
            let rng = &mut entity_rng(input.config.seed, stage::SETTLEMENTS, id as u64);
            let land = mean_habitability(cell, w, h, catchment, input.habitability);
            // Bigger rivers carry more trade: up to +100% on the largest.
            let river = largest_river_nearby(cell, w, h, input.hydrology) / threshold;
            let river_boost = (dmath::log2(river.max(1.0)) / 6.0).min(1.0);
            let boost = 1.0 + river_boost + 0.4 * flags.port as u32 as f32;
            // Skewed so a few places grow much larger than the rest.
            let u = unit_f32(rng);
            let jitter = 0.5 + 1.5 * u * u;
            Settlement {
                id: id as u32,
                cell,
                name: String::new(),
                people: input.peoples.homeland[cell],
                rank: Rank::Village,
                population: (p.base_population * dmath::powf(land, 1.5) * boost * jitter)
                    .round()
                    .max(20.0) as u32,
                flags,
            }
        })
        .collect();

    assign_ranks(&mut settlements, input.config, w);
    name_settlements(&mut settlements, input.peoples);

    let mut at_cell = vec![NO_SETTLEMENT; w * h];
    for s in &settlements {
        at_cell[s.cell] = s.id;
    }
    Settlements {
        width: w,
        height: h,
        settlements,
        at_cell,
    }
}

/// Minimum distance (in 512-map cells) from a site with this habitability to
/// any other settlement: smaller on rich land.
fn spacing(p: &crate::config::SettlementConfig, habitability: f32) -> f32 {
    p.min_spacing * (1.0 - p.fertile_spacing_factor * habitability.clamp(0.0, 1.0))
}

/// Discharge of the biggest river within one cell, or 0.
fn largest_river_nearby(cell: usize, w: usize, h: usize, hydrology: &Hydrology) -> f32 {
    let (x, y) = (cell % w, cell / w);
    let mut best = 0.0f32;
    for ny in y.saturating_sub(1)..(y + 2).min(h) {
        for nx in x.saturating_sub(1)..(x + 2).min(w) {
            let j = ny * w + nx;
            if hydrology.segment_id[j] != NO_SEGMENT {
                best = best.max(hydrology.accumulation[j]);
            }
        }
    }
    best
}

fn mean_habitability(cell: usize, w: usize, h: usize, radius: usize, hab: &Habitability) -> f32 {
    let (x, y) = (cell % w, cell / w);
    let (mut sum, mut n) = (0.0, 0.0);
    for ny in y.saturating_sub(radius)..(y + radius + 1).min(h) {
        for nx in x.saturating_sub(radius)..(x + radius + 1).min(w) {
            sum += hab.value[ny * w + nx];
            n += 1.0;
        }
    }
    sum / n
}

/// Capitals: about `politics.target_realms` of them. Each people's largest
/// settlement is promoted first (largest peoples first), then the largest
/// remaining settlements that aren't too close to another capital. The rest
/// split into cities and towns by population; everything else is a village.
fn assign_ranks(settlements: &mut [Settlement], config: &WorldConfig, w: usize) {
    let p = &config.settlements;
    let mut by_size: Vec<usize> = (0..settlements.len()).collect();
    by_size.sort_by(|&a, &b| {
        settlements[b]
            .population
            .cmp(&settlements[a].population)
            .then(a.cmp(&b))
    });

    let target = (config.politics.target_realms as usize).min(settlements.len());
    let mut capitals: Vec<usize> = Vec::new();
    let mut peoples_with_capital = BTreeSet::new();
    for &i in &by_size {
        if capitals.len() >= target {
            break;
        }
        if peoples_with_capital.insert(settlements[i].people) {
            capitals.push(i);
        }
    }
    let min_gap = p.min_spacing * 3.0 * w as f32 / 512.0;
    for &i in &by_size {
        if capitals.len() >= target {
            break;
        }
        let [x, y] = settlements[i].position(w);
        let far = capitals.iter().all(|&c| {
            let [cx, cy] = settlements[c].position(w);
            let (dx, dy) = (cx as f32 - x as f32, cy as f32 - y as f32);
            (dx * dx + dy * dy).sqrt() >= min_gap
        });
        if far && !capitals.contains(&i) {
            capitals.push(i);
        }
    }
    // If spacing left us short (small or crowded maps), take the largest
    // remaining settlements regardless: the realm target matters more.
    for &i in &by_size {
        if capitals.len() >= target {
            break;
        }
        if !capitals.contains(&i) {
            capitals.push(i);
        }
    }
    for &c in &capitals {
        settlements[c].rank = Rank::Capital;
        // Seats of power draw people in.
        settlements[c].population = (settlements[c].population as f32 * 1.5) as u32;
    }

    let others: Vec<usize> = by_size
        .into_iter()
        .filter(|i| !capitals.contains(i))
        .collect();
    let cities = (others.len() as f32 * p.city_share).round() as usize;
    let towns = (others.len() as f32 * p.town_share).round() as usize;
    for (k, &i) in others.iter().enumerate() {
        settlements[i].rank = if k < cities {
            Rank::City
        } else if k < cities + towns {
            Rank::Town
        } else {
            Rank::Village
        };
    }
}

/// Unique names in each settlement's own language, in id order.
fn name_settlements(settlements: &mut [Settlement], peoples: &Peoples) {
    let mut used: BTreeSet<String> = peoples.peoples.iter().map(|p| p.name.clone()).collect();
    let mut next_index = vec![0u64; peoples.peoples.len()];
    for s in settlements.iter_mut() {
        let people = s.people as usize;
        let language = &peoples.peoples[people].language;
        loop {
            let name = language.place_name(next_index[people]);
            next_index[people] += 1;
            if used.insert(name.clone()) {
                s.name = name;
                break;
            }
        }
    }
}

/// Geographic features of a site.
fn site_flags(i: usize, input: &SettlementInputs) -> SettlementFlags {
    let (w, h) = (input.terrain.width, input.terrain.height);
    let (x, y) = (i % w, i / w);
    let near = |r: usize, test: &dyn Fn(usize) -> bool| {
        (y.saturating_sub(r)..(y + r + 1).min(h))
            .any(|ny| (x.saturating_sub(r)..(x + r + 1).min(w)).any(|nx| test(ny * w + nx)))
    };
    let hy = input.hydrology;
    let threshold = input.config.hydrology.river_threshold;
    let port = near(1, &|j| input.biomes.biome[j] == Biome::Ocean);
    let river_crossing = near(1, &|j| {
        hy.segment_id[j] != NO_SEGMENT && hy.accumulation[j] >= 4.0 * threshold
    });
    let dry = matches!(
        input.biomes.biome[i],
        Biome::Desert | Biome::Savanna | Biome::Shrubland
    ) && input.climate.precipitation[i] < 450.0;
    let oasis = dry
        && near(2, &|j| {
            hy.lake_id[j] != NO_LAKE || hy.segment_id[j] != NO_SEGMENT
        });
    SettlementFlags {
        port,
        river_crossing,
        mountain_pass: is_pass(i, input),
        oasis,
    }
}

/// A saddle: high ground on both sides along one axis and lower ground on both
/// sides along the perpendicular axis, above 600 m.
fn is_pass(i: usize, input: &SettlementInputs) -> bool {
    let (w, h) = (input.terrain.width, input.terrain.height);
    let terrain = input.terrain;
    if elevation_m(terrain.data[i], &input.config.terrain) < 600.0 {
        return false;
    }
    let r = ((3.0 * w as f32 / 512.0).round() as isize).max(1);
    let (x, y) = ((i % w) as isize, (i / w) as isize);
    let at = |dx: isize, dy: isize| -> Option<f32> {
        let (nx, ny) = (x + dx, y + dy);
        (nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h)
            .then(|| terrain.data[ny as usize * w + nx as usize])
    };
    let here = terrain.data[i];
    let margin = 0.004;
    for ((ax, ay), (bx, by)) in [
        ((r, 0), (0, r)),
        ((0, r), (r, 0)),
        ((r, r), (r, -r)),
        ((r, -r), (r, r)),
    ] {
        let (Some(a1), Some(a2), Some(b1), Some(b2)) =
            (at(ax, ay), at(-ax, -ay), at(bx, by), at(-bx, -by))
        else {
            continue;
        };
        if a1 > here + margin && a2 > here + margin && b1 < here - margin && b2 < here - margin {
            return true;
        }
    }
    false
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
            World::generate(&small(5)).settlements,
            World::generate(&small(5)).settlements
        );
    }

    #[test]
    fn no_settlement_on_water_and_all_belong_to_a_people() {
        let world = World::generate(&small(42));
        assert!(!world.settlements.settlements.is_empty());
        for s in &world.settlements.settlements {
            assert!(
                !world.biomes.biome[s.cell].is_water(),
                "{} is on water",
                s.name
            );
            assert_ne!(s.people, NO_PEOPLE, "{} has no people", s.name);
            assert_eq!(world.peoples.homeland[s.cell], s.people);
        }
    }

    #[test]
    fn spacing_holds() {
        let world = World::generate(&small(42));
        let w = world.settlements.width;
        let p = &world.config.settlements;
        let list = &world.settlements.settlements;
        for (i, later) in list.iter().enumerate() {
            let need = spacing(p, world.habitability.value[later.cell]) * w as f32 / 512.0;
            for earlier in &list[..i] {
                let ([ax, ay], [bx, by]) = (earlier.position(w), later.position(w));
                let d =
                    (dmath::sq(ax as f32 - bx as f32) + dmath::sq(ay as f32 - by as f32)).sqrt();
                assert!(
                    d >= need,
                    "{} and {} are {d:.1} apart, need {need:.1}",
                    earlier.name,
                    later.name
                );
            }
        }
    }

    #[test]
    fn capitals_match_the_realm_target_and_have_peoples() {
        for target in [3, 6, 10] {
            let mut config = small(42);
            config.politics.target_realms = target;
            let world = World::generate(&config);
            let capitals: Vec<_> = world
                .settlements
                .settlements
                .iter()
                .filter(|s| s.rank == Rank::Capital)
                .collect();
            assert_eq!(
                capitals.len(),
                (target as usize).min(world.settlements.settlements.len())
            );
            for c in capitals {
                assert!((c.people as usize) < world.peoples.peoples.len());
            }
        }
    }

    #[test]
    fn names_are_unique_and_pronounceable() {
        let world = World::generate(&small(42));
        let mut seen = BTreeSet::new();
        for s in &world.settlements.settlements {
            assert!(crate::names::is_pronounceable(&s.name), "{}", s.name);
            assert!(seen.insert(s.name.clone()), "duplicate name {}", s.name);
        }
    }

    #[test]
    fn settlement_settings_leave_earlier_stages_alone() {
        let base = World::generate(&small(42));
        let mut config = small(42);
        config.settlements.min_spacing = 30.0;
        config.settlements.fertile_spacing_factor = 0.0;
        let changed = World::generate(&config);
        assert_eq!(base.peoples, changed.peoples);
        assert_eq!(base.habitability, changed.habitability);
        assert_ne!(base.settlements, changed.settlements);
    }

    #[test]
    fn lookup_by_cell_matches() {
        let world = World::generate(&small(42));
        let w = world.settlements.width;
        for s in &world.settlements.settlements {
            let [x, y] = s.position(w);
            assert_eq!(world.settlements.at(x, y), Some(s));
        }
    }
}
