//! The event log's vocabulary. Every change to the world after year 0 is one
//! of these, and each records why it happened so chronicles can explain it.

use serde::{Deserialize, Serialize};

/// A ruler: name, house and year of birth (negative = before year 0).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ruler {
    pub name: String,
    pub house: String,
    pub born: i32,
}

/// Why something happened. `Event` points at an earlier entry of the log by
/// index, so causes chain ("the town was renamed because the border moved
/// because...").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cause {
    /// Part of the world as it stood at year 0.
    Founding,
    /// A ruler died of old age.
    OldAge,
    /// A ruler died young (illness, accident, the knife).
    UntimelyDeath,
    /// Neighbours contested a frontier.
    BorderDispute,
    /// An expansionist realm pushed outward.
    Ambition,
    /// A much stronger realm swallowed a weaker one.
    Conquest,
    /// Good land and peace.
    Prosperity,
    /// Poor harvests, sickness, flight.
    Hardship,
    /// A realm grew too large to hold together.
    Overextension,
    /// Settlers moved into empty good land.
    Settlement,
    /// The direct consequence of an earlier event in the log.
    Event(u32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EventKind {
    /// A realm existing at year 0 (its founding ruler takes the throne).
    RealmFounded {
        realm: u16,
        ruler: Ruler,
        cause: Cause,
    },
    /// A new ruler takes the throne.
    RulerSucceeded {
        realm: u16,
        ruler: Ruler,
        cause: Cause,
    },
    /// Cells pass from one realm (or wilderness) to another.
    BorderChanged {
        from: u16,
        to: u16,
        cells: Vec<u32>,
        cause: Cause,
    },
    /// A new settlement appears.
    SettlementFounded {
        settlement: u32,
        cell: u32,
        name: String,
        people: u16,
        population: u32,
        cause: Cause,
    },
    SettlementGrew {
        settlement: u32,
        population: u32,
        cause: Cause,
    },
    SettlementShrank {
        settlement: u32,
        population: u32,
        cause: Cause,
    },
    SettlementRenamed {
        settlement: u32,
        name: String,
        cause: Cause,
    },
    /// A realm ceases to exist.
    RealmFell { realm: u16, cause: Cause },
    /// Part of a realm breaks away as a new realm.
    RealmSplit {
        realm: u16,
        new_realm: u16,
        name: String,
        short_name: String,
        capital: u32,
        colour: u8,
        ruler: Ruler,
        cells: Vec<u32>,
        cause: Cause,
    },
}

/// One entry of the log. Ordered by `(year, seq)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub year: u32,
    /// Order within the year.
    pub seq: u32,
    pub kind: EventKind,
}

impl EventKind {
    /// Realms this event involves, for highlighting.
    pub fn realms(&self) -> Vec<u16> {
        match self {
            EventKind::RealmFounded { realm, .. }
            | EventKind::RulerSucceeded { realm, .. }
            | EventKind::RealmFell { realm, .. } => vec![*realm],
            EventKind::BorderChanged { from, to, .. } => vec![*from, *to],
            EventKind::RealmSplit {
                realm, new_realm, ..
            } => vec![*realm, *new_realm],
            _ => Vec::new(),
        }
    }

    /// Settlements this event involves, for highlighting.
    pub fn settlements(&self) -> Vec<u32> {
        match self {
            EventKind::SettlementFounded { settlement, .. }
            | EventKind::SettlementGrew { settlement, .. }
            | EventKind::SettlementShrank { settlement, .. }
            | EventKind::SettlementRenamed { settlement, .. } => vec![*settlement],
            EventKind::RealmSplit { capital, .. } => vec![*capital],
            _ => Vec::new(),
        }
    }

    /// Whether the event is minor background noise (growth and shrinkage).
    pub fn is_minor(&self) -> bool {
        matches!(
            self,
            EventKind::SettlementGrew { .. } | EventKind::SettlementShrank { .. }
        )
    }
}
