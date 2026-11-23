//! Time-lapse planning: which year each frame shows, and how borders and
//! the Blight move between the states sampled at "ticks".
//!
//! Cutting straight from one sampled state to the next makes borders jump.
//! Instead, every cell that changes hands between two ticks gets a flip
//! time in `0..1` from how far it lies from the land its new owner already
//! held (a multi-source breadth-first search over the changing cells), so
//! conquests sweep outward from the old frontier. The Blight grows out
//! from its old edge and is burnt back from outside the same way. At a
//! tick itself the frame shows exactly `state_at(tick)`.

use std::collections::VecDeque;

use crate::history::{EventKind, History, HistoryState};
use crate::realms::NO_REALM;

/// What the user asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct TimelapseOptions {
    pub from: u32,
    pub to: u32,
    pub fps: u32,
    pub years_per_second: f32,
    /// Pause on major events, for this many seconds.
    pub hold_seconds: f32,
}

impl TimelapseOptions {
    /// Years each frame moves on (may be fractional).
    pub fn years_per_frame(&self) -> f32 {
        (self.years_per_second / self.fps.max(1) as f32).max(1e-3)
    }

    /// Years between sampled states: about four frames per tick, at least
    /// a year, so borders get a few frames to sweep.
    pub fn tick_years(&self) -> u32 {
        (self.years_per_frame() * 4.0).round().max(1.0) as u32
    }
}

/// One frame: the year shown, which tick interval it lies in and how far
/// through it, and the major event it holds on, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSpec {
    /// The year whose events, labels and settlements are shown.
    pub year: u32,
    /// Ticks either side: `tick_from <= year <= tick_to`.
    pub tick_from: u32,
    pub tick_to: u32,
    /// `0..=1` between the two ticks.
    pub fraction: f32,
    /// Index of a major event this frame holds on (repeated frames).
    pub hold: Option<usize>,
}

/// Major events worth pausing on.
pub fn is_major(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::WarDeclared { .. }
            | EventKind::RealmFell { .. }
            | EventKind::RealmSplit { .. }
            | EventKind::RealmAnnexed { .. }
            | EventKind::VolcanoErupted { .. }
            | EventKind::PlagueOutbreak { .. }
            | EventKind::DragonSlain { .. }
            | EventKind::RiverAvulsion { .. }
    )
}

/// Every frame of the time-lapse, in order.
pub fn schedule(history: &History, options: &TimelapseOptions) -> Vec<FrameSpec> {
    let from = options.from.min(history.years);
    let to = options.to.clamp(from, history.years);
    let step = options.years_per_frame();
    let tick = options.tick_years();
    let hold_frames = (options.hold_seconds * options.fps as f32).round() as usize;
    let mut frames = Vec::new();
    let mut shown_until = from;
    let count = ((to - from) as f32 / step).floor() as usize + 1;
    for k in 0..count {
        let position = from as f32 + k as f32 * step;
        let year = (position.floor() as u32).min(to);
        let tick_from = from + (year - from) / tick * tick;
        let tick_to = (tick_from + tick).min(to).max(tick_from);
        let fraction = if tick_to == tick_from {
            0.0
        } else {
            ((position - tick_from as f32) / (tick_to - tick_from) as f32).clamp(0.0, 1.0)
        };
        let spec = FrameSpec {
            year,
            tick_from,
            tick_to,
            fraction,
            hold: None,
        };
        frames.push(spec.clone());
        // Hold on the first major event since the last frame.
        if hold_frames > 0 && (k == 0 || year > shown_until) {
            let first =
                history.first_event_at_or_after(if k == 0 { year } else { shown_until + 1 });
            let major = history.events[first..]
                .iter()
                .take_while(|e| e.year <= year)
                .position(|e| is_major(&e.kind))
                .map(|i| first + i);
            if let Some(event) = major {
                for _ in 0..hold_frames {
                    frames.push(FrameSpec {
                        hold: Some(event),
                        ..spec.clone()
                    });
                }
            }
        }
        shown_until = year;
    }
    frames
}

/// When each changing cell flips between two states, as `0..1`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    /// `(cell, new owner, flip time)` for every cell whose owner changes.
    pub owners: Vec<(u32, u16, f32)>,
    /// `(cell, blighted afterwards, flip time)` for every cell whose Blight
    /// changes.
    pub blight: Vec<(u32, bool, f32)>,
}

impl Sweep {
    pub fn between(a: &HistoryState, b: &HistoryState) -> Self {
        let (w, h) = (a.width, a.height);
        let owners = {
            let changed: Vec<bool> = (0..a.owner.len())
                .map(|i| a.owner[i] != b.owner[i])
                .collect();
            // Fronts: cells the new owner already held before.
            let times = flip_times(w, h, &changed, |cell, neighbour| {
                a.owner[neighbour] == b.owner[cell] && b.owner[cell] != NO_REALM
            });
            (0..a.owner.len())
                .filter(|&i| changed[i])
                .map(|i| (i as u32, b.owner[i], times[i]))
                .collect()
        };
        let blight = {
            let changed: Vec<bool> = (0..a.blight.len())
                .map(|i| a.blight[i] != b.blight[i])
                .collect();
            // Growth spreads from cells already blighted; purges burn in
            // from cells that were already clean.
            let times = flip_times(w, h, &changed, |cell, neighbour| {
                a.blight[neighbour] == b.blight[cell]
            });
            (0..a.blight.len())
                .filter(|&i| changed[i])
                .map(|i| (i as u32, b.blight[i], times[i]))
                .collect()
        };
        Self { owners, blight }
    }

