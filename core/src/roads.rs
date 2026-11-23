//! Stage: roads, river links and sea lanes between settlements, and the
//! transport network they form.
//!
//! Roads are found with A* over the travel-cost grid, nearest neighbours
//! first; cells already on a road are cheaper, so later roads join earlier
//! ones and routes share trunks. Navigable rivers link the settlements along
//! them and sea lanes link nearby ports. Shortest paths over the whole
//! network are precomputed for trade.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeSet, BinaryHeap};

use crate::biome::{Biome, Biomes};
use crate::config::WorldConfig;
use crate::hydrology::Hydrology;
use crate::settlements::Settlements;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkKind {
    Road,
    River,
    Sea,
}

/// A connection between two settlements.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// Settlement ids, lower first.
    pub a: u32,
    pub b: u32,
    pub kind: LinkKind,
    /// Cells from `a` to `b`.
    pub cells: Vec<u32>,
    /// Cost of moving goods along it.
    pub cost: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Roads {
    pub width: usize,
    pub height: usize,
    pub links: Vec<Link>,
    /// Per cell: on a road.
    pub road_cell: Vec<bool>,
    /// Per pair of year-0 settlements: cheapest network cost
    /// (`f32::INFINITY` when unconnected), row-major `n x n`.
    pub distance: Vec<f32>,
    /// Per pair: the next settlement on the cheapest route from the first to
    /// the second (`u32::MAX` when unconnected).
    pub next_hop: Vec<u32>,
    pub settlement_count: usize,
}

impl Roads {
    pub fn distance(&self, from: u32, to: u32) -> f32 {
        let n = self.settlement_count;
        if from as usize >= n || to as usize >= n {
            return f32::INFINITY;
        }
        self.distance[from as usize * n + to as usize]
    }

    /// Settlements along the cheapest route, both ends included.
    pub fn route(&self, from: u32, to: u32) -> Vec<u32> {
        let n = self.settlement_count;
        if !self.distance(from, to).is_finite() {
            return Vec::new();
        }
        let mut path = vec![from];
        let mut at = from;
        while at != to && path.len() <= n {
            at = self.next_hop[at as usize * n + to as usize];
            path.push(at);
        }
        path
    }

