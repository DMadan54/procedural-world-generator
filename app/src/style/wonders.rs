//! Landmarks on the antique map: small engraved vignettes (a coiled dragon,
//! broken columns, a radiant star, towers under the waves), faint leylines,
//! and the Blight as a cross-hatched stain that grows and shrinks with the
//! years.

use worldgen_core::history::{Dragon, HistoryState, LandmarkState};
use worldgen_core::landmarks::LandmarkKind;
use worldgen_core::World;

use super::canvas::{Canvas, Colour};
use super::network::walk;
use super::text::{self, Face};
use super::util::{bilinear, hash};

const INK: Colour = [0.24, 0.18, 0.12];
const PAPER: Colour = [0.93, 0.88, 0.75];
const VERMILION: Colour = [0.62, 0.16, 0.10];
const MADDER: Colour = [0.42, 0.22, 0.48];
const INK_WATER: Colour = [0.33, 0.42, 0.45];
const WATER: Colour = [0.79, 0.81, 0.73];
const BLIGHT: Colour = [0.36, 0.38, 0.20];
const BLIGHT_INK: Colour = [0.22, 0.22, 0.10];

/// Leylines as faint dashed lines in madder, part of the landscape (they
/// never move).
pub fn draw_leylines(canvas: &mut Canvas, world: &World, s: f32) {
    for line in &world.landmarks.leylines {
        let points: Vec<[f32; 2]> = line.points.iter().map(|p| [p[0] * s, p[1] * s]).collect();
        walk(&points, 2.6 * s, |p, dir, k| {
            if k % 2 == 0 {
                let end = [p[0] + dir[0] * 1.5 * s, p[1] + dir[1] * 1.5 * s];
                canvas.line(p, end, 0.4 * s, MADDER, 0.5);
            }
        });
    }
}

/// The Blight at this year: a sickly wash with cross-hatching and a ragged
/// darker edge, smoothed so it reads as a stain rather than cells.
pub fn draw_blight(canvas: &mut Canvas, world: &World, state: &HistoryState, s: f32) {
    if !state.blight.iter().any(|&b| b) {
        return;
    }
    let (w, h) = (state.width, state.height);
    // 3×3 average so the edge falls between cells.
    let mut mask = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            let mut n = 0.0;
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    sum += state.blight[ny * w + nx] as u8 as f32;
                    n += 1.0;
                }
            }
            mask[y * w + x] = sum / n;
        }
    }
    let spacing = (2.2 * s).max(3.0);
    let thickness = (0.35 * s).max(1.0);
    for cy in 0..canvas.height {
        for cx in 0..canvas.width {
            let (x, y) = ((cx as f32 + 0.5) / s - 0.5, (cy as f32 + 0.5) / s - 0.5);
            let v = bilinear(&mask, w, h, x, y);
            if v < 0.3 {
                continue;
            }
            let cell = (cy / s as usize).min(h - 1) * w + (cx / s as usize).min(w - 1);
            if world.biomes.biome[cell].is_water() {
                continue;
            }
            let strength = ((v - 0.3) / 0.25).clamp(0.0, 1.0);
            canvas.blend(cx, cy, BLIGHT, 0.3 * strength);
            let (a, b) = (
                (cx + cy) as f32 % spacing,
                (cx + 4 * canvas.height - cy) as f32 % spacing,
            );
            if a < thickness || b < thickness {
                canvas.blend(cx, cy, BLIGHT_INK, 0.45 * strength);
            }
            // The ragged edge.
            let edge = 1.0 - ((v - 0.36).abs() / 0.06).min(1.0);
            let ragged = 0.6 + 0.4 * hash(cx as i32, cy as i32, 91);
            if edge > 0.0 {
                canvas.blend(cx, cy, BLIGHT_INK, 0.6 * edge * ragged);
            }
        }
    }
}

