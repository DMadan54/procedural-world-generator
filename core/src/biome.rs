//! Stage: biomes. Classifies each cell from its climate and elevation using a
//! simplified Whittaker table, then adds wetlands and cleans up speckle.
//!
//! Steps, in order:
//! 1. Water (sea and lakes) is fixed first and never changed afterwards.
//! 2. Temperature and precipitation are nudged by low-amplitude fractal noise
//!    before classification, so biome borders on smooth climate gradients
//!    come out ragged like real ecotones instead of following contour lines.
//! 3. Flat, wet ground (big flow accumulation on a low slope, or a lake
//!    shore) becomes wetland, or mangrove in the tropics.
//! 4. A few passes of a 3×3 majority filter remove single-cell speckle.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand_chacha::rand_core::RngCore;

use crate::climate::{elevation_m, Climate};
use crate::config::WorldConfig;
use crate::heightmap::Heightmap;
use crate::hydrology::{Hydrology, NO_LAKE};
use crate::rng::{stage, stage_rng, unit_f64};

/// Biomes in a fixed order. The order is used to break ties (lower wins), so
/// new biomes go at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Biome {
    Ocean,
    Lake,
    /// Permanent ice sheet.
    Ice,
    Tundra,
    /// Bare rock and snow above the tree line.
    Alpine,
    BorealForest,
    TemperateForest,
    TemperateRainforest,
    /// Dry summer scrub (Mediterranean-type).
    Shrubland,
    Grassland,
    Desert,
    Savanna,
    TropicalForest,
    TropicalRainforest,
    /// Marsh and fen: waterlogged flats along big rivers and lake shores.
    Wetland,
    /// Tropical tidal or river-mouth swamp forest.
    Mangrove,
}

impl Biome {
    pub const COUNT: usize = 16;

    pub const ALL: [Biome; Biome::COUNT] = [
        Biome::Ocean,
        Biome::Lake,
        Biome::Ice,
        Biome::Tundra,
        Biome::Alpine,
        Biome::BorealForest,
        Biome::TemperateForest,
        Biome::TemperateRainforest,
        Biome::Shrubland,
        Biome::Grassland,
        Biome::Desert,
        Biome::Savanna,
        Biome::TropicalForest,
        Biome::TropicalRainforest,
        Biome::Wetland,
        Biome::Mangrove,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Biome::Ocean => "Ocean",
            Biome::Lake => "Lake",
            Biome::Ice => "Ice",
            Biome::Tundra => "Tundra",
            Biome::Alpine => "Alpine",
            Biome::BorealForest => "Boreal forest",
            Biome::TemperateForest => "Temperate forest",
            Biome::TemperateRainforest => "Temperate rainforest",
            Biome::Shrubland => "Shrubland",
            Biome::Grassland => "Grassland",
            Biome::Desert => "Desert",
            Biome::Savanna => "Savanna",
            Biome::TropicalForest => "Tropical forest",
            Biome::TropicalRainforest => "Tropical rainforest",
            Biome::Wetland => "Wetland",
            Biome::Mangrove => "Mangrove",
        }
    }

    /// Position in [`Biome::ALL`], for per-biome arrays.
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn is_forest(self) -> bool {
        matches!(
            self,
            Biome::BorealForest
                | Biome::TemperateForest
                | Biome::TemperateRainforest
                | Biome::TropicalForest
                | Biome::TropicalRainforest
        )
    }

    pub fn is_wetland(self) -> bool {
        matches!(self, Biome::Wetland | Biome::Mangrove)
    }

    pub fn is_water(self) -> bool {
        matches!(self, Biome::Ocean | Biome::Lake)
    }

    /// Too cold for open water to stay liquid, so never wetland.
    fn is_frozen(self) -> bool {
        matches!(self, Biome::Ice | Biome::Tundra | Biome::Alpine)
    }
}

/// Above this elevation (metres) land is alpine whatever the climate.
pub const TREE_LINE_M: f32 = 3000.0;

/// Wetlands at or above this temperature (°C) are mangrove.
pub const MANGROVE_MIN_C: f32 = 22.0;

