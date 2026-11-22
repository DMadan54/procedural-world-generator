//! Antique cartography: parchment, hatched coasts, inked rivers, mountain and
//! tree glyphs, aged palette.

use worldgen_core::climate::elevation_m;
use worldgen_core::hydrology::{NO_LAKE, NO_SEGMENT};
use worldgen_core::{Biome, World};

use super::canvas::{Canvas, Colour};
use super::util::{bilinear, chaikin, distance_to, hash, mix, value_noise};
use super::MapStyle;

const PARCHMENT: Colour = [0.925, 0.875, 0.745];
const INK: Colour = [0.24, 0.18, 0.12];
const INK_WATER: Colour = [0.33, 0.42, 0.45];
const INK_RIVER: Colour = [0.27, 0.38, 0.48];
const WATER: Colour = [0.79, 0.81, 0.73];
const DEEP_WATER: Colour = [0.71, 0.76, 0.71];
const FOREST_FILL: Colour = [0.63, 0.65, 0.48];
const AGED_EDGE: Colour = [0.72, 0.58, 0.40];

/// Elevation (m) from which hills, then mountains, are drawn.
const HILL_MIN_M: f32 = 450.0;
const MOUNTAIN_MIN_M: f32 = 1300.0;

fn land_tint(biome: Biome) -> Colour {
    match biome {
        Biome::Ice => [0.95, 0.94, 0.90],
        Biome::Tundra => [0.87, 0.85, 0.77],
        Biome::Alpine => [0.87, 0.83, 0.75],
        Biome::BorealForest => [0.82, 0.82, 0.68],
        Biome::TemperateForest => [0.85, 0.84, 0.67],
        Biome::TemperateRainforest => [0.80, 0.82, 0.66],
        Biome::Shrubland => [0.91, 0.85, 0.67],
        Biome::Grassland => [0.91, 0.87, 0.69],
        Biome::Desert => [0.95, 0.86, 0.65],
        Biome::Savanna => [0.93, 0.85, 0.64],
        Biome::TropicalForest => [0.82, 0.83, 0.63],
        Biome::TropicalRainforest => [0.77, 0.81, 0.62],
        Biome::Ocean | Biome::Lake => WATER,
    }
}

/// Draws the world as an old engraved map.
pub struct AntiqueStyle {
    /// Canvas pixels per world cell.
    pub scale: usize,
}

impl Default for AntiqueStyle {
    fn default() -> Self {
        Self { scale: 2 }
    }
}

/// Something drawn on top of the land, painted back to front.
enum Glyph {
    Mountain { base: [f32; 2], dir: [f32; 2], height: f32, half_width: f32, skew: f32, tint: Colour },
    Hill { base: [f32; 2], half_width: f32, height: f32 },
    Tree { at: [f32; 2], kind: Biome },
}

impl Glyph {
    fn depth(&self) -> f32 {
        match self {
            Glyph::Mountain { base, .. } | Glyph::Hill { base, .. } => base[1],
            Glyph::Tree { at, .. } => at[1],
        }
    }
}

