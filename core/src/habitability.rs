//! Stage: habitability. How attractive each cell is to live on, from 0
//! (nobody could) to 1 (prime land). Peoples spread from and settlements are
//! placed on high values.
//!
//! A cell's value is a base from its biome, plus fresh water (rivers scaled
//! by discharge, lake shores), plus sheltered coast (bays score higher than
//! open shore), minus slope and altitude.

use crate::biome::{Biome, Biomes};
use crate::climate::elevation_m;
use crate::config::WorldConfig;
use crate::field::slope_at_reference;
use crate::heightmap::Heightmap;
use crate::hydrology::{Hydrology, NO_LAKE, NO_SEGMENT};

#[derive(Clone, Debug, PartialEq)]
pub struct Habitability {
    pub width: usize,
    pub height: usize,
    /// Per cell, `0.0..=1.0`. Water is 0.
    pub value: Vec<f32>,
}

impl Habitability {
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.value[y * self.width + x]
    }
}

/// Base habitability of a biome before water, coast and terrain.
pub fn biome_base(biome: Biome) -> f32 {
    match biome {
        Biome::Ocean | Biome::Lake | Biome::Ice => 0.0,
        Biome::Alpine => 0.02,
        Biome::Desert => 0.05,
        Biome::Tundra => 0.08,
        Biome::Mangrove => 0.25,
        Biome::Wetland => 0.3,
        Biome::BorealForest => 0.35,
        Biome::TropicalRainforest => 0.4,
        Biome::TropicalForest => 0.55,
        Biome::Savanna | Biome::Shrubland => 0.6,
        Biome::TemperateRainforest => 0.6,
        Biome::TemperateForest => 0.8,
        Biome::Grassland => 0.9,
    }
}

/// Runs the habitability stage.
pub fn generate_habitability(
    config: &WorldConfig,
    terrain: &Heightmap,
    hydrology: &Hydrology,
    biomes: &Biomes,
) -> Habitability {
    let (w, h) = (terrain.width, terrain.height);
    let p = &config.habitability;
    // Radii are given for a 512-wide map; scale them to this one.
    let to_cells = |reference: f32| ((reference * w as f32 / 512.0).round() as usize).max(1);

    let water = fresh_water_bonus(w, h, hydrology, config, to_cells(p.river_radius));
    let coast = coast_shelter(w, h, biomes, to_cells(p.coast_radius));

    let value = (0..w * h)
        .map(|i| {
            let biome = biomes.biome[i];
            if biome.is_water() {
                return 0.0;
            }
            let elevation_km = elevation_m(terrain.data[i], &config.terrain) / 1000.0;
            let slope = slope_at_reference(i, w, h, terrain);
            let v = biome_base(biome) + water[i] + p.coast_weight * coast[i]
                - p.slope_penalty * slope
                - p.elevation_penalty * elevation_km;
            v.clamp(0.0, 1.0)
        })
        .collect();
    Habitability {
        width: w,
        height: h,
        value,
    }
}

/// Bonus for nearby rivers (stronger for bigger rivers) and lake shores,
/// fading linearly over `radius` cells. Nearby water doesn't stack: each cell
/// keeps its best source.
fn fresh_water_bonus(
    w: usize,
    h: usize,
    hydrology: &Hydrology,
    config: &WorldConfig,
    radius: usize,
) -> Vec<f32> {
    let p = &config.habitability;
    let threshold = config.hydrology.river_threshold.max(1e-3);
    let mut bonus = vec![0.0f32; w * h];
    let mut stamp = |cx: usize, cy: usize, strength: f32, radius: usize| {
        let r = radius as isize;
        for dy in -r..=r {
            for dx in -r..=r {
                let (Some(x), Some(y)) = (cx.checked_add_signed(dx), cy.checked_add_signed(dy))
                else {
                    continue;
                };
                if x >= w || y >= h {
                    continue;
                }
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                let falloff = 1.0 - d / (radius as f32 + 1.0);
                if falloff > 0.0 {
                    let i = y * w + x;
                    bonus[i] = bonus[i].max(strength * falloff);
                }
            }
        }
    };
    for i in 0..w * h {
        let (x, y) = (i % w, i / w);
        if hydrology.segment_id[i] != NO_SEGMENT {
            let size = ((hydrology.accumulation[i] / threshold).sqrt() / 3.0).min(1.0);
            stamp(x, y, p.river_weight * size, radius);
        } else if hydrology.lake_id[i] != NO_LAKE {
            stamp(x, y, p.lake_weight, (radius / 2).max(1));
        }
    }
    bonus
}

/// How sheltered each coastal cell is, `0.0..=1.0`: from the share `f` of
/// ocean within `radius`, `4f(1 - f)`, which is 0 inland and on open sea and
/// peaks where the sea half-surrounds the land (bays, inlets, estuaries).
fn coast_shelter(w: usize, h: usize, biomes: &Biomes, radius: usize) -> Vec<f32> {
    // Summed-area table of ocean cells for O(1) box counts.
    let mut sum = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        for x in 0..w {
            let ocean = (biomes.biome[y * w + x] == Biome::Ocean) as u32;
            sum[(y + 1) * (w + 1) + x + 1] =
                ocean + sum[y * (w + 1) + x + 1] + sum[(y + 1) * (w + 1) + x]
                    - sum[y * (w + 1) + x];
        }
    }
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let (x0, y0) = (x.saturating_sub(radius), y.saturating_sub(radius));
            let (x1, y1) = ((x + radius + 1).min(w), (y + radius + 1).min(h));
            let count = sum[y1 * (w + 1) + x1] + sum[y0 * (w + 1) + x0]
                - sum[y0 * (w + 1) + x1]
                - sum[y1 * (w + 1) + x0];
            let f = count as f32 / ((x1 - x0) * (y1 - y0)) as f32;
            4.0 * f * (1.0 - f)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    fn world() -> World {
        let mut config = WorldConfig {
            seed: 42,
            width: 96,
            height: 96,
            ..Default::default()
        };
        config.erosion.droplets = 3000;
        World::generate(&config)
    }

    #[test]
    fn values_are_in_range_and_zero_on_water() {
        let world = world();
        for (i, &v) in world.habitability.value.iter().enumerate() {
            assert!((0.0..=1.0).contains(&v));
            if world.biomes.biome[i].is_water() {
                assert_eq!(v, 0.0);
            }
        }
    }

    #[test]
    fn rivers_make_land_more_habitable() {
        let mut config = world().config;
        let with = World::generate(&config).habitability;
        config.habitability.river_weight = 0.0;
        config.habitability.lake_weight = 0.0;
        let without = World::generate(&config).habitability;
        let sum = |h: &Habitability| h.value.iter().sum::<f32>();
        assert!(sum(&with) > sum(&without));
    }

    #[test]
    fn bays_beat_open_coast_and_inland() {
        // Ocean on the left half of a 9x9 map: the shoreline column sees about
        // half ocean, deep inland sees none.
        let (w, h) = (9, 9);
        let biome = (0..w * h)
            .map(|i| {
                if i % w < 4 {
                    Biome::Ocean
                } else {
                    Biome::Grassland
                }
            })
            .collect();
        let biomes = Biomes {
            width: w,
            height: h,
            biome,
        };
        let shelter = coast_shelter(w, h, &biomes, 2);
        assert!(shelter[4 * w + 4] > 0.9, "shore: {}", shelter[4 * w + 4]);
        assert_eq!(shelter[4 * w + 8], 0.0, "inland");
    }
}
