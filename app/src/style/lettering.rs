//! Settlement glyphs, settlement labels and people names for the antique
//! style.

use worldgen_core::hydrology::NO_SEGMENT;
use worldgen_core::settlements::{Rank, Settlement};
use worldgen_core::World;

use super::canvas::{Canvas, Colour};
use super::text::{self, Face};

const INK: Colour = [0.24, 0.18, 0.12];
const PAPER: Colour = [0.93, 0.88, 0.75];
/// People names in the muted vermilion old maps used for regions: it stays
/// legible over brown ink and green woods without needing a halo.
const PEOPLE_INK: Colour = [0.58, 0.2, 0.13];

/// Radius of a settlement's glyph in canvas pixels.
fn glyph_radius(rank: Rank, s: f32) -> f32 {
    match rank {
        Rank::Capital => 3.4 * s,
        Rank::City => 2.4 * s,
        Rank::Town => 1.7 * s,
        Rank::Village => 1.0 * s,
    }
}

fn label_style(rank: Rank, s: f32) -> (Face, f32) {
    match rank {
        Rank::Capital => (Face::Regular, 11.0 * s),
        Rank::City => (Face::Italic, 8.5 * s),
        Rank::Town => (Face::Italic, 7.0 * s),
        Rank::Village => (Face::Italic, 5.8 * s),
    }
}

fn centre(settlement: &Settlement, width: usize, s: f32) -> [f32; 2] {
    let [x, y] = settlement.position(width);
    [(x as f32 + 0.5) * s, (y as f32 + 0.5) * s]
}

/// Draws every settlement glyph.
pub fn draw_settlements(canvas: &mut Canvas, world: &World, s: f32) {
    let w = world.settlements.width;
    for settlement in &world.settlements.settlements {
        let c = centre(settlement, w, s);
        let r = glyph_radius(settlement.rank, s);
        // Clear a little paper so hatching and trees don't run into the mark.
        canvas.dot(c, r + 0.7 * s, PAPER, 1.0);
        match settlement.rank {
            Rank::Capital => {
                ring(canvas, c, r, 0.55 * s);
                star(canvas, c, r * 0.68, r * 0.28);
            }
            Rank::City => {
                ring(canvas, c, r, 0.5 * s);
                canvas.dot(c, 0.85 * s, INK, 1.0);
            }
            Rank::Town => ring(canvas, c, r, 0.45 * s),
            Rank::Village => canvas.dot(c, r, INK, 1.0),
        }
    }
}

fn ring(canvas: &mut Canvas, c: [f32; 2], r: f32, width: f32) {
    let steps = 28;
    let mut points = Vec::with_capacity(steps + 1);
    for k in 0..=steps {
        let a = k as f32 / steps as f32 * std::f32::consts::TAU;
        points.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
    }
    canvas.polyline(&points, &vec![width; points.len()], INK, 1.0);
}

/// A five-pointed star, point up.
fn star(canvas: &mut Canvas, c: [f32; 2], outer: f32, inner: f32) {
    let point = |k: usize, r: f32, offset: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + (k as f32 + offset) * std::f32::consts::TAU / 5.0;
        [c[0] + r * a.cos(), c[1] + r * a.sin()]
    };
    for k in 0..5 {
        let (tip, left, right) = (
            point(k, outer, 0.0),
            point(k, inner, -0.5),
            point(k, inner, 0.5),
        );
        canvas.triangle(c, left, tip, INK);
        canvas.triangle(c, tip, right, INK);
    }
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl Rect {
    fn overlaps(&self, other: &Rect) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.y0 < other.y1 && other.y0 < self.y1
    }
}

/// Places and draws settlement labels: most important first, each trying
/// eight positions around its glyph. A position is rejected if it overlaps
/// a glyph or an earlier label or leaves the frame; among the rest the one
/// covering the least water and river wins. Labels that fit nowhere are
/// dropped, so the smallest places go first.
pub fn draw_settlement_labels(canvas: &mut Canvas, world: &World, s: f32) {
    let w = world.settlements.width;
    let list = &world.settlements.settlements;
    let mut occupied: Vec<Rect> = list
        .iter()
        .map(|st| {
            let c = centre(st, w, s);
            let r = glyph_radius(st.rank, s) + 0.5 * s;
            Rect {
                x0: c[0] - r,
                y0: c[1] - r,
                x1: c[0] + r,
                y1: c[1] + r,
            }
        })
        .collect();

    let mut order: Vec<&Settlement> = list.iter().collect();
    order.sort_by(|a, b| {
        b.rank
            .cmp(&a.rank)
            .then(b.population.cmp(&a.population))
            .then(a.id.cmp(&b.id))
    });

    let inset = 7.0 * s;
    let (cw, ch) = (canvas.width as f32, canvas.height as f32);
    for settlement in order {
        let (face, size) = label_style(settlement.rank, s);
        let m = text::measure(&settlement.name, face, size, 0.0);
        let c = centre(settlement, w, s);
        let g = glyph_radius(settlement.rank, s) + 0.9 * s;
        let mid = m.ascent * 0.35;
        // Baseline-left origins for E, W, N, S and the diagonals.
        let candidates = [
            [c[0] + g, c[1] + mid],
            [c[0] - g - m.width, c[1] + mid],
            [c[0] - m.width / 2.0, c[1] - g - m.descent],
            [c[0] - m.width / 2.0, c[1] + g + m.ascent],
            [c[0] + g * 0.7, c[1] - g * 0.7],
            [c[0] + g * 0.7, c[1] + g * 0.7 + m.ascent],
            [c[0] - g * 0.7 - m.width, c[1] - g * 0.7],
            [c[0] - g * 0.7 - m.width, c[1] + g * 0.7 + m.ascent],
        ];
        let mut best: Option<(f32, [f32; 2], Rect)> = None;
        for (k, origin) in candidates.into_iter().enumerate() {
            let rect = Rect {
                x0: origin[0],
                y0: origin[1] - m.ascent,
                x1: origin[0] + m.width,
                y1: origin[1] + m.descent,
            };
            if rect.x0 < inset || rect.y0 < inset || rect.x1 > cw - inset || rect.y1 > ch - inset {
                continue;
            }
            if occupied.iter().any(|o| o.overlaps(&rect)) {
                continue;
            }
            // Prefer the classic east position, then by what lies underneath.
            let penalty = water_under(world, &rect, s) + 0.02 * k as f32;
            if best.as_ref().is_none_or(|b| penalty < b.0) {
                best = Some((penalty, origin, rect));
            }
        }
        if let Some((_, origin, rect)) = best {
            occupied.push(rect);
            text::draw(
                canvas,
                &settlement.name,
                face,
                size,
                0.0,
                origin,
                INK,
                Some(PAPER),
            );
        }
    }
}