impl MapStyle for AntiqueStyle {
    fn name(&self) -> &'static str {
        "Antique map"
    }

    fn render(&self, world: &World) -> Canvas {
        let scale = self.scale.max(1);
        let s = scale as f32;
        let terrain = &world.terrain;
        let (mw, mh) = (terrain.width, terrain.height);
        let (cw, ch) = (mw * scale, mh * scale);
        let sea = world.config.terrain.sea_level;
        let hy = &world.hydrology;
        if mw < 2 || mh < 2 {
            return Canvas::new(cw, ch, PARCHMENT);
        }

        // Canvas-resolution water mask: smooth sea coast from the bilinear
        // height, lakes from the lake grid.
        let to_map = |c: usize| (c as f32 + 0.5) / s - 0.5;
        // Lake shores are drawn where the terrain meets the lake surface, like
        // the sea coast, rather than along cell edges. Each lake's surface is
        // spread one cell outward so the contour can fall between cells.
        let mut lake_level = vec![f32::NEG_INFINITY; mw * mh];
        for (i, &id) in hy.lake_id.iter().enumerate() {
            if id == NO_LAKE {
                continue;
            }
            let surface = hy.lakes[id as usize].surface;
            let (x, y) = (i % mw, i / mw);
            for ny in y.saturating_sub(1)..(y + 2).min(mh) {
                for nx in x.saturating_sub(1)..(x + 2).min(mw) {
                    let j = ny * mw + nx;
                    lake_level[j] = lake_level[j].max(surface);
                }
            }
        }
        let mut water = vec![false; cw * ch];
        let mut lake = vec![false; cw * ch];
        for cy in 0..ch {
            for cx in 0..cw {
                let i = cy * cw + cx;
                let (x, y) = (to_map(cx), to_map(cy));
                let h = bilinear(&terrain.data, mw, mh, x, y);
                let level = lake_level[(cy / scale) * mw + cx / scale];
                lake[i] = h >= sea && h < level + 2e-4;
                water[i] = h < sea || lake[i];
            }
        }
        let land: Vec<bool> = water.iter().map(|w| !w).collect();
        let dist_to_land = distance_to(&land, cw, ch);
        let dist_to_water = distance_to(&water, cw, ch);

        // Land tint per world cell, softened so biome edges blend.
        let mut tint = vec![PARCHMENT; mw * mh];
        for (i, t) in tint.iter_mut().enumerate() {
            let b = world.biomes.biome[i];
            if !b.is_water() {
                *t = mix(PARCHMENT, land_tint(b), 0.8);
            }
        }
        let channel = |k: usize| -> Vec<f32> { tint.iter().map(|c| c[k]).collect() };
        let (tr, tg, tb) = (channel(0), channel(1), channel(2));
        let tint_at = |x: f32, y: f32| {
            [
                bilinear(&tr, mw, mh, x, y),
                bilinear(&tg, mw, mh, x, y),
                bilinear(&tb, mw, mh, x, y),
            ]
        };

        let mut canvas = Canvas::new(cw, ch, PARCHMENT);
        let hatch_spacing = 2.6 * s;
        for cy in 0..ch {
            for cx in 0..cw {
                let i = cy * cw + cx;
                let colour = if water[i] {
                    let d = dist_to_land[i];
                    mix(WATER, DEEP_WATER, d / (60.0 * s))
                } else {
                    tint_at(to_map(cx), to_map(cy))
                };
                canvas.pixels[i] = colour;

                if water[i] {
                    // Hatching: lines that follow the shore, fading out to sea.
                    let d = dist_to_land[i];
                    let reach = if lake[i] { 5.0 * s } else { 13.0 * s };
                    if d > 1.2 * s && d < reach {
                        let wobble = (value_noise(cx as f32 / (6.0 * s), cy as f32 / (6.0 * s), 1) - 0.5) * 1.2 * s;
                        let v = (d + wobble) / hatch_spacing;
                        let off = (v - v.round()).abs() * hatch_spacing;
                        let coverage = (0.35 * s + 0.5 - off).clamp(0.0, 1.0);
                        let fade = (1.0 - d / reach).powf(1.4);
                        canvas.blend(cx, cy, INK_WATER, coverage * fade * 0.8);
                    }
                    // Shoreline on the water side.
                    canvas.blend(cx, cy, INK, (1.0 - dist_to_land[i]).clamp(0.0, 1.0) * 0.7);
                } else {
                    // Shoreline on the land side.
                    canvas.blend(cx, cy, INK, (0.6 * s + 0.5 - dist_to_water[i]).clamp(0.0, 1.0) * 0.9);
                }
            }
        }

        // Rivers: smoothed, tapering with discharge.
        let threshold = world.config.hydrology.river_threshold.max(1e-3);
        for segment in &hy.segments {
            let widths: Vec<f32> = segment
                .discharge
                .iter()
                .map(|q| (0.3 + 0.3 * (q / threshold).sqrt()).clamp(0.4, 1.7) * s)
                .collect();
            let points: Vec<[f32; 2]> = segment.points.iter().map(|p| [p[0] * s, p[1] * s]).collect();
            let (points, widths) = chaikin(&points, &widths, 2);
            canvas.polyline(&points, &widths, INK_RIVER, 0.9);
        }

        // Glyphs.
        let mut glyphs = Vec::new();
        self.place_relief(world, s, &tint_at, &mut glyphs);
        self.place_forests(world, s, &mut canvas, &mut glyphs);
        glyphs.sort_by(|a, b| a.depth().total_cmp(&b.depth()));
        for glyph in &glyphs {
            draw_glyph(&mut canvas, glyph, s);
        }

        age(&mut canvas, s);
        frame(&mut canvas, s);
        canvas
    }
}

