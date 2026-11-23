//! Turns generated data into images. Display-only: nothing here feeds back into
//! generation, so recolouring never requires re-running the pipeline.

use eframe::egui::ColorImage;
use worldgen_core::hydrology::NO_LAKE;
use worldgen_core::{
    Biome, Biomes, BoundaryKind, Climate, Heightmap, Hydrology, PlateKind, TectonicMap, NO_BOUNDARY,
};

/// Colour stops for mean temperature, °C.
pub const TEMPERATURE_SCALE: [(f32, Rgb); 6] = [
    (-30.0, [49, 54, 149]),
    (-15.0, [69, 117, 180]),
    (0.0, [224, 243, 248]),
    (10.0, [254, 224, 144]),
    (20.0, [244, 109, 67]),
    (32.0, [165, 0, 38]),
];

/// Colour stops for annual precipitation, mm.
pub const PRECIPITATION_SCALE: [(f32, Rgb); 5] = [
    (0.0, [150, 105, 55]),
    (250.0, [225, 205, 140]),
    (750.0, [150, 200, 105]),
    (1500.0, [40, 140, 80]),
    (3000.0, [25, 85, 170]),
];

/// Temperature colours; ocean is shown a little darker so coasts stay visible.
pub fn temperature(climate: &Climate, map: &Heightmap, sea_level: f32) -> ColorImage {
    scaled_field(&climate.temperature, &TEMPERATURE_SCALE, map, sea_level)
}

/// Precipitation colours; ocean is shown a little darker.
pub fn precipitation(climate: &Climate, map: &Heightmap, sea_level: f32) -> ColorImage {
    scaled_field(&climate.precipitation, &PRECIPITATION_SCALE, map, sea_level)
}

/// What erosion changed: red where material was removed, blue where it was
/// deposited, over the eroded terrain in grey. Colour saturates at a change
/// of `scale` (normalised height units).
pub fn erosion_change(before: &Heightmap, after: &Heightmap, scale: f32) -> ColorImage {
    const ERODED: Rgb = [220, 50, 40];
    const DEPOSITED: Rgb = [40, 110, 230];
    let rgb: Vec<u8> = before
        .data
        .iter()
        .zip(&after.data)
        .flat_map(|(&b, &a)| {
            let grey = [(60.0 + a.clamp(0.0, 1.0) * 150.0) as u8; 3];
            let d = a - b;
            let t = (d.abs() / scale.max(1e-9)).min(1.0);
            lerp(grey, if d < 0.0 { ERODED } else { DEPOSITED }, t)
        })
        .collect();
    ColorImage::from_rgb([after.width, after.height], &rgb)
}

pub const LAKE: Rgb = [70, 135, 190];

/// Flat debug colour per biome.
pub fn biome_colour(biome: Biome) -> Rgb {
    match biome {
        Biome::Ocean => [30, 60, 110],
        Biome::Lake => LAKE,
        Biome::Ice => [240, 245, 250],
        Biome::Tundra => [170, 180, 165],
        Biome::Alpine => [140, 130, 125],
        Biome::BorealForest => [50, 95, 70],
        Biome::TemperateForest => [70, 135, 60],
        Biome::TemperateRainforest => [40, 120, 90],
        Biome::Shrubland => [175, 160, 95],
        Biome::Grassland => [170, 195, 100],
        Biome::Desert => [230, 205, 140],
        Biome::Savanna => [205, 185, 90],
        Biome::TropicalForest => [90, 160, 50],
        Biome::TropicalRainforest => [20, 110, 40],
        Biome::Wetland => [95, 140, 125],
        Biome::Mangrove => [45, 105, 90],
    }
}

/// Colour stops for habitability.
pub const HABITABILITY_SCALE: [(f32, Rgb); 5] = [
    (0.0, [60, 50, 45]),
    (0.2, [140, 90, 60]),
    (0.45, [215, 175, 90]),
    (0.7, [150, 195, 100]),
    (1.0, [40, 130, 70]),
];

/// Habitability on a brown-to-green ramp; water in dark blue.
pub fn habitability(world: &worldgen_core::World) -> ColorImage {
    let h = &world.habitability;
    let rgb: Vec<u8> = (0..h.value.len())
        .flat_map(|i| {
            if world.biomes.biome[i].is_water() {
                [25, 45, 80]
            } else {
                colour_scale(h.value[i], &HABITABILITY_SCALE)
            }
        })
        .collect();
    ColorImage::from_rgb([h.width, h.height], &rgb)
}

/// A distinct, fairly bright colour per people.
pub fn people_colour(id: u16) -> Rgb {
    let hue = (id as f32 * 0.618_034 * 360.0) % 360.0;
    hsv(hue, 0.5, 0.85)
}

/// Peoples' homelands in flat colours over grey land and dark water.
pub fn peoples(world: &worldgen_core::World) -> ColorImage {
    let p = &world.peoples;
    let rgb: Vec<u8> = (0..p.homeland.len())
        .flat_map(|i| {
            let owner = p.homeland[i];
            if world.biomes.biome[i].is_water() {
                [25, 45, 80]
            } else if owner == worldgen_core::peoples::NO_PEOPLE {
                [120, 115, 108]
            } else {
                people_colour(owner)
            }
        })
        .collect();
    ColorImage::from_rgb([p.width, p.height], &rgb)
}

