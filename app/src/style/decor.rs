//! Decorations for exported maps, engraved like the rest of the antique
//! style: a title cartouche, a compass rose and a scale bar.

use super::canvas::{Canvas, Colour};
use super::text::{self, Face};

const INK: Colour = [0.24, 0.18, 0.12];
const PAPER: Colour = [0.93, 0.88, 0.75];
const VERMILION: Colour = [0.62, 0.16, 0.10];

/// What to write in the cartouche, and how big a cell is.
pub struct Decorations<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    /// Leagues per world cell, for the scale bar.
    pub leagues_per_cell: f32,
    /// Canvas pixels per world cell.
    pub cell_px: f32,
}

pub fn decorate(canvas: &mut Canvas, d: &Decorations) {
    // Scale with the image, but never so small it can't be read.
    let s = (canvas.width.min(canvas.height) as f32 / 512.0).max(0.6);
    cartouche(canvas, d.title, d.subtitle, s);
    compass(
        canvas,
        [canvas.width as f32 - 34.0 * s, 40.0 * s],
        22.0 * s,
        s,
    );
    scale_bar(canvas, d, s);
}

/// A filled rectangle, from two triangles.
fn panel(canvas: &mut Canvas, [x0, y0]: [f32; 2], [x1, y1]: [f32; 2], colour: Colour) {
    canvas.triangle([x0, y0], [x1, y0], [x1, y1], colour);
    canvas.triangle([x0, y0], [x1, y1], [x0, y1], colour);
}

fn rule(canvas: &mut Canvas, [x0, y0]: [f32; 2], [x1, y1]: [f32; 2], width: f32) {
    for (p, q) in [
        ([x0, y0], [x1, y0]),
        ([x1, y0], [x1, y1]),
        ([x1, y1], [x0, y1]),
        ([x0, y1], [x0, y0]),
    ] {
        canvas.line(p, q, width, INK, 0.95);
    }
}

/// Title and subtitle in a double-ruled panel at the bottom left, with
/// small diamonds either side of a dividing rule.
fn cartouche(canvas: &mut Canvas, title: &str, subtitle: &str, s: f32) {
    let (title_size, sub_size) = (17.0 * s, 9.5 * s);
    let t = text::measure(title, Face::Regular, title_size, 0.6 * s);
    let u = text::measure(subtitle, Face::Italic, sub_size, 0.0);
    let inner = t.width.max(u.width) + 18.0 * s;
    let height = t.ascent + t.descent + u.ascent + u.descent + 20.0 * s;
    let x0 = 16.0 * s;
    let y1 = canvas.height as f32 - 16.0 * s;
    let (a, b) = ([x0, y1 - height], [x0 + inner, y1]);
    panel(canvas, a, b, PAPER);
    rule(canvas, a, b, 1.1 * s);
    rule(
        canvas,
        [a[0] + 2.5 * s, a[1] + 2.5 * s],
        [b[0] - 2.5 * s, b[1] - 2.5 * s],
        0.5 * s,
    );
    let cx = (a[0] + b[0]) / 2.0;
    let title_y = a[1] + 6.0 * s + t.ascent;
    text::draw(
        canvas,
        title,
        Face::Regular,
        title_size,
        0.6 * s,
        [cx - t.width / 2.0, title_y],
        INK,
        None,
    );
    let rule_y = title_y + t.descent + 3.0 * s;
    let half = t.width.max(u.width) / 2.0 - 6.0 * s;
    canvas.line([cx - half, rule_y], [cx + half, rule_y], 0.5 * s, INK, 0.9);
    for x in [cx - half - 3.0 * s, cx + half + 3.0 * s] {
        let k = 1.8 * s;
        canvas.triangle([x - k, rule_y], [x, rule_y - k], [x + k, rule_y], VERMILION);
        canvas.triangle([x - k, rule_y], [x, rule_y + k], [x + k, rule_y], VERMILION);
    }
    let sub_y = rule_y + 3.0 * s + u.ascent;
    text::draw(
        canvas,
        subtitle,
        Face::Italic,
        sub_size,
        0.0,
        [cx - u.width / 2.0, sub_y],
        INK,
        None,
    );
}