impl AntiqueStyle {
    /// Mountains and hills on a jittered grid, sized by elevation and
    /// stretched along the local ridge direction.
    fn place_relief(&self, world: &World, s: f32, tint_at: &dyn Fn(f32, f32) -> Colour, out: &mut Vec<Glyph>) {
        let terrain = &world.terrain;
        let (mw, mh) = (terrain.width, terrain.height);
        let tc = &world.config.terrain;
        let step = 7.5;
        let (nx, ny) = ((mw as f32 / step) as i32, (mh as f32 / step) as i32);
        let h_at = |x: f32, y: f32| bilinear(&terrain.data, mw, mh, x, y);
        for gy in 0..ny {
            for gx in 0..nx {
                let x = (gx as f32 + 0.5 + (hash(gx, gy, 11) - 0.5) * 0.7) * step;
                let y = (gy as f32 + 0.5 + (hash(gx, gy, 12) - 0.5) * 0.7) * step;
                let (ix, iy) = (x as usize, y as usize);
                if ix >= mw || iy >= mh || near_water_or_river(world, ix, iy) {
                    continue;
                }
                let elevation = elevation_m(h_at(x, y), tc);
                if elevation < HILL_MIN_M {
                    continue;
                }
                let base = [x * s, y * s];
                if elevation < MOUNTAIN_MIN_M {
                    if hash(gx, gy, 13) < 0.55 {
                        out.push(Glyph::Hill { base, half_width: 2.6 * s, height: 1.5 * s });
                    }
                    continue;
                }
                // Ridges run across the slope.
                let r = 3.0;
                let (gxv, gyv) = (h_at(x + r, y) - h_at(x - r, y), h_at(x, y + r) - h_at(x, y - r));
                let mut dir = [-gyv, gxv * 0.4];
                let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
                dir = if len > 1e-6 { [dir[0] / len, dir[1] / len] } else { [1.0, 0.0] };
                if dir[0] < 0.0 {
                    dir = [-dir[0], -dir[1]];
                }
                let t = ((elevation - MOUNTAIN_MIN_M) / (tc.max_elevation_m - MOUNTAIN_MIN_M)).clamp(0.0, 1.0);
                let height = (4.5 + 9.0 * t.sqrt()) * s;
                out.push(Glyph::Mountain {
                    base,
                    dir,
                    height,
                    half_width: height * (0.75 + 0.3 * hash(gx, gy, 14)),
                    skew: (hash(gx, gy, 15) - 0.5) * 0.5,
                    tint: tint_at(x, y),
                });
            }
        }
    }

    /// Stippled ground and clustered tree glyphs in forest biomes.
    fn place_forests(&self, world: &World, s: f32, canvas: &mut Canvas, out: &mut Vec<Glyph>) {
        let terrain = &world.terrain;
        let (mw, mh) = (terrain.width, terrain.height);
        let tc = &world.config.terrain;
        let step = 3.0;
        let (nx, ny) = ((mw as f32 / step) as i32, (mh as f32 / step) as i32);
        for gy in 0..ny {
            for gx in 0..nx {
                let x = (gx as f32 + 0.5 + (hash(gx, gy, 21) - 0.5) * 0.9) * step;
                let y = (gy as f32 + 0.5 + (hash(gx, gy, 22) - 0.5) * 0.9) * step;
                let (ix, iy) = (x as usize, y as usize);
                if ix >= mw || iy >= mh {
                    continue;
                }
                let biome = world.biomes.at(ix, iy);
                if !biome.is_forest() || near_water_or_river(world, ix, iy) {
                    continue;
                }
                if elevation_m(terrain.data[iy * mw + ix], tc) > MOUNTAIN_MIN_M - 150.0 {
                    continue;
                }
                // Stipple the forest floor.
                for k in 0..2 {
                    let (dx, dy) = (hash(gx, gy, 30 + k) - 0.5, hash(gx, gy, 40 + k) - 0.5);
                    canvas.dot([(x + dx * step) * s, (y + dy * step) * s], 0.3 * s, INK, 0.35);
                }
                // Trees grow in clumps.
                let density = match biome {
                    Biome::TropicalRainforest | Biome::TemperateRainforest => 0.9,
                    Biome::BorealForest => 0.75,
                    _ => 0.65,
                };
                let clump = value_noise(x / 9.0, y / 9.0, 23);
                if clump < 1.0 - density * 0.85 || hash(gx, gy, 24) > density {
                    continue;
                }
                out.push(Glyph::Tree { at: [x * s, y * s], kind: biome });
            }
        }
    }
}

fn near_water_or_river(world: &World, x: usize, y: usize) -> bool {
    let (w, h) = (world.terrain.width, world.terrain.height);
    for dy in -1isize..=1 {
        for dx in -1isize..=1 {
            let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
                continue;
            };
            if nx >= w || ny >= h {
                continue;
            }
            let i = ny * w + nx;
            if world.biomes.biome[i].is_water() || world.hydrology.segment_id[i] != NO_SEGMENT {
                return true;
            }
        }
    }
    false
}