/// Share of world cells under `rect` that are water or river.
fn water_under(world: &World, rect: &Rect, s: f32) -> f32 {
    let (w, h) = (world.biomes.width, world.biomes.height);
    let (x0, y0) = (
        (rect.x0 / s).max(0.0) as usize,
        (rect.y0 / s).max(0.0) as usize,
    );
    let (x1, y1) = (
        ((rect.x1 / s) as usize + 1).min(w),
        ((rect.y1 / s) as usize + 1).min(h),
    );
    let (mut wet, mut n) = (0.0, 0.0);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = y * w + x;
            n += 1.0;
            if world.biomes.biome[i].is_water() {
                wet += 1.0;
            } else if world.hydrology.segment_id[i] != NO_SEGMENT {
                wet += 0.6;
            }
        }
    }
    if n == 0.0 {
        0.0
    } else {
        wet / n
    }
}

/// Each sizeable people's name in widely spaced capitals, on a gentle arch
/// along the main axis of its homeland.
pub fn draw_people_names(canvas: &mut Canvas, world: &World, s: f32) {
    let p = &world.peoples;
    let (w, h) = (p.width, p.height);
    let min_cells = (w * h) as u32 / 150;
    for people in &p.peoples {
        if people.cells < min_cells {
            continue;
        }
        // Centroid and principal axis of the homeland.
        let cells: Vec<[f32; 2]> = (0..w * h)
            .filter(|&i| p.homeland[i] == people.id)
            .map(|i| [(i % w) as f32 + 0.5, (i / w) as f32 + 0.5])
            .collect();
        let n = cells.len() as f32;
        let (mx, my) = (
            cells.iter().map(|c| c[0]).sum::<f32>() / n,
            cells.iter().map(|c| c[1]).sum::<f32>() / n,
        );
        let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
        for c in &cells {
            let (dx, dy) = (c[0] - mx, c[1] - my);
            sxx += dx * dx;
            syy += dy * dy;
            sxy += dx * dy;
        }
        let mut angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
        // Keep lettering readable: left to right, never steeper than 30°.
        let limit = 30f32.to_radians();
        angle = angle.clamp(-limit, limit);
        let (sin, cos) = angle.sin_cos();
        let mut along: Vec<f32> = cells
            .iter()
            .map(|c| (c[0] - mx) * cos + (c[1] - my) * sin)
            .collect();
        along.sort_by(|a, b| a.total_cmp(b));
        let span = (along[(along.len() as f32 * 0.95) as usize - 1]
            - along[(along.len() as f32 * 0.05) as usize])
            * s;

        let name = people.name.to_uppercase();
        let letters: Vec<char> = name.chars().collect();
        let unit: f32 = letters
            .iter()
            .map(|&c| text::advance(c, Face::Regular, 1.0))
            .sum();
        let gap_ratio = 0.45;
        let length = 0.7 * span;
        let size =
            (length / (unit + gap_ratio * (letters.len() as f32 - 1.0))).clamp(8.0 * s, 16.0 * s);
        let gap = gap_ratio * size;
        let total: f32 = letters
            .iter()
            .map(|&c| text::advance(c, Face::Regular, size))
            .sum::<f32>()
            + gap * (letters.len() as f32 - 1.0);

        // Walk along a shallow arch (∩): the perpendicular offset grows with
        // the square of the distance from the middle.
        let half = total / 2.0;
        let bow = 0.08 * total;
        let normal = [-sin, cos];
        let centre = [mx * s, my * s];
        let mut t = -half;
        for &c in &letters {
            let a = text::advance(c, Face::Regular, size);
            let mid = t + a / 2.0;
            let u = mid / half;
            let offset = bow * u * u;
            let slope = (2.0 * bow * mid / (half * half)).atan();
            let at = [
                centre[0] + cos * mid + normal[0] * offset,
                centre[1] + sin * mid + normal[1] * offset,
            ];
            text::draw_rotated_char(
                canvas,
                c,
                Face::Regular,
                size,
                at,
                angle + slope,
                PEOPLE_INK,
                0.78,
            );
            t += a + gap;
        }
    }
}
