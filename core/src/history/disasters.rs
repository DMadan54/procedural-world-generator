//! Disasters that come out of the physical world: volcanoes where plates
//! converge, earthquakes along plate boundaries, and what they do to the
//! towns nearby. Each disaster is one event with a physical cause; its
//! consequences are later events that point back to it.
//!
//! Draws from its own stream (`history.disasters`), so turning disasters
//! off or changing their rates leaves the rest of history's dice alone.

use rand_chacha::rand_core::RngCore;
use rand_chacha::ChaCha8Rng;

use super::event::{Cause, EventKind, Ruler};
use super::sim::{HistoryJob, NameKind};
use crate::biome::{Biome, Biomes};
use crate::config::Disaster;
use crate::realms::NO_REALM;
use crate::rng::{entity_rng, stage, unit_f32};
use crate::tectonics::{BoundaryKind, TectonicMap, NO_BOUNDARY};

/// Where droughts start, as `(cell, cumulative weight)` on a coarse grid:
/// dry grassland, scrub and savanna most, deserts (too dry to farm) and wet
/// forests least.
pub(super) fn drought_sites(biomes: &Biomes) -> Vec<(u32, f32)> {
    let (w, h) = (biomes.width, biomes.height);
    let mut total = 0.0;
    let mut out = Vec::new();
    for y in (0..h).step_by(4) {
        for x in (0..w).step_by(4) {
            let i = y * w + x;
            let weight = match biomes.biome[i] {
                Biome::Grassland | Biome::Shrubland | Biome::Savanna => 4.0,
                Biome::Desert => 1.0,
                Biome::TemperateForest | Biome::BorealForest | Biome::TropicalForest => 0.6,
                b if b.is_water() => 0.0,
                _ => 0.3,
            };
            if weight > 0.0 {
                total += weight;
                out.push((i as u32, total));
            }
        }
    }
    out
}

/// Land cells on a plate boundary: convergent ones only if `convergent`.
pub(super) fn boundary_sites(
    tectonics: &TectonicMap,
    biomes: &Biomes,
    convergent: bool,
) -> Vec<u32> {
    (0..tectonics.boundary_id.len())
        .filter(|&i| {
            let b = tectonics.boundary_id[i];
            b != NO_BOUNDARY
                && !biomes.biome[i].is_water()
                && (!convergent
                    || tectonics.boundaries[b as usize].kind == BoundaryKind::Convergent)
        })
        .map(|i| i as u32)
        .collect()
}

/// How many of a disaster happen this year: the whole part of the yearly
/// rate, plus one more with the chance of the fraction.
fn occurrences(disaster: &Disaster, rng: &mut ChaCha8Rng) -> u32 {
    if !disaster.enabled || disaster.per_century <= 0.0 {
        return 0;
    }
    let rate = disaster.per_century / 100.0;
    rate.floor() as u32 + u32::from(unit_f32(rng) < rate.fract())
}

impl HistoryJob {
    pub(super) fn disasters(&mut self, year: u32) {
        let rng = &mut entity_rng(self.ctx.seed, stage::HISTORY_DISASTERS, year as u64);
        for _ in 0..occurrences(&self.ctx.disasters.volcanoes.clone(), rng) {
            self.eruption(year, rng);
        }
        for _ in 0..occurrences(&self.ctx.disasters.earthquakes.clone(), rng) {
            self.earthquake(year, rng);
        }
        for _ in 0..occurrences(&self.ctx.disasters.droughts.clone(), rng) {
            self.drought(year, rng);
        }
        for _ in 0..occurrences(&self.ctx.disasters.plagues.clone(), rng) {
            self.outbreak(year, rng);
        }
        self.plague_year(year, rng);
        self.sieges_ruin_harvests(year);
        self.famines(year, rng);
    }

    fn reference(&self) -> f32 {
        self.ctx.width as f32 / 512.0
    }

    fn distance(&self, a: u32, b: u32) -> f32 {
        let w = self.ctx.width;
        let (a, b) = (a as usize, b as usize);
        let (dx, dy) = (
            (a % w) as f32 - (b % w) as f32,
            (a / w) as f32 - (b / w) as f32,
        );
        (dx * dx + dy * dy).sqrt()
    }

    /// The people living nearest a cell (for naming things there).
    fn people_near(&self, cell: u32) -> u16 {
        self.state
            .settlements
            .iter()
            .filter(|s| s.alive)
            .min_by(|a, b| {
                self.distance(a.cell, cell)
                    .total_cmp(&self.distance(b.cell, cell))
            })
            .map_or(0, |s| s.people)
    }

