//! Landmarks through history: dragons wake when people crowd their peaks,
//! raid the towns below and are slain by heroes; realms find what was left
//! in elder ruins and take leyline nexuses; the Blight spreads from its
//! origin, empties towns and is burnt back by realms that can pay for it.
//!
//! Everything here draws from its own stream (`history.landmarks`), so the
//! rest of history keeps its own randomness whatever happens here.

use rand_chacha::rand_core::RngCore;
use rand_chacha::ChaCha8Rng;

use super::event::{Cause, EventKind};
use super::sim::{HistoryJob, NameKind};
use super::state::Dragon;
use crate::landmarks::LandmarkKind;
use crate::realms::NO_REALM;
use crate::rng::{entity_rng, stage, unit_f32};

/// Years between Blight spreads and purges.
const BLIGHT_STEP: u32 = 5;

fn distance(w: usize, a: u32, b: u32) -> f32 {
    let (a, b) = (a as usize, b as usize);
    let (dx, dy) = (
        (a % w) as f32 - (b % w) as f32,
        (a / w) as f32 - (b / w) as f32,
    );
    (dx * dx + dy * dy).sqrt()
}

impl HistoryJob {
    pub(super) fn landmark_life(&mut self, year: u32) {
        let rng = &mut entity_rng(self.ctx.seed, stage::HISTORY_LANDMARKS, year as u64);
        self.dragons(year, rng);
        self.ruins_and_nexuses(year, rng);
        if self.ctx.magic > 0.0 {
            self.blight(year, rng);
        }
    }

    /// The most recent event matching `f`, as a cause.
    fn last_event(&self, f: impl Fn(&EventKind) -> bool) -> Option<Cause> {
        self.events
            .iter()
            .rposition(|e| f(&e.kind))
            .map(|i| Cause::Event(i as u32))
    }

    fn dragons(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let w = self.ctx.width;
        let reach = 45.0 * w as f32 / 512.0;
        let magic = self.ctx.magic;
        for l in 0..self.state.landmarks.len() {
            let Some(dragon) = self.state.landmarks[l].dragon else {
                continue;
            };
            let den = self.state.landmarks[l].cell;
            let near: Vec<u32> = (0..self.state.settlements.len() as u32)
                .filter(|&s| {
                    let st = &self.state.settlements[s as usize];
                    st.alive && distance(w, st.cell, den) <= reach
                })
                .collect();
            let landmark = l as u32;
            match dragon {
                Dragon::Slain => {}
                // Dragons only wake when people crowd their mountain.
                Dragon::Asleep => {
                    let crowded = near
                        .iter()
                        .any(|&s| self.state.settlements[s as usize].population > 800);
                    if crowded && unit_f32(rng) < 0.004 * magic {
                        self.emit(
                            year,
                            EventKind::DragonAwakened {
                                landmark,
                                cause: Cause::Disturbed,
                            },
                        );
                    }
                }
                Dragon::Awake => {
                    let raided = |k: &EventKind| matches!(k, EventKind::DragonRaid { landmark: d, .. } if *d == landmark);
                    let woke = |k: &EventKind| matches!(k, EventKind::DragonAwakened { landmark: d, .. } if *d == landmark);
                    let last_raid = self.events.iter().rposition(|e| raided(&e.kind));
                    let last_woke = self.events.iter().rposition(|e| woke(&e.kind));
                    // A hero rides out from the last town it burned.
                    if let (Some(raid), Some(woke)) = (last_raid, last_woke) {
                        if raid > woke && unit_f32(rng) < 0.05 {
                            let EventKind::DragonRaid { settlement, .. } = self.events[raid].kind
                            else {
                                unreachable!("matched above")
                            };
                            let realm = self.state.settlement_realm(settlement);
                            if realm != NO_REALM && self.state.realms[realm as usize].alive {
                                let people = self.state.realms[realm as usize].people;
                                let hero = self.next_name(people, NameKind::Person);
                                let hoard = 80.0 + 120.0 * unit_f32(rng);
                                self.emit(
                                    year,
                                    EventKind::DragonSlain {
                                        landmark,
                                        realm,
                                        hero,
                                        hoard,
                                        cause: Cause::Event(raid as u32),
                                    },
                                );
                                continue;
                            }
                        }
                    }
                    if unit_f32(rng) < 0.02 {
                        self.emit(year, EventKind::DragonSlept { landmark });
                        continue;
                    }
                    if !near.is_empty() && unit_f32(rng) < 0.25 {
                        let settlement = near[(rng.next_u32() as usize) % near.len()];
                        let population = self.state.settlements[settlement as usize].population;
                        let deaths = (population as f32 * (0.05 + 0.1 * unit_f32(rng))) as u32;
                        self.emit(
                            year,
                            EventKind::DragonRaid {
                                landmark,
                                settlement,
                                deaths,
                            },
                        );
                    }
                }
            }
        }
    }

