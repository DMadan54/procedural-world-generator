//! Stage: tectonic plates.
//!
//! Scatters plate seeds, grows (domain-warped) Voronoi regions from them, gives
//! each plate a drift vector and an oceanic or continental type, then turns the
//! relative motion at each plate boundary into relief:
//!
//! - convergent boundaries raise mountain belts (or trenches on the subducting
//!   oceanic side), falling off smoothly with distance from the boundary;
//! - divergent boundaries open rifts on land and raise ridges in the ocean;
//! - transform boundaries barely change the terrain.

use std::collections::BTreeMap;

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand_chacha::rand_core::RngCore;

use crate::config::WorldConfig;
use crate::field::gaussian_blur;
use crate::rng::{entity_rng, stage, stage_rng, unit_f32};

/// Marks a pixel that is not on a plate boundary in [`TectonicMap::boundary_id`].
pub const NO_BOUNDARY: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlateKind {
    Oceanic,
    Continental,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plate {
    /// Seed point, in pixels.
    pub centre: [f32; 2],
    /// Direction and speed of motion; length is in `0.2..1.0`.
    pub drift: [f32; 2],
    pub kind: PlateKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryKind {
    /// Plates moving towards each other.
    Convergent,
    /// Plates moving apart.
    Divergent,
    /// Plates sliding past each other, or barely moving relative to each other.
    Transform,
}

/// The boundary between two neighbouring plates.
#[derive(Clone, Debug, PartialEq)]
pub struct Boundary {
    /// The two plate indices, lowest first.
    pub plates: [u16; 2],
    pub kind: BoundaryKind,
    /// Closing speed along the line between the plates. Positive means
    /// converging, negative means diverging.
    pub convergence: f32,
    /// Sliding speed across that line.
    pub shear: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TectonicMap {
    pub width: usize,
    pub height: usize,
    pub plates: Vec<Plate>,
    /// Per pixel: index into `plates`.
    pub plate_id: Vec<u16>,
    /// Sorted by plate pair.
    pub boundaries: Vec<Boundary>,
    /// Per pixel: index into `boundaries` if the pixel touches another plate,
    /// otherwise [`NO_BOUNDARY`].
    pub boundary_id: Vec<u16>,
    /// Tectonic base elevation, unnormalised. Roughly -1 (trenches) to 2
    /// (collision mountains); plate interiors sit near ±0.45.
    pub elevation: Vec<f32>,
}

/// Runs the tectonic stage. The same config always gives the same result.
pub fn generate_tectonics(config: &WorldConfig) -> TectonicMap {
    let (width, height, seed) = (config.width, config.height, config.seed);
    let params = &config.tectonics;
    let count = params.plate_count.clamp(1, NO_BOUNDARY as usize - 1);

    let plates = scatter_plates(seed, count, width, height, params.continental_fraction);
    // Own stream: the warp field is the same whatever the plate count.
    let warp_seed = stage_rng(seed, stage::TECTONIC_WARP).next_u32();
    let plate_id = voronoi(&plates, width, height, warp_seed, params.boundary_warp);
    let (boundaries, boundary_id) = find_boundaries(&plates, &plate_id, width, height);
    let elevation = elevation(&plates, &plate_id, &boundaries, &boundary_id, width, height);

    TectonicMap {
        width,
        height,
        plates,
        plate_id,
        boundaries,
        boundary_id,
        elevation,
    }
}

/// Places plates spread out across the map (best of several random
/// candidates each), then assigns types and drift.
fn scatter_plates(
    seed: u64,
    count: usize,
    width: usize,
    height: usize,
    continental_fraction: f32,
) -> Vec<Plate> {
    const CANDIDATES: usize = 12;
    let rng = &mut stage_rng(seed, stage::TECTONIC_PLATES);
    let (w, h) = (width as f32, height as f32);

    let mut centres: Vec<[f32; 2]> = Vec::with_capacity(count);
    for _ in 0..count {
        let mut best = [0.0, 0.0];
        let mut best_dist = f32::NEG_INFINITY;
        for _ in 0..CANDIDATES {
            let candidate = [unit_f32(rng) * w, unit_f32(rng) * h];
            let nearest = centres
                .iter()
                .map(|&c| dist2(c, candidate))
                .fold(f32::INFINITY, f32::min);
            if nearest > best_dist {
                best_dist = nearest;
                best = candidate;
            }
        }
        centres.push(best);
    }

    // Choose exactly round(count * fraction) continental plates (at least one
    // of each type when there are two or more plates).
    let mut order: Vec<usize> = (0..count).collect();
    for i in (1..count).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    let continental = ((count as f32 * continental_fraction).round() as usize)
        .clamp(1.min(count), count.saturating_sub(1).max(1));
    let mut kinds = vec![PlateKind::Oceanic; count];
    for &i in &order[..continental] {
        kinds[i] = PlateKind::Continental;
    }

    centres
        .into_iter()
        .zip(kinds)
        .enumerate()
        .map(|(i, (centre, kind))| {
            // Per-plate stream: plate i drifts the same way whatever else changes.
            let rng = &mut entity_rng(seed, stage::TECTONIC_DRIFT, i as u64);
            let angle = unit_f32(rng) * std::f32::consts::TAU;
            let speed = 0.2 + 0.8 * unit_f32(rng);
            Plate {
                centre,
                drift: [angle.cos() * speed, angle.sin() * speed],
                kind,
            }
        })
        .collect()
}

/// Nearest plate for each pixel, sampled at a noise-warped position so plate
/// edges wander instead of being straight lines.
fn voronoi(plates: &[Plate], width: usize, height: usize, warp_seed: u32, warp: f64) -> Vec<u16> {
    let fbm = Fbm::<Perlin>::new(warp_seed)
        .set_octaves(3)
        .set_frequency(3.0);
    let amplitude = warp * width as f64;
    let inv = 1.0 / width.max(1) as f64;

    let mut ids = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let (u, v) = (x as f64 * inv, y as f64 * inv);
            let px = (x as f64 + amplitude * fbm.get([u, v])) as f32;
            let py = (y as f64 + amplitude * fbm.get([u + 31.7, v + 47.3])) as f32;
            let mut best = 0;
            let mut best_dist = f32::INFINITY;
            for (i, plate) in plates.iter().enumerate() {
                let d = dist2(plate.centre, [px, py]);
                if d < best_dist {
                    best_dist = d;
                    best = i;
                }
            }
            ids.push(best as u16);
        }
    }
    ids
}

const NEIGHBOURS: [(isize, isize); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// The first neighbouring pixel (in a fixed order) on a different plate.
fn other_plate_nearby(plate_id: &[u16], width: usize, height: usize, x: usize, y: usize) -> Option<u16> {
    let own = plate_id[y * width + x];
    NEIGHBOURS.iter().find_map(|&(dx, dy)| {
        let nx = x.checked_add_signed(dx).filter(|&v| v < width)?;
        let ny = y.checked_add_signed(dy).filter(|&v| v < height)?;
        let other = plate_id[ny * width + nx];
        (other != own).then_some(other)
    })
}

fn find_boundaries(
    plates: &[Plate],
    plate_id: &[u16],
    width: usize,
    height: usize,
) -> (Vec<Boundary>, Vec<u16>) {
    // Collect neighbouring pairs first so boundaries are numbered in a fixed,
    // sorted order.
    let mut pairs: BTreeMap<(u16, u16), u16> = BTreeMap::new();
    let mut touching = vec![None; width * height];
    for y in 0..height {
        for x in 0..width {
            if let Some(other) = other_plate_nearby(plate_id, width, height, x, y) {
                let own = plate_id[y * width + x];
                let pair = (own.min(other), own.max(other));
                pairs.insert(pair, 0);
                touching[y * width + x] = Some(pair);
            }
        }
    }

    let mut boundaries = Vec::with_capacity(pairs.len());
    for (index, (&(a, b), slot)) in pairs.iter_mut().enumerate() {
        *slot = index as u16;
        let (kind, convergence, shear) = classify(&plates[a as usize], &plates[b as usize]);
        boundaries.push(Boundary {
            plates: [a, b],
            kind,
            convergence,
            shear,
        });
    }

    let boundary_id = touching
        .into_iter()
        .map(|pair| pair.map_or(NO_BOUNDARY, |p| pairs[&p]))
        .collect();
    (boundaries, boundary_id)
}

/// Classifies the boundary between plates `a` and `b` from their relative
/// motion along and across the line joining their centres.
/// Returns the kind, the convergence (positive when closing) and the shear.
pub fn classify(a: &Plate, b: &Plate) -> (BoundaryKind, f32, f32) {
    let normal = normalised(sub(b.centre, a.centre));
    let relative = sub(a.drift, b.drift);
    let convergence = dot(relative, normal);
    let shear = (relative[0] * normal[1] - relative[1] * normal[0]).abs();

    let kind = if convergence.abs() < 0.1 || convergence.abs() < 0.5 * shear {
        BoundaryKind::Transform
    } else if convergence > 0.0 {
        BoundaryKind::Convergent
    } else {
        BoundaryKind::Divergent
    };
    (kind, convergence, shear)
}

fn elevation(
    plates: &[Plate],
    plate_id: &[u16],
    boundaries: &[Boundary],
    boundary_id: &[u16],
    width: usize,
    height: usize,
) -> Vec<f32> {
    let map_width = width as f32;

    // Plate interiors: continents up, ocean floor down, blurred so coasts
    // become sloping shelves instead of cliffs.
    let mut base: Vec<f32> = plate_id
        .iter()
        .map(|&p| match plates[p as usize].kind {
            PlateKind::Continental => 0.45,
            PlateKind::Oceanic => -0.45,
        })
        .collect();
    gaussian_blur(&mut base, width, height, 0.015 * map_width, false);

    // Boundary stress: each boundary pixel carries the uplift (or
    // depression) its side of the boundary gets, then the field is blurred so
    // it falls off smoothly with distance. Two bands: broad mountain belts and
    // narrow trenches, ridges, arcs and rifts.
    let mut wide = vec![0.0; base.len()];
    let mut narrow = vec![0.0; base.len()];
    for (i, &b) in boundary_id.iter().enumerate() {
        let Some(boundary) = boundaries.get(b as usize) else {
            continue;
        };
        let own = plate_id[i];
        let other = if boundary.plates[0] == own {
            boundary.plates[1]
        } else {
            boundary.plates[0]
        };
        let stress = boundary_stress(
            boundary.kind,
            boundary.convergence,
            plates[own as usize].kind,
            plates[other as usize].kind,
        );
        wide[i] = stress.wide;
        narrow[i] = stress.narrow;
    }
    gaussian_blur(&mut wide, width, height, WIDE_SIGMA * map_width, true);
    gaussian_blur(&mut narrow, width, height, NARROW_SIGMA * map_width, true);

    base.iter()
        .zip(&wide)
        .zip(&narrow)
        .map(|((b, w), n)| b + w + n)
        .collect()
}

/// Falloff of mountain belts, as a fraction of map width.
const WIDE_SIGMA: f32 = 0.035;
/// Falloff of trenches, ridges, island arcs and rifts.
const NARROW_SIGMA: f32 = 0.012;

/// Peak relief a boundary contributes on one side, split into a broad band
/// (mountain belts) and a narrow one (trenches, ridges, arcs, rifts).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stress {
    pub wide: f32,
    pub narrow: f32,
}

/// The relief at a boundary, on a plate of type `own` facing a plate of type
/// `other`.
pub fn boundary_stress(
    kind: BoundaryKind,
    convergence: f32,
    own: PlateKind,
    other: PlateKind,
) -> Stress {
    use PlateKind::{Continental, Oceanic};
    let wide = |v: f32| Stress { wide: v, narrow: 0.0 };
    let narrow = |v: f32| Stress { wide: 0.0, narrow: v };
    match kind {
        BoundaryKind::Convergent => {
            let s = convergence.clamp(0.0, 1.5);
            match (own, other) {
                // Continental collision: broad, high ranges.
                (Continental, Continental) => wide(1.0 * s),
                // Subduction under a continent: coastal ranges on land...
                (Continental, Oceanic) => wide(0.8 * s),
                // ...and a trench on the ocean side.
                (Oceanic, Continental) => narrow(-0.6 * s),
                // Ocean-ocean subduction: island arcs.
                (Oceanic, Oceanic) => narrow(0.5 * s),
            }
        }
        BoundaryKind::Divergent => {
            let s = (-convergence).clamp(0.0, 1.5);
            match (own, other) {
                // Mid-ocean ridge.
                (Oceanic, Oceanic) => narrow(0.3 * s),
                // Rift valley where a continent is pulled apart.
                _ => narrow(-0.45 * s),
            }
        }
        BoundaryKind::Transform => narrow(0.04),
    }
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

fn dist2(a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = sub(a, b);
    dot(d, d)
}

fn normalised(v: [f32; 2]) -> [f32; 2] {
    let len = dot(v, v).sqrt();
    if len <= f32::EPSILON {
        [0.0, 0.0]
    } else {
        [v[0] / len, v[1] / len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MAX_PLATES, MIN_PLATES};

    const W: usize = 96;
    const H: usize = 80;

    fn config(seed: u64, plate_count: usize) -> WorldConfig {
        let mut config = WorldConfig {
            seed,
            width: W,
            height: H,
            ..Default::default()
        };
        config.tectonics.plate_count = plate_count;
        config
    }

    fn plate(centre: [f32; 2], drift: [f32; 2]) -> Plate {
        Plate {
            centre,
            drift,
            kind: PlateKind::Continental,
        }
    }

    #[test]
    fn deterministic() {
        let a = generate_tectonics(&config(5, 12));
        let b = generate_tectonics(&config(5, 12));
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_differ() {
        let a = generate_tectonics(&config(5, 12));
        let b = generate_tectonics(&config(6, 12));
        assert_ne!(a.plate_id, b.plate_id);
    }

    #[test]
    fn makes_the_requested_plates() {
        for n in [MIN_PLATES, 20, MAX_PLATES] {
            let map = generate_tectonics(&config(1, n));
            assert_eq!(map.plates.len(), n);
            assert!(map.plate_id.iter().all(|&p| (p as usize) < n));
            assert_eq!(map.plate_id.len(), W * H);
            assert_eq!(map.elevation.len(), W * H);
        }
    }

    #[test]
    fn has_both_plate_kinds() {
        let map = generate_tectonics(&config(1, MIN_PLATES));
        assert!(map.plates.iter().any(|p| p.kind == PlateKind::Continental));
        assert!(map.plates.iter().any(|p| p.kind == PlateKind::Oceanic));
    }

    #[test]
    fn boundaries_join_the_plates_they_sit_between() {
        let map = generate_tectonics(&config(2, 10));
        assert!(!map.boundaries.is_empty());
        for (i, &b) in map.boundary_id.iter().enumerate() {
            if b == NO_BOUNDARY {
                continue;
            }
            let boundary = &map.boundaries[b as usize];
            assert_ne!(boundary.plates[0], boundary.plates[1]);
            assert!(boundary.plates.contains(&map.plate_id[i]));
        }
    }

    #[test]
    fn plate_drift_is_per_plate() {
        // Adding plates moves the seeds, but plate i keeps its own drift.
        let a = generate_tectonics(&config(4, 10));
        let b = generate_tectonics(&config(4, 20));
        for i in 0..10 {
            assert_eq!(a.plates[i].drift, b.plates[i].drift);
        }
    }

    #[test]
    fn unaffected_by_other_stages_settings() {
        let mut other = config(4, 12);
        other.noise.octaves = 2;
        other.blend.tectonic_weight = 0.2;
        assert_eq!(generate_tectonics(&config(4, 12)), generate_tectonics(&other));
    }

    #[test]
    fn head_on_plates_converge() {
        let a = plate([0.0, 0.0], [1.0, 0.0]);
        let b = plate([10.0, 0.0], [-1.0, 0.0]);
        let (kind, convergence, _) = classify(&a, &b);
        assert_eq!(kind, BoundaryKind::Convergent);
        assert!(convergence > 0.0);
    }

    #[test]
    fn separating_plates_diverge() {
        let a = plate([0.0, 0.0], [-1.0, 0.0]);
        let b = plate([10.0, 0.0], [1.0, 0.0]);
        let (kind, convergence, _) = classify(&a, &b);
        assert_eq!(kind, BoundaryKind::Divergent);
        assert!(convergence < 0.0);
    }

    #[test]
    fn sliding_plates_are_transform() {
        let a = plate([0.0, 0.0], [0.0, 1.0]);
        let b = plate([10.0, 0.0], [0.0, -1.0]);
        assert_eq!(classify(&a, &b).0, BoundaryKind::Transform);
    }

    #[test]
    fn plates_moving_together_are_transform() {
        let a = plate([0.0, 0.0], [0.5, 0.5]);
        let b = plate([10.0, 0.0], [0.5, 0.5]);
        assert_eq!(classify(&a, &b).0, BoundaryKind::Transform);
    }

    #[test]
    fn convergent_continents_raise_broad_mountains() {
        use PlateKind::Continental as C;
        let s = boundary_stress(BoundaryKind::Convergent, 1.0, C, C);
        assert!(s.wide > 0.5);
    }

    #[test]
    fn subduction_makes_a_trench_on_the_ocean_side() {
        use PlateKind::{Continental as C, Oceanic as O};
        assert!(boundary_stress(BoundaryKind::Convergent, 1.0, O, C).narrow < 0.0);
        assert!(boundary_stress(BoundaryKind::Convergent, 1.0, C, O).wide > 0.0);
    }

    #[test]
    fn divergence_makes_ridges_at_sea_and_rifts_on_land() {
        use PlateKind::{Continental as C, Oceanic as O};
        assert!(boundary_stress(BoundaryKind::Divergent, -1.0, O, O).narrow > 0.0);
        assert!(boundary_stress(BoundaryKind::Divergent, -1.0, C, C).narrow < 0.0);
    }

    #[test]
    fn transform_boundaries_do_little() {
        use PlateKind::Continental as C;
        let t = boundary_stress(BoundaryKind::Transform, 0.0, C, C);
        let c = boundary_stress(BoundaryKind::Convergent, 1.0, C, C);
        assert!(t.wide.abs() + t.narrow.abs() < 0.1 * c.wide);
    }

}