    /// The link joining two settlements directly, if any.
    pub fn link_between(&self, a: u32, b: u32) -> Option<&Link> {
        let (a, b) = (a.min(b), a.max(b));
        self.links.iter().find(|l| l.a == a && l.b == b)
    }
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

/// Runs the roads stage.
pub fn generate_roads(
    config: &WorldConfig,
    hydrology: &Hydrology,
    biomes: &Biomes,
    settlements: &Settlements,
    travel_cost: &[f32],
) -> Roads {
    let (w, h) = (biomes.width, biomes.height);
    let p = &config.roads;
    let reference = w as f32 / 512.0;
    let list = &settlements.settlements;
    let n = list.len();
    let pos = |s: u32| {
        let c = list[s as usize].cell;
        ((c % w) as f32, (c / w) as f32)
    };
    let dist = |a: u32, b: u32| {
        let ((ax, ay), (bx, by)) = (pos(a), pos(b));
        ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
    };

    let mut links = Vec::new();
    let mut road_cell = vec![false; w * h];

    // Roads: each settlement to its nearest neighbours, shortest pairs first.
    let max_link = p.max_link_distance * reference;
    let mut pairs = BTreeSet::new();
    for a in 0..n as u32 {
        let mut near: Vec<u32> = (0..n as u32)
            .filter(|&b| b != a && dist(a, b) <= max_link)
            .collect();
        near.sort_by(|&x, &y| dist(a, x).total_cmp(&dist(a, y)).then(x.cmp(&y)));
        for &b in near.iter().take(p.neighbours as usize) {
            pairs.insert((a.min(b), a.max(b)));
        }
    }
    let mut ordered: Vec<(u32, u32)> = pairs.into_iter().collect();
    ordered.sort_by(|x, y| dist(x.0, x.1).total_cmp(&dist(y.0, y.1)).then(x.cmp(y)));
    let min_cost = travel_cost
        .iter()
        .cloned()
        .fold(f32::INFINITY, f32::min)
        .max(1e-3)
        * p.road_factor;
    for (a, b) in ordered {
        let (from, to) = (list[a as usize].cell, list[b as usize].cell);
        let limit = dist(a, b) * 8.0 * travel_cost[from].max(1.0) + 1.0;
        let cost = |c: usize| travel_cost[c] * if road_cell[c] { p.road_factor } else { 1.0 };
        if let Some((cells, total)) = astar(w, h, from, to, &cost, min_cost, limit, &|_| true) {
            for &c in &cells {
                road_cell[c as usize] = true;
            }
            links.push(Link {
                a,
                b,
                kind: LinkKind::Road,
                cells,
                cost: total,
            });
        }
    }

    // River links: consecutive settlements along the same navigable river.
    let reach = 1usize.max(reference.round() as usize);
    for segment in &hydrology.segments {
        if segment.outflow() < p.navigable_discharge {
            continue;
        }
        let mut along: Vec<(usize, u32)> = Vec::new();
        for (k, point) in segment.points.iter().enumerate() {
            let (px, py) = (point[0] as usize, point[1] as usize);
            for s in list {
                let (sx, sy) = (s.cell % w, s.cell / w);
                if sx.abs_diff(px) <= reach
                    && sy.abs_diff(py) <= reach
                    && !along.iter().any(|&(_, id)| id == s.id)
                {
                    along.push((k, s.id));
                }
            }
        }
        along.sort_unstable();
        for pair in along.windows(2) {
            let ((ka, a), (kb, b)) = (pair[0], pair[1]);
            let cells: Vec<u32> = segment.points[ka..=kb]
                .iter()
                .map(|p| (p[1] as usize * w + p[0] as usize) as u32)
                .collect();
            let cost = p.river_cost * cells.len() as f32 / reference;
            let (a, b) = (a.min(b), a.max(b));
            if a != b {
                links.push(Link {
                    a,
                    b,
                    kind: LinkKind::River,
                    cells,
                    cost,
                });
            }
        }
    }

    // Sea lanes between nearby ports, over open water.
    let ports: Vec<u32> = list.iter().filter(|s| s.flags.port).map(|s| s.id).collect();
    let sea_range = p.sea_range * reference;
    let mut lanes = BTreeSet::new();
    for &a in &ports {
        let mut near: Vec<u32> = ports
            .iter()
            .copied()
            .filter(|&b| b != a && dist(a, b) <= sea_range)
            .collect();
        near.sort_by(|&x, &y| dist(a, x).total_cmp(&dist(a, y)).then(x.cmp(&y)));
        for &b in near.iter().take(2) {
            lanes.insert((a.min(b), a.max(b)));
        }
    }
    let water = |c: usize| biomes.biome[c] == Biome::Ocean;
    for (a, b) in lanes {
        let (Some(from), Some(to)) = (
            shore_cell(list[a as usize].cell, w, h, &water),
            shore_cell(list[b as usize].cell, w, h, &water),
        ) else {
            continue;
        };
        let limit = dist(a, b) * 3.0 + 4.0;
        if let Some((cells, length)) = astar(w, h, from, to, &|_| 1.0, 1.0, limit, &water) {
            let cost = p.sea_cost * length / reference;
            links.push(Link {
                a,
                b,
                kind: LinkKind::Sea,
                cells,
                cost,
            });
        }
    }
    links.sort_by_key(|x| (x.a, x.b, x.kind));

    let (distance, next_hop) = all_pairs(n, &links);
    Roads {
        width: w,
        height: h,
        links,
        road_cell,
        distance,
        next_hop,
        settlement_count: n,
    }
}

/// The first ocean cell next to `cell`, in a fixed neighbour order.
fn shore_cell(cell: usize, w: usize, h: usize, water: &impl Fn(usize) -> bool) -> Option<usize> {
    let (x, y) = (cell % w, cell / w);
    for (dx, dy) in [
        (1isize, 0isize),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
    ] {
        let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
            continue;
        };
        if nx < w && ny < h && water(ny * w + nx) {
            return Some(ny * w + nx);
        }
    }
    None
}

/// A* from `from` to `to` over 8-connected cells where `passable` (endpoints
/// always allowed), stepping cost = mean of the two cells' costs × length.
/// `min_cost` makes the straight-line heuristic admissible. Gives up past
/// `limit`. The queue orders by (estimate, cell), so ties resolve the same
/// way every time.
#[allow(clippy::too_many_arguments)]
fn astar(
    w: usize,
    h: usize,
    from: usize,
    to: usize,
    cost: &dyn Fn(usize) -> f32,
    min_cost: f32,
    limit: f32,
    passable: &dyn Fn(usize) -> bool,
) -> Option<(Vec<u32>, f32)> {
    let (tx, ty) = ((to % w) as f32, (to / w) as f32);
    let heuristic = |c: usize| {
        let (dx, dy) = ((c % w) as f32 - tx, (c / w) as f32 - ty);
        (dx * dx + dy * dy).sqrt() * min_cost
    };
    let mut best = std::collections::BTreeMap::new();
    let mut came = std::collections::BTreeMap::new();
    let mut queue = BinaryHeap::new();
    best.insert(from, 0.0f32);
    queue.push(Reverse((Key(heuristic(from)), from)));
    while let Some(Reverse((_, c))) = queue.pop() {
        let g = best[&c];
        if c == to {
            let mut path = vec![to as u32];
            let mut at = to;
            while let Some(&prev) = came.get(&at) {
                path.push(prev as u32);
                at = prev;
            }
            path.reverse();
            return Some((path, g));
        }
        let (x, y) = (c % w, c / w);
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
            if n != to && !passable(n) {
                continue;
            }
            let step = if dx != 0 && dy != 0 {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };
            let next = g + 0.5 * (cost(c) + cost(n)) * step;
            if next > limit {
                continue;
            }
            if best.get(&n).is_none_or(|&b| next < b) {
                best.insert(n, next);
                came.insert(n, c);
                queue.push(Reverse((Key(next + heuristic(n)), n)));
            }
        }
    }
    None
}

