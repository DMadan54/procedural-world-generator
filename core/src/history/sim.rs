//! The year-by-year simulation that writes the event log.
//!
//! Each year draws all its randomness from `entity_rng(seed, "history",
//! year)`, walks realms in id order and settlements in index order, and
//! applies every event to the running state the moment it is created, so
//! later decisions in the same year see it. Runs incrementally: a job steps
//! a few years at a time so the web build stays responsive.

use rand_chacha::rand_core::RngCore;
use rand_chacha::ChaCha8Rng;

use super::event::{Cause, Event, EventKind, Ruler};
use super::state::HistoryState;
use super::History;
use crate::biome::Biomes;
use crate::config::WorldConfig;
use crate::habitability::Habitability;
use crate::names::Language;
use crate::peoples::Peoples;
use crate::realms::{Government, Realms, NO_REALM, PALETTE_SIZE};
use crate::rng::{entity_rng, stage, unit_f32};
use crate::settlements::{Rank, Settlements};

/// What the simulation needs to know about the world, copied in so the job
/// can run while the world is borrowed elsewhere.
#[derive(Clone, Debug, PartialEq)]
struct Context {
    seed: u64,
    width: usize,
    height: usize,
    habitability: Vec<f32>,
    /// Sea and lakes: never claimed.
    water: Vec<bool>,
    languages: Vec<Language>,
    expansionism: Vec<f32>,
    governments: Vec<Government>,
    /// Good unsettled sites, best first.
    candidates: Vec<u32>,
    /// Per settlement: the population its land can support. Growth slows
    /// as a settlement approaches it. Extended as settlements are founded.
    capacity: Vec<f32>,
    /// Population a new settlement on perfect land could reach.
    base_population: f32,
    /// Minimum distance between settlements, in cells.
    spacing: f32,
    empire_formation: f32,
    fragmentation: f32,
}

/// A history run in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryJob {
    ctx: Context,
    years: u32,
    interval: u32,
    initial: HistoryState,
    state: HistoryState,
    events: Vec<Event>,
    snapshots: Vec<HistoryState>,
    next_year: u32,
    /// Per realm: border cells sampled along each neighbour, refreshed every
    /// few years. `(neighbour or NO_REALM for wilderness, cell on our side)`.
    frontiers: Vec<Vec<(u16, u32)>>,
    /// Names handed out so far, per people (keeps successive names distinct).
    names_used: Vec<u64>,
}

impl HistoryJob {
    pub fn new(
        config: &WorldConfig,
        biomes: &Biomes,
        habitability: &Habitability,
        peoples: &Peoples,
        settlements: &Settlements,
        realms: &Realms,
    ) -> Self {
        let (w, h) = (realms.width, realms.height);
        let spacing = config.settlements.min_spacing * w as f32 / 512.0;
        let ctx = Context {
            seed: config.seed,
            width: w,
            height: h,
            habitability: habitability.value.clone(),
            water: biomes.biome.iter().map(|b| b.is_water()).collect(),
            languages: peoples.peoples.iter().map(|p| p.language.clone()).collect(),
            expansionism: peoples.peoples.iter().map(|p| p.expansionism).collect(),
            governments: peoples
                .peoples
                .iter()
                .map(|p| Government::from_trait(p.trait_))
                .collect(),
            candidates: candidate_sites(habitability, settlements, spacing),
            // Year-0 settlements sit somewhat below what their land allows;
            // capitals draw people in and can grow further.
            capacity: settlements
                .settlements
                .iter()
                .map(|s| s.population as f32 * if s.rank == Rank::Capital { 2.0 } else { 1.5 })
                .collect(),
            base_population: config.settlements.base_population,
            spacing,
            empire_formation: config.politics.empire_formation,
            fragmentation: config.politics.fragmentation_rate,
        };
        let initial = HistoryState::initial(realms, settlements);
        let mut job = Self {
            years: config.history.years,
            interval: config.history.snapshot_interval.max(1),
            state: initial.clone(),
            initial,
            events: Vec::new(),
            snapshots: Vec::new(),
            next_year: 1,
            frontiers: Vec::new(),
            names_used: vec![0; peoples.peoples.len()],
            ctx,
        };
        job.found_year_zero(realms);
        job
    }

    /// Fraction of years simulated, `0.0..=1.0`.
    pub fn progress(&self) -> f32 {
        if self.years == 0 {
            1.0
        } else {
            (self.next_year - 1) as f32 / self.years as f32
        }
    }

    pub fn is_done(&self) -> bool {
        self.next_year > self.years
    }

