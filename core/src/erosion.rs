//! Stage: droplet-based hydraulic erosion.
//!
//! Each droplet starts at a random point, rolls downhill following the
//! bilinear-interpolated gradient (with some inertia), picks up sediment in
//! proportion to slope, speed and water up to its carrying capacity, drops
//! sediment when over capacity or climbing, and evaporates as it goes.
//!
//! Droplets run in fixed chunks of [`CHUNK_DROPLETS`], [`CHUNKS_PER_ROUND`]
//! chunks per round. Within a round every chunk simulates against its own
//! copy of the map as it was at the start of the round, then the changes are
//! merged in chunk order. So the result is the same whether chunks run one
//! after another or in parallel (the `parallel` feature), on any number of
//! threads. The work is split into rounds so callers can run it a little at a
//! time and show progress.

use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::config::{ErosionConfig, WorldConfig};
use crate::heightmap::Heightmap;
use crate::rng::{stage, stage_rng, unit_f32};

/// Droplets per chunk. Part of the world format: changing it changes output.
pub const CHUNK_DROPLETS: u32 = 512;
/// Chunks simulated against the same snapshot of the map. Part of the world
/// format.
pub const CHUNKS_PER_ROUND: usize = 8;

/// An erosion run in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct ErosionJob {
    map: Heightmap,
    params: ErosionConfig,
    brush: Vec<(isize, isize, f32)>,
    /// One RNG seed per chunk, drawn up front from the erosion stage RNG.
    chunk_seeds: Vec<u64>,
    next_chunk: usize,
}

impl ErosionJob {
    pub fn new(config: &WorldConfig, input: &Heightmap) -> Self {
        let params = config.erosion.clone();
        let chunks = params.droplets.div_ceil(CHUNK_DROPLETS) as usize;
        let mut rng = stage_rng(config.seed, stage::EROSION);
        let chunk_seeds = (0..chunks).map(|_| rng.next_u64()).collect();
        Self {
            map: input.clone(),
            brush: brush(params.brush_radius),
            params,
            chunk_seeds,
            next_chunk: 0,
        }
    }

    /// Fraction of droplets simulated so far, `0.0..=1.0`.
    pub fn progress(&self) -> f32 {
        if self.chunk_seeds.is_empty() {
            1.0
        } else {
            self.next_chunk as f32 / self.chunk_seeds.len() as f32
        }
    }

    pub fn is_done(&self) -> bool {
        self.next_chunk >= self.chunk_seeds.len()
    }

    /// Simulates up to `max_rounds` more rounds.
    pub fn step(&mut self, max_rounds: usize) {
        for _ in 0..max_rounds {
            if self.is_done() {
                break;
            }
            self.run_round();
        }
    }

    /// Runs whatever is left and returns the eroded map, clamped to `0..=1`.
    pub fn finish(mut self) -> Heightmap {
        self.step(usize::MAX);
        self.map
            .data
            .iter_mut()
            .for_each(|h| *h = h.clamp(0.0, 1.0));
        self.map
    }

    fn run_round(&mut self) {
        let start = self.next_chunk;
        let end = (start + CHUNKS_PER_ROUND).min(self.chunk_seeds.len());
        let total = self.params.droplets;
        let droplets_in = |chunk: usize| {
            let first = chunk as u32 * CHUNK_DROPLETS;
            CHUNK_DROPLETS.min(total - first)
        };
        let simulate = |chunk: usize| {
            simulate_chunk(
                &self.map,
                &self.params,
                &self.brush,
                self.chunk_seeds[chunk],
                droplets_in(chunk),
            )
        };

        #[cfg(feature = "parallel")]
        let changes: Vec<Vec<(u32, f32)>> = {
            use rayon::prelude::*;
            (start..end).into_par_iter().map(simulate).collect()
        };
        #[cfg(not(feature = "parallel"))]
        let changes: Vec<Vec<(u32, f32)>> = (start..end).map(simulate).collect();

        for chunk in changes {
            for (i, delta) in chunk {
                self.map.data[i as usize] += delta;
            }
        }
        self.next_chunk = end;
    }
}

/// Runs the whole erosion stage at once.
pub fn erode(config: &WorldConfig, input: &Heightmap) -> Heightmap {
    ErosionJob::new(config, input).finish()
}