/// A sixteen-point compass rose: shaded cardinal points, plain
/// half-cardinals and fine by-points, two rings, and the N above.
fn compass(canvas: &mut Canvas, [x, y]: [f32; 2], r: f32, s: f32) {
    canvas.dot([x, y], r * 1.05, PAPER, 0.75);
    let point = |canvas: &mut Canvas, angle: f32, len: f32, half: f32, shaded: bool| {
        let (dx, dy) = (angle.sin(), -angle.cos());
        let tip = [x + dx * len, y + dy * len];
        let (lx, ly) = (x - dy * half, y + dx * half);
        let (rx, ry) = (x + dy * half, y - dx * half);
        // Each point is two half-triangles, one dark, one light, as if lit
        // from the west.
        canvas.triangle([x, y], tip, [lx, ly], if shaded { INK } else { PAPER });
        canvas.triangle([x, y], tip, [rx, ry], if shaded { PAPER } else { INK });
        canvas.polyline(&[[lx, ly], tip, [rx, ry]], &[0.35 * s; 3], INK, 0.95);
    };
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_8};
    for i in 0..8 {
        point(
            canvas,
            FRAC_PI_8 + i as f32 * FRAC_PI_4,
            0.55 * r,
            0.08 * r,
            false,
        );
    }
    for i in 0..4 {
        point(
            canvas,
            FRAC_PI_4 + i as f32 * FRAC_PI_2,
            0.75 * r,
            0.13 * r,
            true,
        );
    }
    for i in 0..4 {
        point(canvas, i as f32 * FRAC_PI_2, r, 0.17 * r, true);
    }
    for (radius, width) in [(0.42 * r, 0.4 * s), (0.36 * r, 0.25 * s)] {
        let ring: Vec<[f32; 2]> = (0..=48)
            .map(|k| {
                let a = k as f32 / 48.0 * std::f32::consts::TAU;
                [x + radius * a.cos(), y + radius * a.sin()]
            })
            .collect();
        canvas.polyline(&ring, &vec![width; ring.len()], INK, 0.9);
    }
    canvas.dot([x, y], 0.07 * r, VERMILION, 1.0);
    let size = 0.5 * r;
    let m = text::measure("N", Face::Regular, size, 0.0);
    text::draw(
        canvas,
        "N",
        Face::Regular,
        size,
        0.0,
        [x - m.width / 2.0, y - r - 2.0 * s],
        INK,
        Some(PAPER),
    );
}

/// A bar of alternating ink and paper blocks at the bottom right, a round
/// number of leagues long.
fn scale_bar(canvas: &mut Canvas, d: &Decorations, s: f32) {
    let px_per_league = d.cell_px / d.leagues_per_cell.max(1e-6);
    // About a sixth of the map wide, rounded to 1, 2 or 5 × a power of ten.
    let target = canvas.width as f32 / 6.0 / px_per_league;
    let magnitude = 10f32.powf(target.max(1e-6).log10().floor());
    let leagues = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * magnitude)
        .rfind(|&l| l <= target)
        .unwrap_or(magnitude);
    let length = leagues * px_per_league;
    let (x1, y) = (
        canvas.width as f32 - 20.0 * s,
        canvas.height as f32 - 26.0 * s,
    );
    let x0 = x1 - length;
    let h = 3.0 * s;
    for i in 0..4 {
        let (a, b) = (
            x0 + length * i as f32 / 4.0,
            x0 + length * (i + 1) as f32 / 4.0,
        );
        panel(
            canvas,
            [a, y],
            [b, y + h],
            if i % 2 == 0 { INK } else { PAPER },
        );
    }
    rule(canvas, [x0, y], [x1, y + h], 0.5 * s);
    let size = 7.0 * s;
    for (x, label) in [(x0, "0".to_owned()), (x1, format!("{leagues:.0}"))] {
        let m = text::measure(&label, Face::Regular, size, 0.0);
        text::draw(
            canvas,
            &label,
            Face::Regular,
            size,
            0.0,
            [x - m.width / 2.0, y - 2.0 * s],
            INK,
            Some(PAPER),
        );
    }
    let caption = "Leagues";
    let m = text::measure(caption, Face::Italic, size, 0.0);
    text::draw(
        canvas,
        caption,
        Face::Italic,
        size,
        0.0,
        [(x0 + x1) / 2.0 - m.width / 2.0, y + h + m.ascent + 1.5 * s],
        INK,
        Some(PAPER),
    );
}