    /// Simulates up to `max_years` more years.
    pub fn step(&mut self, max_years: u32) {
        for _ in 0..max_years {
            if self.is_done() {
                break;
            }
            let year = self.next_year;
            self.simulate_year(year);
            if year.is_multiple_of(self.interval) {
                self.snapshots.push(self.state.clone());
            }
            self.next_year += 1;
        }
    }

    /// Runs whatever is left and returns the finished history.
    pub fn finish(mut self) -> History {
        self.step(u32::MAX);
        History {
            years: self.years,
            events: self.events,
            initial: self.initial,
            snapshot_interval: self.interval,
            snapshots: self.snapshots,
        }
    }

    fn emit(&mut self, year: u32, kind: EventKind) -> u32 {
        let seq = self
            .events
            .iter()
            .rev()
            .take_while(|e| e.year == year)
            .count() as u32;
        let event = Event { year, seq, kind };
        self.state.apply(&event);
        self.events.push(event);
        self.events.len() as u32 - 1
    }

    fn found_year_zero(&mut self, realms: &Realms) {
        let rng = &mut entity_rng(self.ctx.seed, stage::HISTORY, 0);
        for realm in &realms.realms {
            let ruler = Ruler {
                name: realm.dynasty.founder.clone(),
                house: realm.dynasty.house.clone(),
                born: -(20 + (rng.next_u32() % 30) as i32),
            };
            self.emit(
                0,
                EventKind::RealmFounded {
                    realm: realm.id,
                    ruler,
                    cause: Cause::Founding,
                },
            );
        }
        self.snapshots.push(self.state.clone());
    }

    fn simulate_year(&mut self, year: u32) {
        let rng = &mut entity_rng(self.ctx.seed, stage::HISTORY, year as u64);
        if self.frontiers.is_empty() || year.is_multiple_of(10) {
            self.frontiers = sample_frontiers(&self.state, &self.ctx.water);
        }
        self.successions(year, rng);
        self.conquests(year, rng);
        self.border_drift(year, rng);
        self.splits(year, rng);
        self.settlement_life(year, rng);
        self.foundings(year, rng);
    }

    fn next_name(&mut self, people: u16, kind: NameKind) -> String {
        let counter = &mut self.names_used[people as usize];
        *counter += 1;
        let language = &self.ctx.languages[people as usize];
        match kind {
            NameKind::Person => language.personal_name(1_000_000 + *counter),
            NameKind::Place => language.place_name(1_000_000 + *counter),
        }
    }

    /// Rulers age and die; successors usually continue the house.
    fn successions(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        for id in 0..self.state.realms.len() {
            let realm = &self.state.realms[id];
            let Some(ruler) = realm.ruler.clone() else {
                continue;
            };
            if !realm.alive {
                continue;
            }
            let age = year as i32 - ruler.born;
            let death = (0.004 * (0.06 * (age - 30) as f32).exp()).clamp(0.003, 0.5);
            if unit_f32(rng) >= death {
                continue;
            }
            let people = realm.people;
            let cause = if age >= 55 {
                Cause::OldAge
            } else {
                Cause::UntimelyDeath
            };
            // One succession in seven brings a new house to the throne.
            let house = if rng.next_u32().is_multiple_of(7) {
                self.next_name(people, NameKind::Place)
            } else {
                ruler.house.clone()
            };
            let successor = Ruler {
                name: self.next_name(people, NameKind::Person),
                house,
                born: year as i32 - (16 + (rng.next_u32() % 25) as i32),
            };
            self.emit(
                year,
                EventKind::RulerSucceeded {
                    realm: id as u16,
                    ruler: successor,
                    cause,
                },
            );
        }
    }

    /// Total population of each realm's settlements: its strength.
    fn power(&self) -> Vec<f32> {
        let mut power = vec![0.0f32; self.state.realms.len()];
        for s in self.state.settlements.iter().filter(|s| s.alive) {
            let owner = self.state.owner[s.cell as usize];
            if owner != NO_REALM {
                power[owner as usize] += s.population as f32;
            }
        }
        power
    }

