//! Turns generated data into images. Display-only: nothing here feeds back into
//! generation, so recolouring never requires re-running the pipeline.

use eframe::egui::ColorImage;
use worldgen_core::{BoundaryKind, Heightmap, PlateKind, TectonicMap, NO_BOUNDARY};

/// Heights at or above this are drawn as snow, whatever the sea level.
pub const SNOW_LINE: f32 = 0.85;
/// Height band just above sea level drawn as beach.
const BEACH_BAND: f32 = 0.02;

pub type Rgb = [u8; 3];

pub const CONVERGENT: Rgb = [225, 40, 40];
pub const DIVERGENT: Rgb = [255, 205, 20];
pub const TRANSFORM: Rgb = [250, 250, 250];

const DEEP_OCEAN: Rgb = [12, 38, 84];
const SHALLOW_WATER: Rgb = [64, 140, 200];
const BEACH: Rgb = [222, 206, 150];
const LOWLAND: Rgb = [76, 140, 60];
const HIGHLAND: Rgb = [132, 102, 70];
const SNOW: Rgb = [242, 244, 246];

/// Greyscale: black is lowest, white is highest.
pub fn greyscale(map: &Heightmap) -> ColorImage {
    let grey: Vec<u8> = map
        .data
        .iter()
        .map(|&h| (h.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    ColorImage::from_gray([map.width, map.height], &grey)
}

/// Any field, stretched so its own min is black and max is white.
pub fn stretched_greyscale(width: usize, height: usize, values: &[f32]) -> ColorImage {
    let (min, max) = values
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let range = (max - min).max(f32::EPSILON);
    let grey: Vec<u8> = values
        .iter()
        .map(|&v| (((v - min) / range) * 255.0).round() as u8)
        .collect();
    ColorImage::from_gray([width, height], &grey)
}

/// Plates in flat colours (blues for oceanic, earth tones for continental),
/// with boundaries coloured by type.
pub fn plates(map: &TectonicMap) -> ColorImage {
    let rgb: Vec<u8> = map
        .plate_id
        .iter()
        .zip(&map.boundary_id)
        .flat_map(|(&plate, &boundary)| {
            if boundary != NO_BOUNDARY {
                boundary_colour(map.boundaries[boundary as usize].kind)
            } else {
                plate_colour(plate as usize, map.plates[plate as usize].kind)
            }
        })
        .collect();
    ColorImage::from_rgb([map.width, map.height], &rgb)
}

pub fn boundary_colour(kind: BoundaryKind) -> Rgb {
    match kind {
        BoundaryKind::Convergent => CONVERGENT,
        BoundaryKind::Divergent => DIVERGENT,
        BoundaryKind::Transform => TRANSFORM,
    }
}

/// A flat colour per plate. Golden-ratio steps keep neighbouring indices
/// visibly different.
pub fn plate_colour(index: usize, kind: PlateKind) -> Rgb {
    let a = (index as f32 * 0.618_034).fract();
    let b = (index as f32 * 0.381_966 + 0.5).fract();
    match kind {
        PlateKind::Oceanic => hsv(195.0 + 40.0 * a, 0.40 + 0.25 * b, 0.50 + 0.25 * (1.0 - a)),
        PlateKind::Continental => hsv(30.0 + 85.0 * a, 0.35 + 0.20 * b, 0.62 + 0.20 * b),
    }
}

fn hsv(hue: f32, sat: f32, val: f32) -> Rgb {
    let c = val * sat;
    let h = (hue.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = val - c;
    [r, g, b].map(|v| ((v + m) * 255.0).round() as u8)
}

/// Hypsometric tint: water below `sea_level`, then beach, lowland to highland,
/// and snow above [`SNOW_LINE`].
pub fn hypsometric(map: &Heightmap, sea_level: f32) -> ColorImage {
    let rgb: Vec<u8> = map
        .data
        .iter()
        .flat_map(|&h| hypsometric_colour(h, sea_level))
        .collect();
    ColorImage::from_rgb([map.width, map.height], &rgb)
}

/// Colour for one height. Both arguments are in `0.0..=1.0`.
pub fn hypsometric_colour(height: f32, sea_level: f32) -> Rgb {
    let h = height.clamp(0.0, 1.0);
    let sea = sea_level.clamp(0.0, 1.0);

    if h < sea {
        // 0 at the deepest possible point, 1 at the shoreline. Squared so the
        // shallow tint stays near the coast and open water reads as deep.
        let t = h / sea;
        return lerp(DEEP_OCEAN, SHALLOW_WATER, t * t);
    }
    if h >= SNOW_LINE {
        return SNOW;
    }
    if h < sea + BEACH_BAND {
        return BEACH;
    }
    let land_start = sea + BEACH_BAND;
    let t = (h - land_start) / (SNOW_LINE - land_start);
    lerp(LOWLAND, HIGHLAND, t)
}

fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepest_point_is_deep_ocean() {
        assert_eq!(hypsometric_colour(0.0, 0.5), DEEP_OCEAN);
    }

    #[test]
    fn water_gets_lighter_towards_the_shore() {
        let deep = hypsometric_colour(0.1, 0.5);
        let shallow = hypsometric_colour(0.49, 0.5);
        assert!(shallow[2] > deep[2]);
        assert!(shallow[1] > deep[1]);
    }

    #[test]
    fn just_above_sea_level_is_beach() {
        assert_eq!(hypsometric_colour(0.5, 0.5), BEACH);
        assert_eq!(hypsometric_colour(0.51, 0.5), BEACH);
    }

    #[test]
    fn land_runs_from_lowland_to_highland() {
        let low = hypsometric_colour(0.53, 0.5);
        let high = hypsometric_colour(0.84, 0.5);
        assert!(low[1] > high[1], "lowland should be greener");
        assert!(high[0] > low[0], "highland should be browner");
    }

    #[test]
    fn snow_above_the_snow_line() {
        assert_eq!(hypsometric_colour(SNOW_LINE, 0.5), SNOW);
        assert_eq!(hypsometric_colour(1.0, 0.5), SNOW);
    }

    #[test]
    fn sea_level_one_is_all_water() {
        for h in [0.0, 0.5, 0.99] {
            let [r, _, b] = hypsometric_colour(h, 1.0);
            assert!(b > r, "height {h} should be water");
        }
    }

    #[test]
    fn sea_level_zero_has_no_water() {
        assert_eq!(hypsometric_colour(0.0, 0.0), BEACH);
    }

    #[test]
    fn plate_colours_follow_plate_kind() {
        for i in 0..40 {
            let [r, _, b] = plate_colour(i, PlateKind::Oceanic);
            assert!(b > r, "oceanic plate {i} should be blue");
            let [r, _, b] = plate_colour(i, PlateKind::Continental);
            assert!(r > b, "continental plate {i} should be earthy");
        }
    }

    #[test]
    fn neighbouring_plates_get_different_colours() {
        assert_ne!(
            plate_colour(3, PlateKind::Oceanic),
            plate_colour(4, PlateKind::Oceanic)
        );
    }

    #[test]
    fn sea_above_snow_line_still_floods_peaks() {
        let [r, _, b] = hypsometric_colour(0.9, 0.95);
        assert!(b > r);
    }
}