fn draw_glyph(canvas: &mut Canvas, glyph: &Glyph, s: f32) {
    match *glyph {
        Glyph::Mountain { base, dir, height, half_width, skew, tint } => {
            let left = [base[0] - dir[0] * half_width, base[1] - dir[1] * half_width];
            let right = [base[0] + dir[0] * half_width, base[1] + dir[1] * half_width];
            let peak = [base[0] + dir[0] * half_width * skew, base[1] - height + dir[1] * half_width * skew];
            // Paper-coloured body hides glyphs behind it.
            canvas.triangle(left, peak, right, tint);
            // Shadow on the right flank: short strokes from the slope towards
            // the foot of the mountain.
            let lerp = |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            let lines = 2 + (height / (3.0 * s)) as usize;
            for k in 1..=lines {
                let start = lerp(peak, right, k as f32 / (lines + 1) as f32);
                canvas.line(start, lerp(start, base, 0.45), 0.4 * s, INK, 0.7);
            }
            canvas.line(left, peak, 0.55 * s, INK, 1.0);
            canvas.line(peak, right, 0.55 * s, INK, 1.0);
        }
        Glyph::Hill { base, half_width, height } => {
            let mut points = Vec::with_capacity(7);
            for k in 0..=6 {
                let u = k as f32 / 6.0;
                points.push([
                    base[0] + (u - 0.5) * 2.0 * half_width,
                    base[1] - height * (std::f32::consts::PI * u).sin(),
                ]);
            }
            let widths = vec![0.45 * s; points.len()];
            canvas.polyline(&points, &widths, INK, 0.85);
        }
        Glyph::Tree { at, kind } => {
            let trunk = |canvas: &mut Canvas, top: f32, bottom: f32| {
                canvas.line([at[0], top], [at[0], bottom], 0.35 * s, INK, 0.9);
            };
            match kind {
                Biome::BorealForest => {
                    let (top, base_y, hw) = (at[1] - 2.0 * s, at[1] + 0.8 * s, 1.0 * s);
                    canvas.triangle([at[0] - hw, base_y], [at[0], top], [at[0] + hw, base_y], FOREST_FILL);
                    canvas.line([at[0] - hw, base_y], [at[0], top], 0.35 * s, INK, 0.9);
                    canvas.line([at[0], top], [at[0] + hw, base_y], 0.35 * s, INK, 0.9);
                    trunk(canvas, base_y, base_y + 0.7 * s);
                }
                Biome::TropicalForest | Biome::TropicalRainforest => {
                    for (dx, dy) in [(-0.7, 0.3), (0.7, 0.3), (0.0, -0.6)] {
                        canvas.dot([at[0] + dx * s, at[1] + dy * s], 0.75 * s, INK, 0.85);
                        canvas.dot([at[0] + dx * s, at[1] + dy * s], 0.4 * s, FOREST_FILL, 1.0);
                    }
                }
                _ => {
                    let r = 1.2 * s;
                    trunk(canvas, at[1] + r * 0.6, at[1] + r + 0.8 * s);
                    canvas.dot(at, r, INK, 0.9);
                    canvas.dot(at, r - 0.45 * s, FOREST_FILL, 1.0);
                }
            }
        }
    }
}

/// Paper grain, mottling and darkened, browned edges.
fn age(canvas: &mut Canvas, s: f32) {
    let (w, h) = (canvas.width, canvas.height);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32, y as f32);
            let grain = (hash(x as i32, y as i32, 77) - 0.5) * 0.05;
            let mottle = (value_noise(fx / (24.0 * s), fy / (24.0 * s), 78) - 0.5) * 0.08
                + (value_noise(fx / (5.0 * s), fy / (5.0 * s), 79) - 0.5) * 0.03;
            let (dx, dy) = (fx / w as f32 * 2.0 - 1.0, fy / h as f32 * 2.0 - 1.0);
            let edge = ((dx * dx + dy * dy) * 0.5).powf(1.6);
            let p = &mut canvas.pixels[y * w + x];
            *p = mix(*p, AGED_EDGE, edge * 0.35);
            let k = 1.0 + grain + mottle - edge * 0.12;
            *p = p.map(|v| v * k);
        }
    }
}

/// Double-ruled border.
fn frame(canvas: &mut Canvas, s: f32) {
    let (w, h) = (canvas.width as f32, canvas.height as f32);
    for (inset, width) in [(3.0 * s, 0.9 * s), (5.0 * s, 0.45 * s)] {
        let (a, b, c, d) = ([inset, inset], [w - inset, inset], [w - inset, h - inset], [inset, h - inset]);
        for (p, q) in [(a, b), (b, c), (c, d), (d, a)] {
            canvas.line(p, q, width, INK, 0.9);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen_core::WorldConfig;

    fn small_world() -> World {
        let mut config = WorldConfig {
            width: 96,
            height: 96,
            ..Default::default()
        };
        config.erosion.droplets = 2000;
        World::generate(&config)
    }

    #[test]
    fn renders_at_the_requested_scale() {
        let world = small_world();
        let canvas = AntiqueStyle { scale: 2 }.render(&world);
        assert_eq!((canvas.width, canvas.height), (192, 192));
        assert!(canvas.pixels.iter().all(|p| p.iter().all(|v| v.is_finite())));
    }

    #[test]
    fn rendering_is_deterministic() {
        let world = small_world();
        let style = AntiqueStyle::default();
        assert_eq!(style.render(&world), style.render(&world));
    }
}