    /// A much stronger realm annexes a weak neighbour outright.
    fn conquests(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let chance = 0.006 * self.ctx.empire_formation;
        for attacker in 0..self.state.realms.len() as u16 {
            if !self.state.realms[attacker as usize].alive || unit_f32(rng) >= chance {
                continue;
            }
            let power = self.power();
            let mut targets: Vec<u16> = self.frontiers[attacker as usize]
                .iter()
                .map(|&(n, _)| n)
                .filter(|&n| n != NO_REALM && self.state.realms[n as usize].alive)
                .filter(|&n| power[attacker as usize] > 3.0 * power[n as usize])
                .collect();
            targets.sort_unstable();
            targets.dedup();
            let Some(&victim) = targets.get((rng.next_u32() as usize) % targets.len().max(1))
            else {
                continue;
            };
            let cells: Vec<u32> = (0..self.state.owner.len() as u32)
                .filter(|&c| self.state.owner[c as usize] == victim)
                .collect();
            let border = self.emit(
                year,
                EventKind::BorderChanged {
                    from: victim,
                    to: attacker,
                    cells: cells.clone(),
                    cause: Cause::Conquest,
                },
            );
            self.emit(
                year,
                EventKind::RealmFell {
                    realm: victim,
                    cause: Cause::Event(border),
                },
            );
            self.rename_conquered(year, &cells, attacker, border, rng);
            self.frontiers = sample_frontiers(&self.state, &self.ctx.water);
        }
    }

    /// Frontiers creep: an expansionist realm takes a bite of a neighbour or
    /// of the wilderness.
    fn border_drift(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let area = (self.ctx.width * self.ctx.height) as f32 / (512.0 * 512.0);
        for id in 0..self.state.realms.len() as u16 {
            let realm = &self.state.realms[id as usize];
            if !realm.alive {
                continue;
            }
            let drive = self.ctx.expansionism[realm.people as usize];
            let chance = 0.03 * (0.5 + drive) * (0.5 + self.ctx.empire_formation);
            if unit_f32(rng) >= chance || self.frontiers[id as usize].is_empty() {
                continue;
            }
            let options = &self.frontiers[id as usize];
            let (target, start) = options[(rng.next_u32() as usize) % options.len()];
            if target != NO_REALM && !self.state.realms[target as usize].alive {
                continue;
            }
            let protect = (target != NO_REALM).then(|| {
                let capital = self.state.realms[target as usize].capital;
                self.state.settlements[capital as usize].cell
            });
            let size = ((10.0 + 50.0 * unit_f32(rng)) * area).round().max(2.0) as usize;
            let cells = grab(&self.state, &self.ctx.water, start, target, size, protect);
            if cells.is_empty() {
                continue;
            }
            let cause = if target == NO_REALM || drive > 0.6 {
                Cause::Ambition
            } else {
                Cause::BorderDispute
            };
            let border = self.emit(
                year,
                EventKind::BorderChanged {
                    from: target,
                    to: id,
                    cells: cells.clone(),
                    cause,
                },
            );
            if target != NO_REALM && self.state.realms[target as usize].cells == 0 {
                self.emit(
                    year,
                    EventKind::RealmFell {
                        realm: target,
                        cause: Cause::Event(border),
                    },
                );
            }
            self.rename_conquered(year, &cells, id, border, rng);
        }
    }

    /// Settlements of another people that change hands are sometimes renamed
    /// in the new rulers' tongue.
    fn rename_conquered(
        &mut self,
        year: u32,
        cells: &[u32],
        new_owner: u16,
        cause: u32,
        rng: &mut ChaCha8Rng,
    ) {
        let people = self.state.realms[new_owner as usize].people;
        let taken: Vec<u32> = (0..self.state.settlements.len() as u32)
            .filter(|&s| {
                let st = &self.state.settlements[s as usize];
                st.alive && st.people != people && cells.binary_search(&st.cell).is_ok()
            })
            .collect();
        for s in taken {
            if rng.next_u32().is_multiple_of(4) {
                let name = self.next_name(people, NameKind::Place);
                self.emit(
                    year,
                    EventKind::SettlementRenamed {
                        settlement: s,
                        name,
                        cause: Cause::Event(cause),
                    },
                );
            }
        }
    }

