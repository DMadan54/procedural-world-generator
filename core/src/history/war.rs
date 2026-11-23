//! Diplomacy and war: how realms come to love, distrust and fight each other.
//!
//! Every few years opinions between realms drift with shared borders, trade,
//! kinship, marriages, past wars and common enemies. From opinion come
//! alliances, trade pacts, marriages, rivalries (each with a concrete
//! grievance) and vassalage. Rivals at war fight one named battle a year at
//! a real place on their front; the winner pushes the front forward through
//! ordinary border changes, capitals can be besieged, allies are called in,
//! and wars end in peace terms once a side is exhausted.

use crate::dmath;
use std::collections::BTreeSet;

use rand_chacha::rand_core::RngCore;
use rand_chacha::ChaCha8Rng;

use super::event::{Cause, EventKind, Grievance, PeaceTerms};
use super::sim::{grab, sample_frontiers, HistoryJob, NameKind};
use crate::realms::NO_REALM;
use crate::rng::unit_f32;

/// Years between opinion updates.
const OPINION_INTERVAL: u32 = 5;

impl HistoryJob {
    /// Pairs of living realms that border each other or already have a
    /// relation, lower id first, sorted.
    fn realm_pairs(&self) -> Vec<(u16, u16)> {
        let alive = |r: u16| self.state.realms.get(r as usize).is_some_and(|x| x.alive);
        let mut pairs = BTreeSet::new();
        for (a, list) in self.frontiers.iter().enumerate() {
            for &(b, _) in list {
                if b != NO_REALM && alive(a as u16) && alive(b) && a as u16 != b {
                    pairs.insert(((a as u16).min(b), (a as u16).max(b)));
                }
            }
        }
        let mut out: Vec<(u16, u16)> = pairs.into_iter().collect();
        for r in &self.state.relations {
            if alive(r.a) && alive(r.b) {
                out.push((r.a, r.b));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn neighbours(&self, a: u16, b: u16) -> bool {
        self.frontiers
            .get(a as usize)
            .is_some_and(|list| list.iter().any(|&(n, _)| n == b))
    }

    /// Goods that moved between the two realms' settlements last season.
    fn trade_between(&self, a: u16, b: u16) -> f32 {
        let realm_of = |s: u32| self.state.owner[self.state.settlements[s as usize].cell as usize];
        self.state
            .trade
            .iter()
            .filter(|f| {
                let (x, y) = (realm_of(f.from), realm_of(f.to));
                (x == a && y == b) || (x == b && y == a)
            })
            .map(|f| f.shipped)
            .sum()
    }

    fn at_war(&self, realm: u16) -> bool {
        self.state
            .relations
            .iter()
            .any(|r| (r.a == realm || r.b == realm) && r.war.is_some())
    }

    /// Opinions, alliances, pacts, marriages, rivalries and vassalage.
    pub(super) fn diplomacy(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let pairs = self.realm_pairs();
        if year.is_multiple_of(OPINION_INTERVAL) {
            self.shift_opinions(year, &pairs);
        }
        let succeeded_now: BTreeSet<u16> = self
            .events
            .iter()
            .rev()
            .take_while(|e| e.year == year)
            .filter_map(|e| match e.kind {
                EventKind::RulerSucceeded { realm, .. } => Some(realm),
                _ => None,
            })
            .collect();
        for &(a, b) in &pairs {
            let r = self.state.relation(a, b).cloned();
            let opinion = r.as_ref().map_or(0.0, |r| r.opinion);
            let at_war = r.as_ref().is_some_and(|r| r.war.is_some());
            let allied = r.as_ref().is_some_and(|r| r.alliance);
            let rival = r.as_ref().is_some_and(|r| r.rivalry.is_some());
            let roll = unit_f32(rng);
            if at_war {
                continue;
            }
            if allied && opinion < -5.0 {
                self.emit(
                    year,
                    EventKind::AllianceBroken {
                        a,
                        b,
                        cause: Cause::Estrangement,
                    },
                );
            } else if !allied && !rival && opinion > 30.0 && roll < 0.08 {
                self.emit(
                    year,
                    EventKind::AllianceFormed {
                        a,
                        b,
                        cause: Cause::Friendship,
                    },
                );
            }
            let pact = r.as_ref().is_some_and(|r| r.trade_pact);
            if !pact && opinion > 10.0 && self.trade_between(a, b) > 2.0 && unit_f32(rng) < 0.06 {
                self.emit(
                    year,
                    EventKind::TradePactSigned {
                        a,
                        b,
                        cause: Cause::Trade,
                    },
                );
            }
            let married = r.as_ref().is_some_and(|r| r.marriage);
            if !married && opinion > 15.0 && unit_f32(rng) < 0.025 {
                self.arrange_marriage(year, a, b);
            }
            if !rival && opinion < -25.0 && unit_f32(rng) < 0.08 {
                if let Some(grievance) = self.grievance(a, b, married, &succeeded_now) {
                    self.emit(year, EventKind::RivalryDeclared { a, b, grievance });
                }
            }
        }
        self.vassalage(year, rng);
    }

    fn shift_opinions(&mut self, year: u32, pairs: &[(u16, u16)]) {
        let rivals: Vec<(u16, u16)> = self
            .state
            .relations
            .iter()
            .filter(|r| r.rivalry.is_some())
            .map(|r| (r.a, r.b))
            .collect();
        let rivals_of = |x: u16| -> BTreeSet<u16> {
            rivals
                .iter()
                .filter_map(|&(a, b)| {
                    if a == x {
                        Some(b)
                    } else if b == x {
                        Some(a)
                    } else {
                        None
                    }
                })
                .collect()
        };
        let mut changes = Vec::new();
        for &(a, b) in pairs {
            let r = self.state.relation(a, b).cloned();
            let mut drive = 0.0;
            if self.neighbours(a, b) {
                drive -= 1.5;
            }
            drive += (self.trade_between(a, b) * 0.02).min(3.0);
            let (pa, pb) = (
                self.state.realms[a as usize].people,
                self.state.realms[b as usize].people,
            );
            if pa == pb {
                drive += 1.0;
            }
            if let Some(r) = &r {
                drive += if r.marriage { 1.0 } else { 0.0 } + if r.alliance { 0.5 } else { 0.0 };
                drive -= if r.rivalry.is_some() { 1.0 } else { 0.0 } + 0.5 * r.wars_fought as f32;
                if r.war.is_some() {
                    drive -= 3.0;
                }
            }
            // The enemy of my enemy.
            if !rivals_of(a).is_disjoint(&rivals_of(b)) {
                drive += 1.0;
            }
            let old = r.map_or(0.0, |r| r.opinion);
            let new = (0.92 * old + drive * OPINION_INTERVAL as f32).clamp(-100.0, 100.0);
            if (new - old).abs() > 0.01 {
                changes.push((a, b, new - old));
            }
        }
        if !changes.is_empty() {
            self.emit(year, EventKind::OpinionsShifted { changes });
        }
    }

    fn arrange_marriage(&mut self, year: u32, a: u16, b: u16) {
        let (Some(ra), Some(rb)) = (
            self.state.realms[a as usize].ruler.clone(),
            self.state.realms[b as usize].ruler.clone(),
        ) else {
            return;
        };
        let (pa, pb) = (
            self.state.realms[a as usize].people,
            self.state.realms[b as usize].people,
        );
        let bride = self.next_name(pa, NameKind::Person);
        let groom = self.next_name(pb, NameKind::Person);
        self.emit(
            year,
            EventKind::MarriageArranged {
                a,
                b,
                bride,
                bride_house: ra.house,
                groom,
                groom_house: rb.house,
                cause: Cause::Friendship,
            },
        );
    }

    /// The most specific thing two realms could fall out over.
    fn grievance(
        &self,
        a: u16,
        b: u16,
        married: bool,
        succeeded: &BTreeSet<u16>,
    ) -> Option<Grievance> {
        let house = |r: u16| {
            self.state.realms[r as usize]
                .ruler
                .as_ref()
                .map(|x| x.house.clone())
        };
        if married && (succeeded.contains(&a) || succeeded.contains(&b)) {
            let claimant = if succeeded.contains(&a) { b } else { a };
            if let Some(house) = house(claimant) {
                return Some(Grievance::SuccessionClaim { house });
            }
        }
        if let Some(&(_, cell)) = self
            .frontiers
            .get(a as usize)
            .and_then(|l| l.iter().find(|&&(n, _)| n == b))
        {
            return Some(Grievance::DisputedBorder { cell });
        }
        let realm_of = |s: u32| self.state.owner[self.state.settlements[s as usize].cell as usize];
        if let Some(&(from, to)) = self.state.routes.iter().find(|&&(f, t)| {
            (realm_of(f) == a && realm_of(t) == b) || (realm_of(f) == b && realm_of(t) == a)
        }) {
            return Some(Grievance::ContestedTradeRoute { from, to });
        }
        if married {
            if let Some(house) = house(b) {
                return Some(Grievance::WeddingSlight { house });
            }
        }
        if let Some(war) = self.state.wars.iter().rposition(|w| {
            (w.attackers.contains(&a) && w.defenders.contains(&b))
                || (w.attackers.contains(&b) && w.defenders.contains(&a))
        }) {
            return Some(Grievance::OldWar { war: war as u32 });
        }
        if self.state.realms[a as usize].people != self.state.realms[b as usize].people {
            return Some(Grievance::CulturalSplit);
        }
        None
    }

    fn vassalage(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let power = self.power();
        for v in 0..self.state.realms.len() as u16 {
            let realm = &self.state.realms[v as usize];
            if !realm.alive {
                continue;
            }
            match realm.overlord {
                Some(o) => {
                    if power[v as usize] > 0.8 * power[o as usize] && unit_f32(rng) < 0.05 {
                        self.emit(
                            year,
                            EventKind::VassalFreed {
                                vassal: v,
                                overlord: o,
                                cause: Cause::Independence,
                            },
                        );
                    } else if !self.at_war(v) && unit_f32(rng) < 0.004 * self.ctx.empire_formation {
                        self.emit(
                            year,
                            EventKind::RealmAnnexed {
                                realm: v,
                                by: o,
                                cause: Cause::Absorption,
                            },
                        );
                    }
                }
                None => {
                    if self.at_war(v)
                        || self
                            .state
                            .realms
                            .iter()
                            .any(|r| r.alive && r.overlord == Some(v))
                    {
                        continue;
                    }
                    let protector = self.frontiers[v as usize]
                        .iter()
                        .map(|&(n, _)| n)
                        .filter(|&n| n != NO_REALM && self.state.realms[n as usize].alive)
                        .filter(|&n| self.state.realms[n as usize].overlord.is_none())
                        .filter(|&n| power[n as usize] > 4.0 * power[v as usize])
                        .filter(|&n| {
                            self.state
                                .relation(v, n)
                                .is_some_and(|r| r.opinion > 20.0 && r.war.is_none())
                        })
                        .min();
                    if let Some(o) = protector {
                        if unit_f32(rng) < 0.02 {
                            self.emit(
                                year,
                                EventKind::VassalSworn {
                                    vassal: v,
                                    overlord: o,
                                    cause: Cause::Protection,
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    /// Rivals go to war; ongoing wars fight a battle each year.
    pub(super) fn wars(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        self.declare_wars(year, rng);
        for war in 0..self.state.wars.len() as u32 {
            let w = &self.state.wars[war as usize];
            if w.ended.is_none() && w.started < year {
                self.campaign(year, war, rng);
            }
        }
    }

    fn declare_wars(&mut self, year: u32, rng: &mut ChaCha8Rng) {
        let power = self.power();
        let candidates: Vec<(u16, u16)> = self
            .state
            .relations
            .iter()
            .filter(|r| {
                r.rivalry.is_some() && r.war.is_none() && !r.alliance && r.truce_until <= year
            })
            .map(|r| (r.a, r.b))
            .collect();
        for (a, b) in candidates {
            let (ra, rb) = (
                &self.state.realms[a as usize],
                &self.state.realms[b as usize],
            );
            if !ra.alive || !rb.alive || ra.overlord == Some(b) || rb.overlord == Some(a) {
                continue;
            }
            let drive_a = self.ctx.expansionism[ra.people as usize];
            let drive_b = self.ctx.expansionism[rb.people as usize];
            let chance = 0.02 * (0.5 + self.ctx.empire_formation) * (0.5 + drive_a.max(drive_b));
            if unit_f32(rng) >= chance {
                continue;
            }
            // The stronger, hungrier realm strikes first.
            let (attacker, defender) =
                if power[a as usize] * (0.5 + drive_a) >= power[b as usize] * (0.5 + drive_b) {
                    (a, b)
                } else {
                    (b, a)
                };
            let Some(grievance) = self.state.relation(a, b).and_then(|r| r.rivalry.clone()) else {
                continue;
            };
            let war = self.state.wars.len() as u32;
            self.emit(
                year,
                EventKind::WarDeclared {
                    war,
                    attacker,
                    defender,
                    grievance,
                },
            );
            self.ctx.war_cells.push(0);
            self.call_allies(year, war, attacker, defender, rng);
        }
    }

    /// Allies of the defender answer the call; allies of the attacker
    /// sometimes do. Nobody fights an ally.
    fn call_allies(
        &mut self,
        year: u32,
        war: u32,
        attacker: u16,
        defender: u16,
        rng: &mut ChaCha8Rng,
    ) {
        let allies_of = |job: &Self, x: u16| -> Vec<u16> {
            job.state
                .relations
                .iter()
                .filter(|r| r.alliance && (r.a == x || r.b == x))
                .map(|r| if r.a == x { r.b } else { r.a })
                .filter(|&y| job.state.realms[y as usize].alive)
                .collect()
        };
        for ally in allies_of(self, defender) {
            let w = &self.state.wars[war as usize];
            let friendly_to_enemy = w
                .attackers
                .iter()
                .any(|&e| self.state.relation(ally, e).is_some_and(|r| r.alliance));
            if ally != attacker
                && !w.defenders.contains(&ally)
                && !friendly_to_enemy
                && !self.at_war(ally)
            {
                self.emit(
                    year,
                    EventKind::WarJoined {
                        war,
                        realm: ally,
                        on_attacking_side: false,
                        cause: Cause::Alliance,
                    },
                );
            }
        }
        for ally in allies_of(self, attacker) {
            let w = &self.state.wars[war as usize];
            let friendly_to_enemy = w
                .defenders
                .iter()
                .any(|&e| self.state.relation(ally, e).is_some_and(|r| r.alliance));
            if ally != defender
                && !w.attackers.contains(&ally)
                && !friendly_to_enemy
                && !self.at_war(ally)
                && unit_f32(rng) < 0.5
            {
                self.emit(
                    year,
                    EventKind::WarJoined {
                        war,
                        realm: ally,
                        on_attacking_side: true,
                        cause: Cause::Alliance,
                    },
                );
            }
        }
    }

    /// One year of a war: a battle on the front, the front moving, perhaps a
    /// siege, and perhaps peace.
    fn campaign(&mut self, year: u32, war: u32, rng: &mut ChaCha8Rng) {
        let w = self.state.wars[war as usize].clone();
        let alive = |job: &Self, r: u16| job.state.realms[r as usize].alive;
        let attackers: Vec<u16> = w
            .attackers
            .iter()
            .copied()
            .filter(|&r| alive(self, r))
            .collect();
        let defenders: Vec<u16> = w
            .defenders
            .iter()
            .copied()
            .filter(|&r| alive(self, r))
            .collect();
        if !alive(self, w.attacker) || !alive(self, w.defender) {
            let victor = if alive(self, w.attacker) {
                w.attacker
            } else {
                w.defender
            };
            self.make_peace(year, war, victor, 0.0, false);
            return;
        }

        // Where the armies meet: a cell on the attackers' side of the front.
        let mut fronts: Vec<(u16, u16, u32)> = Vec::new();
        for &x in &attackers {
            for &(n, c) in &self.frontiers[x as usize] {
                if defenders.contains(&n) {
                    fronts.push((x, n, c));
                }
            }
        }
        if fronts.is_empty() {
            // No shared border: raids at sea and little else.
            if year - w.started >= 4 {
                self.make_peace(year, war, w.defender, 0.0, false);
            }
            return;
        }
        let (x, y, site) = fronts[(rng.next_u32() as usize) % fronts.len()];

        let power = self.power();
        let strength = |side: &[u16]| -> f32 {
            side.iter()
                .map(|&r| {
                    power[r as usize] / 1000.0
                        * (1.0 + self.state.realms[r as usize].wealth / 2000.0)
                })
                .sum::<f32>()
                .max(0.1)
        };
        // Attacking over rough ground is costly.
        let ground = self.ctx.travel_cost[site as usize] * self.ctx.width as f32 / 512.0;
        let attack = strength(&attackers) / (1.0 + ground / 5.0) * (0.6 + 0.8 * unit_f32(rng));
        let defend = strength(&defenders) * (0.6 + 0.8 * unit_f32(rng));
        let attacker_won = attack > defend;
        let (army_a, army_d) = (strength(&attackers) * 120.0, strength(&defenders) * 120.0);
        let (loss_win, loss_lose) = (0.05 + 0.07 * unit_f32(rng), 0.15 + 0.15 * unit_f32(rng));
        let (attacker_losses, defender_losses) = if attacker_won {
            (army_a * loss_win, army_d * loss_lose)
        } else {
            (army_a * loss_lose, army_d * loss_win)
        };
        let name = self.battle_name(site, y);
        let battle = self.emit(
            year,
            EventKind::Battle {
                war,
                name,
                site,
                attacker_won,
                attacker_losses: attacker_losses as u32,
                defender_losses: defender_losses as u32,
            },
        );

        // The winner pushes the front forward.
        let (winner, loser, start) = if attacker_won {
            (x, y, site)
        } else {
            let back = self.frontiers[y as usize]
                .iter()
                .find(|&&(n, _)| n == x)
                .map(|&(_, c)| c);
            match back {
                Some(c) => (y, x, c),
                None => (y, x, site),
            }
        };
        let protect = {
            let capital = self.state.realms[loser as usize].capital;
            let cell = self.state.settlements[capital as usize].cell;
            // Capitals fall to sieges, not to the front sweeping past.
            Some(cell)
        };
        let size = ((10.0 + 40.0 * unit_f32(rng)) * (self.ctx.width * self.ctx.height) as f32
            / (512.0 * 512.0))
            .round()
            .max(2.0) as usize;
        let cells = grab(&self.state, &self.ctx.water, start, loser, size, protect);
        if !cells.is_empty() {
            self.ctx.war_cells[war as usize] += cells.len() as u32;
            let border = self.emit(
                year,
                EventKind::BorderChanged {
                    from: loser,
                    to: winner,
                    cells,
                    cause: Cause::Event(battle),
                },
            );
            if self.state.realms[loser as usize].cells == 0 {
                self.emit(
                    year,
                    EventKind::RealmFell {
                        realm: loser,
                        cause: Cause::Event(border),
                    },
                );
            }
        }

        // A crushing year can bring the winner to the loser's capital.
        let capital = self.state.realms[loser as usize].capital;
        if self.state.realms[loser as usize].alive && unit_f32(rng) < 0.12 {
            let fell = unit_f32(rng) < 0.5;
            let siege = self.emit(
                year,
                EventKind::Siege {
                    war,
                    settlement: capital,
                    fell,
                },
            );
            let small = self.state.realms[loser as usize].cells * 3
                < self.state.realms[winner as usize].cells;
            if fell && small {
                self.emit(
                    year,
                    EventKind::RealmAnnexed {
                        realm: loser,
                        by: winner,
                        cause: Cause::Event(siege),
                    },
                );
                self.make_peace(year, war, winner, 0.0, false);
                return;
            }
            if fell {
                let leader_won = if attackers.contains(&winner) {
                    w.attacker
                } else {
                    w.defender
                };
                self.make_peace(year, war, leader_won, 0.2, true);
                return;
            }
        }

        // Exhaustion: long wars and lopsided ones end.
        let length = year - w.started;
        let score = self.state.wars[war as usize].score;
        if length >= 15 || score.unsigned_abs() >= 5 || unit_f32(rng) < 0.05 + 0.01 * length as f32
        {
            let victor = if score > 0 { w.attacker } else { w.defender };
            let vassal = score.unsigned_abs() >= 4 && unit_f32(rng) < 0.5;
            self.make_peace(year, war, victor, 0.15, vassal);
        }
    }

    fn make_peace(
        &mut self,
        year: u32,
        war: u32,
        victor: u16,
        tribute_share: f32,
        vassalage: bool,
    ) {
        let w = self.state.wars[war as usize].clone();
        let loser = if victor == w.attacker {
            w.defender
        } else {
            w.attacker
        };
        let tribute = tribute_share * self.state.realms[loser as usize].wealth.max(0.0);
        let vassalage = vassalage
            && self.state.realms[loser as usize].alive
            && self.state.realms[loser as usize].overlord.is_none()
            && !self
                .state
                .realms
                .iter()
                .any(|r| r.alive && r.overlord == Some(loser));
        let terms = PeaceTerms {
            victor,
            territory: self.ctx.war_cells[war as usize],
            tribute,
            vassalage,
        };
        let peace = self.emit(
            year,
            EventKind::PeaceSigned {
                war,
                attacker: w.attacker,
                defender: w.defender,
                terms,
            },
        );
        if vassalage && self.state.realms[victor as usize].overlord.is_none() {
            self.emit(
                year,
                EventKind::VassalSworn {
                    vassal: loser,
                    overlord: victor,
                    cause: Cause::Event(peace),
                },
            );
        }
        self.frontiers = sample_frontiers(&self.state, &self.ctx.water);
    }

    /// Battles are named for the nearest settlement, as a ford or a pass when
    /// that's what the place is, or for open fields otherwise.
    fn battle_name(&mut self, site: u32, defender: u16) -> String {
        let w = self.ctx.width;
        let (sx, sy) = ((site as usize % w) as f32, (site as usize / w) as f32);
        let reach = 12.0 * w as f32 / 512.0;
        let nearest = self
            .state
            .settlements
            .iter()
            .enumerate()
            .filter(|(_, s)| s.alive)
            .map(|(i, s)| {
                let (x, y) = ((s.cell as usize % w) as f32, (s.cell as usize / w) as f32);
                (dmath::sq(x - sx) + dmath::sq(y - sy), i)
            })
            .filter(|&(d2, _)| d2 <= reach * reach)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        match nearest {
            Some((_, i)) => {
                let name = self.state.settlements[i].name.clone();
                if self.ctx.crossing.get(i).copied().unwrap_or(false) {
                    format!("Battle of {name} Ford")
                } else if self.ctx.pass.get(i).copied().unwrap_or(false) {
                    format!("Battle of the {name} Pass")
                } else {
                    format!("Battle of {name}")
                }
            }
            None => {
                // Fields are named by region, so battles fought over the same
                // ground share a name.
                let people = self.state.realms[defender as usize].people;
                let region = (sx / 16.0) as u64 * 1024 + (sy / 16.0) as u64;
                let field = self.ctx.languages[people as usize].place_name(2_000_000 + region);
                format!("Battle of the {field} Fields")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{EventKind, History, HistoryState};
    use crate::config::WorldConfig;
    use crate::world::World;

    fn warlike(seed: u64) -> History {
        let mut config = WorldConfig {
            seed,
            width: 128,
            height: 128,
            ..Default::default()
        };
        config.erosion.droplets = 3000;
        config.history.years = 800;
        config.politics.empire_formation = 0.9;
        World::generate(&config).history
    }

    /// Replays the log, checking `check` before and after every event.
    fn replay(history: &History, mut check: impl FnMut(&HistoryState, &EventKind, &HistoryState)) {
        let mut state = history.initial.clone();
        for event in &history.events {
            let before = state.clone();
            state.apply(event);
            check(&before, &event.kind, &state);
        }
    }

    #[test]
    fn wars_happen() {
        let wars: usize = [1, 2, 3]
            .into_iter()
            .map(|seed| {
                warlike(seed)
                    .events
                    .iter()
                    .filter(|e| matches!(e.kind, EventKind::WarDeclared { .. }))
                    .count()
            })
            .sum();
        assert!(wars > 0, "no wars in three warlike worlds");
    }

    #[test]
    fn a_vassal_never_has_two_overlords() {
        for seed in [1, 2, 3] {
            replay(&warlike(seed), |before, kind, _| {
                if let EventKind::VassalSworn {
                    vassal, overlord, ..
                } = kind
                {
                    assert_eq!(
                        before.realms[*vassal as usize].overlord, None,
                        "{vassal} already sworn"
                    );
                    assert_ne!(vassal, overlord);
                }
            });
        }
    }

    #[test]
    fn nobody_fights_an_ally() {
        for seed in [1, 2, 3] {
            replay(&warlike(seed), |_, _, after| {
                for r in &after.relations {
                    assert!(
                        !(r.alliance && r.war.is_some()),
                        "{} and {} are allies at war",
                        r.a,
                        r.b
                    );
                }
            });
        }
    }

    #[test]
    fn every_peace_ends_a_declared_war_between_the_same_parties() {
        for seed in [1, 2, 3] {
            let history = warlike(seed);
            let mut declared = std::collections::BTreeMap::new();
            for e in &history.events {
                match &e.kind {
                    EventKind::WarDeclared {
                        war,
                        attacker,
                        defender,
                        ..
                    } => {
                        declared.insert(*war, (*attacker, *defender, e.year));
                    }
                    EventKind::PeaceSigned {
                        war,
                        attacker,
                        defender,
                        ..
                    } => {
                        let &(a, d, year) = declared.get(war).expect("peace without a war");
                        assert_eq!((a, d), (*attacker, *defender));
                        assert!(year <= e.year);
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn battles_belong_to_open_wars() {
        replay(&warlike(2), |before, kind, _| {
            if let EventKind::Battle { war, name, .. } = kind {
                assert!(before.wars[*war as usize].ended.is_none());
                assert!(name.starts_with("Battle of"), "{name}");
            }
        });
    }
}