    fn ruins_and_nexuses(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        for l in 0..self.state.landmarks.len() {
            let lm = &self.state.landmarks[l];
            let owner = self.state.owner[lm.cell as usize];
            if owner == NO_REALM || !self.state.realms[owner as usize].alive {
                continue;
            }
            let landmark = l as u32;
            match lm.kind {
                LandmarkKind::ElderRuin if lm.discovered_by.is_none() => {
                    if unit_f32(rng) < 0.01 {
                        let treasure = 20.0 + 60.0 * unit_f32(rng);
                        self.emit(
                            year,
                            EventKind::RuinDiscovered {
                                landmark,
                                realm: owner,
                                treasure,
                            },
                        );
                    }
                }
                // Checked every few years so a wavering border doesn't
                // flip the claim back and forth every season.
                LandmarkKind::LeylineNexus
                    if year.is_multiple_of(BLIGHT_STEP) && lm.holder != Some(owner) =>
                {
                    let boon = 25.0 * self.ctx.magic;
                    self.emit(
                        year,
                        EventKind::NexusClaimed {
                            landmark,
                            realm: owner,
                            boon,
                        },
                    );
                }
                _ => {}
            }
        }
    }

    fn blight(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let Some(origin) = self.ctx.blight_origin else {
            return;
        };
        let start = self.ctx.blight_start;
        if year < start || !(year - start).is_multiple_of(BLIGHT_STEP) {
            return;
        }
        let (w, h) = (self.ctx.width, self.ctx.height);
        let mut fresh = Vec::new();
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
        let cause = if year == start {
            // It wakes where someone last meddled with its source.
            let origin_cell = self.state.landmarks[origin as usize].cell as usize;
            fresh.push(origin_cell as u32);
            fresh.extend(
                neighbours(origin_cell)
                    .filter(|&n| !self.ctx.water[n])
                    .map(|n| n as u32),
            );
            self.last_event(|k| match k {
                EventKind::RuinDiscovered { landmark, .. }
                | EventKind::NexusClaimed { landmark, .. } => *landmark == origin,
                _ => false,
            })
            .unwrap_or(Cause::Delving)
        } else {
            // Every later spread traces back to the first.
            let Some(first) = self
                .events
                .iter()
                .position(|e| matches!(e.kind, EventKind::BlightSpread { .. }))
                .map(|i| Cause::Event(i as u32))
            else {
                return;
            };
            let size = self.state.blight_extent() as f32;
            if size == 0.0 {
                // Burnt out entirely: the Blight is over.
                return;
            }
            let cap = 0.03 * self.ctx.land_cells as f32 * self.ctx.magic.min(1.5);
            let chance = 0.45 * (1.0 - size / cap).max(0.0);
            for c in 0..self.state.blight.len() {
                if !self.state.blight[c] {
                    continue;
                }
                for n in neighbours(c) {
                    if !self.state.blight[n] && !self.ctx.water[n] && unit_f32(rng) < chance {
                        fresh.push(n as u32);
                    }
                }
            }
            fresh.sort_unstable();
            fresh.dedup();
            first
        };
        if !fresh.is_empty() {
            self.emit(
                year,
                EventKind::BlightSpread {
                    cells: fresh.clone(),
                    cause,
                },
            );
        }
        self.blight_reaches_towns(year, &fresh);
        self.purge_blight(year, rng);
    }

    /// Towns newly under the Blight lose many of their people; smaller ones
    /// are abandoned to it and become ruins. Capitals hold on.
    /// A town is struck once; if the Blight is burnt back and returns, the
    /// survivors have learnt to live beside it.
    fn blight_reaches_towns(&mut self, year: u32, fresh: &[u32]) {
        let struck: Vec<u32> = self
            .events
            .iter()
            .filter_map(|e| match e.kind {
                EventKind::BlightReachedSettlement { settlement, .. } => Some(settlement),
                _ => None,
            })
            .collect();
        for s in 0..self.state.settlements.len() {
            let st = &self.state.settlements[s];
            if !st.alive || fresh.binary_search(&st.cell).is_err() || struck.contains(&(s as u32)) {
                continue;
            }
            let deaths = (st.population as f32 * 0.4) as u32;
            let capital = self
                .state
                .living_realms()
                .any(|(_, r)| r.capital == s as u32);
            let abandoned = !capital && st.population - deaths < 1200;
            let ruin = format!("the Ruins of {}", st.name);
            self.emit(
                year,
                EventKind::BlightReachedSettlement {
                    settlement: s as u32,
                    deaths,
                    abandoned,
                    ruin,
                },
            );
        }
    }