    /// Living settlements within `radius` of `cell`, with their distance.
    fn settlements_near(&self, cell: u32, radius: f32) -> Vec<(u32, f32)> {
        (0..self.state.settlements.len() as u32)
            .filter(|&s| self.state.settlements[s as usize].alive)
            .map(|s| {
                (
                    s,
                    self.distance(self.state.settlements[s as usize].cell, cell),
                )
            })
            .filter(|&(_, d)| d <= radius)
            .collect()
    }

    pub(super) fn is_capital(&self, settlement: u32) -> Option<u16> {
        self.state
            .living_realms()
            .find(|(_, r)| r.capital == settlement)
            .map(|(id, _)| id)
    }

    /// Kills `share` of a settlement. Small places hit hard are abandoned
    /// and become ruins; capitals always hold on. Returns the event index.
    fn strike(&mut self, year: u32, settlement: u32, share: f32, how: &str, cause: Cause) -> u32 {
        let st = &self.state.settlements[settlement as usize];
        let deaths = (st.population as f32 * share.clamp(0.0, 0.95)) as u32;
        let left = st.population - deaths;
        let abandoned = self.is_capital(settlement).is_none() && (share > 0.6 || left < 150);
        let ruin = format!("the Ruins of {}", st.name);
        let hook = format!(
            "{}: once the {} of {}, {how} in the year {year}.",
            capitalise(&ruin),
            st.rank.name(),
            st.name
        );
        self.emit(
            year,
            EventKind::SettlementStruck {
                settlement,
                deaths,
                abandoned,
                ruin,
                hook,
                cause,
            },
        )
    }

    /// A ruler dies in the disaster and a new house takes the throne.
    pub(super) fn topple(&mut self, year: u32, realm: u16, cause: Cause, rng: &mut ChaCha8Rng) {
        let people = self.state.realms[realm as usize].people;
        let ruler = Ruler {
            name: self.next_name(people, NameKind::Person),
            house: self.next_name(people, NameKind::Place),
            born: year as i32 - (18 + (rng.next_u32() % 25) as i32),
        };
        self.emit(
            year,
            EventKind::RulerSucceeded {
                realm,
                ruler,
                cause,
            },
        );
    }

    /// A volcano on a convergent boundary: towns close by are buried, those
    /// further out suffer ash and failed harvests for a generation.
    fn eruption(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        if self.ctx.volcano_sites.is_empty() {
            return;
        }
        let cell = self.ctx.volcano_sites[(rng.next_u32() as usize) % self.ctx.volcano_sites.len()];
        // A mountain erupts again under the name it was given the first time.
        let name = match self
            .state
            .eruptions
            .iter()
            .find(|e| self.distance(e.cell, cell) < 6.0)
        {
            Some(e) => e.name.clone(),
            None => {
                let people = self.people_near(cell);
                format!("Mount {}", self.next_name(people, NameKind::Place))
            }
        };
        let eruption = self.emit(
            year,
            EventKind::VolcanoErupted {
                cell,
                name,
                cause: Cause::Tectonics,
            },
        );
        let reference = self.reference();
        let (buried, ash) = (12.0 * reference, 35.0 * reference);
        let near = self.settlements_near(cell, ash);
        let mut ashen = Vec::new();
        for &(s, d) in &near {
            if d <= buried {
                let share = 0.8 * (1.0 - d / buried) + 0.2 * unit_f32(rng);
                self.strike(year, s, share, "buried under ash", Cause::Event(eruption));
            }
            if self.state.settlements[s as usize].alive {
                ashen.push(s);
            }
        }
        if !ashen.is_empty() {
            let until = year + 20 + rng.next_u32() % 20;
            self.hard_times(year, ashen, until, Cause::Event(eruption));
        }
        // A great eruption darkens the sky: a year without summer far around.
        if unit_f32(rng) < 0.3 {
            let summerless: Vec<u32> = self
                .settlements_near(cell, 90.0 * reference)
                .into_iter()
                .map(|(s, _)| s)
                .collect();
            if !summerless.is_empty() {
                self.hard_times(year, summerless, year + 2, Cause::Event(eruption));
            }
        }
    }

    /// Logs hard times and remembers which event caused them, so famines
    /// can point back to it.
    fn hard_times(&mut self, year: u32, settlements: Vec<u32>, until: u32, cause: Cause) {
        let event = self.emit(
            year,
            EventKind::HardTimes {
                settlements: settlements.clone(),
                until,
                cause,
            },
        );
        for s in settlements {
            self.ctx.hardship_cause.insert(s, event);
        }
    }

