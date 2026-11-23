//! Floods and river avulsion. A great flood on one of the big rivers drowns
//! part of the towns along its lower course, and sometimes the river breaks
//! its levee and cuts a new channel to the sea. Towns left on the old bed
//! lose their river trade and decline; the map shows the river in its new
//! course from then on, with the old bed as a dotted oxbow.
//!
//! The new channel is the cheapest path from the breach to open water over
//! the depression-filled surface, where climbing costs far more than
//! distance, so it runs downhill wherever the land allows. It may not touch
//! the old channel's corridor below the breach (or it would just be the old
//! river again).

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use rand_chacha::rand_core::RngCore;
use rand_chacha::ChaCha8Rng;

use super::event::{Cause, EventKind};
use super::sim::HistoryJob;
use crate::hydrology::{Hydrology, RiverEnd};
use crate::rng::unit_f32;

/// Cost of climbing one unit of (normalised) height, in cells of distance.
const CLIMB_COST: f32 = 20000.0;
/// The most a new channel may climb in all, in normalised height (a
/// breached levee, a few tens of metres): more and it isn't a river.
pub(super) const MAX_CLIMB: f32 = 0.003;
/// How far around the old course the search may roam, in cells (at 512²).
const SEARCH_MARGIN: f32 = 40.0;

/// The lower course of a big river that reaches the sea.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Trunk {
    pub(super) river: u32,
    pub(super) segment: u32,
    /// Cells along the trunk segment, upstream to the mouth.
    pub(super) cells: Vec<u32>,
    pub(super) discharge: f32,
    pub(super) name: String,
}

/// The biggest rivers' trunks, largest first.
pub(super) fn trunks(hydrology: &Hydrology, names: &[String]) -> Vec<Trunk> {
    let w = hydrology.width;
    let mut out: Vec<Trunk> = hydrology
        .rivers
        .iter()
        .filter_map(|river| {
            let segment = &hydrology.segments[river.trunk as usize];
            if segment.end != RiverEnd::Ocean || segment.points.len() < 16 {
                return None;
            }
            let cells = segment
                .points
                .iter()
                .map(|p| p[1] as u32 * w as u32 + p[0] as u32)
                .collect();
            Some(Trunk {
                river: river.id,
                segment: river.trunk,
                cells,
                discharge: segment.outflow(),
                name: names.get(river.id as usize).cloned().unwrap_or_default(),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.discharge
            .total_cmp(&a.discharge)
            .then(a.river.cmp(&b.river))
    });
    out.truncate(12);
    out
}

/// A new channel from `cells[at]` to open water that avoids the old
/// channel below the breach, or `None` if there is no such path nearby.
pub(super) fn new_channel(
    trunk: &Trunk,
    at: usize,
    filled: &[f32],
    water: &[bool],
    width: usize,
    height: usize,
) -> Option<Vec<u32>> {
    let (w, h) = (width, height);
    let xy = |c: u32| ((c as usize % w) as i64, (c as usize / w) as i64);
    // The corridor: the old bed below the breach and two cells either side,
    // except right at the breach, where the water leaves the channel.
    let mut blocked = vec![false; w * h];
    for (k, &c) in trunk.cells.iter().enumerate().skip(at + 1) {
        let r = if k <= at + 3 { 0 } else { 2 };
        let (x, y) = xy(c);
        for dy in -r..=r {
            for dx in -r..=r {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                    blocked[ny as usize * w + nx as usize] = true;
                }
            }
        }
    }
    let start = trunk.cells[at];
    blocked[start as usize] = false;
    let margin = SEARCH_MARGIN * w as f32 / 512.0;
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for &c in &trunk.cells[at..] {
        let (x, y) = xy(c);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    let m = margin as i64;
    let inside = |c: usize| {
        let (x, y) = ((c % w) as i64, (c / w) as i64);
        x >= x0 - m && x <= x1 + m && y >= y0 - m && y <= y1 + m
    };

    // Dijkstra; costs are kept as integers (fixed point) so ties and order
    // are exact on every platform.
    let mut cost = vec![u64::MAX; w * h];
    let mut from = vec![u32::MAX; w * h];
    let mut heap = BinaryHeap::new();
    cost[start as usize] = 0;
    heap.push(Reverse((0u64, start)));
    while let Some(Reverse((c_cost, c))) = heap.pop() {
        if c_cost > cost[c as usize] {
            continue;
        }
        let ci = c as usize;
        // Open water below the breach: the sea, or a lake lower down.
        if water[ci] && filled[ci] < filled[start as usize] {
            let mut path = vec![c];
            let mut at = c;
            while at != start {
                at = from[at as usize];
                path.push(at);
            }
            path.reverse();
            let climb: f32 = path
                .windows(2)
                .map(|p| (filled[p[1] as usize] - filled[p[0] as usize]).max(0.0))
                .sum();
            return (climb <= MAX_CLIMB).then_some(path);
        }
        let (x, y) = (ci % w, ci / w);
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx < 0 || ny < 0 || nx as usize >= w || ny as usize >= h {
                    continue;
                }
                let n = ny as usize * w + nx as usize;
                if blocked[n] || !inside(n) {
                    continue;
                }
                let step = if dx != 0 && dy != 0 { 1.414 } else { 1.0 };
                let climb = (filled[n] - filled[ci]).max(0.0) * CLIMB_COST;
                let next = c_cost + ((step + climb) * 1000.0) as u64;
                if next < cost[n] {
                    cost[n] = next;
                    from[n] = c;
                    heap.push(Reverse((next, n as u32)));
                }
            }
        }
    }
    None
}