/// Land biome for a mean temperature (°C), annual precipitation (mm) and
/// elevation (m).
pub fn classify(temperature: f32, precipitation: f32, elevation_m: f32) -> Biome {
    if temperature < -10.0 {
        return Biome::Ice;
    }
    if elevation_m > TREE_LINE_M {
        return Biome::Alpine;
    }
    if temperature < -2.0 {
        return Biome::Tundra;
    }
    if temperature < 5.0 {
        return if precipitation < 250.0 {
            Biome::Tundra
        } else {
            Biome::BorealForest
        };
    }
    if temperature < 18.0 {
        return match precipitation {
            p if p < 250.0 => Biome::Desert,
            p if p < 500.0 && temperature > 10.0 => Biome::Shrubland,
            p if p < 600.0 => Biome::Grassland,
            p if p < 1800.0 => Biome::TemperateForest,
            _ => Biome::TemperateRainforest,
        };
    }
    match precipitation {
        p if p < 300.0 => Biome::Desert,
        p if p < 900.0 => Biome::Savanna,
        p if p < 1800.0 => Biome::TropicalForest,
        _ => Biome::TropicalRainforest,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Biomes {
    pub width: usize,
    pub height: usize,
    pub biome: Vec<Biome>,
}

impl Biomes {
    pub fn at(&self, x: usize, y: usize) -> Biome {
        self.biome[y * self.width + x]
    }

    /// Share of land covered by each land biome, largest first (ties by enum
    /// order). Water is excluded. Shares sum to 1 when there is any land.
    pub fn coverage(&self) -> Vec<(Biome, f32)> {
        let mut counts = [0u32; Biome::COUNT];
        for b in &self.biome {
            if !b.is_water() {
                counts[b.index()] += 1;
            }
        }
        let land: u32 = counts.iter().sum();
        if land == 0 {
            return Vec::new();
        }
        let mut shares: Vec<(Biome, f32)> = Biome::ALL
            .into_iter()
            .filter(|b| counts[b.index()] > 0)
            .map(|b| (b, counts[b.index()] as f32 / land as f32))
            .collect();
        shares.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        shares
    }
}

/// Runs the biome stage over the final terrain, climate and hydrology.
pub fn generate_biomes(
    config: &WorldConfig,
    terrain: &Heightmap,
    climate: &Climate,
    hydrology: &Hydrology,
) -> Biomes {
    let (w, h) = (terrain.width, terrain.height);
    let params = &config.biomes;
    let sea = config.terrain.sea_level;

    // Low-frequency noise for ragged ecotones, two independent channels.
    let mut rng = stage_rng(config.seed, stage::BIOMES);
    let fbm = Fbm::<Perlin>::new(rng.next_u32())
        .set_octaves(4)
        .set_frequency(12.0);
    let (ox, oy) = (unit_f64(&mut rng) * 10_000.0, unit_f64(&mut rng) * 10_000.0);
    let inv = 1.0 / w.max(1) as f64;
    let jitter = |x: usize, y: usize, channel: f64| {
        fbm.get([ox + x as f64 * inv + channel * 71.9, oy + y as f64 * inv]) as f32
    };

    let mut biome: Vec<Biome> = (0..w * h)
        .map(|i| {
            let height = terrain.data[i];
            if height < sea {
                return Biome::Ocean;
            }
            if hydrology.lake_id[i] != NO_LAKE {
                return Biome::Lake;
            }
            let (x, y) = (i % w, i / w);
            let t = climate.temperature[i] + params.edge_jitter_c * jitter(x, y, 0.0);
            let p = climate.precipitation[i] * (1.0 + params.edge_jitter_frac * jitter(x, y, 1.0));
            let land = classify(t, p.max(0.0), elevation_m(height, &config.terrain));
            if is_wet_ground(i, w, h, land, terrain, hydrology, params) {
                if t >= MANGROVE_MIN_C {
                    Biome::Mangrove
                } else {
                    Biome::Wetland
                }
            } else {
                land
            }
        })
        .collect();

    majority_filter(&mut biome, w, h, params.smoothing_passes);
    Biomes {
        width: w,
        height: h,
        biome,
    }
}

/// Steepest drop from a cell to one of its four neighbours, in normalised
/// height units per cell.
fn slope(i: usize, w: usize, h: usize, terrain: &Heightmap) -> f32 {
    let (x, y) = (i % w, i / w);
    let here = terrain.data[i];
    let mut steepest = 0.0f32;
    for (dx, dy) in [(1isize, 0isize), (-1, 0), (0, 1), (0, -1)] {
        let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
            continue;
        };
        if nx < w && ny < h {
            steepest = steepest.max((here - terrain.data[ny * w + nx]).abs());
        }
    }
    steepest
}

/// The wetland thresholds at this map width: (max slope per cell, min flow
/// accumulation). The config gives them for a 512-cell-wide map. A coarser
/// map's cells span more ground, so the same terrain has a steeper slope per
/// cell and fewer cells upstream; scaling keeps wetland cover similar
/// whatever the world size.
fn wet_thresholds(w: usize, params: &crate::config::BiomeConfig) -> (f32, f32) {
    let cells_per_reference = 512.0 / w.max(1) as f32;
    (
        params.wetland_max_slope * cells_per_reference,
        params.wetland_accumulation / cells_per_reference,
    )
}

/// Wetland rule: non-frozen land on a low slope that either collects a lot
/// of water or borders a lake.
fn is_wet_ground(
    i: usize,
    w: usize,
    h: usize,
    land: Biome,
    terrain: &Heightmap,
    hydrology: &Hydrology,
    params: &crate::config::BiomeConfig,
) -> bool {
    let (max_slope, min_accumulation) = wet_thresholds(w, params);
    if land.is_frozen() || slope(i, w, h, terrain) > max_slope {
        return false;
    }
    if hydrology.accumulation[i] >= min_accumulation {
        return true;
    }
    let (x, y) = (i % w, i / w);
    (y.saturating_sub(1)..(y + 2).min(h)).any(|ny| {
        (x.saturating_sub(1)..(x + 2).min(w)).any(|nx| hydrology.lake_id[ny * w + nx] != NO_LAKE)
    })
}

/// Removes speckle: each land cell takes the most common land biome in its
/// 3×3 neighbourhood (itself included) if that strictly outnumbers its own.
/// Ties go to the lower enum value. Water is never counted or changed. Each
/// pass reads the previous pass's result, so the outcome doesn't depend on
/// visiting order.
pub fn majority_filter(biome: &mut [Biome], w: usize, h: usize, passes: u32) {
    for _ in 0..passes {
        let before = biome.to_vec();
        for i in 0..w * h {
            let own = before[i];
            if own.is_water() {
                continue;
            }
            let (x, y) = (i % w, i / w);
            let mut counts = [0u32; Biome::COUNT];
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    let b = before[ny * w + nx];
                    if !b.is_water() {
                        counts[b.index()] += 1;
                    }
                }
            }
            // Highest count; the first (lowest enum value) wins ties.
            let mut best = own;
            for b in Biome::ALL {
                if counts[b.index()] > counts[best.index()] {
                    best = b;
                }
            }
            biome[i] = best;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    #[test]
    fn whittaker_corners() {
        assert_eq!(classify(-20.0, 500.0, 0.0), Biome::Ice);
        assert_eq!(classify(-5.0, 300.0, 0.0), Biome::Tundra);
        assert_eq!(classify(2.0, 600.0, 0.0), Biome::BorealForest);
        assert_eq!(classify(12.0, 1000.0, 0.0), Biome::TemperateForest);
        assert_eq!(classify(12.0, 2500.0, 0.0), Biome::TemperateRainforest);
        assert_eq!(classify(12.0, 100.0, 0.0), Biome::Desert);
        assert_eq!(classify(8.0, 400.0, 0.0), Biome::Grassland);
        assert_eq!(classify(15.0, 400.0, 0.0), Biome::Shrubland);
        assert_eq!(classify(25.0, 100.0, 0.0), Biome::Desert);
        assert_eq!(classify(25.0, 600.0, 0.0), Biome::Savanna);
        assert_eq!(classify(25.0, 1200.0, 0.0), Biome::TropicalForest);
        assert_eq!(classify(25.0, 3000.0, 0.0), Biome::TropicalRainforest);
    }

    #[test]
    fn high_ground_is_alpine() {
        assert_eq!(classify(10.0, 1000.0, 3500.0), Biome::Alpine);
    }

    #[test]
    fn biome_list_is_complete_and_ordered() {
        for (i, b) in Biome::ALL.iter().enumerate() {
            assert_eq!(b.index(), i);
            assert!(!b.name().is_empty());
        }
    }

    use Biome::{Desert as D, Grassland as G, Ocean as O, TemperateForest as F};

    #[test]
    fn majority_filter_removes_speckle() {
        #[rustfmt::skip]
        let mut grid = vec![
            G, G, G, G, G,
            G, G, G, G, G,
            G, G, D, G, G,
            G, G, G, G, G,
            G, G, G, G, G,
        ];
        majority_filter(&mut grid, 5, 5, 1);
        assert!(grid.iter().all(|&b| b == G));
    }

    #[test]
    fn majority_filter_never_touches_water() {
        #[rustfmt::skip]
        let mut grid = vec![
            G, G, G, G, G,
            G, G, G, G, G,
            G, G, O, G, G,
            G, G, G, G, G,
            O, O, G, G, G,
        ];
        let before = grid.clone();
        majority_filter(&mut grid, 5, 5, 3);
        for (a, b) in before.iter().zip(&grid) {
            assert_eq!(a.is_water(), b.is_water());
        }
        assert_eq!(grid[12], O);
    }

    #[test]
    fn majority_filter_keeps_real_regions_and_breaks_ties_low() {
        #[rustfmt::skip]
        let mut grid = vec![
            F, F, G, G, G,
            F, F, G, G, G,
            F, F, G, G, G,
            F, F, G, G, G,
            F, F, G, G, G,
        ];
        let before = grid.clone();
        majority_filter(&mut grid, 5, 5, 2);
        assert_eq!(grid, before, "a straight border is stable");

        // A cell with three of each around it keeps its own biome, and a cell
        // outnumbered equally by two others takes the lower enum value.
        let mut tie = vec![D, D, D, G, G, G, F, F, F];
        majority_filter(&mut tie, 3, 3, 1);
        assert_eq!(tie[4], G, "own biome ties with Desert: keep Grassland");
        let mut outnumbered = vec![D, D, D, F, O, F, F, D, F];
        outnumbered[4] = G;
        majority_filter(&mut outnumbered, 3, 3, 1);
        assert_eq!(
            outnumbered[4],
            F.min(D),
            "4 forest vs 4 desert -> lower enum"
        );
    }

    fn small(seed: u64) -> WorldConfig {
        let mut config = WorldConfig {
            seed,
            width: 96,
            height: 96,
            ..Default::default()
        };
        config.erosion.droplets = 3000;
        config
    }

    #[test]
    fn coverage_sums_to_one_and_is_sorted() {
        let world = World::generate(&small(42));
        let coverage = world.biomes.coverage();
        let total: f32 = coverage.iter().map(|c| c.1).sum();
        assert!((total - 1.0).abs() < 1e-4);
        assert!(coverage.windows(2).all(|p| p[0].1 >= p[1].1));
        assert!(coverage.iter().all(|c| !c.0.is_water()));
    }

    #[test]
    fn deterministic() {
        assert_eq!(
            World::generate(&small(7)).biomes,
            World::generate(&small(7)).biomes
        );
    }

    #[test]
    fn water_stays_water_and_land_stays_land() {
        let world = World::generate(&small(42));
        let sea = world.config.terrain.sea_level;
        for (i, b) in world.biomes.biome.iter().enumerate() {
            let water = world.terrain.data[i] < sea || world.hydrology.lake_id[i] != NO_LAKE;
            assert_eq!(b.is_water(), water, "cell {i}");
        }
    }

    #[test]
    fn biome_settings_leave_earlier_stages_alone() {
        let base = World::generate(&small(42));
        let mut config = small(42);
        config.biomes.edge_jitter_c = 6.0;
        config.biomes.smoothing_passes = 0;
        config.biomes.wetland_accumulation = 5.0;
        let changed = World::generate(&config);
        assert_eq!(base.terrain, changed.terrain);
        assert_eq!(base.climate, changed.climate);
        assert_eq!(base.hydrology, changed.hydrology);
        assert_ne!(base.biomes, changed.biomes);
    }

    #[test]
    fn wetlands_follow_the_rule() {
        let world = World::generate(&small(42));
        let (w, h) = (world.biomes.width, world.biomes.height);
        let (max_slope, min_accumulation) = wet_thresholds(w, &world.config.biomes);
        // Before smoothing every wetland satisfies the rule; smoothing can
        // grow or shrink them, so check the unsmoothed result.
        let mut config = small(42);
        config.biomes.smoothing_passes = 0;
        let raw = generate_biomes(&config, &world.terrain, &world.climate, &world.hydrology);
        let mut wet = 0;
        for i in 0..w * h {
            if raw.biome[i].is_wetland() {
                wet += 1;
                assert!(slope(i, w, h, &world.terrain) <= max_slope);
                let near_lake = (0..w * h).any(|j| {
                    world.hydrology.lake_id[j] != NO_LAKE
                        && (j % w).abs_diff(i % w) <= 1
                        && (j / w).abs_diff(i / w) <= 1
                });
                assert!(world.hydrology.accumulation[i] >= min_accumulation || near_lake);
            }
        }
        assert!(wet > 0, "a normal world has some wetland");
    }

    #[test]
    fn mangroves_are_tropical_wetlands() {
        let mut config = small(3);
        config.climate.latitude_north = 20.0;
        config.climate.latitude_south = -20.0;
        let world = World::generate(&config);
        assert!(world.biomes.biome.contains(&Biome::Mangrove));
    }
}