    /// Overgrown realms break apart around a distant city.
    fn splits(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let alive: Vec<u32> = self
            .state
            .realms
            .iter()
            .filter(|r| r.alive)
            .map(|r| r.cells)
            .collect();
        if alive.is_empty() {
            return;
        }
        let mean = alive.iter().sum::<u32>() as f32 / alive.len() as f32;
        let chance = 0.006 * self.ctx.fragmentation;
        let w = self.ctx.width;
        for id in 0..self.state.realms.len() as u16 {
            let realm = self.state.realms[id as usize].clone();
            if !realm.alive || (realm.cells as f32) < 2.2 * mean || unit_f32(rng) >= chance {
                continue;
            }
            let capital_cell = self.state.settlements[realm.capital as usize].cell as usize;
            let (cx, cy) = ((capital_cell % w) as f32, (capital_cell / w) as f32);
            // The new seat: a big settlement far from the old capital.
            let seat = (0..self.state.settlements.len() as u32)
                .filter(|&s| s != realm.capital && self.state.settlements[s as usize].alive)
                .filter(|&s| self.state.settlement_realm(s) == id)
                .max_by(|&a, &b| {
                    let score = |s: u32| {
                        let st = &self.state.settlements[s as usize];
                        let c = st.cell as usize;
                        let d2 = ((c % w) as f32 - cx).powi(2) + ((c / w) as f32 - cy).powi(2);
                        d2 * st.population as f32
                    };
                    score(a).total_cmp(&score(b)).then(b.cmp(&a))
                });
            let Some(seat) = seat else {
                continue;
            };
            let seat_cell = self.state.settlements[seat as usize].cell as usize;
            let (sx, sy) = ((seat_cell % w) as f32, (seat_cell / w) as f32);
            let cells: Vec<u32> = (0..self.state.owner.len() as u32)
                .filter(|&c| self.state.owner[c as usize] == id)
                .filter(|&c| {
                    let (x, y) = ((c as usize % w) as f32, (c as usize / w) as f32);
                    (x - sx).powi(2) + (y - sy).powi(2) < (x - cx).powi(2) + (y - cy).powi(2)
                })
                .collect();
            if (cells.len() as f32) < 0.1 * realm.cells as f32 {
                continue;
            }
            let people = self.state.settlements[seat as usize].people;
            let new_realm = self.state.realms.len() as u16;
            let short_name = self.next_name(people, NameKind::Place);
            let government = self.ctx.governments[people as usize];
            let name = format!("{} of {}", government.name(), short_name);
            let colour = free_colour(&self.state, &self.frontiers, id);
            let ruler = Ruler {
                name: self.next_name(people, NameKind::Person),
                house: self.next_name(people, NameKind::Place),
                born: year as i32 - (20 + (rng.next_u32() % 25) as i32),
            };
            self.emit(
                year,
                EventKind::RealmSplit {
                    realm: id,
                    new_realm,
                    name,
                    short_name,
                    capital: seat,
                    colour,
                    ruler,
                    cells,
                    cause: Cause::Overextension,
                },
            );
            self.frontiers = sample_frontiers(&self.state, &self.ctx.water);
        }
    }

    /// Settlements grow on good land and shrink on poor, each looked at once
    /// a decade (staggered by id). Only changes of 5% or more are recorded.
    fn settlement_life(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        for s in 0..self.state.settlements.len() as u32 {
            let st = &self.state.settlements[s as usize];
            if !st.alive || !(s + year).is_multiple_of(10) {
                continue;
            }
            // Logistic growth towards what the land supports, plus good and
            // bad decades.
            let capacity = self.ctx.capacity[s as usize].max(1.0);
            let before = st.population as f32;
            let growth = 0.25 * (1.0 - before / capacity) + (unit_f32(rng) - 0.5) * 0.16;
            let after = (before * (1.0 + growth)).round().max(20.0);
            if (after - before).abs() < 0.08 * before {
                continue;
            }
            let population = after as u32;
            let kind = if after > before {
                EventKind::SettlementGrew {
                    settlement: s,
                    population,
                    cause: Cause::Prosperity,
                }
            } else {
                EventKind::SettlementShrank {
                    settlement: s,
                    population,
                    cause: Cause::Hardship,
                }
            };
            self.emit(year, kind);
        }
    }

    /// Realms plant new villages on good empty land.
    fn foundings(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let w = self.ctx.width;
        for id in 0..self.state.realms.len() as u16 {
            let realm = &self.state.realms[id as usize];
            if !realm.alive {
                continue;
            }
            let people = realm.people;
            let chance = 0.01 * (0.5 + self.ctx.expansionism[people as usize]);
            if unit_f32(rng) >= chance {
                continue;
            }
            let spacing2 = self.ctx.spacing * self.ctx.spacing;
            let site = self.ctx.candidates.iter().copied().find(|&c| {
                self.state.owner[c as usize] == id
                    && self.state.settlements.iter().filter(|s| s.alive).all(|s| {
                        let (a, b) = (s.cell as usize, c as usize);
                        let (dx, dy) = (
                            (a % w) as f32 - (b % w) as f32,
                            (a / w) as f32 - (b / w) as f32,
                        );
                        dx * dx + dy * dy >= spacing2
                    })
            });
            let Some(cell) = site else {
                continue;
            };
            let name = self.next_name(people, NameKind::Place);
            let population = 150 + rng.next_u32() % 250;
            let settlement = self.state.settlements.len() as u32;
            let land = self.ctx.habitability[cell as usize];
            self.ctx
                .capacity
                .push(self.ctx.base_population * 0.6 * land.powf(1.5));
            self.emit(
                year,
                EventKind::SettlementFounded {
                    settlement,
                    cell,
                    name,
                    people,
                    population,
                    cause: Cause::Settlement,
                },
            );
        }
    }
}

