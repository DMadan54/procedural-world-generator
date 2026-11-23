//! Roads, sea lanes and resource marks for the antique style.

use worldgen_core::resources::Resource;
use worldgen_core::roads::LinkKind;
use worldgen_core::settlements::NO_SETTLEMENT;
use worldgen_core::World;

use super::canvas::{Canvas, Colour};
use super::util::{chaikin, hash};

const INK: Colour = [0.24, 0.18, 0.12];
const INK_WATER: Colour = [0.33, 0.42, 0.45];

fn smoothed(world: &World, cells: &[u32], s: f32) -> Vec<[f32; 2]> {
    let w = world.roads.width;
    let points: Vec<[f32; 2]> = cells
        .iter()
        .map(|&c| {
            [
                ((c as usize % w) as f32 + 0.5) * s,
                ((c as usize / w) as f32 + 0.5) * s,
            ]
        })
        .collect();
    let widths = vec![1.0; points.len()];
    chaikin(&points, &widths, 2).0
}

/// Calls `mark` at points every `step` pixels along a polyline, with the
/// local direction.
fn walk(points: &[[f32; 2]], step: f32, mut mark: impl FnMut([f32; 2], [f32; 2], usize)) {
    let mut carry = 0.0;
    let mut count = 0;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            continue;
        }
        let dir = [dx / len, dy / len];
        let mut t = step - carry;
        while t <= len {
            mark([a[0] + dir[0] * t, a[1] + dir[1] * t], dir, count);
            count += 1;
            t += step;
        }
        carry = (carry + len) % step;
    }
}

/// Roads as fine dotted lines; sea lanes as faint dashes with a small ship
/// now and then. River links follow rivers already drawn, so they're left
/// alone.
pub fn draw_network(canvas: &mut Canvas, world: &World, s: f32) {
    for link in &world.roads.links {
        let points = smoothed(world, &link.cells, s);
        match link.kind {
            LinkKind::Road => walk(&points, 1.4 * s, |p, _, _| {
                canvas.dot(p, 0.42 * s, INK, 0.85)
            }),
            LinkKind::Sea => {
                let length: f32 = points
                    .windows(2)
                    .map(|p| ((p[1][0] - p[0][0]).powi(2) + (p[1][1] - p[0][1]).powi(2)).sqrt())
                    .sum();
                walk(&points, 2.4 * s, |p, dir, k| {
                    if k % 2 == 0 {
                        let end = [p[0] + dir[0] * 1.4 * s, p[1] + dir[1] * 1.4 * s];
                        canvas.line(p, end, 0.3 * s, INK_WATER, 0.55);
                    }
                });
                // A ship on longer lanes, about a third of the way along.
                if length > 40.0 * s && hash(link.a as i32, link.b as i32, 61) < 0.7 {
                    let mut placed = false;
                    walk(&points, length / 3.0, |p, _, k| {
                        if k == 0 && !placed {
                            ship(canvas, p, s);
                            placed = true;
                        }
                    });
                }
            }
            LinkKind::River => {}
        }
    }
}

/// A small sailing ship: hull, mast and a sail.
fn ship(canvas: &mut Canvas, at: [f32; 2], s: f32) {
    let [x, y] = at;
    let hull = [
        [x - 2.0 * s, y],
        [x - 1.2 * s, y + 0.9 * s],
        [x + 1.2 * s, y + 0.9 * s],
        [x + 2.0 * s, y],
    ];
    canvas.polyline(&hull, &[0.35 * s; 4], INK, 0.85);
    canvas.line([x, y], [x, y - 2.8 * s], 0.25 * s, INK, 0.85);
    canvas.triangle(
        [x + 0.2 * s, y - 2.6 * s],
        [x + 0.2 * s, y - 0.4 * s],
        [x + 1.6 * s, y - 0.6 * s],
        [0.9, 0.86, 0.74],
    );
    canvas.line(
        [x + 0.2 * s, y - 2.6 * s],
        [x + 1.6 * s, y - 0.6 * s],
        0.22 * s,
        INK,
        0.8,
    );
}

/// Small marks for the richer deposits (timber is left to the forests):
/// crossed picks for ore, a cut stone for gems, a sheaf for grain and a
/// ruled pan for salt. Kept sparse and away from settlements.
pub fn draw_resources(canvas: &mut Canvas, world: &World, s: f32) {
    let w = world.resources.width;
    let h = world.resources.height;
    for deposit in &world.resources.deposits {
        if deposit.resource == Resource::Timber || deposit.richness < 0.55 {
            continue;
        }
        let (cx, cy) = (deposit.cell as usize % w, deposit.cell as usize / w);
        let crowded = (cy.saturating_sub(3)..(cy + 4).min(h)).any(|y| {
            (cx.saturating_sub(3)..(cx + 4).min(w))
                .any(|x| world.settlements.at_cell[y * w + x] != NO_SETTLEMENT)
        });
        if crowded {
            continue;
        }
        let [x, y] = [(cx as f32 + 0.5) * s, (cy as f32 + 0.5) * s];
        let k = 1.1 * s;
        canvas.dot([x, y], 2.2 * k, [0.93, 0.88, 0.75], 0.85);
        match deposit.resource {
            Resource::Ore => {
                canvas.line([x - k, y + k], [x + k, y - k], 0.3 * s, INK, 0.9);
                canvas.line([x + k, y + k], [x - k, y - k], 0.3 * s, INK, 0.9);
                canvas.line(
                    [x + 0.5 * k, y - 1.3 * k],
                    [x + 1.4 * k, y - 0.6 * k],
                    0.3 * s,
                    INK,
                    0.9,
                );
                canvas.line(
                    [x - 0.5 * k, y - 1.3 * k],
                    [x - 1.4 * k, y - 0.6 * k],
                    0.3 * s,
                    INK,
                    0.9,
                );
            }
            Resource::Gems => {
                let d = [
                    [x, y - 1.3 * k],
                    [x + k, y - 0.3 * k],
                    [x, y + 1.3 * k],
                    [x - k, y - 0.3 * k],
                    [x, y - 1.3 * k],
                ];
                canvas.polyline(&d, &[0.28 * s; 5], INK, 0.9);
                canvas.line(
                    [x - k, y - 0.3 * k],
                    [x + k, y - 0.3 * k],
                    0.22 * s,
                    INK,
                    0.8,
                );
            }
            Resource::Grain => {
                for dx in [-0.7, 0.0, 0.7] {
                    canvas.line(
                        [x, y + 1.2 * k],
                        [x + dx * k, y - 1.2 * k],
                        0.28 * s,
                        INK,
                        0.9,
                    );
                }
                canvas.line(
                    [x - 0.45 * k, y + 0.2 * k],
                    [x + 0.45 * k, y + 0.2 * k],
                    0.3 * s,
                    INK,
                    0.9,
                );
            }
            Resource::Salt => {
                let r = [
                    [x - k, y - 0.7 * k],
                    [x + k, y - 0.7 * k],
                    [x + k, y + 0.7 * k],
                    [x - k, y + 0.7 * k],
                    [x - k, y - 0.7 * k],
                ];
                canvas.polyline(&r, &[0.25 * s; 5], INK, 0.85);
                for dy in [-0.25, 0.25] {
                    canvas.line(
                        [x - 0.7 * k, y + dy * k],
                        [x + 0.7 * k, y + dy * k],
                        0.18 * s,
                        INK_WATER,
                        0.8,
                    );
                }
            }
            Resource::Timber => {}
        }
    }
}
