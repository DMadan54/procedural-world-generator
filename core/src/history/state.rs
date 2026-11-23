//! The world's political and settlement state at a given year. It is only
//! ever changed by applying events, so the state at any year is exactly the
//! year-0 state with every earlier event replayed onto it.

use serde::{Deserialize, Serialize};

use super::event::{Event, EventKind, Flow, Grievance, Ruler};
use crate::landmarks::{LandmarkKind, Landmarks};
use crate::realms::{Realms, NO_REALM};
use crate::settlements::{Rank, Settlements};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RealmState {
    pub name: String,
    pub short_name: String,
    /// Settlement id of the capital.
    pub capital: u32,
    pub people: u16,
    /// Index into the realm palette.
    pub colour: u8,
    pub ruler: Option<Ruler>,
    pub alive: bool,
    /// Territory size in cells.
    pub cells: u32,
    pub founded: u32,
    /// Treasure from tolls on trade through the realm's land.
    pub wealth: f32,
    /// The realm this one is a vassal of.
    pub overlord: Option<u16>,
}

/// How two realms stand with each other.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    /// Lower realm id first.
    pub a: u16,
    pub b: u16,
    /// Goodwill, `-100..=100`.
    pub opinion: f32,
    pub alliance: bool,
    /// The grievance, while the realms are rivals.
    pub rivalry: Option<Grievance>,
    pub trade_pact: bool,
    /// The houses are joined by marriage.
    pub marriage: bool,
    /// No war before this year.
    pub truce_until: u32,
    /// The war they're fighting, by index into `HistoryState::wars`.
    pub war: Option<u32>,
    /// Wars fought between them so far.
    pub wars_fought: u32,
}

