//! Stage: final heightmap, blending tectonic relief with fractal noise.
//!
//! The blend is mapped to `0.0..1.0` on a fixed scale, not stretched to each
//! world's own lowest and highest points. So a given height means the same
//! thing in every world: sea level cuts continental plates at the same place
//! however tall the mountains are, and the land share follows the plates.
//! Heights beyond the usual range are eased into the ends with a soft limit
//! rather than clipped.

use crate::config::{BlendConfig, WorldConfig};
use crate::dmath;
use crate::tectonics::TectonicMap;

/// A grid of heights, row-major, each in `0.0..1.0`.
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

/// Below this height (and above `HIGH_KNEE`) values are eased towards the
/// end of the range instead of continuing linearly.
const LOW_KNEE: f32 = 0.08;
const HIGH_KNEE: f32 = 0.85;

/// Adds weighted tectonic relief and noise and maps the result to heights.
pub fn blend_heightmap(config: &WorldConfig, tectonics: &TectonicMap, noise: &[f32]) -> Heightmap {
    let params = &config.blend;
    assert_eq!(
        tectonics.elevation.len(),
        noise.len(),
        "tectonic and noise fields must be the same size"
    );
    let data = tectonics
        .elevation
        .iter()
        .zip(noise)
        .map(|(&t, &n)| to_height(params.tectonic_weight * t + params.noise_weight * n, params))
        .collect();
    Heightmap {
        width: tectonics.width,
        height: tectonics.height,
        data,
    }
}

/// Maps a raw blended value to a height in `0.0..1.0`: linear in the usual
/// range, easing smoothly into the ends beyond it. Strictly increasing.
pub fn to_height(raw: f32, params: &BlendConfig) -> f32 {
    let linear = params.height_offset + params.height_scale * raw;
    soft_limit(linear)
}

fn soft_limit(h: f32) -> f32 {
    if h > HIGH_KNEE {
        let span = 1.0 - HIGH_KNEE;
        HIGH_KNEE + span * dmath::tanh((h - HIGH_KNEE) / span)
    } else if h < LOW_KNEE {
        LOW_KNEE - LOW_KNEE * dmath::tanh((LOW_KNEE - h) / LOW_KNEE)
    } else {
        h
    }
}

/// Rescales values to span exactly `0.0..=1.0`. Test helper.
#[cfg(test)]
pub(crate) fn normalise(data: &mut [f32]) {
    let (min, max) = data
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
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

    #[test]
    fn normalise_spans_zero_to_one() {
        let mut v = vec![-3.0, 1.0, 5.0];
        normalise(&mut v);
        assert_eq!(v, vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn normalise_flat_field_is_zero() {
        let mut v = vec![2.0; 4];
        normalise(&mut v);
        assert_eq!(v, vec![0.0; 4]);
    }

    #[test]
    fn heights_stay_inside_the_range_and_keep_their_order() {
        let p = BlendConfig::default();
        let mut last = f32::NEG_INFINITY;
        for i in -400..=400 {
            let h = to_height(i as f32 * 0.02, &p);
            assert!((0.0..=1.0).contains(&h), "raw {} gave {h}", i as f32 * 0.02);
            assert!(h >= last);
            last = h;
        }
    }

    #[test]
    fn soft_limit_is_continuous_at_the_knees() {
        for knee in [LOW_KNEE, HIGH_KNEE] {
            assert!((soft_limit(knee - 1e-4) - soft_limit(knee + 1e-4)).abs() < 1e-3);
        }
    }

    #[test]
    fn plate_interiors_sit_either_side_of_default_sea_level() {
        // Continental (+0.45) and oceanic (-0.45) plate interiors must land
        // clearly above and below the default sea level whatever else is
        // going on in the world.
        let p = BlendConfig::default();
        let sea = crate::config::TerrainConfig::default().sea_level;
        assert!(to_height(0.45 * p.tectonic_weight, &p) > sea + 0.05);
        assert!(to_height(-0.45 * p.tectonic_weight, &p) < sea - 0.1);
    }
}
