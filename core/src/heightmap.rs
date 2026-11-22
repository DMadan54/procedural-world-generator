//! Stage 1: fractal (FBM) noise heightmap.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand_chacha::rand_core::RngCore;

use crate::rng::{Stage, WorldSeed};

/// Parameters for heightmap generation.
#[derive(Clone, Debug, PartialEq)]
pub struct HeightmapParams {
    pub width: usize,
    pub height: usize,
    /// Number of noise layers summed together.
    pub octaves: usize,
    /// Base frequency, in noise cycles per map width.
    pub frequency: f64,
    /// Frequency multiplier between octaves.
    pub lacunarity: f64,
    /// Amplitude multiplier between octaves.
    pub persistence: f64,
}

impl Default for HeightmapParams {
    fn default() -> Self {
        Self {
            width: 512,
            height: 512,
            octaves: 6,
            frequency: 4.0,
            lacunarity: 2.0,
            persistence: 0.5,
        }
    }
}

/// A grid of heights, row-major, each normalised to `0.0..=1.0`.
#[derive(Clone, Debug, PartialEq)]
pub struct Heightmap {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Heightmap {
    pub fn get(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.width + x]
    }
}

/// Generates a heightmap. The same seed and params always give the same result.
pub fn generate_heightmap(seed: WorldSeed, params: &HeightmapParams) -> Heightmap {
    let mut rng = seed.stage_rng(Stage::Heightmap);

    let noise_seed = rng.next_u32();
    // A random offset into noise space, so nearby seeds don't share structure
    // around the origin.
    let offset_x = unit_f64(&mut rng) * 10_000.0;
    let offset_y = unit_f64(&mut rng) * 10_000.0;

    let fbm = Fbm::<Perlin>::new(noise_seed)
        .set_octaves(params.octaves)
        .set_frequency(params.frequency)
        .set_lacunarity(params.lacunarity)
        .set_persistence(params.persistence);

    // Sample in units of "map widths" so the frequency is resolution-independent.
    let scale = 1.0 / params.width.max(1) as f64;
    let mut data = Vec::with_capacity(params.width * params.height);
    for y in 0..params.height {
        for x in 0..params.width {
            let nx = offset_x + x as f64 * scale;
            let ny = offset_y + y as f64 * scale;
            data.push(fbm.get([nx, ny]) as f32);
        }
    }

    normalise(&mut data);
    Heightmap {
        width: params.width,
        height: params.height,
        data,
    }
}

/// A uniform value in `0.0..1.0`.
fn unit_f64(rng: &mut impl RngCore) -> f64 {
    // Top 53 bits, the precision of an f64 mantissa.
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// Rescales values to span exactly `0.0..=1.0`.
fn normalise(data: &mut [f32]) {
    let (min, max) = data
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let range = max - min;
    if !range.is_finite() || range <= f32::EPSILON {
        data.iter_mut().for_each(|v| *v = 0.0);
        return;
    }
    data.iter_mut().for_each(|v| *v = (*v - min) / range);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> HeightmapParams {
        HeightmapParams {
            width: 64,
            height: 48,
            ..Default::default()
        }
    }

    #[test]
    fn same_seed_and_params_are_identical() {
        let a = generate_heightmap(WorldSeed(42), &small());
        let b = generate_heightmap(WorldSeed(42), &small());
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_differ() {
        let a = generate_heightmap(WorldSeed(42), &small());
        let b = generate_heightmap(WorldSeed(43), &small());
        assert_ne!(a.data, b.data);
    }

    #[test]
    fn dimensions_match_params() {
        let map = generate_heightmap(WorldSeed(1), &small());
        assert_eq!(map.width, 64);
        assert_eq!(map.height, 48);
        assert_eq!(map.data.len(), 64 * 48);
    }

    #[test]
    fn values_are_normalised() {
        let map = generate_heightmap(WorldSeed(9), &small());
        assert!(map.data.iter().all(|v| (0.0..=1.0).contains(v)));
        let min = map.data.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = map.data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(min, 0.0);
        assert_eq!(max, 1.0);
    }
}
