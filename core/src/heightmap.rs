//! Stage: final heightmap, blending tectonic relief with fractal noise.

use crate::config::WorldConfig;
use crate::tectonics::TectonicMap;

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

/// Adds weighted tectonic relief and noise, then normalises to `0.0..=1.0`.
pub fn blend_heightmap(config: &WorldConfig, tectonics: &TectonicMap, noise: &[f32]) -> Heightmap {
    let params = &config.blend;
    assert_eq!(
        tectonics.elevation.len(),
        noise.len(),
        "tectonic and noise fields must be the same size"
    );
    let mut data: Vec<f32> = tectonics
        .elevation
        .iter()
        .zip(noise)
        .map(|(&t, &n)| params.tectonic_weight * t + params.noise_weight * n)
        .collect();
    normalise(&mut data);
    Heightmap {
        width: tectonics.width,
        height: tectonics.height,
        data,
    }
}

/// Rescales values to span exactly `0.0..=1.0`.
pub(crate) fn normalise(data: &mut [f32]) {
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
}
