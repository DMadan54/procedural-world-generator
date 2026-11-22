//! A small software canvas with anti-aliased primitives. Independent of egui
//! so styles can also render for export.

pub type Colour = [f32; 3];

/// RGB image, components in `0.0..=1.0` (sRGB-encoded).
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Colour>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, fill: Colour) -> Self {
        Self {
            width,
            height,
            pixels: vec![fill; width * height],
        }
    }

    /// Mixes `colour` into a pixel with opacity `alpha`.
    #[inline]
    pub fn blend(&mut self, x: usize, y: usize, colour: Colour, alpha: f32) {
        if x >= self.width || y >= self.height || alpha <= 0.0 {
            return;
        }
        let a = alpha.min(1.0);
        let p = &mut self.pixels[y * self.width + x];
        for k in 0..3 {
            p[k] += (colour[k] - p[k]) * a;
        }
    }

    /// Pixel range covering `[lo, hi]` on one axis, clipped to the canvas.
    fn span(lo: f32, hi: f32, len: usize) -> std::ops::Range<usize> {
        let a = lo.floor().max(0.0) as usize;
        let b = (hi.ceil().max(0.0) as usize).min(len);
        a..b.max(a)
    }

    /// Anti-aliased line with round caps.
    pub fn line(&mut self, a: [f32; 2], b: [f32; 2], width: f32, colour: Colour, alpha: f32) {
        let r = width * 0.5;
        let pad = r + 1.0;
        let (ab_x, ab_y) = (b[0] - a[0], b[1] - a[1]);
        let len2 = (ab_x * ab_x + ab_y * ab_y).max(1e-12);
        for y in Self::span(a[1].min(b[1]) - pad, a[1].max(b[1]) + pad, self.height) {
            for x in Self::span(a[0].min(b[0]) - pad, a[0].max(b[0]) + pad, self.width) {
                let (px, py) = (x as f32 + 0.5 - a[0], y as f32 + 0.5 - a[1]);
                let t = ((px * ab_x + py * ab_y) / len2).clamp(0.0, 1.0);
                let (dx, dy) = (px - t * ab_x, py - t * ab_y);
                let d = (dx * dx + dy * dy).sqrt();
                let coverage = (r + 0.5 - d).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    self.blend(x, y, colour, alpha * coverage);
                }
            }
        }
    }

    /// Connected line through `points`, with the width varying per point.
    pub fn polyline(&mut self, points: &[[f32; 2]], widths: &[f32], colour: Colour, alpha: f32) {
        for i in 1..points.len() {
            let w = 0.5 * (widths[i - 1] + widths[i]);
            self.line(points[i - 1], points[i], w, colour, alpha);
        }
    }

    /// Anti-aliased filled disc.
    pub fn dot(&mut self, centre: [f32; 2], radius: f32, colour: Colour, alpha: f32) {
        let pad = radius + 1.0;
        for y in Self::span(centre[1] - pad, centre[1] + pad, self.height) {
            for x in Self::span(centre[0] - pad, centre[0] + pad, self.width) {
                let (dx, dy) = (x as f32 + 0.5 - centre[0], y as f32 + 0.5 - centre[1]);
                let coverage = (radius + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    self.blend(x, y, colour, alpha * coverage);
                }
            }
        }
    }

    /// Filled triangle (hard edges; outline it for smoothness).
    pub fn triangle(&mut self, a: [f32; 2], b: [f32; 2], c: [f32; 2], colour: Colour) {
        let edge = |p: [f32; 2], q: [f32; 2], x: f32, y: f32| {
            (q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0])
        };
        let area = edge(a, b, c[0], c[1]);
        if area.abs() < 1e-6 {
            return;
        }
        let sign = area.signum();
        let ys = Self::span(
            a[1].min(b[1]).min(c[1]),
            a[1].max(b[1]).max(c[1]),
            self.height,
        );
        let xs = Self::span(
            a[0].min(b[0]).min(c[0]),
            a[0].max(b[0]).max(c[0]),
            self.width,
        );
        for y in ys {
            for x in xs.clone() {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                if edge(a, b, px, py) * sign >= 0.0
                    && edge(b, c, px, py) * sign >= 0.0
                    && edge(c, a, px, py) * sign >= 0.0
                {
                    self.pixels[y * self.width + x] = colour;
                }
            }
        }
    }

    /// 8-bit RGB bytes, row-major.
    pub fn to_rgb8(&self) -> Vec<u8> {
        self.pixels
            .iter()
            .flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Colour = [1.0, 1.0, 1.0];
    const K: Colour = [0.0, 0.0, 0.0];

    #[test]
    fn line_covers_its_path_and_not_far_away() {
        let mut c = Canvas::new(20, 20, W);
        c.line([2.0, 10.0], [18.0, 10.0], 2.0, K, 1.0);
        assert!(c.pixels[10 * 20 + 10][0] < 0.1);
        assert_eq!(c.pixels[2 * 20 + 10], W);
    }

    #[test]
    fn dot_is_round() {
        let mut c = Canvas::new(20, 20, W);
        c.dot([10.0, 10.0], 4.0, K, 1.0);
        assert!(c.pixels[10 * 20 + 10][0] < 0.1);
        assert_eq!(c.pixels[19 * 20 + 19], W);
    }

    #[test]
    fn triangle_fills_inside_only() {
        let mut c = Canvas::new(20, 20, W);
        c.triangle([2.0, 18.0], [10.0, 2.0], [18.0, 18.0], K);
        assert_eq!(c.pixels[15 * 20 + 10], K);
        assert_eq!(c.pixels[3 * 20 + 2], W);
    }

    #[test]
    fn drawing_off_canvas_is_safe() {
        let mut c = Canvas::new(10, 10, W);
        c.line([-50.0, -50.0], [60.0, 70.0], 3.0, K, 1.0);
        c.dot([-5.0, 3.0], 4.0, K, 1.0);
        c.triangle([-10.0, -10.0], [30.0, 0.0], [0.0, 30.0], K);
    }
}
