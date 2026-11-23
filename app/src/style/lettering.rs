//! Settlement glyphs, settlement labels and people names for the antique
//! style.

use worldgen_core::history::{HistoryState, SettlementState};
use worldgen_core::hydrology::NO_SEGMENT;
use worldgen_core::settlements::Rank;
use worldgen_core::World;

use super::canvas::{Canvas, Colour};
use super::text::{self, Face};

const INK: Colour = [0.24, 0.18, 0.12];
const PAPER: Colour = [0.93, 0.88, 0.75];

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

fn centre(settlement: &SettlementState, width: usize, s: f32) -> [f32; 2] {
    let (x, y) = (
        settlement.cell as usize % width,
        settlement.cell as usize / width,
    );
    [(x as f32 + 0.5) * s, (y as f32 + 0.5) * s]
}

/// How a settlement is drawn in this year: a capital whose realm has fallen
/// is just a city.
fn shown_rank(state: &HistoryState, id: usize) -> Rank {
    let rank = state.settlements[id].rank;
    if rank == Rank::Capital
        && !state
            .realms
            .iter()
            .any(|r| r.alive && r.capital as usize == id)
    {
        Rank::City
    } else {
        rank
    }
}

/// Living settlements with how they're drawn, in id order.
fn living(state: &HistoryState) -> Vec<(usize, &SettlementState, Rank)> {
    state
        .settlements
        .iter()
        .enumerate()
        .filter(|(_, st)| st.alive)
        .map(|(id, st)| (id, st, shown_rank(state, id)))
        .collect()
}

