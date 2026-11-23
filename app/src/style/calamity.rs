//! Disasters on the antique map: rivers in their course for the year (with
//! abandoned beds as dotted oxbows), smoking volcanoes, plague crosses on
//! afflicted towns, and migration arrows that fade.

use worldgen_core::history::{EventKind, HistoryState};
use worldgen_core::World;

use super::canvas::{Canvas, Colour};
use super::network::walk;
use super::util::chaikin;

pub const INK_RIVER: Colour = [0.27, 0.38, 0.48];
const INK: Colour = [0.24, 0.18, 0.12];
const PAPER: Colour = [0.93, 0.88, 0.75];
const VERMILION: Colour = [0.62, 0.16, 0.10];
const ASH: Colour = [0.45, 0.42, 0.38];

/// River width on the map for a discharge, as the landscape draws it.
pub fn river_width(discharge: f32, threshold: f32, s: f32) -> f32 {
    (0.3 + 0.3 * (discharge / threshold).sqrt()).clamp(0.4, 1.7) * s
}

/// Where each river that moves during history leaves its old course:
/// `(segment, at)`. The landscape draws those segments only down to `at`;
/// the rest depends on the year.
pub fn avulsions(world: &World) -> Vec<(u32, u32)> {
    world
        .history
        .events
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::RiverAvulsion { segment, at, .. } => Some((segment, at)),
            _ => None,
        })
        .collect()
}

/// The moving rivers as they run in this year: the old lower course before
/// the river breaks out; afterwards the new channel, with the old bed as a
/// faint dotted oxbow.
pub fn draw_moving_rivers(canvas: &mut Canvas, world: &World, state: &HistoryState, s: f32) {
    let threshold = world.config.hydrology.river_threshold.max(1e-3);
    for e in &world.history.events {
        let EventKind::RiverAvulsion {
            segment,
            at,
            old_course,
            new_course,
            ..
        } = &e.kind
        else {
            continue;
        };
        let seg = &world.hydrology.segments[*segment as usize];
        let at = *at as usize;
        let to_px = |p: &[f32; 2]| [(p[0] + 0.5) * s, (p[1] + 0.5) * s];
        if state.year < e.year {
            let points: Vec<[f32; 2]> = seg.points[at..]
                .iter()
                .map(|p| [p[0] * s, p[1] * s])
                .collect();
            let widths: Vec<f32> = seg.discharge[at..]
                .iter()
                .map(|&q| river_width(q, threshold, s))
                .collect();
            let (points, widths) = chaikin(&points, &widths, 2);
            canvas.polyline(&points, &widths, INK_RIVER, 0.9);
            continue;
        }
        // The new course carries the whole river.
        let q = seg.discharge[at.min(seg.discharge.len() - 1)];
        let points: Vec<[f32; 2]> = new_course.iter().map(to_px).collect();
        let widths = vec![river_width(q, threshold, s); points.len()];
        let (points, widths) = chaikin(&points, &widths, 2);
        canvas.polyline(&points, &widths, INK_RIVER, 0.9);
        // The abandoned bed.
        let old: Vec<[f32; 2]> = old_course.iter().map(to_px).collect();
        let (old, _) = chaikin(&old, &vec![1.0; old.len()], 2);
        walk(&old, 1.3 * s, |p, _, _| {
            canvas.dot(p, 0.3 * s, INK_RIVER, 0.55)
        });
    }
}

/// A volcano for each eruption: a cone with a crater, smoking for a few
/// decades after it last erupted, a quiet cone after that.
pub fn draw_volcanoes(canvas: &mut Canvas, state: &HistoryState, s: f32) {
    let w = state.width;
    // Each mountain once, as of its latest eruption.
    let mut latest: Vec<&worldgen_core::history::Eruption> = Vec::new();
    for e in &state.eruptions {
        match latest.iter_mut().find(|l| l.name == e.name) {
            Some(l) => *l = e,
            None => latest.push(e),
        }
    }
    for e in latest {
        let c = e.cell as usize;
        let [x, y] = [((c % w) as f32 + 0.5) * s, ((c / w) as f32 + 0.5) * s];
        let k = 4.2 * s;
        let age = state.year.saturating_sub(e.year) as f32;
        canvas.dot([x, y - 0.5 * k], 1.6 * k, PAPER, 0.5);
        // The cone, shaded on its right flank like the relief glyphs.
        let (left, right, top_l, top_r) = (
            [x - 1.3 * k, y + 0.6 * k],
            [x + 1.3 * k, y + 0.6 * k],
            [x - 0.35 * k, y - 0.7 * k],
            [x + 0.35 * k, y - 0.7 * k],
        );
        canvas.triangle(top_r, right, [x + 0.2 * k, y + 0.6 * k], [0.80, 0.72, 0.58]);
        canvas.polyline(&[left, top_l, top_r, right], &[0.55 * s; 4], INK, 0.95);
        // Hachures down the flanks.
        for i in 1..4 {
            let t = i as f32 / 4.0;
            let top = [top_l[0] + (top_r[0] - top_l[0]) * t, top_l[1]];
            let foot = [left[0] + (right[0] - left[0]) * t, left[1] - 0.15 * k];
            canvas.line(
                top,
                [
                    top[0] + (foot[0] - top[0]) * 0.55,
                    top[1] + (foot[1] - top[1]) * 0.55,
                ],
                0.25 * s,
                INK,
                0.6,
            );
        }
        if age > 40.0 {
            continue;
        }
        // Fresh: fire in the crater and a plume of smoke, thinning with age.
        let fresh = 1.0 - age / 40.0;
        canvas.triangle(top_l, top_r, [x, y - 0.35 * k], VERMILION);
        for i in 0..5 {
            let t = i as f32;
            let r = (0.35 + 0.18 * t) * k;
            let at = [x + 0.25 * k * t * t * 0.25, y - 1.0 * k - 0.55 * k * t];
            canvas.dot(at, r, ASH, 0.45 * fresh);
            canvas.dot(at, r * 0.6, PAPER, 0.25 * fresh);
        }
    }
}