    /// Realms with Blight on their land and gold to spend burn back its
    /// edges.
    fn purge_blight(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let w = self.ctx.width;
        let h = self.ctx.height;
        let mut held = vec![0u32; self.state.realms.len()];
        for (c, &b) in self.state.blight.iter().enumerate() {
            let owner = self.state.owner[c];
            if b && owner != NO_REALM {
                held[owner as usize] += 1;
            }
        }
        for realm in 0..self.state.realms.len() as u16 {
            let r = &self.state.realms[realm as usize];
            if !r.alive || held[realm as usize] < 15 || r.wealth < 60.0 {
                continue;
            }
            if unit_f32(rng) >= 0.2 {
                continue;
            }
            let cells: Vec<u32> = (0..self.state.blight.len())
                .filter(|&c| self.state.blight[c] && self.state.owner[c] == realm)
                .filter(|&c| {
                    let (x, y) = (c % w, c / w);
                    (x == 0 || !self.state.blight[c - 1])
                        || (x + 1 == w || !self.state.blight[c + 1])
                        || (y == 0 || !self.state.blight[c - w])
                        || (y + 1 == h || !self.state.blight[c + w])
                })
                .map(|c| c as u32)
                .collect();
            if cells.is_empty() {
                continue;
            }
            let people = r.people;
            let hero = self.next_name(people, NameKind::Person);
            self.emit(
                year,
                EventKind::BlightPurged {
                    cells,
                    realm,
                    hero,
                    cost: 40.0,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{MagicLevel, WorldConfig};
    use crate::history::{Dragon, EventKind};
    use crate::world::World;

    fn magical(seed: u64, level: MagicLevel) -> World {
        let mut config = WorldConfig {
            seed,
            width: 128,
            height: 128,
            ..Default::default()
        };
        config.erosion.droplets = 4000;
        config.history.years = 900;
        config.history.snapshot_interval = 50;
        config.magic.level = level;
        World::generate(&config)
    }

    fn is_magical(kind: &EventKind) -> bool {
        matches!(
            kind,
            EventKind::DragonAwakened { .. }
                | EventKind::DragonRaid { .. }
                | EventKind::DragonSlept { .. }
                | EventKind::DragonSlain { .. }
                | EventKind::NexusClaimed { .. }
                | EventKind::BlightSpread { .. }
                | EventKind::BlightPurged { .. }
                | EventKind::BlightReachedSettlement { .. }
        )
    }

    #[test]
    fn without_magic_there_are_no_dragons_or_blight() {
        let world = magical(42, MagicLevel::None);
        assert!(!world.history.events.iter().any(|e| is_magical(&e.kind)));
    }

    #[test]
    fn blight_extent_at_any_year_equals_replay() {
        let mut seen = false;
        for seed in [1, 42] {
            let world = magical(seed, MagicLevel::Saturated);
            for year in (0..=world.history.years).step_by(37) {
                let state = world.history.state_at(year);
                assert_eq!(state.blight, world.history.replay_from_start(year).blight);
                assert_eq!(
                    state.landmarks,
                    world.history.replay_from_start(year).landmarks
                );
                seen |= state.blight_extent() > 0;
            }
        }
        assert!(seen, "the Blight never spread");
    }

    #[test]
    fn dragons_raid_only_while_awake_and_die_once() {
        for seed in [1, 2, 42] {
            let world = magical(seed, MagicLevel::Saturated);
            let mut state = world.history.initial.clone();
            for event in &world.history.events {
                match &event.kind {
                    EventKind::DragonRaid { landmark, .. }
                    | EventKind::DragonSlain { landmark, .. }
                    | EventKind::DragonSlept { landmark } => {
                        assert_eq!(
                            state.landmarks[*landmark as usize].dragon,
                            Some(Dragon::Awake)
                        );
                    }
                    EventKind::DragonAwakened { landmark, .. } => {
                        assert_eq!(
                            state.landmarks[*landmark as usize].dragon,
                            Some(Dragon::Asleep)
                        );
                    }
                    _ => {}
                }
                state.apply(event);
            }
        }
    }

    #[test]
    fn abandoned_towns_stay_abandoned() {
        for seed in [1, 2, 42] {
            let world = magical(seed, MagicLevel::Saturated);
            let mut state = world.history.initial.clone();
            for event in &world.history.events {
                for s in event.kind.settlements() {
                    if let Some(st) = state.settlements.get(s as usize) {
                        assert!(st.alive, "{:?} touches abandoned {}", event.kind, st.name);
                    }
                }
                state.apply(event);
            }
        }
    }
}