impl HistoryJob {
    /// A great flood on a big river; sometimes the river jumps its banks.
    pub(super) fn flood(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let total: f32 = self.ctx.trunks.iter().map(|t| t.discharge).sum();
        if total <= 0.0 {
            return;
        }
        let mut pick = unit_f32(rng) * total;
        let mut k = self.ctx.trunks.len() - 1;
        for (i, t) in self.ctx.trunks.iter().enumerate() {
            if pick < t.discharge {
                k = i;
                break;
            }
            pick -= t.discharge;
        }
        let trunk = self.ctx.trunks[k].clone();
        // The river's current lower course: the new channel once it has moved.
        let course: Vec<u32> = self
            .state
            .rivers
            .iter()
            .rev()
            .find(|r| r.river == trunk.river)
            .map_or_else(
                || trunk.cells.clone(),
                |r| {
                    let w = self.ctx.width as u32;
                    r.new_course
                        .iter()
                        .map(|p| p[1] as u32 * w + p[0] as u32)
                        .collect()
                },
            );
        let flood = self.emit(
            year,
            EventKind::Flood {
                river: trunk.river,
                name: trunk.name.clone(),
                cause: Cause::Weather,
            },
        );
        // Towns along the lower half are flooded.
        let reach = 3.0 * self.ctx.width as f32 / 512.0 + 1.5;
        let lower = &course[course.len() / 2..];
        let flooded: Vec<u32> = (0..self.state.settlements.len() as u32)
            .filter(|&s| {
                let st = &self.state.settlements[s as usize];
                st.alive
                    && lower
                        .iter()
                        .any(|&c| self.cell_distance(c, st.cell) <= reach)
            })
            .collect();
        for s in flooded {
            let share = 0.03 + 0.1 * unit_f32(rng);
            self.strike(
                year,
                s,
                share,
                "swept away in a great flood",
                Cause::Event(flood),
            );
        }
        // A river jumps its banks once at most in this history.
        if self.ctx.avulsed.contains(&trunk.river) || unit_f32(rng) >= 0.3 {
            return;
        }
        let len = trunk.cells.len();
        let mut offsets = [len * 35 / 100, len / 2, len * 65 / 100];
        // Try the breach points in a seeded order.
        let r = (rng.next_u32() % 3) as usize;
        offsets.rotate_left(r);
        for at in offsets {
            let Some(path) = new_channel(
                &trunk,
                at,
                &self.ctx.filled,
                &self.ctx.water,
                self.ctx.width,
                self.ctx.height,
            ) else {
                continue;
            };
            self.avulse(year, &trunk, at, path, flood);
            return;
        }
    }