/// Erosion brush: offsets within `radius`, weighted by closeness, summing
/// to 1.
fn brush(radius: u32) -> Vec<(isize, isize, f32)> {
    let r = radius.max(1) as isize;
    let mut cells = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            let w = r as f32 - ((dx * dx + dy * dy) as f32).sqrt();
            if w > 0.0 {
                cells.push((dx, dy, w));
            }
        }
    }
    let total: f32 = cells.iter().map(|c| c.2).sum();
    cells
        .iter()
        .map(|&(dx, dy, w)| (dx, dy, w / total))
        .collect()
}

/// Simulates one chunk of droplets on a private copy of `snapshot` and
/// returns the height changes it made, as (index, delta).
fn simulate_chunk(
    snapshot: &Heightmap,
    p: &ErosionConfig,
    brush: &[(isize, isize, f32)],
    seed: u64,
    droplets: u32,
) -> Vec<(u32, f32)> {
    let (w, h) = (snapshot.width, snapshot.height);
    if w < 2 || h < 2 {
        return Vec::new();
    }
    let mut map = snapshot.data.clone();
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let (max_x, max_y) = ((w - 1) as f32, (h - 1) as f32);

    for _ in 0..droplets {
        let mut x = (unit_f32(&mut rng) * max_x).min(max_x - 1e-3);
        let mut y = (unit_f32(&mut rng) * max_y).min(max_y - 1e-3);
        let (mut dir_x, mut dir_y) = (0.0f32, 0.0f32);
        let mut speed = 1.0f32;
        let mut water = 1.0f32;
        let mut sediment = 0.0f32;

        for _ in 0..p.max_lifetime {
            let (cx, cy) = (x as usize, y as usize);
            let (u, v) = (x - cx as f32, y - cy as f32);
            let (height, grad_x, grad_y) = height_and_gradient(&map, w, x, y);

            dir_x = dir_x * p.inertia - grad_x * (1.0 - p.inertia);
            dir_y = dir_y * p.inertia - grad_y * (1.0 - p.inertia);
            let len = (dir_x * dir_x + dir_y * dir_y).sqrt();
            if len < 1e-9 {
                break; // Perfectly flat: nowhere to go.
            }
            dir_x /= len;
            dir_y /= len;
            x += dir_x;
            y += dir_y;
            if !(x >= 0.0 && x < max_x && y >= 0.0 && y < max_y) {
                break; // Ran off the map.
            }

            let new_height = height_and_gradient(&map, w, x, y).0;
            let dh = new_height - height;
            let capacity = (-dh * speed * water * p.sediment_capacity).max(p.min_sediment_capacity);

            if sediment > capacity || dh > 0.0 {
                // Climbing: fill the dip behind. Otherwise drop the excess.
                let amount = if dh > 0.0 {
                    dh.min(sediment)
                } else {
                    (sediment - capacity) * p.deposition_rate
                };
                sediment -= amount;
                let i = cy * w + cx;
                map[i] += amount * (1.0 - u) * (1.0 - v);
                map[i + 1] += amount * u * (1.0 - v);
                map[i + w] += amount * (1.0 - u) * v;
                map[i + w + 1] += amount * u * v;
            } else {
                // Never dig deeper than the drop to the next position.
                let amount = ((capacity - sediment) * p.erosion_rate).min(-dh);
                for &(dx, dy, weight) in brush {
                    let (Some(nx), Some(ny)) =
                        (cx.checked_add_signed(dx), cy.checked_add_signed(dy))
                    else {
                        continue;
                    };
                    if nx >= w || ny >= h {
                        continue;
                    }
                    let j = ny * w + nx;
                    let take = (amount * weight).min(map[j].max(0.0));
                    map[j] -= take;
                    sediment += take;
                }
            }

            // Downhill (dh < 0) speeds the droplet up, uphill slows it.
            speed = (speed * speed - dh * p.gravity).max(0.0).sqrt();
            water *= 1.0 - p.evaporation;
        }

        // A droplet that stops on the map leaves its load where it stopped,
        // so material is only lost off the map edges.
        if sediment > 0.0 && x >= 0.0 && x < max_x && y >= 0.0 && y < max_y {
            let (cx, cy) = (x as usize, y as usize);
            let (u, v) = (x - cx as f32, y - cy as f32);
            let i = cy * w + cx;
            map[i] += sediment * (1.0 - u) * (1.0 - v);
            map[i + 1] += sediment * u * (1.0 - v);
            map[i + w] += sediment * (1.0 - u) * v;
            map[i + w + 1] += sediment * u * v;
        }
    }

    map.iter()
        .zip(&snapshot.data)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, (a, b))| (i as u32, a - b))
        .collect()
}

