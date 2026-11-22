//! Stage: fractal (FBM) noise detail.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand_chacha::rand_core::RngCore;

use crate::config::WorldConfig;
use crate::rng::{stage, stage_rng, unit_f64};

/// A `width * height` row-major field of raw FBM values, roughly in `-1.0..1.0`.
pub fn fbm_field(config: &WorldConfig) -> Vec<f32> {
    let (width, height) = (config.width, config.height);
    let params = &config.noise;
    let mut rng = stage_rng(config.seed, stage::NOISE);

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
    let scale = 1.0 / width.max(1) as f64;
    let mut data = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let nx = offset_x + x as f64 * scale;
            let ny = offset_y + y as f64 * scale;
            data.push(fbm.get([nx, ny]) as f32);
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(seed: u64) -> WorldConfig {
        WorldConfig {
            seed,
            width: 32,
            height: 24,
            ..Default::default()
        }
    }

    #[test]
    fn same_seed_is_identical() {
        assert_eq!(fbm_field(&config(42)), fbm_field(&config(42)));
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(fbm_field(&config(42)), fbm_field(&config(43)));
    }

    #[test]
    fn has_requested_size() {
        assert_eq!(fbm_field(&config(1)).len(), 32 * 24);
    }

    #[test]
    fn unaffected_by_other_stages_settings() {
        let mut other = config(42);
        other.tectonics.plate_count = 33;
        other.blend.noise_weight = 0.1;
        assert_eq!(fbm_field(&config(42)), fbm_field(&other));
    }
}