/// All-pairs cheapest costs and next hops over the settlement graph
/// (Dijkstra from every settlement; the graph is small).
fn all_pairs(n: usize, links: &[Link]) -> (Vec<f32>, Vec<u32>) {
    let mut adjacency: Vec<Vec<(u32, f32)>> = vec![Vec::new(); n];
    for l in links {
        adjacency[l.a as usize].push((l.b, l.cost));
        adjacency[l.b as usize].push((l.a, l.cost));
    }
    for list in &mut adjacency {
        list.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
    }
    let mut distance = vec![f32::INFINITY; n * n];
    let mut next_hop = vec![u32::MAX; n * n];
    for source in 0..n {
        let row = source * n;
        // first[v]: the first step on the best route from source to v.
        let mut first = vec![u32::MAX; n];
        let mut queue = BinaryHeap::new();
        distance[row + source] = 0.0;
        first[source] = source as u32;
        queue.push(Reverse((Key(0.0), source as u32)));
        while let Some(Reverse((Key(d), v))) = queue.pop() {
            if d > distance[row + v as usize] {
                continue;
            }
            for &(u, c) in &adjacency[v as usize] {
                let next = d + c;
                if next < distance[row + u as usize] {
                    distance[row + u as usize] = next;
                    first[u as usize] = if v as usize == source {
                        u
                    } else {
                        first[v as usize]
                    };
                    queue.push(Reverse((Key(next), u)));
                }
            }
        }
        next_hop[row..row + n].copy_from_slice(&first);
    }
    (distance, next_hop)
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
            World::generate(&small(5)).roads,
            World::generate(&small(5)).roads
        );
    }

    #[test]
    fn every_link_connects_two_settlements() {
        let world = World::generate(&small(42));
        let r = &world.roads;
        assert!(r.links.iter().any(|l| l.kind == LinkKind::Road));
        for l in &r.links {
            assert!(l.a < l.b, "{l:?}");
            assert!((l.b as usize) < world.settlements.settlements.len());
            assert!(!l.cells.is_empty() && l.cost.is_finite() && l.cost > 0.0);
            if l.kind == LinkKind::Road {
                let (sa, sb) = (
                    &world.settlements.settlements[l.a as usize],
                    &world.settlements.settlements[l.b as usize],
                );
                assert_eq!(l.cells.first(), Some(&(sa.cell as u32)));
                assert_eq!(l.cells.last(), Some(&(sb.cell as u32)));
                assert!(l.cells.windows(2).all(|p| {
                    let (a, b) = (p[0] as usize, p[1] as usize);
                    (a % r.width).abs_diff(b % r.width) <= 1
                        && (a / r.width).abs_diff(b / r.width) <= 1
                }));
            }
        }
    }

    #[test]
    fn network_distances_are_symmetric_and_routes_follow_links() {
        let world = World::generate(&small(42));
        let r = &world.roads;
        let n = r.settlement_count as u32;
        for a in 0..n {
            for b in 0..n {
                let (ab, ba) = (r.distance(a, b), r.distance(b, a));
                assert!((ab - ba).abs() < 1e-3 || (ab.is_infinite() && ba.is_infinite()));
                if a != b && ab.is_finite() {
                    let route = r.route(a, b);
                    assert_eq!((route[0], *route.last().unwrap()), (a, b));
                    assert!(route
                        .windows(2)
                        .all(|p| r.link_between(p[0], p[1]).is_some()));
                }
            }
        }
    }

    #[test]
    fn road_settings_leave_earlier_stages_alone() {
        let base = World::generate(&small(42));
        let mut config = small(42);
        config.roads.neighbours = 4;
        let changed = World::generate(&config);
        assert_eq!(base.settlements, changed.settlements);
        assert_eq!(base.resources, changed.resources);
        assert_ne!(base.roads, changed.roads);
    }
}