/// Realms at a year of history, in their colours.
pub fn realms_at(
    world: &worldgen_core::World,
    state: &worldgen_core::history::HistoryState,
) -> ColorImage {
    let rgb: Vec<u8> = (0..state.owner.len())
        .flat_map(|i| {
            let owner = state.owner[i];
            if world.biomes.biome[i].is_water() {
                [25, 45, 80]
            } else if owner == worldgen_core::realms::NO_REALM {
                [120, 115, 108]
            } else {
                realm_rgb(state.realms[owner as usize].colour)
            }
        })
        .collect();
    ColorImage::from_rgb([state.width, state.height], &rgb)
}

/// Realms in their wash colours (stronger than on the map), wilderness grey.
pub fn realms(world: &worldgen_core::World) -> ColorImage {
    let r = &world.realms;
    let rgb: Vec<u8> = (0..r.owner.len())
        .flat_map(|i| {
            let owner = r.owner[i];
            if world.biomes.biome[i].is_water() {
                [25, 45, 80]
            } else if owner == worldgen_core::realms::NO_REALM {
                [120, 115, 108]
            } else {
                realm_rgb(r.realms[owner as usize].colour)
            }
        })
        .collect();
    ColorImage::from_rgb([r.width, r.height], &rgb)
}

pub fn realm_rgb(colour: u8) -> Rgb {
    crate::style::realm_colour(colour).map(|v| (v * 255.0) as u8)
}

/// Biomes in flat colours.
pub fn biomes(biomes: &Biomes) -> ColorImage {
    let rgb: Vec<u8> = biomes.biome.iter().flat_map(|&b| biome_colour(b)).collect();
    ColorImage::from_rgb([biomes.width, biomes.height], &rgb)
}
pub const RIVER: Rgb = [45, 105, 185];

/// Hypsometric terrain with lakes drawn as flat water.
pub fn terrain_with_lakes(map: &Heightmap, hydrology: &Hydrology, sea_level: f32) -> ColorImage {
    let rgb: Vec<u8> = map
        .data
        .iter()
        .zip(&hydrology.lake_id)
        .flat_map(|(&h, &lake)| {
            if lake != NO_LAKE {
                LAKE
            } else {
                hypsometric_colour(h, sea_level)
            }
        })
        .collect();
    ColorImage::from_rgb([map.width, map.height], &rgb)
}

/// Flow accumulation on a log scale (dark = little upstream area), with sea
/// and lakes in blue.
pub fn flow_accumulation(map: &Heightmap, hydrology: &Hydrology, sea_level: f32) -> ColorImage {
    let max = hydrology
        .accumulation
        .iter()
        .cloned()
        .fold(1.0f32, f32::max)
        .ln_1p();
    let rgb: Vec<u8> = (0..map.data.len())
        .flat_map(|i| {
            if map.data[i] < sea_level {
                [20, 40, 80]
            } else if hydrology.lake_id[i] != NO_LAKE {
                LAKE
            } else {
                let t = hydrology.accumulation[i].ln_1p() / max;
                [(t.sqrt() * 255.0) as u8; 3]
            }
        })
        .collect();
    ColorImage::from_rgb([map.width, map.height], &rgb)
}

/// Muted terrain to draw wind arrows over.
pub fn wind_background(map: &Heightmap, sea_level: f32) -> ColorImage {
    let rgb: Vec<u8> = map
        .data
        .iter()
        .flat_map(|&h| {
            let c = hypsometric_colour(h, sea_level);
            let grey = (c[0] as f32 * 0.3 + c[1] as f32 * 0.59 + c[2] as f32 * 0.11) as u8;
            lerp(c, [grey; 3], 0.6)
        })
        .collect();
    ColorImage::from_rgb([map.width, map.height], &rgb)
}

fn scaled_field(
    values: &[f32],
    scale: &[(f32, Rgb)],
    map: &Heightmap,
    sea_level: f32,
) -> ColorImage {
    let rgb: Vec<u8> = values
        .iter()
        .zip(&map.data)
        .flat_map(|(&v, &h)| {
            let c = colour_scale(v, scale);
            if h < sea_level {
                c.map(|v| (v as f32 * 0.7) as u8)
            } else {
                c
            }
        })
        .collect();
    ColorImage::from_rgb([map.width, map.height], &rgb)
}

/// Piecewise-linear colour scale, clamped at both ends.
pub fn colour_scale(value: f32, stops: &[(f32, Rgb)]) -> Rgb {
    let (first, last) = (stops[0], stops[stops.len() - 1]);
    if value <= first.0 {
        return first.1;
    }
    for pair in stops.windows(2) {
        let ((a, ca), (b, cb)) = (pair[0], pair[1]);
        if value <= b {
            return lerp(ca, cb, (value - a) / (b - a));
        }
    }
    last.1
}

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
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
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
    fn colour_scale_clamps_and_interpolates() {
        let stops = [(0.0, [0, 0, 0]), (10.0, [100, 200, 50])];
        assert_eq!(colour_scale(-5.0, &stops), [0, 0, 0]);
        assert_eq!(colour_scale(50.0, &stops), [100, 200, 50]);
        assert_eq!(colour_scale(5.0, &stops), [50, 100, 25]);
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