    /// A drought, most likely in dry grassland, scrub and savanna: the
    /// harvests of every town in reach fail for a few years.
    fn drought(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let Some(&(_, total)) = self.ctx.drought_sites.last() else {
            return;
        };
        let pick = unit_f32(rng) * total;
        let at = self
            .ctx
            .drought_sites
            .partition_point(|&(_, cum)| cum <= pick);
        let cell = self.ctx.drought_sites[at.min(self.ctx.drought_sites.len() - 1)].0;
        let radius = (25.0 + 25.0 * unit_f32(rng)) * self.reference();
        let drought = self.emit(
            year,
            EventKind::Drought {
                cell,
                radius,
                cause: Cause::Weather,
            },
        );
        let parched: Vec<u32> = self
            .settlements_near(cell, radius)
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        if !parched.is_empty() {
            let until = year + 3 + rng.next_u32() % 5;
            self.hard_times(year, parched, until, Cause::Event(drought));
        }
    }

    /// A town that falls to a siege loses its harvest.
    fn sieges_ruin_harvests(&mut self, year: u32) {
        let first = self.events.partition_point(|e| e.year < year);
        let fallen: Vec<(u32, u32)> = self.events[first..]
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e.kind {
                EventKind::Siege {
                    settlement,
                    fell: true,
                    ..
                } => Some((settlement, (first + i) as u32)),
                _ => None,
            })
            .collect();
        for (settlement, siege) in fallen {
            if self.state.settlements[settlement as usize].alive {
                self.hard_times(year, vec![settlement], year + 4, Cause::Event(siege));
            }
        }
    }

    /// Towns in hard times that also can't buy grain starve. Survivors walk
    /// to the nearest town with food; a realm whose capital starves may
    /// rise against its ruler.
    fn famines(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        for s in 0..self.state.settlements.len() as u32 {
            let st = &self.state.settlements[s as usize];
            if !st.alive || year >= st.hardship_until || !self.ctx.short.contains(&s) {
                continue;
            }
            if self.ctx.last_famine.get(&s).is_some_and(|&y| year < y + 10) {
                continue;
            }
            let cause = self
                .ctx
                .hardship_cause
                .get(&s)
                .map_or(Cause::Hardship, |&e| Cause::Event(e));
            let deaths = (st.population as f32 * (0.08 + 0.15 * unit_f32(rng))) as u32;
            let famine = self.emit(
                year,
                EventKind::Famine {
                    settlement: s,
                    deaths,
                    cause,
                },
            );
            self.ctx.last_famine.insert(s, year);
            // The hungry leave for the nearest town with food to spare.
            let from_cell = self.state.settlements[s as usize].cell;
            let refuge = (0..self.state.settlements.len() as u32)
                .filter(|&t| {
                    let other = &self.state.settlements[t as usize];
                    t != s
                        && other.alive
                        && year >= other.hardship_until
                        && !self.ctx.short.contains(&t)
                })
                .map(|t| {
                    let n = self.ctx.roads.settlement_count as u32;
                    let by_road = if s < n && t < n {
                        self.ctx.roads.distance(s, t)
                    } else {
                        f32::INFINITY
                    };
                    let d = if by_road.is_finite() {
                        by_road
                    } else {
                        // Off the network: overland, as the crow flies but slower.
                        3.0 * self.distance(from_cell, self.state.settlements[t as usize].cell)
                    };
                    (t, d)
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let left = self.state.settlements[s as usize].population;
            if let Some((to, _)) = refuge {
                let people = (left as f32 * (0.15 + 0.15 * unit_f32(rng))) as u32;
                if people > 0 {
                    self.emit(
                        year,
                        EventKind::Migration {
                            from: s,
                            to,
                            people,
                            cause: Cause::Event(famine),
                        },
                    );
                }
            }
            // Revolt: a starving capital turns on its ruler.
            if let Some(realm) = self.is_capital(s) {
                if self.state.realms[realm as usize].ruler.is_some() && unit_f32(rng) < 0.35 {
                    self.topple(year, realm, Cause::Event(famine), rng);
                }
            }
        }
    }

    /// An earthquake along a plate boundary: damage falls off with
    /// distance; a capital hit hard may lose its ruler.
    fn earthquake(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        if self.ctx.quake_sites.is_empty() {
            return;
        }
        let cell = self.ctx.quake_sites[(rng.next_u32() as usize) % self.ctx.quake_sites.len()];
        let magnitude = 5.5 + 2.5 * unit_f32(rng).powi(2);
        let quake = self.emit(
            year,
            EventKind::Earthquake {
                cell,
                magnitude,
                cause: Cause::Tectonics,
            },
        );
        let radius = (magnitude - 4.5) * 18.0 * self.reference();
        let strength = (magnitude - 5.0) / 3.0;
        for (s, d) in self.settlements_near(cell, radius) {
            let share = 0.45 * strength * (1.0 - d / radius).powf(1.5);
            if share < 0.03 {
                continue;
            }
            self.strike(
                year,
                s,
                share,
                "thrown down by an earthquake",
                Cause::Event(quake),
            );
            if share > 0.15 && unit_f32(rng) < 0.5 {
                if let Some(realm) = self.is_capital(s) {
                    if self.state.realms[realm as usize].ruler.is_some()
                        && self.state.owner[self.state.settlements[s as usize].cell as usize]
                            != NO_REALM
                    {
                        self.topple(year, realm, Cause::Event(quake), rng);
                    }
                }
            }
        }
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::config::WorldConfig;
    use crate::history::EventKind;
    use crate::tectonics::{BoundaryKind, NO_BOUNDARY};
    use crate::world::World;

    fn config(seed: u64) -> WorldConfig {
        let mut config = WorldConfig {
            seed,
            width: 128,
            height: 128,
            ..Default::default()
        };
        config.erosion.droplets = 4000;
        config.history.years = 1000;
        config
    }

    #[test]
    fn eruptions_only_where_plates_converge() {
        for seed in [1, 2, 42] {
            let world = World::generate(&config(seed));
            for e in &world.history.events {
                if let EventKind::VolcanoErupted { cell, .. } = e.kind {
                    let b = world.tectonics.boundary_id[cell as usize];
                    assert_ne!(b, NO_BOUNDARY);
                    assert_eq!(
                        world.tectonics.boundaries[b as usize].kind,
                        BoundaryKind::Convergent
                    );
                    assert!(!world.biomes.biome[cell as usize].is_water());
                }
                if let EventKind::Earthquake { cell, .. } = e.kind {
                    assert_ne!(world.tectonics.boundary_id[cell as usize], NO_BOUNDARY);
                }
            }
        }
    }

    #[test]
    fn disabled_disasters_never_happen() {
        let mut c = config(42);
        c.disasters.volcanoes.enabled = false;
        c.disasters.earthquakes.per_century = 0.0;
        let world = World::generate(&c);
        assert!(!world.history.events.iter().any(|e| matches!(
            e.kind,
            EventKind::VolcanoErupted { .. } | EventKind::Earthquake { .. }
        )));
    }

    #[test]
    fn rates_match_per_century() {
        let mut c = config(7);
        c.history.years = 4000;
        c.disasters.earthquakes.per_century = 3.0;
        let world = World::generate(&c);
        let quakes = world
            .history
            .events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Earthquake { .. }))
            .count() as f32;
        // Expected 120; a Poisson count is within ±4σ (≈44) essentially always.
        assert!(
            (quakes - 120.0).abs() < 44.0,
            "{quakes} earthquakes in 40 centuries"
        );
    }

    #[test]
    fn famines_and_migrations_trace_back_to_their_causes() {
        let mut famines = 0;
        for seed in [1, 2, 42] {
            let mut c = config(seed);
            c.disasters.droughts.per_century = 6.0;
            let world = World::generate(&c);
            let events = &world.history.events;
            for e in events {
                match &e.kind {
                    EventKind::Famine {
                        settlement, cause, ..
                    } => {
                        famines += 1;
                        let crate::history::Cause::Event(i) = cause else {
                            panic!("famine without a cause");
                        };
                        let EventKind::HardTimes { settlements, .. } = &events[*i as usize].kind
                        else {
                            panic!("famine caused by {:?}", events[*i as usize].kind);
                        };
                        assert!(settlements.contains(settlement));
                    }
                    EventKind::Migration { from, cause, .. } => {
                        let crate::history::Cause::Event(i) = cause else {
                            panic!("migration without a cause");
                        };
                        assert!(matches!(
                            events[*i as usize].kind,
                            EventKind::Famine { settlement, .. } if settlement == *from
                        ));
                    }
                    _ => {}
                }
            }
        }
        assert!(famines > 0, "no famines at all");
    }

    #[test]
    fn disaster_settings_leave_earlier_stages_alone() {
        let base = World::generate(&config(42));
        let mut c = config(42);
        c.disasters.volcanoes.per_century = 5.0;
        let changed = World::generate(&c);
        assert_eq!(base.landmarks, changed.landmarks);
        assert_eq!(base.realms, changed.realms);
        assert_ne!(base.history, changed.history);
    }
}