/// Every landmark standing at this year, with a small italic name.
pub fn draw_landmarks(canvas: &mut Canvas, state: &HistoryState, s: f32) {
    let w = state.width;
    for landmark in &state.landmarks {
        let c = landmark.cell as usize;
        let at = [((c % w) as f32 + 0.5) * s, ((c / w) as f32 + 0.5) * s];
        let k = 3.0 * s;
        // A soft paper ground so the vignette reads over hatching and relief.
        canvas.dot([at[0], at[1] - 0.3 * k], 1.9 * k, PAPER, 0.55);
        canvas.dot([at[0], at[1] - 0.3 * k], 1.5 * k, PAPER, 0.45);
        // Strokes a little heavier than the base scale, to suit the size.
        let pen = 1.6 * s;
        let ink = match landmark.kind {
            LandmarkKind::DragonDen => dragon(canvas, at, k, pen, landmark.dragon),
            LandmarkKind::ElderRuin => columns(canvas, at, k, pen, INK),
            LandmarkKind::BlightedRuin => columns(canvas, at, k, pen, BLIGHT_INK),
            LandmarkKind::LeylineNexus => star(canvas, at, k, pen),
            LandmarkKind::DrownedCity => towers(canvas, at, k, pen),
        };
        label(canvas, landmark, at, k, s, ink);
    }
}

fn label(canvas: &mut Canvas, landmark: &LandmarkState, at: [f32; 2], k: f32, s: f32, ink: Colour) {
    let size = 5.0 * s;
    let m = text::measure(&landmark.name, Face::Italic, size, 0.0);
    let origin = [at[0] - m.width / 2.0, at[1] + 1.3 * k + m.ascent];
    text::draw(
        canvas,
        &landmark.name,
        Face::Italic,
        size,
        0.0,
        origin,
        ink,
        Some(PAPER),
    );
}