impl Relation {
    fn new(a: u16, b: u16) -> Self {
        Self {
            a: a.min(b),
            b: a.max(b),
            opinion: 0.0,
            alliance: false,
            rivalry: None,
            trade_pact: false,
            marriage: false,
            truce_until: 0,
            war: None,
            wars_fought: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WarState {
    pub attacker: u16,
    pub defender: u16,
    /// Everyone fighting on each side, leaders first.
    pub attackers: Vec<u16>,
    pub defenders: Vec<u16>,
    pub grievance: Grievance,
    pub started: u32,
    pub ended: Option<u32>,
    /// Battles won minus battles lost, from the attacker's side.
    pub score: i32,
    /// Cells that changed hands.
    pub territory: u32,
}

/// A battle site, kept so maps can mark it for a while afterwards.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BattleRecord {
    pub year: u32,
    pub site: u32,
    pub name: String,
    pub war: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettlementState {
    pub name: String,
    pub cell: u32,
    pub people: u16,
    pub rank: Rank,
    pub population: u32,
    pub alive: bool,
    /// Goods passing through or arriving in the last trade season.
    pub trade: f32,
    pub hub: bool,
    /// Ash, drought or ruin: no growth and poor harvests before this year.
    pub hardship_until: u32,
    /// Share of the people sick with plague (0 when healthy).
    pub plague: f32,
    /// The latest outbreak to reach it (a town is struck once per outbreak).
    pub outbreak: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MigrationRecord {
    pub year: u32,
    pub from: u32,
    pub to: u32,
    pub people: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Eruption {
    pub year: u32,
    pub cell: u32,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dragon {
    Asleep,
    Awake,
    Slain,
}

/// A landmark as it stands in a given year.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LandmarkState {
    pub name: String,
    pub kind: LandmarkKind,
    pub cell: u32,
    /// Dens only.
    pub dragon: Option<Dragon>,
    /// Ruins: the realm that found what was inside.
    pub discovered_by: Option<u16>,
    /// Nexuses: the realm that holds it.
    pub holder: Option<u16>,
    /// The story hook (for landmarks made during history, how they came to be).
    pub hook: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryState {
    /// Events up to and including this year have been applied.
    pub year: u32,
    /// How many events of the log have been applied, so replay can resume
    /// exactly where it stopped.
    pub applied: u32,
    pub width: usize,
    pub height: usize,
    /// Per cell: owning realm or `NO_REALM`.
    pub owner: Vec<u16>,
    pub realms: Vec<RealmState>,
    pub settlements: Vec<SettlementState>,
    /// Flows of the latest trade season.
    pub trade: Vec<Flow>,
    /// Routes goods flow along regularly, as `(from, to)`, sorted.
    pub routes: Vec<(u32, u32)>,
    /// Relations between realms that have any, sorted by `(a, b)`.
    pub relations: Vec<Relation>,
    pub wars: Vec<WarState>,
    pub battles: Vec<BattleRecord>,
    /// Year-0 landmarks first, then ruins left by the Blight and disasters.
    pub landmarks: Vec<LandmarkState>,
    /// Every eruption so far, for the volcano glyphs.
    pub eruptions: Vec<Eruption>,
    /// Every migration so far, for the arrows on the map.
    pub migrations: Vec<MigrationRecord>,
    /// Per cell: under the Blight.
    pub blight: Vec<bool>,
}

impl HistoryState {
    /// The state before any event: the realms and settlements stages.
    pub fn initial(realms: &Realms, settlements: &Settlements, landmarks: &Landmarks) -> Self {
        Self {
            year: 0,
            applied: 0,
            width: realms.width,
            height: realms.height,
            owner: realms.owner.clone(),
            realms: realms
                .realms
                .iter()
                .map(|r| RealmState {
                    name: r.name.clone(),
                    short_name: r.short_name.clone(),
                    capital: r.capital,
                    people: r.people,
                    colour: r.colour,
                    ruler: None,
                    alive: true,
                    cells: r.cells,
                    founded: 0,
                    wealth: 0.0,
                    overlord: None,
                })
                .collect(),
            settlements: settlements
                .settlements
                .iter()
                .map(|s| SettlementState {
                    name: s.name.clone(),
                    cell: s.cell as u32,
                    people: s.people,
                    rank: s.rank,
                    population: s.population,
                    alive: true,
                    trade: 0.0,
                    hub: false,
                    hardship_until: 0,
                    plague: 0.0,
                    outbreak: None,
                })
                .collect(),
            trade: Vec::new(),
            routes: Vec::new(),
            relations: Vec::new(),
            wars: Vec::new(),
            battles: Vec::new(),
            landmarks: landmarks
                .landmarks
                .iter()
                .map(|l| LandmarkState {
                    name: l.name.clone(),
                    kind: l.kind,
                    cell: l.cell,
                    dragon: (l.kind == LandmarkKind::DragonDen).then_some(Dragon::Asleep),
                    discovered_by: None,
                    holder: None,
                    hook: l.hook.clone(),
                })
                .collect(),
            blight: vec![false; realms.width * realms.height],
            eruptions: Vec::new(),
            migrations: Vec::new(),
        }
    }

    /// The realm owning a settlement's cell.
    pub fn settlement_realm(&self, settlement: u32) -> u16 {
        self.settlements
            .get(settlement as usize)
            .map_or(NO_REALM, |s| self.owner[s.cell as usize])
    }

    /// The relation between two realms, if they have one.
    pub fn relation(&self, a: u16, b: u16) -> Option<&Relation> {
        let key = (a.min(b), a.max(b));
        self.relations
            .binary_search_by(|r| (r.a, r.b).cmp(&key))
            .ok()
            .map(|i| &self.relations[i])
    }

    /// The relation between two realms, created if missing.
    pub fn relation_mut(&mut self, a: u16, b: u16) -> &mut Relation {
        let key = (a.min(b), a.max(b));
        let i = match self.relations.binary_search_by(|r| (r.a, r.b).cmp(&key)) {
            Ok(i) => i,
            Err(i) => {
                self.relations.insert(i, Relation::new(a, b));
                i
            }
        };
        &mut self.relations[i]
    }

    pub fn living_realms(&self) -> impl Iterator<Item = (u16, &RealmState)> {
        self.realms
            .iter()
            .enumerate()
            .filter(|(_, r)| r.alive)
            .map(|(i, r)| (i as u16, r))
    }

    /// Applies one event. This is the only way history changes the state.
    pub fn apply(&mut self, event: &Event) {
        self.year = event.year;
        self.applied += 1;
        match &event.kind {
            EventKind::RealmFounded { realm, ruler, .. }
            | EventKind::RulerSucceeded { realm, ruler, .. } => {
                self.realms[*realm as usize].ruler = Some(ruler.clone());
            }
            EventKind::BorderChanged {
                from, to, cells, ..
            } => {
                for &cell in cells {
                    self.owner[cell as usize] = *to;
                }
                let n = cells.len() as u32;
                if *from != NO_REALM {
                    let r = &mut self.realms[*from as usize];
                    r.cells = r.cells.saturating_sub(n);
                }
                if *to != NO_REALM {
                    self.realms[*to as usize].cells += n;
                }
            }
            EventKind::SettlementFounded {
                settlement,
                cell,
                name,
                people,
                population,
                ..
            } => {
                debug_assert_eq!(*settlement as usize, self.settlements.len());
                self.settlements.push(SettlementState {
                    name: name.clone(),
                    cell: *cell,
                    people: *people,
                    rank: Rank::Village,
                    population: *population,
                    alive: true,
                    trade: 0.0,
                    hub: false,
                    hardship_until: 0,
                    plague: 0.0,
                    outbreak: None,
                });
            }
            EventKind::SettlementGrew {
                settlement,
                population,
                ..
            }
            | EventKind::SettlementShrank {
                settlement,
                population,
                ..
            } => {
                self.settlements[*settlement as usize].population = *population;
            }
            EventKind::SettlementRenamed {
                settlement, name, ..
            } => {
                self.settlements[*settlement as usize].name = name.clone();
            }
            EventKind::RealmFell { realm, .. } => {
                for other in &mut self.realms {
                    if other.overlord == Some(*realm) {
                        other.overlord = None;
                    }
                }
                let r = &mut self.realms[*realm as usize];
                r.alive = false;
                r.ruler = None;
                r.overlord = None;
            }
            EventKind::RealmSplit {
                realm,
                new_realm,
                name,
                short_name,
                capital,
                colour,
                ruler,
                cells,
                ..
            } => {
                debug_assert_eq!(*new_realm as usize, self.realms.len());
                let people = self.settlements[*capital as usize].people;
                for &cell in cells {
                    self.owner[cell as usize] = *new_realm;
                }
                let n = cells.len() as u32;
                let parent = &mut self.realms[*realm as usize];
                parent.cells = parent.cells.saturating_sub(n);
                self.realms.push(RealmState {
                    name: name.clone(),
                    short_name: short_name.clone(),
                    capital: *capital,
                    people,
                    colour: *colour,
                    ruler: Some(ruler.clone()),
                    alive: true,
                    cells: n,
                    founded: event.year,
                    wealth: 0.0,
                    overlord: None,
                });
                self.settlements[*capital as usize].rank = Rank::Capital;
            }
            EventKind::TradeSeason { flows, tolls } => {
                for s in &mut self.settlements {
                    s.trade = 0.0;
                }
                for f in flows {
                    self.settlements[f.from as usize].trade += f.shipped;
                    self.settlements[f.to as usize].trade += f.received;
                }
                for &(realm, toll) in tolls {
                    self.realms[realm as usize].wealth += toll;
                }
                self.trade = flows.clone();
            }
            EventKind::TradeRouteOpened { from, to, .. } => {
                if let Err(at) = self.routes.binary_search(&(*from, *to)) {
                    self.routes.insert(at, (*from, *to));
                }
            }
            EventKind::TradeRouteClosed { from, to, .. } => {
                if let Ok(at) = self.routes.binary_search(&(*from, *to)) {
                    self.routes.remove(at);
                }
            }
            EventKind::TradeHub { settlement, .. } => {
                self.settlements[*settlement as usize].hub = true;
            }
            EventKind::Shortage { .. } => {}
            EventKind::OpinionsShifted { changes } => {
                for &(a, b, change) in changes {
                    let r = self.relation_mut(a, b);
                    r.opinion = (r.opinion + change).clamp(-100.0, 100.0);
                }
            }
            EventKind::AllianceFormed { a, b, .. } => self.relation_mut(*a, *b).alliance = true,
            EventKind::AllianceBroken { a, b, .. } => self.relation_mut(*a, *b).alliance = false,
            EventKind::RivalryDeclared { a, b, grievance } => {
                self.relation_mut(*a, *b).rivalry = Some(grievance.clone());
            }
            EventKind::TradePactSigned { a, b, .. } => self.relation_mut(*a, *b).trade_pact = true,
            EventKind::MarriageArranged { a, b, .. } => self.relation_mut(*a, *b).marriage = true,
            EventKind::VassalSworn {
                vassal, overlord, ..
            } => {
                self.realms[*vassal as usize].overlord = Some(*overlord);
            }
            EventKind::VassalFreed { vassal, .. } => self.realms[*vassal as usize].overlord = None,
            EventKind::WarDeclared {
                war,
                attacker,
                defender,
                grievance,
            } => {
                debug_assert_eq!(*war as usize, self.wars.len());
                self.wars.push(WarState {
                    attacker: *attacker,
                    defender: *defender,
                    attackers: vec![*attacker],
                    defenders: vec![*defender],
                    grievance: grievance.clone(),
                    started: event.year,
                    ended: None,
                    score: 0,
                    territory: 0,
                });
                let r = self.relation_mut(*attacker, *defender);
                r.war = Some(*war);
                r.alliance = false;
            }
            EventKind::WarJoined {
                war,
                realm,
                on_attacking_side,
                ..
            } => {
                let w = &mut self.wars[*war as usize];
                let (side, enemies) = if *on_attacking_side {
                    (&mut w.attackers, w.defenders.clone())
                } else {
                    (&mut w.defenders, w.attackers.clone())
                };
                side.push(*realm);
                for enemy in enemies {
                    let r = self.relation_mut(*realm, enemy);
                    r.war = Some(*war);
                    r.alliance = false;
                }
            }
            EventKind::Battle {
                war,
                name,
                site,
                attacker_won,
                ..
            } => {
                let w = &mut self.wars[*war as usize];
                w.score += if *attacker_won { 1 } else { -1 };
                self.battles.push(BattleRecord {
                    year: event.year,
                    site: *site,
                    name: name.clone(),
                    war: *war,
                });
            }
            EventKind::Siege { .. } => {}
            EventKind::PeaceSigned { war, terms, .. } => {
                let loser = {
                    let w = &self.wars[*war as usize];
                    if terms.victor == w.attacker {
                        w.defender
                    } else {
                        w.attacker
                    }
                };
                self.realms[loser as usize].wealth -= terms.tribute;
                self.realms[terms.victor as usize].wealth += terms.tribute;
                let w = &mut self.wars[*war as usize];
                w.ended = Some(event.year);
                w.territory = terms.territory;
                let (attackers, defenders) = (w.attackers.clone(), w.defenders.clone());
                for &x in &attackers {
                    for &y in &defenders {
                        let r = self.relation_mut(x, y);
                        if r.war == Some(*war) {
                            r.war = None;
                            r.truce_until = event.year + 20;
                            r.wars_fought += 1;
                        }
                    }
                }
            }
            EventKind::RealmAnnexed { realm, by, .. } => {
                let cells: Vec<usize> = (0..self.owner.len())
                    .filter(|&c| self.owner[c] == *realm)
                    .collect();
                for &c in &cells {
                    self.owner[c] = *by;
                }
                let n = cells.len() as u32;
                self.realms[*by as usize].cells += n;
                let r = &mut self.realms[*realm as usize];
                r.cells = 0;
                r.alive = false;
                r.ruler = None;
                r.overlord = None;
                for other in &mut self.realms {
                    if other.overlord == Some(*realm) {
                        other.overlord = None;
                    }
                }
            }
            EventKind::DragonAwakened { landmark, .. } => {
                self.landmarks[*landmark as usize].dragon = Some(Dragon::Awake);
            }
            EventKind::DragonSlept { landmark } => {
                self.landmarks[*landmark as usize].dragon = Some(Dragon::Asleep);
            }
            EventKind::DragonRaid {
                settlement, deaths, ..
            } => {
                let s = &mut self.settlements[*settlement as usize];
                s.population = s.population.saturating_sub(*deaths);
            }
            EventKind::DragonSlain {
                landmark,
                realm,
                hoard,
                ..
            } => {
                self.landmarks[*landmark as usize].dragon = Some(Dragon::Slain);
                self.realms[*realm as usize].wealth += hoard;
            }
            EventKind::RuinDiscovered {
                landmark,
                realm,
                treasure,
            } => {
                self.landmarks[*landmark as usize].discovered_by = Some(*realm);
                self.realms[*realm as usize].wealth += treasure;
            }
            EventKind::NexusClaimed {
                landmark,
                realm,
                boon,
            } => {
                self.landmarks[*landmark as usize].holder = Some(*realm);
                self.realms[*realm as usize].wealth += boon;
            }
            EventKind::VolcanoErupted { cell, name, .. } => {
                self.eruptions.push(Eruption {
                    year: event.year,
                    cell: *cell,
                    name: name.clone(),
                });
            }
            EventKind::Earthquake { .. } | EventKind::Drought { .. } => {}
            EventKind::PlagueOutbreak {
                outbreak,
                settlement,
                ..
            }
            | EventKind::PlagueSpread {
                outbreak,
                to: settlement,
                ..
            } => {
                let s = &mut self.settlements[*settlement as usize];
                s.plague = 0.02;
                s.outbreak = Some(*outbreak);
            }
            EventKind::PlagueToll {
                settlement,
                deaths,
                infected,
            } => {
                let s = &mut self.settlements[*settlement as usize];
                s.population = s.population.saturating_sub(*deaths);
                s.plague = *infected;
            }
            EventKind::Famine {
                settlement, deaths, ..
            } => {
                let s = &mut self.settlements[*settlement as usize];
                s.population = s.population.saturating_sub(*deaths);
            }
            EventKind::Migration {
                from, to, people, ..
            } => {
                let f = &mut self.settlements[*from as usize];
                let moved = (*people).min(f.population);
                f.population -= moved;
                self.settlements[*to as usize].population += moved;
                self.migrations.push(MigrationRecord {
                    year: event.year,
                    from: *from,
                    to: *to,
                    people: moved,
                });
            }
            EventKind::HardTimes {
                settlements, until, ..
            } => {
                for &s in settlements {
                    let st = &mut self.settlements[s as usize];
                    st.hardship_until = st.hardship_until.max(*until);
                }
            }
            EventKind::SettlementStruck {
                settlement,
                deaths,
                abandoned,
                ruin,
                hook,
                ..
            } => {
                let s = &mut self.settlements[*settlement as usize];
                s.population = s.population.saturating_sub(*deaths);
                if *abandoned {
                    s.alive = false;
                    let cell = s.cell;
                    self.landmarks.push(LandmarkState {
                        name: ruin.clone(),
                        kind: LandmarkKind::FallenTown,
                        cell,
                        dragon: None,
                        discovered_by: None,
                        holder: None,
                        hook: hook.clone(),
                    });
                }
            }
            EventKind::BlightSpread { cells, .. } => {
                for &c in cells {
                    self.blight[c as usize] = true;
                }
            }
            EventKind::BlightPurged {
                cells, realm, cost, ..
            } => {
                for &c in cells {
                    self.blight[c as usize] = false;
                }
                self.realms[*realm as usize].wealth -= cost;
            }
            EventKind::BlightReachedSettlement {
                settlement,
                deaths,
                abandoned,
                ruin,
            } => {
                let s = &mut self.settlements[*settlement as usize];
                s.population = s.population.saturating_sub(*deaths);
                if *abandoned {
                    s.alive = false;
                    let (name, cell) = (s.name.clone(), s.cell);
                    self.landmarks.push(LandmarkState {
                        name: ruin.clone(),
                        kind: LandmarkKind::BlightedRuin,
                        cell,
                        dragon: None,
                        discovered_by: None,
                        holder: None,
                        hook: format!(
                            "{ruin}: once the town of {name}, emptied by the Blight in the year {}.",
                            event.year
                        ),
                    });
                }
            }
        }
    }

    /// Cells under the Blight.
    pub fn blight_extent(&self) -> usize {
        self.blight.iter().filter(|&&b| b).count()
    }
}
