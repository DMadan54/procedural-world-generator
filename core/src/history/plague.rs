//! Plague: an outbreak starts at a busy port or trade hub and spreads along
//! the trade routes, more readily along the busiest. Each afflicted town
//! runs a small SIR model: the sick infect the healthy, then recover or
//! die. Once a tenth are sick a town closes its gates to trade, so routes
//! close behind the plague, and a capital that loses too many may lose its ruler.

use rand_chacha::rand_core::RngCore;
use rand_chacha::ChaCha8Rng;

use super::event::{Cause, EventKind};
use super::sim::HistoryJob;
use crate::rng::unit_f32;

/// Infection rate per year among the healthy.
const BETA: f32 = 1.8;
/// Share of the sick who stop being sick each year (recover or die).
const GAMMA: f32 = 0.6;
/// Share of those who stop being sick who die.
const FATALITY: f32 = 0.35;

impl HistoryJob {
    /// A new outbreak at a port or hub, likelier where more trade passes.
    pub(super) fn outbreak(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let candidates: Vec<(u32, f32)> = (0..self.state.settlements.len() as u32)
            .filter_map(|s| {
                let st = &self.state.settlements[s as usize];
                let port = self.ctx.ports.get(s as usize).copied().unwrap_or(false);
                (st.alive && st.plague == 0.0 && (port || st.hub)).then_some((s, 1.0 + st.trade))
            })
            .collect();
        let total: f32 = candidates.iter().map(|c| c.1).sum();
        if candidates.is_empty() || total <= 0.0 {
            return;
        }
        let mut pick = unit_f32(rng) * total;
        let mut settlement = candidates[candidates.len() - 1].0;
        for &(s, weight) in &candidates {
            if pick < weight {
                settlement = s;
                break;
            }
            pick -= weight;
        }
        let outbreak = self.ctx.outbreaks;
        self.ctx.outbreaks += 1;
        let town = self.state.settlements[settlement as usize].name.clone();
        let name = match rng.next_u32() % 3 {
            0 => format!("the {town} Fever"),
            1 => format!("the Sweating Sickness of {town}"),
            _ => format!("the {town} Plague"),
        };
        self.emit(
            year,
            EventKind::PlagueOutbreak {
                outbreak,
                settlement,
                name,
                cause: Cause::Trade,
            },
        );
        self.ctx.recovered.insert(settlement, 0.0);
    }

    /// One year of every running plague: spread along routes, then the
    /// toll in each afflicted town.
    pub(super) fn plague_year(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let afflicted: Vec<u32> = (0..self.state.settlements.len() as u32)
            .filter(|&s| self.state.settlements[s as usize].plague > 0.0)
            .collect();
        if afflicted.is_empty() {
            return;
        }
        // Spread first, from the towns sick enough to carry it.
        for &from in &afflicted {
            let st = &self.state.settlements[from as usize];
            let (infected, outbreak) = (st.plague, st.outbreak);
            if infected < 0.03 {
                continue;
            }
            let neighbours: Vec<u32> = self
                .state
                .routes
                .iter()
                .filter_map(|&(a, b)| match (a == from, b == from) {
                    (true, _) => Some(b),
                    (_, true) => Some(a),
                    _ => None,
                })
                .collect();
            for to in neighbours {
                let target = &self.state.settlements[to as usize];
                if !target.alive || target.plague > 0.0 || target.outbreak == outbreak {
                    continue;
                }
                let volume: f32 = self
                    .state
                    .trade
                    .iter()
                    .filter(|f| (f.from == from && f.to == to) || (f.from == to && f.to == from))
                    .map(|f| f.shipped)
                    .sum();
                let chance = (3.0 * infected * volume / (volume + 1.0)).min(0.9);
                if unit_f32(rng) < chance {
                    self.emit(
                        year,
                        EventKind::PlagueSpread {
                            outbreak: outbreak.unwrap_or(0),
                            from,
                            to,
                        },
                    );
                    self.ctx.recovered.insert(to, 0.0);
                }
            }
        }
        // Then the sickness runs its course in each town.
        for s in afflicted {
            let st = &self.state.settlements[s as usize];
            let infected = st.plague;
            let recovered = self.ctx.recovered.get(&s).copied().unwrap_or(0.0);
            let healthy = (1.0 - infected - recovered).max(0.0);
            let leaving = GAMMA * infected;
            let new_infected = (infected + BETA * infected * healthy - leaving).clamp(0.0, 1.0);
            let deaths = (st.population as f32 * FATALITY * leaving) as u32;
            let over = new_infected < 0.01;
            self.ctx
                .recovered
                .insert(s, recovered + leaving * (1.0 - FATALITY));
            let population = st.population;
            let toll = self.emit(
                year,
                EventKind::PlagueToll {
                    settlement: s,
                    deaths,
                    infected: if over { 0.0 } else { new_infected },
                },
            );
            // A capital losing a sixth of its people in a year may lose its
            // ruler too.
            if deaths as f32 > population as f32 / 6.0 {
                if let Some(realm) = self.is_capital(s) {
                    if self.state.realms[realm as usize].ruler.is_some() && unit_f32(rng) < 0.4 {
                        self.topple(year, realm, Cause::Event(toll), rng);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::WorldConfig;
    use crate::history::EventKind;
    use crate::world::World;

    fn plaguey(seed: u64) -> World {
        let mut config = WorldConfig {
            seed,
            width: 160,
            height: 160,
            ..Default::default()
        };
        config.erosion.droplets = 5000;
        config.history.years = 800;
        config.disasters.plagues.per_century = 3.0;
        World::generate(&config)
    }

    #[test]
    fn plague_only_travels_along_trade_routes() {
        let mut spreads = 0;
        for seed in [1, 2, 42] {
            let world = plaguey(seed);
            let mut state = world.history.initial.clone();
            for event in &world.history.events {
                if let EventKind::PlagueSpread { from, to, .. } = event.kind {
                    spreads += 1;
                    assert!(
                        state.routes.contains(&(from, to)) || state.routes.contains(&(to, from)),
                        "plague jumped from {from} to {to} with no route between"
                    );
                    assert!(state.settlements[from as usize].plague > 0.0);
                }
                state.apply(event);
            }
        }
        assert!(spreads > 0, "the plague never spread");
    }

    #[test]
    fn plague_burns_out_in_each_town() {
        // The SIR model runs its course: no town stays sick for decades.
        let world = plaguey(42);
        let mut sick_since = std::collections::BTreeMap::new();
        for event in &world.history.events {
            match event.kind {
                EventKind::PlagueOutbreak { settlement, .. }
                | EventKind::PlagueSpread { to: settlement, .. } => {
                    sick_since.insert(settlement, event.year);
                }
                EventKind::PlagueToll {
                    settlement,
                    infected,
                    ..
                } => {
                    let since = sick_since[&settlement];
                    assert!(event.year - since < 30, "{settlement} sick since {since}");
                    if infected == 0.0 {
                        sick_since.remove(&settlement);
                    }
                }
                _ => {}
            }
        }
    }
}
