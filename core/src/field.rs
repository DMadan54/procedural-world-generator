//! Helpers for 2D scalar fields shared by several stages.

use crate::heightmap::Heightmap;

/// Steepest drop from a cell to one of its four neighbours, in normalised
/// height units per cell. Multiply by `width / 512` to compare across map
/// sizes (see [`slope_at_reference`]).
pub(crate) fn slope(i: usize, w: usize, h: usize, terrain: &Heightmap) -> f32 {
    let (x, y) = (i % w, i / w);
    let here = terrain.data[i];
    let mut steepest = 0.0f32;
    for (dx, dy) in [(1isize, 0isize), (-1, 0), (0, 1), (0, -1)] {
        let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
            continue;
        };
        if nx < w && ny < h {
            steepest = steepest.max((here - terrain.data[ny * w + nx]).abs());
        }
    }
    steepest
}

/// [`slope`] expressed for a 512-cell-wide map, so thresholds mean the same
/// at every world size: a coarser map's cells span more ground, so the same
/// hillside drops more per cell.
pub(crate) fn slope_at_reference(i: usize, w: usize, h: usize, terrain: &Heightmap) -> f32 {
    slope(i, w, h, terrain) * w as f32 / 512.0
}

/// Approximate Gaussian blur (three box passes per axis).
///
/// With `line_gain`, the result is rescaled so a boundary line (two pixels
/// wide, one on each plate) keeps its original peak value at its centre and
/// falls off from there, instead of being smeared down to a fraction of it.
pub(crate) fn gaussian_blur(
    data: &mut [f32],
    width: usize,
    height: usize,
    sigma: f32,
    line_gain: bool,
) {
    // Three box passes of radius r have variance r * (r + 1).
    let radius = ((sigma * sigma + 0.25).sqrt() - 0.5).round().max(1.0) as usize;
    for _ in 0..3 {
        box_blur(data, width, height, radius);
    }
    if line_gain {
        let gain = 1.0 / line_peak(radius);
        data.iter_mut().for_each(|v| *v *= gain);
    }
}

/// Peak of a two-pixel-wide line of ones after three 1D box passes.
fn line_peak(radius: usize) -> f32 {
    let len = 8 * radius + 8;
    let mut line = vec![0.0; len];
    line[len / 2 - 1] = 1.0;
    line[len / 2] = 1.0;
    for _ in 0..3 {
        box_blur(&mut line, len, 1, radius);
    }
    line.iter().cloned().fold(0.0, f32::max)
}

/// Separable box blur (running sums), averaging over the part of the window
/// that lies inside the map.
pub(crate) fn box_blur(data: &mut [f32], width: usize, height: usize, radius: usize) {
    let mut line = Vec::with_capacity(width.max(height));
    for y in 0..height {
        line.clear();
        line.extend((0..width).map(|x| data[y * width + x]));
        blur_line(&line, radius, |x, v| data[y * width + x] = v);
    }
    for x in 0..width {
        line.clear();
        line.extend((0..height).map(|y| data[y * width + x]));
        blur_line(&line, radius, |y, v| data[y * width + x] = v);
    }
}

fn blur_line(src: &[f32], radius: usize, mut write: impl FnMut(usize, f32)) {
    let n = src.len();
    // Prefix sums in f64 so long rows stay accurate.
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0f64);
    for &v in src {
        prefix.push(prefix[prefix.len() - 1] + v as f64);
    }
    for i in 0..n {
        let lo = i.saturating_sub(radius);
        let hi = (i + radius + 1).min(n);
        write(i, ((prefix[hi] - prefix[lo]) / (hi - lo) as f64) as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_relief_keeps_its_peak_and_falls_off() {
        // A two-pixel vertical line of stress 1.0, blurred like a boundary.
        let (w, h) = (128, 32);
        let mut field = vec![0.0; w * h];
        for y in 0..h {
            field[y * w + 63] = 1.0;
            field[y * w + 64] = 1.0;
        }
        gaussian_blur(&mut field, w, h, 6.0, true);
        let row = &field[16 * w..17 * w];
        assert!((row[63] - 1.0).abs() < 0.05, "peak was {}", row[63]);
        assert!(row[63] > row[58] && row[58] > row[50]);
        assert!(row[30] < 0.01);
    }

    #[test]
    fn box_blur_preserves_a_flat_field() {
        let mut field = vec![0.7; 20 * 10];
        box_blur(&mut field, 20, 10, 3);
        assert!(field.iter().all(|v| (v - 0.7).abs() < 1e-6));
    }
}