    fn cell_distance(&self, a: u32, b: u32) -> f32 {
        let w = self.ctx.width;
        let (a, b) = (a as usize, b as usize);
        let (dx, dy) = (
            (a % w) as f32 - (b % w) as f32,
            (a / w) as f32 - (b / w) as f32,
        );
        (dx * dx + dy * dy).sqrt()
    }

    fn avulse(&mut self, year: u32, trunk: &Trunk, at: usize, path: Vec<u32>, flood: u32) {
        let w = self.ctx.width;
        let centre = |c: u32| [(c as usize % w) as f32, (c as usize / w) as f32];
        let old: Vec<u32> = trunk.cells[at..].to_vec();
        let event = self.emit(
            year,
            EventKind::RiverAvulsion {
                river: trunk.river,
                segment: trunk.segment,
                at: at as u32,
                name: trunk.name.clone(),
                old_course: old.iter().map(|&c| centre(c)).collect(),
                new_course: path.iter().map(|&c| centre(c)).collect(),
                cause: Cause::Event(flood),
            },
        );
        self.ctx.avulsed.push(trunk.river);
        // Towns on the old bed and not on the new one are stranded.
        let reach = 3.0 * w as f32 / 512.0 + 1.5;
        let stranded: Vec<u32> = (0..self.state.settlements.len() as u32)
            .filter(|&s| {
                let st = &self.state.settlements[s as usize];
                st.alive
                    && old.iter().any(|&c| self.cell_distance(c, st.cell) <= reach)
                    && !path
                        .iter()
                        .any(|&c| self.cell_distance(c, st.cell) <= reach)
            })
            .collect();
        for s in stranded {
            self.emit(
                year,
                EventKind::SettlementStranded {
                    settlement: s,
                    cause: Cause::Event(event),
                },
            );
            // Without the river its land and trade support far fewer people.
            if let Some(c) = self.ctx.capacity.get_mut(s as usize) {
                *c *= 0.5;
            }
            if let Some(p) = self.ctx.ports.get_mut(s as usize) {
                *p = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::WorldConfig;
    use crate::history::EventKind;
    use crate::world::World;

    fn floody(seed: u64) -> World {
        let mut config = WorldConfig {
            seed,
            width: 192,
            height: 192,
            ..Default::default()
        };
        config.erosion.droplets = 6000;
        config.history.years = 1000;
        config.disasters.floods.per_century = 10.0;
        World::generate(&config)
    }

    #[test]
    fn avulsion_leaves_a_valid_drainage_network() {
        let mut seen = 0;
        for seed in [1, 2, 3, 42] {
            let world = floody(seed);
            let w = world.terrain.width;
            let filled = &world.hydrology.filled;
            for e in &world.history.events {
                let EventKind::RiverAvulsion {
                    old_course,
                    new_course,
                    ..
                } = &e.kind
                else {
                    continue;
                };
                seen += 1;
                let cell = |p: &[f32; 2]| p[1] as usize * w + p[0] as usize;
                // It leaves the old channel at the breach...
                assert_eq!(cell(&old_course[0]), cell(&new_course[0]));
                // ...each step is to a neighbouring cell...
                for pair in new_course.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    assert!((a[0] - b[0]).abs() <= 1.0 && (a[1] - b[1]).abs() <= 1.0);
                    assert_ne!(cell(&a), cell(&b));
                }
                // ...it reaches open water, lower than it started...
                let end = cell(new_course.last().unwrap());
                assert!(world.biomes.biome[end].is_water());
                assert!(filled[end] < filled[cell(&new_course[0])]);
                // ...and it climbs no more than a breached levee.
                let climb: f32 = new_course
                    .windows(2)
                    .map(|p| (filled[cell(&p[1])] - filled[cell(&p[0])]).max(0.0))
                    .sum();
                assert!(climb <= super::MAX_CLIMB, "new course climbs {climb}");
            }
        }
        assert!(seen > 0, "no river ever moved");
    }
}
