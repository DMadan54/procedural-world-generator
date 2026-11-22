//! Travel cost over the map, and a deterministic multi-source Dijkstra over
//! it. Peoples spread with it; realms, roads and armies reuse it.
//!
//! Costs are per cell entered, expressed for a 512-wide map so totals mean
//! the same distance at every world size.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use crate::biome::{Biome, Biomes};
use crate::climate::elevation_m;
use crate::config::WorldConfig;
use crate::field::slope_at_reference;
use crate::heightmap::Heightmap;
use crate::hydrology::{Hydrology, NO_SEGMENT};

/// Marks a cell nobody reached.
pub const UNOWNED: u16 = u16::MAX;

/// Cost of entering each cell.
pub fn travel_cost_grid(
    config: &WorldConfig,
    terrain: &Heightmap,
    hydrology: &Hydrology,
    biomes: &Biomes,
) -> Vec<f32> {
    let (w, h) = (terrain.width, terrain.height);
    let p = &config.travel;
    let per_reference_cell = 512.0 / w.max(1) as f32;
    (0..w * h)
        .map(|i| {
            let biome = biomes.biome[i];
            let cost = match biome {
                Biome::Ocean => p.sea_cost,
                Biome::Lake => p.lake_cost,
                _ => {
                    let rough = match biome {
                        Biome::Desert | Biome::Ice | Biome::Tundra | Biome::Alpine => p.barren_cost,
                        Biome::Wetland | Biome::Mangrove | Biome::TropicalRainforest => {
                            p.swamp_cost
                        }
                        b if b.is_forest() => p.forest_cost,
                        _ => 0.0,
                    };
                    let elevation_km = elevation_m(terrain.data[i], &config.terrain) / 1000.0;
                    let mountain = p.mountain_cost * (elevation_km - 1.5).max(0.0);
                    let slope = p.slope_cost * slope_at_reference(i, w, h, terrain);
                    let land = 1.0 + rough + mountain + slope;
                    // Rivers are corridors: boats, tow paths, valley floors.
                    if hydrology.segment_id[i] != NO_SEGMENT {
                        land * p.river_factor
                    } else {
                        land
                    }
                }
            };
            cost * per_reference_cell
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Key(f32);
impl Eq for Key {}
impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// Result of [`spread`]: who reached each cell first, and at what cost.
#[derive(Clone, Debug, PartialEq)]
pub struct Reach {
    /// Per cell: the id of the source that reached it, or [`UNOWNED`].
    pub owner: Vec<u16>,
    /// Per cell: the travel cost from that source (infinite if unreached).
    pub cost: Vec<f32>,
}

/// Multi-source Dijkstra: every source claims cells outward over the cost
/// grid until `max_cost`. Each step costs the mean of the two cells' costs
/// times the step length (√2 diagonally).
///
/// Ties are broken explicitly so the result never depends on heap internals:
/// the queue orders by (cost, cell index, source id), so when two sources
/// reach a cell at exactly the same cost the lower id wins.
pub fn spread(cost: &[f32], w: usize, h: usize, sources: &[(usize, u16)], max_cost: f32) -> Reach {
    let mut owner = vec![UNOWNED; w * h];
    let mut best = vec![f32::INFINITY; w * h];
    let mut queue = BinaryHeap::new();
    for &(cell, id) in sources {
        if cell < w * h && 0.0 < best[cell] {
            best[cell] = 0.0;
            queue.push(Reverse((Key(0.0), cell, id)));
        }
    }
    while let Some(Reverse((Key(d), cell, id))) = queue.pop() {
        if owner[cell] != UNOWNED || d > best[cell] {
            continue; // Already settled, or a stale queue entry.
        }
        owner[cell] = id;
        let (x, y) = (cell % w, cell / w);
        for (dx, dy) in [
            (1isize, 0isize),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
                continue;
            };
            if nx >= w || ny >= h {
                continue;
            }
            let n = ny * w + nx;
            if owner[n] != UNOWNED {
                continue;
            }
            let length = if dx != 0 && dy != 0 {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };
            let next = d + 0.5 * (cost[cell] + cost[n]) * length;
            if next <= max_cost && next < best[n] {
                best[n] = next;
                queue.push(Reverse((Key(next), n, id)));
            } else if next <= max_cost && next == best[n] {
                // Equal cost from another source: queue it too and let the
                // (cost, cell, id) order pick the lower id.
                queue.push(Reverse((Key(next), n, id)));
            }
        }
    }
    Reach { owner, cost: best }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_source_wins_and_ties_go_to_the_lower_id() {
        let (w, h) = (9, 1);
        let cost = vec![1.0; w * h];
        let reach = spread(&cost, w, h, &[(0, 7), (8, 3)], f32::INFINITY);
        assert_eq!(&reach.owner[..4], &[7, 7, 7, 7]);
        assert_eq!(&reach.owner[5..], &[3, 3, 3, 3]);
        assert_eq!(reach.owner[4], 3, "equidistant cell goes to the lower id");
    }

    #[test]
    fn expensive_cells_act_as_borders() {
        let (w, h) = (9, 1);
        let mut cost = vec![1.0; w * h];
        cost[2] = 100.0;
        let reach = spread(&cost, w, h, &[(0, 0), (8, 1)], f32::INFINITY);
        assert_eq!(
            reach.owner[3], 1,
            "the wall at cell 2 pushes the border west"
        );
    }

    #[test]
    fn max_cost_leaves_far_cells_unowned() {
        let (w, h) = (20, 1);
        let cost = vec![1.0; w * h];
        let reach = spread(&cost, w, h, &[(0, 0)], 5.0);
        assert_eq!(reach.owner[5], 0);
        assert_eq!(reach.owner[6], UNOWNED);
    }

    #[test]
    fn spreading_is_deterministic() {
        let (w, h) = (30, 30);
        let cost: Vec<f32> = (0..w * h)
            .map(|i| 1.0 + (i * 7919 % 13) as f32 / 4.0)
            .collect();
        let sources = [(31, 0), (500, 1), (880, 2)];
        assert_eq!(
            spread(&cost, w, h, &sources, 400.0),
            spread(&cost, w, h, &sources, 400.0)
        );
    }
}
