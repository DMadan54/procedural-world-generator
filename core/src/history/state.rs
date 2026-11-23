//! The world's political and settlement state at a given year. It is only
//! ever changed by applying events, so the state at any year is exactly the
//! year-0 state with every earlier event replayed onto it.

use serde::{Deserialize, Serialize};

use super::event::{Event, EventKind, Ruler};
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettlementState {
    pub name: String,
    pub cell: u32,
    pub people: u16,
    pub rank: Rank,
    pub population: u32,
    pub alive: bool,
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
}

impl HistoryState {
    /// The state before any event: the realms and settlements stages.
    pub fn initial(realms: &Realms, settlements: &Settlements) -> Self {
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
                })
                .collect(),
        }
    }

    /// The realm owning a settlement's cell.
    pub fn settlement_realm(&self, settlement: u32) -> u16 {
        self.settlements
            .get(settlement as usize)
            .map_or(NO_REALM, |s| self.owner[s.cell as usize])
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
                let r = &mut self.realms[*realm as usize];
                r.alive = false;
                r.ruler = None;
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
                });
                self.settlements[*capital as usize].rank = Rank::Capital;
            }
        }
    }
}
