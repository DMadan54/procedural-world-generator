//! Stage: hydrology. Lakes, drainage and river networks.
//!
//! 1. Priority-flood (with a small epsilon) fills every depression so each
//!    land cell has a downhill path to the sea or the map edge. Depressions
//!    deeper than a hair become lakes with a flat surface.
//! 2. D8: each land cell drains to its steepest-descent neighbour on the
//!    filled surface.
//! 3. Flow accumulation: each cell's runoff (its precipitation) is passed
//!    downstream, highest cells first.
//! 4. Cells whose accumulated discharge passes a threshold become river
//!    cells, which are traced into connected polylines.
//!
//! Discharge is relative: 1.0 is the yearly runoff of one cell receiving
//! 1000 mm of precipitation.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use crate::climate::Climate;
use crate::config::WorldConfig;
use crate::heightmap::Heightmap;

pub const NO_LAKE: u32 = u32::MAX;
pub const NO_SEGMENT: u32 = u32::MAX;
/// Marks a cell with no downstream neighbour (sea, or flowing off the map).
pub const NO_FLOW: u8 = u8::MAX;

/// D8 neighbour offsets, indexed by flow direction.
pub const D8: [(isize, isize); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Lake {
    pub id: u32,
    /// Water surface, in normalised height units.
    pub surface: f32,
    pub cells: u32,
    /// Where the lake spills out, if it overflows into a river or the land
    /// rather than straight into the sea.
    pub outlet: Option<[usize; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiverStart {
    /// A headwater where accumulated flow first passes the threshold.
    Source,
    /// Where two or more segments meet.
    Junction,
    /// The outflow of a lake.
    Lake(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiverEnd {
    Ocean,
    Lake(u32),
    /// Flows into another segment, by index into `Hydrology::segments`.
    Junction(u32),
    /// Leaves the map.
    MapEdge,
}

/// A stretch of river between a source, junction or lake outlet and the next
/// junction, lake, coast or map edge.
#[derive(Clone, Debug, PartialEq)]
pub struct RiverSegment {
    /// Which river (network) this belongs to, by index into `Hydrology::rivers`.
    pub river: u32,
    /// Cell centres in pixel coordinates, upstream to downstream. A segment
    /// that ends in a junction shares its last point with the next segment's
    /// first; one that reaches the sea or a lake ends on that water cell.
    pub points: Vec<[f32; 2]>,
    /// Discharge at each point.
    pub discharge: Vec<f32>,
    pub start: RiverStart,
    pub end: RiverEnd,
    /// Segments flowing into this one's first point.
    pub upstream: Vec<u32>,
}

impl RiverSegment {
    /// Discharge where the segment ends (its largest).
    pub fn outflow(&self) -> f32 {
        self.discharge.last().copied().unwrap_or(0.0)
    }
}

/// A whole river network: every segment draining to one mouth.
#[derive(Clone, Debug, PartialEq)]
pub struct River {
    pub id: u32,
    /// Indices into `Hydrology::segments`, sorted.
    pub segments: Vec<u32>,
    /// The segment that reaches the mouth.
    pub trunk: u32,
    pub mouth: [f32; 2],
    pub end: RiverEnd,
    pub discharge: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hydrology {
    pub width: usize,
    pub height: usize,
    /// Depression-filled surface used for routing (normalised heights).
    pub filled: Vec<f32>,
    /// Per cell: index into [`D8`], or [`NO_FLOW`].
    pub flow_direction: Vec<u8>,
    /// Per cell: discharge flowing out of it (including its own runoff).
    pub accumulation: Vec<f32>,
    /// Per cell: index into `lakes`, or [`NO_LAKE`].
    pub lake_id: Vec<u32>,
    pub lakes: Vec<Lake>,
    pub segments: Vec<RiverSegment>,
    pub rivers: Vec<River>,
    /// Per cell: the segment running through it, or [`NO_SEGMENT`].
    pub segment_id: Vec<u32>,
}

impl Hydrology {
    /// The river segment through a cell, if any.
    pub fn segment_at(&self, x: usize, y: usize) -> Option<&RiverSegment> {
        self.segments.get(self.segment_id[y * self.width + x] as usize)
    }

    /// The lake covering a cell, if any.
    pub fn lake_at(&self, x: usize, y: usize) -> Option<&Lake> {
        self.lakes.get(self.lake_id[y * self.width + x] as usize)
    }

    /// The cell a cell drains into, if it drains anywhere on the map.
    pub fn downstream(&self, x: usize, y: usize) -> Option<[usize; 2]> {
        let d = self.flow_direction[y * self.width + x];
        let (dx, dy) = *D8.get(d as usize)?;
        Some([x.checked_add_signed(dx)?, y.checked_add_signed(dy)?])
    }
}

/// f32 with a total order, for the priority queue.
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

/// Runs the hydrology stage over the final terrain and climate.
pub fn generate_hydrology(config: &WorldConfig, terrain: &Heightmap, climate: &Climate) -> Hydrology {
    let (w, h) = (terrain.width, terrain.height);
    let sea = config.terrain.sea_level;
    let params = &config.hydrology;
    let heights = &terrain.data;
    let ocean: Vec<bool> = heights.iter().map(|&v| v < sea).collect();
    let neighbours = |i: usize| {
        let (x, y) = (i % w, i / w);
        D8.iter().enumerate().filter_map(move |(d, &(dx, dy))| {
            let nx = x.checked_add_signed(dx).filter(|&v| v < w)?;
            let ny = y.checked_add_signed(dy).filter(|&v| v < h)?;
            Some((d, ny * w + nx))
        })
    };

    // 1. Priority-flood from the sea and the map edge. `filled` gets an
    // epsilon slope so flats drain; `level` is the flat water surface.
    let mut filled = heights.clone();
    let mut level = heights.clone();
    let mut done = vec![false; w * h];
    let mut queue = BinaryHeap::new();
    for i in 0..w * h {
        let (x, y) = (i % w, i / w);
        if ocean[i] || x == 0 || y == 0 || x == w - 1 || y == h - 1 {
            done[i] = true;
            queue.push(Reverse((Key(filled[i]), i)));
        }
    }
    while let Some(Reverse((_, c))) = queue.pop() {
        for (_, n) in neighbours(c) {
            if done[n] {
                continue;
            }
            done[n] = true;
            filled[n] = heights[n].max(filled[c] + params.fill_epsilon);
            level[n] = heights[n].max(level[c]);
            queue.push(Reverse((Key(filled[n]), n)));
        }
    }

    // 2. D8 steepest descent on the filled surface. Sea cells are sinks.
    let mut flow_direction = vec![NO_FLOW; w * h];
    for i in 0..w * h {
        if ocean[i] {
            continue;
        }
        let mut best = (0.0f32, NO_FLOW);
        for (d, n) in neighbours(i) {
            let distance = if d % 2 == 0 { 1.0 } else { std::f32::consts::SQRT_2 };
            let slope = (filled[i] - filled[n]) / distance;
            if slope > best.0 {
                best = (slope, d as u8);
            }
        }
        flow_direction[i] = best.1;
    }
    let downstream = |i: usize| -> Option<usize> {
        let (dx, dy) = *D8.get(flow_direction[i] as usize)?;
        let x = (i % w).checked_add_signed(dx)?;
        let y = (i / w).checked_add_signed(dy)?;
        Some(y * w + x)
    };

    // 3. Flow accumulation, highest filled cells first.
    let mut order: Vec<usize> = (0..w * h).filter(|&i| !ocean[i]).collect();
    order.sort_by(|&a, &b| filled[b].total_cmp(&filled[a]).then(a.cmp(&b)));
    let mut accumulation: Vec<f32> = (0..w * h)
        .map(|i| if ocean[i] { 0.0 } else { climate.precipitation[i].max(0.0) / 1000.0 })
        .collect();
    for &i in &order {
        if let Some(n) = downstream(i) {
            if !ocean[n] {
                accumulation[n] += accumulation[i];
            }
        }
    }

    // Lakes: connected cells whose water surface stands above the ground.
    let (lake_id, lakes) = find_lakes(w, h, heights, &level, &ocean, params.min_lake_cells, &downstream, &accumulation);

    // 4. Rivers.
    let river_cell: Vec<bool> = (0..w * h)
        .map(|i| !ocean[i] && lake_id[i] == NO_LAKE && accumulation[i] >= params.river_threshold)
        .collect();
    let (segments, rivers, segment_id) = trace_rivers(w, h, &river_cell, &ocean, &lake_id, &accumulation, &downstream);

    Hydrology {
        width: w,
        height: h,
        filled,
        flow_direction,
        accumulation,
        lake_id,
        lakes,
        segments,
        rivers,
        segment_id,
    }
}

#[allow(clippy::too_many_arguments)]
fn find_lakes(
    w: usize,
    h: usize,
    heights: &[f32],
    level: &[f32],
    ocean: &[bool],
    min_cells: u32,
    downstream: &impl Fn(usize) -> Option<usize>,
    accumulation: &[f32],
) -> (Vec<u32>, Vec<Lake>) {
    const MIN_DEPTH: f32 = 1e-4;
    let wet: Vec<bool> = (0..w * h)
        .map(|i| !ocean[i] && level[i] - heights[i] > MIN_DEPTH)
        .collect();
    let mut lake_id = vec![NO_LAKE; w * h];
    let mut seen = vec![false; w * h];
    let mut lakes = Vec::new();
    for start in 0..w * h {
        if !wet[start] || seen[start] {
            continue;
        }
        // Flood-fill one component (4-connected) in index order.
        let mut cells = vec![start];
        seen[start] = true;
        let mut k = 0;
        while k < cells.len() {
            let c = cells[k];
            k += 1;
            let (x, y) = (c % w, c / w);
            for (dx, dy) in [(1isize, 0isize), (-1, 0), (0, 1), (0, -1)] {
                let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
                    continue;
                };
                if nx >= w || ny >= h {
                    continue;
                }
                let n = ny * w + nx;
                if wet[n] && !seen[n] {
                    seen[n] = true;
                    cells.push(n);
                }
            }
        }
        if (cells.len() as u32) < min_cells {
            continue;
        }
        let id = lakes.len() as u32;
        cells.sort_unstable();
        for &c in &cells {
            lake_id[c] = id;
        }
        // The outlet: where the most water leaves the lake onto land.
        let outlet = cells
            .iter()
            .filter_map(|&c| downstream(c).map(|n| (c, n)))
            .filter(|&(_, n)| lake_id[n] != id && !ocean[n])
            .max_by(|a, b| accumulation[a.0].total_cmp(&accumulation[b.0]).then(b.1.cmp(&a.1)))
            .map(|(_, n)| [n % w, n / w]);
        lakes.push(Lake {
            id,
            surface: level[cells[0]],
            cells: cells.len() as u32,
            outlet,
        });
    }
    (lake_id, lakes)
}

type Traced = (Vec<RiverSegment>, Vec<River>, Vec<u32>);

fn trace_rivers(
    w: usize,
    h: usize,
    river_cell: &[bool],
    ocean: &[bool],
    lake_id: &[u32],
    accumulation: &[f32],
    downstream: &impl Fn(usize) -> Option<usize>,
) -> Traced {
    let centre = |i: usize| [(i % w) as f32 + 0.5, (i / w) as f32 + 0.5];

    // How many river cells flow into each cell, and whether a lake does.
    let mut inflow = vec![0u8; w * h];
    let mut lake_inflow = vec![NO_LAKE; w * h];
    for i in 0..w * h {
        if let Some(n) = downstream(i) {
            if river_cell[i] {
                inflow[n] = inflow[n].saturating_add(1);
            } else if lake_id[i] != NO_LAKE && lake_inflow[n] == NO_LAKE {
                lake_inflow[n] = lake_id[i];
            }
        }
    }

    // Segment heads: river cells that are not simply the continuation of
    // exactly one upstream river cell.
    let mut head_segment = vec![NO_SEGMENT; w * h];
    let mut segment_id = vec![NO_SEGMENT; w * h];
    let mut segments = Vec::new();
    let mut end_cells = Vec::new();
    for head in 0..w * h {
        if !river_cell[head] || inflow[head] == 1 {
            continue;
        }
        let index = segments.len() as u32;
        head_segment[head] = index;
        let start = if inflow[head] >= 2 {
            RiverStart::Junction
        } else if lake_inflow[head] != NO_LAKE {
            RiverStart::Lake(lake_inflow[head])
        } else {
            RiverStart::Source
        };
        let mut points = vec![centre(head)];
        let mut discharge = vec![accumulation[head]];
        segment_id[head] = index;
        let mut cell = head;
        let end = loop {
            let Some(next) = downstream(cell) else {
                break (RiverEnd::MapEdge, None);
            };
            if ocean[next] {
                points.push(centre(next));
                discharge.push(accumulation[cell]);
                break (RiverEnd::Ocean, None);
            }
            if lake_id[next] != NO_LAKE {
                points.push(centre(next));
                discharge.push(accumulation[cell]);
                break (RiverEnd::Lake(lake_id[next]), None);
            }
            points.push(centre(next));
            discharge.push(accumulation[next]);
            if inflow[next] != 1 {
                // A junction: it heads the next segment. Resolved below.
                break (RiverEnd::MapEdge, Some(next));
            }
            segment_id[next] = index;
            cell = next;
        };
        end_cells.push(end.1);
        segments.push(RiverSegment {
            river: 0,
            points,
            discharge,
            start,
            end: end.0,
            upstream: Vec::new(),
        });
    }

    // Resolve junction ends and upstream links.
    for (index, junction) in end_cells.iter().enumerate() {
        if let Some(cell) = junction {
            let next = head_segment[*cell];
            segments[index].end = RiverEnd::Junction(next);
            segments[next as usize].upstream.push(index as u32);
        }
    }

    // Group into rivers by the trunk segment each one drains through.
    let trunk_of = |mut s: usize| {
        while let RiverEnd::Junction(next) = segments[s].end {
            s = next as usize;
        }
        s
    };
    let trunks: Vec<usize> = (0..segments.len()).map(trunk_of).collect();
    let mut river_of_trunk = std::collections::BTreeMap::new();
    let mut rivers: Vec<River> = Vec::new();
    for (s, &t) in trunks.iter().enumerate() {
        let id = *river_of_trunk.entry(t).or_insert_with(|| {
            let trunk = &segments[t];
            rivers.push(River {
                id: rivers.len() as u32,
                segments: Vec::new(),
                trunk: t as u32,
                mouth: *trunk.points.last().expect("segments have points"),
                end: trunk.end,
                discharge: trunk.outflow(),
            });
            rivers.len() as u32 - 1
        });
        rivers[id as usize].segments.push(s as u32);
    }
    for (s, &t) in trunks.iter().enumerate() {
        segments[s].river = river_of_trunk[&t];
    }
    (segments, rivers, segment_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: usize = 64;

    fn config(threshold: f32) -> WorldConfig {
        let mut config = WorldConfig {
            width: N,
            height: N,
            ..Default::default()
        };
        config.terrain.sea_level = 0.2;
        config.hydrology.river_threshold = threshold;
        config
    }

    fn uniform_rain(mm: f32) -> Climate {
        Climate {
            width: N,
            height: N,
            temperature: vec![15.0; N * N],
            precipitation: vec![mm; N * N],
            wind: vec![[1.0, 0.0]; N * N],
        }
    }

    /// Land sloping down to the sea along the bottom edge, with a few
    /// valleys and a closed bowl in the middle.
    fn landscape() -> Heightmap {
        let mut data = vec![0.0; N * N];
        for y in 0..N {
            for x in 0..N {
                let slope = 0.9 - 0.8 * y as f32 / (N - 1) as f32;
                let valleys = 0.03 * ((x as f32 / N as f32) * std::f32::consts::TAU * 3.0).cos();
                let (dx, dy) = (x as f32 - 32.0, y as f32 - 24.0);
                let r = (dx * dx + dy * dy).sqrt();
                let bowl = if r < 6.0 { -0.25 * (1.0 - r / 6.0) } else { 0.0 };
                data[y * N + x] = (slope + valleys + bowl).clamp(0.0, 1.0);
            }
        }
        Heightmap {
            width: N,
            height: N,
            data,
        }
    }

    fn run(threshold: f32) -> Hydrology {
        generate_hydrology(&config(threshold), &landscape(), &uniform_rain(1000.0))
    }

    #[test]
    fn deterministic() {
        assert_eq!(run(20.0), run(20.0));
    }

    #[test]
    fn every_land_cell_drains_to_the_sea_or_edge() {
        let hy = run(20.0);
        for start in 0..N * N {
            let (mut x, mut y) = (start % N, start / N);
            for step in 0.. {
                assert!(step <= N * N, "cycle from cell {start}");
                match hy.downstream(x, y) {
                    Some([nx, ny]) => (x, y) = (nx, ny),
                    None => break,
                }
            }
        }
    }

    #[test]
    fn filled_surface_never_lies_below_the_ground() {
        let map = landscape();
        let hy = run(20.0);
        assert!(hy.filled.iter().zip(&map.data).all(|(f, h)| f >= h));
    }

    #[test]
    fn the_bowl_becomes_a_lake_with_an_outlet() {
        let hy = run(20.0);
        let lake = hy.lake_at(32, 24).expect("bowl centre should be a lake");
        assert!(lake.cells > 10);
        assert!(lake.outlet.is_some());
        assert!(hy.lake_at(5, 5).is_none());
    }

    #[test]
    fn accumulation_is_conserved() {
        let map = landscape();
        let hy = run(20.0);
        let sea = config(20.0).terrain.sea_level;
        // Runoff of all land cells must arrive at a sink: either a land cell
        // flowing into the sea or off the map.
        let land_runoff = map.data.iter().filter(|&&v| v >= sea).count() as f32;
        let mut reaching_sinks = 0.0;
        for y in 0..N {
            for x in 0..N {
                let i = y * N + x;
                if map.data[i] < sea {
                    continue;
                }
                let sink = match hy.downstream(x, y) {
                    None => true,
                    Some([nx, ny]) => map.data[ny * N + nx] < sea,
                };
                if sink {
                    reaching_sinks += hy.accumulation[i];
                }
            }
        }
        assert!((reaching_sinks - land_runoff).abs() < 1e-2 * land_runoff);
    }

    #[test]
    fn segments_connect_and_discharge_grows_downstream() {
        let hy = run(20.0);
        assert!(!hy.segments.is_empty());
        for (i, s) in hy.segments.iter().enumerate() {
            assert_eq!(s.points.len(), s.discharge.len());
            assert!(s.discharge.windows(2).all(|d| d[1] >= d[0] - 1e-4), "segment {i}");
            if let RiverEnd::Junction(next) = s.end {
                let next = &hy.segments[next as usize];
                assert_eq!(s.points.last(), next.points.first());
                assert!(next.upstream.contains(&(i as u32)));
                assert!(next.outflow() >= s.outflow());
                assert_eq!(next.river, s.river);
            }
        }
        for r in &hy.rivers {
            assert!(r.segments.contains(&r.trunk));
            assert!(!matches!(r.end, RiverEnd::Junction(_)));
        }
    }

    #[test]
    fn segment_lookup_matches_points() {
        let hy = run(20.0);
        let s = &hy.segments[0];
        let [x, y] = s.points[0];
        assert_eq!(hy.segment_at(x as usize, y as usize), Some(s));
    }

    #[test]
    fn higher_threshold_means_fewer_rivers() {
        let count = |hy: &Hydrology| hy.segment_id.iter().filter(|&&s| s != NO_SEGMENT).count();
        assert!(count(&run(10.0)) > count(&run(60.0)));
    }

    #[test]
    fn drier_land_means_less_discharge() {
        let map = landscape();
        let wet = generate_hydrology(&config(20.0), &map, &uniform_rain(2000.0));
        let dry = generate_hydrology(&config(20.0), &map, &uniform_rain(500.0));
        let total = |hy: &Hydrology| hy.rivers.iter().map(|r| r.discharge).sum::<f32>();
        assert!(total(&wet) > total(&dry));
    }
}