/// Bilinear height and gradient at a point with `0 <= x < width - 1`,
/// `0 <= y < height - 1`.
fn height_and_gradient(map: &[f32], width: usize, x: f32, y: f32) -> (f32, f32, f32) {
    let (cx, cy) = (x as usize, y as usize);
    let (u, v) = (x - cx as f32, y - cy as f32);
    let i = cy * width + cx;
    let (nw, ne, sw, se) = (map[i], map[i + 1], map[i + width], map[i + width + 1]);
    let grad_x = (ne - nw) * (1.0 - v) + (se - sw) * v;
    let grad_y = (sw - nw) * (1.0 - u) + (se - ne) * u;
    let height = nw * (1.0 - u) * (1.0 - v) + ne * u * (1.0 - v) + sw * (1.0 - u) * v + se * u * v;
    (height, grad_x, grad_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fractal::fbm_field;
    use crate::heightmap::normalise;

    fn config(droplets: u32) -> WorldConfig {
        let mut config = WorldConfig {
            seed: 3,
            width: 96,
            height: 96,
            ..Default::default()
        };
        config.erosion.droplets = droplets;
        config
    }

    fn noise_map(config: &WorldConfig) -> Heightmap {
        let mut data = fbm_field(config);
        normalise(&mut data);
        Heightmap {
            width: config.width,
            height: config.height,
            data,
        }
    }

    #[test]
    fn deterministic() {
        let c = config(5000);
        let map = noise_map(&c);
        assert_eq!(erode(&c, &map), erode(&c, &map));
    }

    #[test]
    fn zero_droplets_changes_nothing() {
        let c = config(0);
        let map = noise_map(&c);
        assert_eq!(erode(&c, &map), map);
    }

    #[test]
    fn erosion_changes_the_terrain_and_stays_in_range() {
        let c = config(5000);
        let map = noise_map(&c);
        let eroded = erode(&c, &map);
        assert_ne!(eroded, map);
        assert!(eroded.data.iter().all(|h| (0.0..=1.0).contains(h)));
    }

    #[test]
    fn mostly_moves_material_rather_than_creating_it() {
        let c = config(5000);
        let map = noise_map(&c);
        let eroded = erode(&c, &map);
        let before: f32 = map.data.iter().sum();
        let after: f32 = eroded.data.iter().sum();
        // Only sediment carried off the map edges is lost. On this small
        // map many droplets reach an edge within their lifetime.
        assert!(after <= before + 1e-2, "before {before}, after {after}");
        assert!(after > 0.98 * before, "before {before}, after {after}");
    }

    #[test]
    fn stepping_in_pieces_matches_running_at_once() {
        let c = config(5000);
        let map = noise_map(&c);
        let mut job = ErosionJob::new(&c, &map);
        let mut last = job.progress();
        while !job.is_done() {
            job.step(1);
            assert!(job.progress() > last);
            last = job.progress();
        }
        assert_eq!(job.finish(), erode(&c, &map));
    }

    #[test]
    fn rounds_merge_identically_to_one_chunk_at_a_time() {
        // The round structure, not execution order, defines the result:
        // simulating each round's chunks separately and merging in order must
        // match the job, which is what the parallel path relies on.
        let c = config(3 * CHUNK_DROPLETS);
        let map = noise_map(&c);
        let job = ErosionJob::new(&c, &map);
        let mut manual = map.clone();
        let changes: Vec<_> = (0..3)
            .map(|k| {
                simulate_chunk(
                    &map,
                    &c.erosion,
                    &job.brush,
                    job.chunk_seeds[k],
                    CHUNK_DROPLETS,
                )
            })
            .collect();
        for chunk in changes {
            for (i, d) in chunk {
                manual.data[i as usize] += d;
            }
        }
        manual.data.iter_mut().for_each(|h| *h = h.clamp(0.0, 1.0));
        assert_eq!(job.finish(), manual);
    }

    #[test]
    fn brush_weights_sum_to_one() {
        for r in 1..6 {
            let total: f32 = brush(r).iter().map(|c| c.2).sum();
            assert!((total - 1.0).abs() < 1e-5);
        }
    }
}