    /// `a` moved `fraction` of the way to `b`: each changing cell takes its
    /// new value once its flip time has passed. At 0 this is `a`; at 1 it
    /// is `b`.
    pub fn apply(&self, state: &mut HistoryState, fraction: f32) {
        for &(cell, owner, t) in &self.owners {
            if fraction >= 1.0 || (fraction > 0.0 && t <= fraction) {
                state.owner[cell as usize] = owner;
            }
        }
        for &(cell, blighted, t) in &self.blight {
            if fraction >= 1.0 || (fraction > 0.0 && t <= fraction) {
                state.blight[cell as usize] = blighted;
            }
        }
    }
}

/// Breadth-first distance through `changed` cells from those touching a
/// source (as `is_source(cell, neighbour)` says), normalised to `(0, 1]`.
/// Changing cells no front reaches (a realm appearing from nothing) flip
/// halfway.
fn flip_times(
    w: usize,
    h: usize,
    changed: &[bool],
    is_source: impl Fn(usize, usize) -> bool,
) -> Vec<f32> {
    let mut distance = vec![u32::MAX; changed.len()];
    let mut queue = VecDeque::new();
    let neighbours = |c: usize| {
        let (x, y) = (c % w, c / w);
        [
            (x > 0).then(|| c - 1),
            (x + 1 < w).then(|| c + 1),
            (y > 0).then(|| c - w),
            (y + 1 < h).then(|| c + w),
        ]
        .into_iter()
        .flatten()
    };
    for c in 0..changed.len() {
        if changed[c] && neighbours(c).any(|n| !changed[n] && is_source(c, n)) {
            distance[c] = 1;
            queue.push_back(c);
        }
    }
    while let Some(c) = queue.pop_front() {
        for n in neighbours(c) {
            if changed[n] && distance[n] == u32::MAX {
                distance[n] = distance[c] + 1;
                queue.push_back(n);
            }
        }
    }
    let max = distance
        .iter()
        .filter(|&&d| d != u32::MAX)
        .max()
        .copied()
        .unwrap_or(1)
        .max(1);
    distance
        .iter()
        .map(|&d| {
            if d == u32::MAX {
                0.5
            } else {
                d as f32 / max as f32
            }
        })
        .collect()
}

/// The state a frame shows: everything as at its year, with borders and
/// the Blight swept between its two ticks.
pub fn frame_state(history: &History, frame: &FrameSpec, sweep: &Sweep) -> HistoryState {
    let mut state = history.state_at(frame.year);
    let start = history.state_at(frame.tick_from);
    state.owner = start.owner;
    state.blight = start.blight;
    sweep.apply(&mut state, frame.fraction);
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldConfig;
    use crate::world::World;

    fn world() -> World {
        let mut config = WorldConfig {
            seed: 42,
            width: 96,
            height: 96,
            ..Default::default()
        };
        config.erosion.droplets = 3000;
        config.history.years = 300;
        World::generate(&config)
    }

    fn options() -> TimelapseOptions {
        TimelapseOptions {
            from: 0,
            to: 300,
            fps: 10,
            years_per_second: 20.0,
            hold_seconds: 0.0,
        }
    }

    #[test]
    fn frame_count_matches_the_settings() {
        let world = world();
        // 300 years at 2 years a frame, plus the first.
        assert_eq!(schedule(&world.history, &options()).len(), 151);
        let slow = TimelapseOptions {
            years_per_second: 5.0,
            fps: 20,
            ..options()
        };
        assert_eq!(schedule(&world.history, &slow).len(), 1201);
        // Holding adds `hold_seconds × fps` frames per major event shown.
        let held = TimelapseOptions {
            hold_seconds: 0.5,
            ..options()
        };
        let frames = schedule(&world.history, &held);
        let holds = frames.iter().filter(|f| f.hold.is_some()).count();
        assert_eq!(holds % 5, 0);
        assert_eq!(frames.len(), 151 + holds);
    }

    #[test]
    fn ticks_show_exactly_the_state_at_that_year() {
        let world = world();
        let opts = options();
        let tick = opts.tick_years();
        for t in (0..300).step_by(tick as usize) {
            let a = world.history.state_at(t);
            let b = world.history.state_at(t + tick);
            let sweep = Sweep::between(&a, &b);
            let mut start = a.clone();
            sweep.apply(&mut start, 0.0);
            assert_eq!(start.owner, a.owner, "tick {t}");
            let mut end = a.clone();
            sweep.apply(&mut end, 1.0);
            assert_eq!(end.owner, b.owner, "tick {}", t + tick);
            assert_eq!(end.blight, b.blight);
        }
        for frame in schedule(&world.history, &opts)
            .iter()
            .filter(|f| f.fraction == 0.0)
        {
            let sweep = Sweep::between(
                &world.history.state_at(frame.tick_from),
                &world.history.state_at(frame.tick_to),
            );
            let state = frame_state(&world.history, frame, &sweep);
            assert_eq!(state.owner, world.history.state_at(frame.tick_from).owner);
        }
    }

    #[test]
    fn borders_sweep_from_the_old_frontier() {
        // A strip conquered left to right flips left first.
        let (w, h) = (6, 1);
        let changed = vec![false, true, true, true, true, false];
        let times = flip_times(w, h, &changed, |_, n| n == 0);
        assert!(times[1] < times[2] && times[2] < times[3] && times[3] < times[4]);
        assert_eq!(times[4], 1.0);
    }
}
