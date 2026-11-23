//! Political colouring for the antique style: a hand-applied watercolour
//! wash just inside each realm's edge, and dotted frontiers between realms.

use worldgen_core::history::HistoryState;
use worldgen_core::realms::NO_REALM;
use worldgen_core::World;

use super::canvas::{Canvas, Colour};
use super::util::{bilinear, distance_to};

const INK: Colour = [0.24, 0.18, 0.12];

/// The muted wash colours of hand-coloured antique maps. Realm colour
/// indices come from graph colouring, so neighbours always differ.
pub const PALETTE: [Colour; 10] = [
    [0.80, 0.45, 0.42], // rose
    [0.85, 0.68, 0.32], // ochre
    [0.52, 0.66, 0.45], // sage
    [0.45, 0.55, 0.72], // slate blue
    [0.62, 0.50, 0.70], // lavender
    [0.78, 0.52, 0.36], // terracotta
    [0.62, 0.62, 0.32], // olive
    [0.40, 0.62, 0.62], // teal grey
    [0.78, 0.55, 0.62], // dusty pink
    [0.58, 0.45, 0.35], // umber
];

pub fn realm_colour(index: u8) -> Colour {
    PALETTE[index as usize % PALETTE.len()]
}

/// Washes colour along the inside of every realm's edge (coasts included),
/// strongest at the edge and fading inward, the way colourists tinted
/// printed maps by hand.
pub fn wash(canvas: &mut Canvas, state: &HistoryState, s: f32) {
    let r = state;
    let (w, h) = (r.width, r.height);
    if w == 0 || h == 0 {
        return;
    }
    // Edge cells: owned cells with a 4-neighbour owned by anyone else.
    let edge: Vec<bool> = (0..w * h)
        .map(|i| {
            let o = r.owner[i];
            if o == NO_REALM {
                return false;
            }
            let (x, y) = (i % w, i / w);
            [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ]
            .iter()
            .any(|&(nx, ny)| nx >= w || ny >= h || r.owner[ny * w + nx] != o)
        })
        .collect();
    let distance = distance_to(&edge, w, h);
    let band = 7.0 * w as f32 / 512.0;
    let scale = s as usize;
    for cy in 0..canvas.height {
        for cx in 0..canvas.width {
            let (mx, my) = (cx / scale, cy / scale);
            if mx >= w || my >= h {
                continue;
            }
            let owner = r.owner[my * w + mx];
            if owner == NO_REALM {
                continue;
            }
            let d = bilinear(
                &distance,
                w,
                h,
                (cx as f32 + 0.5) / s - 0.5,
                (cy as f32 + 0.5) / s - 0.5,
            );
            if d < band {
                let strength = 0.45 * (1.0 - d / band).powf(1.3);
                canvas.blend(
                    cx,
                    cy,
                    realm_colour(r.realms[owner as usize].colour),
                    strength,
                );
            }
        }
    }
}

/// Dotted frontiers: firm dots between two realms, faint sparse dots where a
/// realm gives way to unclaimed wilderness.
pub fn frontiers(canvas: &mut Canvas, world: &World, state: &HistoryState, s: f32) {
    let r = state;
    let (w, h) = (r.width, r.height);
    let land = |i: usize| !world.biomes.biome[i].is_water();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !land(i) {
                continue;
            }
            for (nx, ny, horizontal) in [(x + 1, y, true), (x, y + 1, false)] {
                if nx >= w || ny >= h {
                    continue;
                }
                let j = ny * w + nx;
                let (a, b) = (r.owner[i], r.owner[j]);
                if a == b || !land(j) {
                    continue;
                }
                // The midpoint of the shared cell edge.
                let at = if horizontal {
                    [(x + 1) as f32 * s, (y as f32 + 0.5) * s]
                } else {
                    [(x as f32 + 0.5) * s, (y + 1) as f32 * s]
                };
                if a != NO_REALM && b != NO_REALM {
                    if (x + y) % 2 == 0 {
                        canvas.dot(at, 0.5 * s, INK, 0.85);
                    }
                } else if (x + y) % 3 == 0 {
                    canvas.dot(at, 0.35 * s, INK, 0.45);
                }
            }
        }
    }
}