/// Draws every settlement glyph.
pub fn draw_settlements(canvas: &mut Canvas, state: &HistoryState, s: f32) {
    let w = state.width;
    for (_, settlement, rank) in living(state) {
        let c = centre(settlement, w, s);
        let r = glyph_radius(rank, s);
        // Clear a little paper so hatching and trees don't run into the mark.
        canvas.dot(c, r + 0.7 * s, PAPER, 1.0);
        match rank {
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
pub fn draw_settlement_labels(canvas: &mut Canvas, world: &World, state: &HistoryState, s: f32) {
    let w = state.width;
    let list = living(state);
    let mut occupied: Vec<Rect> = list
        .iter()
        .map(|&(_, st, rank)| {
            let c = centre(st, w, s);
            let r = glyph_radius(rank, s) + 0.5 * s;
            Rect {
                x0: c[0] - r,
                y0: c[1] - r,
                x1: c[0] + r,
                y1: c[1] + r,
            }
        })
        .collect();

    let mut order = list.clone();
    order.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then(b.1.population.cmp(&a.1.population))
            .then(a.0.cmp(&b.0))
    });

    let inset = 7.0 * s;
    let (cw, ch) = (canvas.width as f32, canvas.height as f32);
    for (_, settlement, rank) in order {
        let (face, size) = label_style(rank, s);
        let m = text::measure(&settlement.name, face, size, 0.0);
        let c = centre(settlement, w, s);
        let g = glyph_radius(rank, s) + 0.9 * s;
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

/// How an arched region name is lettered.
struct ArchStyle {
    face: Face,
    colour: Colour,
    alpha: f32,
    /// Size limits in canvas pixels per world-cell scale.
    min_size: f32,
    max_size: f32,
    /// Gap between letters as a fraction of the size.
    gap_ratio: f32,
    /// Share of the region's length the name should span.
    coverage: f32,
}

/// Realm names: large vermilion capitals, as old maps lettered countries.
const REALM_ARCH: ArchStyle = ArchStyle {
    face: Face::Regular,
    colour: [0.58, 0.2, 0.13],
    alpha: 0.8,
    min_size: 8.0,
    max_size: 17.0,
    gap_ratio: 0.45,
    coverage: 0.7,
};

/// People names: smaller dark italic capitals, for peoples split between
/// realms (the political map already names the rest).
const PEOPLE_ARCH: ArchStyle = ArchStyle {
    face: Face::Italic,
    colour: [0.3, 0.22, 0.15],
    alpha: 0.6,
    min_size: 5.5,
    max_size: 9.0,
    gap_ratio: 0.6,
    coverage: 0.5,
};

/// Each living realm's name arched along its territory's main axis.
pub fn draw_realm_names(canvas: &mut Canvas, state: &HistoryState, s: f32) {
    let (w, h) = (state.width, state.height);
    let min_cells = (w * h) as u32 / 400;
    for (id, realm) in state.living_realms() {
        if realm.cells >= min_cells {
            let cells = cells_where(w, h, |i| state.owner[i] == id);
            draw_arched(
                canvas,
                &cells,
                &realm.short_name.to_uppercase(),
                s,
                &REALM_ARCH,
            );
        }
    }
}

/// Names of sizeable peoples whose homeland spans two or more realms, placed
/// over the part outside their main realm's name.
pub fn draw_people_names(canvas: &mut Canvas, world: &World, state: &HistoryState, s: f32) {
    let p = &world.peoples;
    let (w, h) = (p.width, p.height);
    let min_cells = (w * h) as u32 / 150;
    for people in &p.peoples {
        if people.cells < min_cells {
            continue;
        }
        let realms: std::collections::BTreeSet<u16> = (0..w * h)
            .filter(|&i| p.homeland[i] == people.id)
            .map(|i| state.owner[i])
            .filter(|&o| o != worldgen_core::realms::NO_REALM)
            .collect();
        if realms.len() < 2 {
            continue;
        }
        let cells = cells_where(w, h, |i| p.homeland[i] == people.id);
        draw_arched(canvas, &cells, &people.name.to_uppercase(), s, &PEOPLE_ARCH);
    }
}

fn cells_where(w: usize, h: usize, keep: impl Fn(usize) -> bool) -> Vec<[f32; 2]> {
    (0..w * h)
        .filter(|&i| keep(i))
        .map(|i| [(i % w) as f32 + 0.5, (i / w) as f32 + 0.5])
        .collect()
}

/// Letters `name` in spaced capitals on a gentle arch (∩) along the main axis
/// of a region (its centroid and principal axis from the covariance of its
/// cells), sized to span part of its length.
fn draw_arched(canvas: &mut Canvas, cells: &[[f32; 2]], name: &str, s: f32, style: &ArchStyle) {
    if cells.len() < 2 || name.is_empty() {
        return;
    }
    let n = cells.len() as f32;
    let (mx, my) = (
        cells.iter().map(|c| c[0]).sum::<f32>() / n,
        cells.iter().map(|c| c[1]).sum::<f32>() / n,
    );
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for c in cells {
        let (dx, dy) = (c[0] - mx, c[1] - my);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    // Keep lettering readable: left to right, never steeper than 30°.
    let limit = 30f32.to_radians();
    let angle = (0.5 * (2.0 * sxy).atan2(sxx - syy)).clamp(-limit, limit);
    let (sin, cos) = angle.sin_cos();
    let mut along: Vec<f32> = cells
        .iter()
        .map(|c| (c[0] - mx) * cos + (c[1] - my) * sin)
        .collect();
    along.sort_by(|a, b| a.total_cmp(b));
    let last = along.len() - 1;
    let span = (along[(last as f32 * 0.95) as usize] - along[(last as f32 * 0.05) as usize]) * s;

    let letters: Vec<char> = name.chars().collect();
    let unit: f32 = letters
        .iter()
        .map(|&c| text::advance(c, style.face, 1.0))
        .sum();
    let gaps = letters.len() as f32 - 1.0;
    let size = (style.coverage * span / (unit + style.gap_ratio * gaps))
        .clamp(style.min_size * s, style.max_size * s);
    let gap = style.gap_ratio * size;
    let total: f32 = letters
        .iter()
        .map(|&c| text::advance(c, style.face, size))
        .sum::<f32>()
        + gap * gaps;

    // The perpendicular offset grows with the square of the distance from
    // the middle, giving a shallow arch.
    let half = total / 2.0;
    let bow = 0.08 * total;
    let normal = [-sin, cos];
    let centre = [mx * s, my * s];
    let mut t = -half;
    for &c in &letters {
        let a = text::advance(c, style.face, size);
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
            style.face,
            size,
            at,
            angle + slope,
            style.colour,
            style.alpha,
        );
        t += a + gap;
    }
}
