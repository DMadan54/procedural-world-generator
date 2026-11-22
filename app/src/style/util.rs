//! Small deterministic helpers for styles: hashing, value noise, sampling and
//! distance fields. Styles must look the same every time a world is drawn,
//! so no RNG state is involved.

/// Uniform value in `0.0..1.0` from integer coordinates and a salt.
pub fn hash(x: i32, y: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ salt.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth value noise in `0.0..1.0`, one lattice cell per unit.
pub fn value_noise(x: f32, y: f32, salt: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (u, v) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let a = hash(ix, iy, salt);
    let b = hash(ix + 1, iy, salt);
    let c = hash(ix, iy + 1, salt);
    let d = hash(ix + 1, iy + 1, salt);
    a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}

/// Bilinear sample of a row-major grid at cell-centre coordinates, clamped.
pub fn bilinear(data: &[f32], width: usize, height: usize, x: f32, y: f32) -> f32 {
    let x = x.clamp(0.0, (width - 1) as f32);
    let y = y.clamp(0.0, (height - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
    let (u, v) = (x - x0 as f32, y - y0 as f32);
    let top = data[y0 * width + x0] * (1.0 - u) + data[y0 * width + x1] * u;
    let bottom = data[y1 * width + x0] * (1.0 - u) + data[y1 * width + x1] * u;
    top * (1.0 - v) + bottom * v
}

/// Approximate Euclidean distance (two-pass chamfer) from every pixel to the
/// nearest pixel where `source` is true.
pub fn distance_to(source: &[bool], width: usize, height: usize) -> Vec<f32> {
    const D: f32 = std::f32::consts::SQRT_2;
    let mut dist: Vec<f32> = source
        .iter()
        .map(|&s| if s { 0.0 } else { f32::INFINITY })
        .collect();
    let mut relax = |x: usize, y: usize, steps: &[(isize, isize, f32)]| {
        let i = y * width + x;
        for &(dx, dy, cost) in steps {
            let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
                continue;
            };
            if nx < width && ny < height {
                let candidate = dist[ny * width + nx] + cost;
                if candidate < dist[i] {
                    dist[i] = candidate;
                }
            }
        }
    };
    let forward = [(-1, 0, 1.0), (-1, -1, D), (0, -1, 1.0), (1, -1, D)];
    let backward = [(1, 0, 1.0), (1, 1, D), (0, 1, 1.0), (-1, 1, D)];
    for y in 0..height {
        for x in 0..width {
            relax(x, y, &forward);
        }
    }
    for y in (0..height).rev() {
        for x in (0..width).rev() {
            relax(x, y, &backward);
        }
    }
    dist
}

/// Chaikin corner cutting: rounds a polyline while keeping its endpoints.
/// `values` (e.g. widths) are interpolated alongside the points.
pub fn chaikin(points: &[[f32; 2]], values: &[f32], iterations: usize) -> (Vec<[f32; 2]>, Vec<f32>) {
    let (mut p, mut v) = (points.to_vec(), values.to_vec());
    for _ in 0..iterations {
        if p.len() < 3 {
            break;
        }
        let mut np = vec![p[0]];
        let mut nv = vec![v[0]];
        for i in 0..p.len() - 1 {
            let (a, b) = (p[i], p[i + 1]);
            let lerp = |t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            let vl = |t: f32| v[i] + (v[i + 1] - v[i]) * t;
            if i > 0 {
                np.push(lerp(0.25));
                nv.push(vl(0.25));
            }
            if i < p.len() - 2 {
                np.push(lerp(0.75));
                nv.push(vl(0.75));
            }
        }
        np.push(*p.last().unwrap());
        nv.push(*v.last().unwrap());
        p = np;
        v = nv;
    }
    (p, v)
}

pub fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_in_range() {
        assert_eq!(hash(3, 4, 5), hash(3, 4, 5));
        assert_ne!(hash(3, 4, 5), hash(4, 3, 5));
        for i in 0..1000 {
            assert!((0.0..1.0).contains(&hash(i, -i, 9)));
        }
    }

    #[test]
    fn distance_field_measures_from_sources() {
        let mut src = vec![false; 10 * 10];
        src[0] = true;
        let d = distance_to(&src, 10, 10);
        assert_eq!(d[0], 0.0);
        assert!((d[5] - 5.0).abs() < 1e-5);
        assert!((d[9 * 10 + 9] - 9.0 * std::f32::consts::SQRT_2).abs() < 1e-3);
    }

    #[test]
    fn chaikin_keeps_endpoints() {
        let pts = [[0.0, 0.0], [5.0, 5.0], [10.0, 0.0]];
        let (p, v) = chaikin(&pts, &[1.0, 2.0, 3.0], 2);
        assert_eq!(p.first(), Some(&[0.0, 0.0]));
        assert_eq!(p.last(), Some(&[10.0, 0.0]));
        assert_eq!(p.len(), v.len());
    }
}
