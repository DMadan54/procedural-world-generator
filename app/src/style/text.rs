//! Map lettering: IM Fell English (a 17th-century typeface, SIL OFL; see
//! `app/assets/fonts/OFL.txt`) rendered straight into a [`Canvas`] with
//! `ab_glyph`, so labels are part of the image and survive export.

use std::sync::OnceLock;

use ab_glyph::{point, Font, FontRef, Glyph, PxScale, ScaleFont};

use super::canvas::{Canvas, Colour};

pub static REGULAR: &[u8] = include_bytes!("../../assets/fonts/IMFeENrm28P.ttf");
static ITALIC: &[u8] = include_bytes!("../../assets/fonts/IMFeENit28P.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Regular,
    Italic,
}

fn font(face: Face) -> &'static FontRef<'static> {
    static FONTS: OnceLock<(FontRef<'static>, FontRef<'static>)> = OnceLock::new();
    let (regular, italic) = FONTS.get_or_init(|| {
        (
            FontRef::try_from_slice(REGULAR).expect("bundled regular font is valid"),
            FontRef::try_from_slice(ITALIC).expect("bundled italic font is valid"),
        )
    });
    match face {
        Face::Regular => regular,
        Face::Italic => italic,
    }
}

/// Size of a line of text: advance width plus the font's ascent and descent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

/// Measures `text` at `size` pixels, with `tracking` extra pixels between
/// letters.
pub fn measure(text: &str, face: Face, size: f32, tracking: f32) -> Metrics {
    let scaled = font(face).as_scaled(PxScale::from(size));
    let mut width = 0.0;
    let mut previous = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(prev) = previous {
            width += scaled.kern(prev, id) + tracking;
        }
        width += scaled.h_advance(id);
        previous = Some(id);
    }
    Metrics {
        width,
        ascent: scaled.ascent(),
        descent: -scaled.descent(),
    }
}

/// Draws `text` with its baseline starting at `origin`. A `halo` colour
/// first paints a soft outline so the ink stays legible over hatching.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    canvas: &mut Canvas,
    text: &str,
    face: Face,
    size: f32,
    tracking: f32,
    origin: [f32; 2],
    ink: Colour,
    halo: Option<Colour>,
) {
    let scaled = font(face).as_scaled(PxScale::from(size));
    let mut glyphs = Vec::new();
    let mut x = origin[0];
    let mut previous = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(prev) = previous {
            x += scaled.kern(prev, id) + tracking;
        }
        glyphs.push(id.with_scale_and_position(size, point(x, origin[1])));
        x += scaled.h_advance(id);
        previous = Some(id);
    }
    if let Some(halo) = halo {
        let r = (size / 12.0).max(1.0);
        for (dx, dy) in [
            (-r, 0.0),
            (r, 0.0),
            (0.0, -r),
            (0.0, r),
            (-r, -r),
            (r, r),
            (-r, r),
            (r, -r),
        ] {
            for g in &glyphs {
                let mut g = g.clone();
                g.position.x += dx;
                g.position.y += dy;
                rasterise(canvas, face, g, halo, 0.55);
            }
        }
    }
    for g in glyphs {
        rasterise(canvas, face, g, ink, 1.0);
    }
}

fn rasterise(canvas: &mut Canvas, face: Face, glyph: Glyph, colour: Colour, alpha: f32) {
    if let Some(outlined) = font(face).outline_glyph(glyph) {
        let bounds = outlined.px_bounds();
        outlined.draw(|gx, gy, coverage| {
            let (x, y) = (
                bounds.min.x as i32 + gx as i32,
                bounds.min.y as i32 + gy as i32,
            );
            if x >= 0 && y >= 0 {
                canvas.blend(x as usize, y as usize, colour, coverage * alpha);
            }
        });
    }
}