/// A red cross on every town where plague is raging this year.
pub fn draw_plague(canvas: &mut Canvas, state: &HistoryState, s: f32) {
    let w = state.width;
    for st in state
        .settlements
        .iter()
        .filter(|st| st.alive && st.plague > 0.0)
    {
        let c = st.cell as usize;
        let [x, y] = [((c % w) as f32 + 0.5) * s, ((c / w) as f32 + 0.5) * s];
        let k = (1.0 + 2.0 * st.plague.sqrt()) * s;
        let at = [x + 1.6 * s, y - 2.2 * s];
        canvas.dot(at, 0.9 * k, PAPER, 0.8);
        canvas.line(
            [at[0] - 0.7 * k, at[1]],
            [at[0] + 0.7 * k, at[1]],
            0.4 * s,
            VERMILION,
            0.95,
        );
        canvas.line(
            [at[0], at[1] - 0.7 * k],
            [at[0], at[1] + 0.7 * k],
            0.4 * s,
            VERMILION,
            0.95,
        );
    }
}

/// Arrows from each settlement people fled to where they went, fading over
/// fifteen years.
pub fn draw_migrations(canvas: &mut Canvas, state: &HistoryState, s: f32) {
    const FADE: f32 = 15.0;
    let w = state.width;
    for m in &state.migrations {
        let age = state.year.saturating_sub(m.year) as f32;
        if age > FADE {
            continue;
        }
        let alpha = 0.85 * (1.0 - age / FADE);
        let centre = |id: u32| {
            let c = state.settlements[id as usize].cell as usize;
            [((c % w) as f32 + 0.5) * s, ((c / w) as f32 + 0.5) * s]
        };
        let (a, b) = (centre(m.from), centre(m.to));
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 4.0 * s {
            continue;
        }
        let (ux, uy) = (dx / len, dy / len);
        // A gentle arc, so arrows read as journeys rather than borders.
        let bow = 0.12 * len;
        let mid = [
            (a[0] + b[0]) / 2.0 - uy * bow,
            (a[1] + b[1]) / 2.0 + ux * bow,
        ];
        let start = [a[0] + ux * 3.0 * s, a[1] + uy * 3.0 * s];
        let end = [b[0] - ux * 3.0 * s, b[1] - uy * 3.0 * s];
        let points: Vec<[f32; 2]> = (0..=20)
            .map(|i| {
                let t = i as f32 / 20.0;
                let u = 1.0 - t;
                [
                    u * u * start[0] + 2.0 * u * t * mid[0] + t * t * end[0],
                    u * u * start[1] + 2.0 * u * t * mid[1] + t * t * end[1],
                ]
            })
            .collect();
        let width = (0.25 + 0.15 * (m.people as f32 / 500.0).min(3.0)) * s;
        walk(&points, 1.6 * s, |p, dir, k| {
            if k % 2 == 0 {
                canvas.line(
                    p,
                    [p[0] + dir[0] * 1.0 * s, p[1] + dir[1] * 1.0 * s],
                    width,
                    VERMILION,
                    alpha,
                );
            }
        });
        // The head, along the curve's last direction.
        let (p, q) = (points[18], points[20]);
        let (hx, hy) = (q[0] - p[0], q[1] - p[1]);
        let hl = (hx * hx + hy * hy).sqrt().max(1e-3);
        let (hx, hy) = (hx / hl, hy / hl);
        let back = [q[0] - hx * 1.8 * s, q[1] - hy * 1.8 * s];
        canvas.triangle(
            q,
            [back[0] - hy * 0.9 * s, back[1] + hx * 0.9 * s],
            [back[0] + hy * 0.9 * s, back[1] - hx * 0.9 * s],
            VERMILION,
        );
    }
}