/// A dragon coiled on its hoard: a tapering spiral body, a wedge of a head
/// and one bat wing. Ink while it sleeps, vermilion when awake, and a faint
/// ghost with a sword through it once slain. Returns the label ink.
fn dragon(canvas: &mut Canvas, [x, y]: [f32; 2], k: f32, s: f32, state: Option<Dragon>) -> Colour {
    let (ink, alpha) = match state {
        Some(Dragon::Awake) => (VERMILION, 0.95),
        Some(Dragon::Slain) => (INK, 0.55),
        _ => (INK, 0.9),
    };
    let centre = [x, y + 0.25 * k];
    let n = 40;
    let mut points = Vec::with_capacity(n + 1);
    let mut widths = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let angle = -0.6 + t * 3.2 * std::f32::consts::PI;
        let r = 1.25 * k * (1.0 - 0.78 * t);
        points.push([
            centre[0] + r * angle.cos(),
            centre[1] + 0.7 * r * angle.sin(),
        ]);
        widths.push((0.65 - 0.5 * t) * s);
    }
    // Wing first, so the body crosses over it.
    let root = [x - 0.1 * k, y - 0.2 * k];
    let tips = [
        [x + 0.3 * k, y - 1.9 * k],
        [x + 1.0 * k, y - 1.6 * k],
        [x + 1.5 * k, y - 1.0 * k],
    ];
    canvas.triangle(root, tips[0], tips[2], PAPER);
    for tip in tips {
        canvas.line(root, tip, 0.22 * s, ink, alpha);
    }
    canvas.polyline(&[tips[0], tips[1], tips[2]], &[0.2 * s; 3], ink, alpha);
    canvas.polyline(&points, &widths, ink, alpha);
    // The head: a wedge at the outer end, pointing along the coil.
    let (head, next) = (points[0], points[2]);
    let (dx, dy) = (head[0] - next[0], head[1] - next[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (ux, uy) = (dx / len, dy / len);
    let snout = [head[0] + ux * 0.7 * k, head[1] + uy * 0.7 * k];
    let side = [-uy * 0.3 * k, ux * 0.3 * k];
    canvas.triangle(
        [head[0] + side[0], head[1] + side[1]],
        [head[0] - side[0], head[1] - side[1]],
        snout,
        ink,
    );
    if state == Some(Dragon::Slain) {
        // A sword driven into the coil.
        let (top, point) = ([x + 0.2 * k, y - 1.2 * k], [x - 0.1 * k, y + 0.6 * k]);
        canvas.line(top, point, 0.3 * s, INK, 0.9);
        canvas.line(
            [x - 0.25 * k, y - 0.75 * k],
            [x + 0.55 * k, y - 0.6 * k],
            0.28 * s,
            INK,
            0.9,
        );
        return INK;
    }
    ink
}

/// Broken columns on a stepped base.
fn columns(canvas: &mut Canvas, [x, y]: [f32; 2], k: f32, s: f32, ink: Colour) -> Colour {
    let base = y + 0.9 * k;
    canvas.line([x - 1.4 * k, base], [x + 1.4 * k, base], 0.4 * s, ink, 0.9);
    canvas.line(
        [x - 1.1 * k, base - 0.25 * k],
        [x + 1.1 * k, base - 0.25 * k],
        0.25 * s,
        ink,
        0.8,
    );
    let shafts = [(-0.95, 1.9), (-0.32, 1.2), (0.32, 1.7), (0.95, 0.7)];
    for (dx, height) in shafts {
        let cx = x + dx * k;
        let top = base - 0.25 * k - height * k;
        canvas.line([cx, base - 0.25 * k], [cx, top], 0.5 * s, ink, 0.9);
        // Fluting: a pale line down the shaft.
        canvas.line(
            [cx, base - 0.35 * k],
            [cx, top + 0.15 * k],
            0.12 * s,
            PAPER,
            0.8,
        );
        // A jagged break on top.
        canvas.line(
            [cx - 0.25 * k, top + 0.1 * k],
            [cx + 0.25 * k, top - 0.12 * k],
            0.22 * s,
            ink,
            0.9,
        );
    }
    // A lintel still resting on the two tallest.
    let lintel = base - 0.25 * k - 1.9 * k;
    canvas.line(
        [x - 1.25 * k, lintel],
        [x - 0.05 * k, lintel + 0.25 * k],
        0.35 * s,
        ink,
        0.9,
    );
    ink
}

/// A radiant compass star: four long and four short rays.
fn star(canvas: &mut Canvas, [x, y]: [f32; 2], k: f32, s: f32) -> Colour {
    for i in 0..8 {
        let angle = i as f32 * std::f32::consts::FRAC_PI_4;
        let len = if i % 2 == 0 { 1.8 * k } else { 1.05 * k };
        let (dx, dy) = (angle.cos(), angle.sin());
        let tip = [x + dx * len, y + dy * len];
        let side = [-dy * 0.22 * k, dx * 0.22 * k];
        canvas.triangle(
            [x + side[0], y + side[1]],
            [x - side[0], y - side[1]],
            tip,
            MADDER,
        );
    }
    canvas.dot([x, y], 0.5 * k, MADDER, 1.0);
    canvas.dot([x, y], 0.32 * k, PAPER, 1.0);
    canvas.dot([x, y], 0.12 * k, MADDER, 1.0);
    let _ = s;
    MADDER
}

/// Spired towers half under the sea, with waves drawn across them.
fn towers(canvas: &mut Canvas, [x, y]: [f32; 2], k: f32, s: f32) -> Colour {
    let waterline = y + 0.2 * k;
    for (dx, height) in [(-0.8, 1.3), (0.0, 2.0), (0.75, 1.0)] {
        let cx = x + dx * k;
        let top = waterline - height * k;
        canvas.line(
            [cx, waterline + 0.8 * k],
            [cx, top],
            0.55 * s,
            INK_WATER,
            0.85,
        );
        canvas.triangle(
            [cx - 0.38 * k, top + 0.05 * k],
            [cx + 0.38 * k, top + 0.05 * k],
            [cx, top - 0.7 * k],
            INK_WATER,
        );
    }
    // Water over the lower half, then wave lines across it.
    for row in 0..3 {
        let wy = waterline + row as f32 * 0.4 * k;
        canvas.line(
            [x - 1.4 * k, wy + 0.15 * k],
            [x + 1.4 * k, wy + 0.15 * k],
            0.35 * k,
            WATER,
            0.8,
        );
        let points: Vec<[f32; 2]> = (0..=16)
            .map(|i| {
                let t = i as f32 / 16.0;
                [
                    x - 1.5 * k + 3.0 * k * t,
                    wy + 0.12 * k * (t * 4.0 * std::f32::consts::TAU).sin(),
                ]
            })
            .collect();
        canvas.polyline(&points, &vec![0.2 * s; points.len()], INK_WATER, 0.9);
    }
    INK_WATER
}