/// Draws one character centred on `centre`, rotated by `angle` radians
/// (clockwise in screen space), for lettering that follows a curve.
#[allow(clippy::too_many_arguments)]
pub fn draw_rotated_char(
    canvas: &mut Canvas,
    c: char,
    face: Face,
    size: f32,
    centre: [f32; 2],
    angle: f32,
    ink: Colour,
    alpha: f32,
) {
    let font = font(face);
    let scaled = font.as_scaled(PxScale::from(size));
    let id = scaled.glyph_id(c);
    let advance = scaled.h_advance(id);
    // Rasterise upright into a small buffer with the glyph's centre at the
    // buffer centre, then sample it rotated.
    let half = size.ceil() as i32 + 2;
    let side = (2 * half) as usize;
    let mut buffer = vec![0.0f32; side * side];
    let baseline = half as f32 + (scaled.ascent() + scaled.descent()) * 0.5;
    let glyph = id.with_scale_and_position(size, point(half as f32 - advance * 0.5, baseline));
    let Some(outlined) = font.outline_glyph(glyph) else {
        return;
    };
    let bounds = outlined.px_bounds();
    outlined.draw(|gx, gy, coverage| {
        let (x, y) = (
            bounds.min.x as i32 + gx as i32,
            bounds.min.y as i32 + gy as i32,
        );
        if (0..side as i32).contains(&x) && (0..side as i32).contains(&y) {
            buffer[y as usize * side + x as usize] = coverage;
        }
    });
    let (sin, cos) = angle.sin_cos();
    let reach = half as f32 * std::f32::consts::SQRT_2;
    let (x0, y0) = (
        (centre[0] - reach).floor().max(0.0) as usize,
        (centre[1] - reach).floor().max(0.0) as usize,
    );
    let x1 = ((centre[0] + reach).ceil().max(0.0) as usize).min(canvas.width);
    let y1 = ((centre[1] + reach).ceil().max(0.0) as usize).min(canvas.height);
    for y in y0..y1 {
        for x in x0..x1 {
            // Inverse-rotate the canvas pixel into the upright buffer.
            let (dx, dy) = (x as f32 + 0.5 - centre[0], y as f32 + 0.5 - centre[1]);
            let (u, v) = (
                cos * dx + sin * dy + half as f32,
                -sin * dx + cos * dy + half as f32,
            );
            let coverage = sample(&buffer, side, u - 0.5, v - 0.5);
            if coverage > 0.0 {
                canvas.blend(x, y, ink, coverage * alpha);
            }
        }
    }
}

fn sample(buffer: &[f32], side: usize, x: f32, y: f32) -> f32 {
    if x < 0.0 || y < 0.0 || x >= (side - 1) as f32 || y >= (side - 1) as f32 {
        return 0.0;
    }
    let (x0, y0) = (x as usize, y as usize);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |x: usize, y: usize| buffer[y * side + x];
    let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
    let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// Advance width of one character, for laying letters along a curve.
pub fn advance(c: char, face: Face, size: f32) -> f32 {
    let scaled = font(face).as_scaled(PxScale::from(size));
    scaled.h_advance(scaled.glyph_id(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_load_and_measure() {
        let m = measure("Velmar", Face::Italic, 20.0, 0.0);
        assert!(m.width > 30.0 && m.width < 120.0, "{m:?}");
        assert!(m.ascent > 10.0);
        let wider = measure("Velmar", Face::Italic, 20.0, 3.0);
        assert!(
            (wider.width - m.width - 15.0).abs() < 0.01,
            "tracking adds 5 gaps"
        );
    }

    #[test]
    fn drawing_puts_ink_on_the_canvas() {
        let mut canvas = Canvas::new(120, 40, [1.0; 3]);
        draw(
            &mut canvas,
            "Gra",
            Face::Regular,
            24.0,
            0.0,
            [10.0, 30.0],
            [0.0; 3],
            Some([0.9; 3]),
        );
        assert!(canvas.pixels.iter().any(|p| p[0] < 0.3));
        let mut rotated = Canvas::new(60, 60, [1.0; 3]);
        draw_rotated_char(
            &mut rotated,
            'W',
            Face::Regular,
            30.0,
            [30.0, 30.0],
            0.4,
            [0.0; 3],
            1.0,
        );
        assert!(rotated.pixels.iter().any(|p| p[0] < 0.3));
    }
}