#[derive(Clone, Copy)]
enum NameKind {
    Person,
    Place,
}

/// Good land not yet settled at year 0, best first, for later foundings.
fn candidate_sites(
    habitability: &Habitability,
    settlements: &Settlements,
    spacing: f32,
) -> Vec<u32> {
    let w = habitability.width;
    let mut cells: Vec<u32> = (0..habitability.value.len() as u32)
        .filter(|&c| habitability.value[c as usize] >= 0.45)
        .filter(|&c| {
            settlements.settlements.iter().all(|s| {
                let (a, b) = (s.cell, c as usize);
                let (dx, dy) = (
                    (a % w) as f32 - (b % w) as f32,
                    (a / w) as f32 - (b / w) as f32,
                );
                dx * dx + dy * dy >= spacing * spacing
            })
        })
        .collect();
    cells.sort_by(|&a, &b| {
        habitability.value[b as usize]
            .total_cmp(&habitability.value[a as usize])
            .then(a.cmp(&b))
    });
    cells.truncate(4000);
    cells
}

/// For each realm, up to a few cells along its border with each neighbour
/// (and with wilderness), in cell order.
fn sample_frontiers(state: &HistoryState, water: &[bool]) -> Vec<Vec<(u16, u32)>> {
    const PER_NEIGHBOUR: usize = 6;
    let (w, h) = (state.width, state.height);
    let mut found: Vec<Vec<(u16, u32)>> = vec![Vec::new(); state.realms.len()];
    let mut counts: Vec<std::collections::BTreeMap<u16, usize>> =
        vec![Default::default(); state.realms.len()];
    for c in 0..w * h {
        let own = state.owner[c];
        if own == NO_REALM {
            continue;
        }
        let (x, y) = (c % w, c / w);
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if nx >= w || ny >= h {
                continue;
            }
            let other = state.owner[ny * w + nx];
            if other == own || (other == NO_REALM && water[ny * w + nx]) {
                continue;
            }
            let count = counts[own as usize].entry(other).or_insert(0);
            // Spread the samples out rather than taking the first few cells.
            if *count < PER_NEIGHBOUR && (c / 7) % (1 + *count) == 0 {
                found[own as usize].push((other, c as u32));
                *count += 1;
            }
        }
    }
    found
}

/// Breadth-first from `start` (a cell of the grabbing realm) into `target`'s
/// cells (or wilderness), up to `size` cells, never taking `protect`.
fn grab(
    state: &HistoryState,
    water: &[bool],
    start: u32,
    target: u16,
    size: usize,
    protect: Option<u32>,
) -> Vec<u32> {
    let (w, h) = (state.width, state.height);
    let start = start as usize;
    let mut taken = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut queue = std::collections::VecDeque::new();
    let (x, y) = (start % w, start / w);
    for (nx, ny) in [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ] {
        if nx < w && ny < h {
            queue.push_back(ny * w + nx);
        }
    }
    while let Some(c) = queue.pop_front() {
        if taken.len() >= size || !seen.insert(c) {
            continue;
        }
        let claimable = state.owner[c] == target && Some(c as u32) != protect && !water[c];
        if !claimable {
            continue;
        }
        taken.push(c as u32);
        let (x, y) = (c % w, c / w);
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if nx < w && ny < h {
                queue.push_back(ny * w + nx);
            }
        }
    }
    taken.sort_unstable();
    taken
}

/// A palette colour used by neither `realm` nor any realm it borders.
fn free_colour(state: &HistoryState, frontiers: &[Vec<(u16, u32)>], realm: u16) -> u8 {
    let mut taken = vec![state.realms[realm as usize].colour];
    for &(n, _) in &frontiers[realm as usize] {
        if n != NO_REALM {
            taken.push(state.realms[n as usize].colour);
        }
    }
    (0..PALETTE_SIZE).find(|c| !taken.contains(c)).unwrap_or(0)
}
