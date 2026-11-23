//! The event log's vocabulary. Every change to the world after year 0 is one
//! of these, and each records why it happened so chronicles can explain it.

use serde::{Deserialize, Serialize};

use crate::resources::Resource;

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
    /// A settlement needed goods it couldn't make.
    Demand,
    /// The need that kept a route open has passed.
    NoLongerNeeded,
    /// Goods flowed through a place in quantity.
    Trade,
    /// Long goodwill between two realms.
    Friendship,
    /// Goodwill between allies soured.
    Estrangement,
    /// A powerful neighbour offered protection a weak realm couldn't refuse.
    Protection,
    /// A vassal grew strong enough, or its overlord weak enough, to break free.
    Independence,
    /// A long-held vassal was absorbed outright.
    Absorption,
    /// Ties of blood between ruling houses.
    Alliance,
    /// Settlers, miners or herders came too close to something sleeping.
    Disturbed,
    /// Someone delved where they should not have.
    Delving,
    /// Plates grinding against each other deep below.
    Tectonics,
    /// The rains failed.
    Weather,
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
    RealmFell {
        realm: u16,
        cause: Cause,
    },
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
    /// A trade season: every flow of goods and the tolls realms collected.
    TradeSeason {
        flows: Vec<Flow>,
        /// `(realm, toll)`, sorted by realm.
        tolls: Vec<(u16, f32)>,
    },
    /// Goods began to flow regularly between two settlements.
    TradeRouteOpened {
        from: u32,
        to: u32,
        cause: Cause,
    },
    TradeRouteClosed {
        from: u32,
        to: u32,
        cause: Cause,
    },
    /// A settlement became a trade hub.
    TradeHub {
        settlement: u32,
        cause: Cause,
    },
    /// A settlement couldn't get enough of something.
    Shortage {
        settlement: u32,
        resource: Resource,
        cause: Cause,
    },
    /// Opinions between realms drift (every few years): `(a, b, change)`.
    OpinionsShifted {
        changes: Vec<(u16, u16, f32)>,
    },
    AllianceFormed {
        a: u16,
        b: u16,
        cause: Cause,
    },
    AllianceBroken {
        a: u16,
        b: u16,
        cause: Cause,
    },
    RivalryDeclared {
        a: u16,
        b: u16,
        grievance: Grievance,
    },
    TradePactSigned {
        a: u16,
        b: u16,
        cause: Cause,
    },
    /// A marriage between the ruling houses of `a` (the bride's) and `b`.
    MarriageArranged {
        a: u16,
        b: u16,
        bride: String,
        bride_house: String,
        groom: String,
        groom_house: String,
        cause: Cause,
    },
    VassalSworn {
        vassal: u16,
        overlord: u16,
        cause: Cause,
    },
    VassalFreed {
        vassal: u16,
        overlord: u16,
        cause: Cause,
    },
    /// `war` is the index the war gets in `HistoryState::wars`.
    WarDeclared {
        war: u32,
        attacker: u16,
        defender: u16,
        grievance: Grievance,
    },
    /// An ally takes up arms.
    WarJoined {
        war: u32,
        realm: u16,
        on_attacking_side: bool,
        cause: Cause,
    },
    Battle {
        war: u32,
        name: String,
        site: u32,
        attacker_won: bool,
        attacker_losses: u32,
        defender_losses: u32,
    },
    Siege {
        war: u32,
        settlement: u32,
        fell: bool,
    },
    PeaceSigned {
        war: u32,
        attacker: u16,
        defender: u16,
        terms: PeaceTerms,
    },
    /// A realm is swallowed whole by another.
    RealmAnnexed {
        realm: u16,
        by: u16,
        cause: Cause,
    },
    /// `landmark` indexes `HistoryState::landmarks` (a dragon den).
    DragonAwakened {
        landmark: u32,
        cause: Cause,
    },
    DragonRaid {
        landmark: u32,
        settlement: u32,
        deaths: u32,
    },
    /// The dragon goes back to sleep on its hoard.
    DragonSlept {
        landmark: u32,
    },
    /// A hero of `realm` kills the dragon and carries off its hoard.
    DragonSlain {
        landmark: u32,
        realm: u16,
        hero: String,
        hoard: f32,
        cause: Cause,
    },
    /// A realm finds an elder ruin on its land and what was left in it.
    RuinDiscovered {
        landmark: u32,
        realm: u16,
        treasure: f32,
    },
    /// A realm takes a leyline nexus and the power that pools there.
    NexusClaimed {
        landmark: u32,
        realm: u16,
        boon: f32,
    },
    /// The Blight takes these cells.
    BlightSpread {
        cells: Vec<u32>,
        cause: Cause,
    },
    /// A realm burns the Blight out of these cells.
    BlightPurged {
        cells: Vec<u32>,
        realm: u16,
        hero: String,
        cost: f32,
    },
    /// A volcano erupts where plates converge.
    VolcanoErupted {
        cell: u32,
        /// The mountain's name, in the local tongue.
        name: String,
        cause: Cause,
    },
    /// The ground shakes along a plate boundary.
    Earthquake {
        cell: u32,
        magnitude: f32,
        cause: Cause,
    },
    /// The rains fail around `cell`.
    Drought {
        cell: u32,
        radius: f32,
        cause: Cause,
    },
    /// A settlement in hard times that cannot buy grain starves.
    Famine {
        settlement: u32,
        deaths: u32,
        cause: Cause,
    },
    /// A plague breaks out at a port or trade hub. `outbreak` numbers
    /// outbreaks from 0.
    PlagueOutbreak {
        outbreak: u32,
        settlement: u32,
        name: String,
        cause: Cause,
    },
    /// The plague travels along a trade route.
    PlagueSpread {
        outbreak: u32,
        from: u32,
        to: u32,
    },
    /// A year of plague in a settlement: `infected` is the share now sick
    /// (0 when it has passed).
    PlagueToll {
        settlement: u32,
        deaths: u32,
        infected: f32,
    },
    /// People leave a settlement for another (fleeing famine, mostly).
    Migration {
        from: u32,
        to: u32,
        people: u32,
        cause: Cause,
    },
    /// Ash or ruined fields: these settlements can't grow, and grow less
    /// grain, until `until`.
    HardTimes {
        settlements: Vec<u32>,
        until: u32,
        cause: Cause,
    },
    /// A disaster kills part of a settlement. If `abandoned`, the
    /// survivors leave and it becomes a ruined-town landmark named `ruin`
    /// with story `hook`.
    SettlementStruck {
        settlement: u32,
        deaths: u32,
        abandoned: bool,
        ruin: String,
        hook: String,
        cause: Cause,
    },
    /// The Blight reaches a settlement. If `abandoned`, it becomes a ruin
    /// (a new landmark named `ruin`).
    BlightReachedSettlement {
        settlement: u32,
        deaths: u32,
        abandoned: bool,
        ruin: String,
    },
}

/// What a rivalry or war is about. Each names the place, route or house in
/// question so chronicles can say exactly why.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Grievance {
    /// Land both realms claim, near this cell.
    DisputedBorder { cell: u32 },
    /// A trade route between two settlements both want to control.
    ContestedTradeRoute { from: u32, to: u32 },
    /// A marriage gave this house a claim to the other's throne.
    SuccessionClaim { house: String },
    /// The realms' peoples have drifted apart in custom and faith.
    CulturalSplit,
    /// A slight at the wedding that joined these houses.
    WeddingSlight { house: String },
    /// An earlier war (index into the realms' wars) never forgiven.
    OldWar { war: u32 },
}

/// What a peace settled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PeaceTerms {
    /// The side that came out ahead.
    pub victor: u16,
    /// Cells that changed hands during the war.
    pub territory: u32,
    /// Treasure the loser paid.
    pub tribute: f32,
    /// The loser bent the knee as a vassal.
    pub vassalage: bool,
}

/// Goods moved in one trade season.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    pub from: u32,
    pub to: u32,
    pub resource: Resource,
    /// Amount sent.
    pub shipped: f32,
    /// Amount that arrived; the rest was lost on the way.
    pub received: f32,
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
            EventKind::AllianceFormed { a, b, .. }
            | EventKind::AllianceBroken { a, b, .. }
            | EventKind::RivalryDeclared { a, b, .. }
            | EventKind::TradePactSigned { a, b, .. }
            | EventKind::MarriageArranged { a, b, .. } => vec![*a, *b],
            EventKind::VassalSworn {
                vassal, overlord, ..
            }
            | EventKind::VassalFreed {
                vassal, overlord, ..
            } => {
                vec![*vassal, *overlord]
            }
            EventKind::WarDeclared {
                attacker, defender, ..
            }
            | EventKind::PeaceSigned {
                attacker, defender, ..
            } => {
                vec![*attacker, *defender]
            }
            EventKind::WarJoined { realm, .. } => vec![*realm],
            EventKind::RealmAnnexed { realm, by, .. } => vec![*realm, *by],
            EventKind::DragonSlain { realm, .. }
            | EventKind::RuinDiscovered { realm, .. }
            | EventKind::NexusClaimed { realm, .. }
            | EventKind::BlightPurged { realm, .. } => vec![*realm],
            EventKind::RealmSplit {
                realm, new_realm, ..
            } => vec![*realm, *new_realm],
            _ => Vec::new(),
        }
    }

    /// The landmark this event concerns, if any (index into
    /// `HistoryState::landmarks`).
    pub fn landmark(&self) -> Option<u32> {
        match self {
            EventKind::DragonAwakened { landmark, .. }
            | EventKind::DragonRaid { landmark, .. }
            | EventKind::DragonSlept { landmark }
            | EventKind::DragonSlain { landmark, .. }
            | EventKind::RuinDiscovered { landmark, .. }
            | EventKind::NexusClaimed { landmark, .. } => Some(*landmark),
            _ => None,
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
            EventKind::TradeRouteOpened { from, to, .. }
            | EventKind::TradeRouteClosed { from, to, .. } => vec![*from, *to],
            EventKind::TradeHub { settlement, .. }
            | EventKind::Shortage { settlement, .. }
            | EventKind::Siege { settlement, .. }
            | EventKind::DragonRaid { settlement, .. }
            | EventKind::BlightReachedSettlement { settlement, .. }
            | EventKind::SettlementStruck { settlement, .. }
            | EventKind::Famine { settlement, .. } => vec![*settlement],
            EventKind::Migration { from, to, .. } | EventKind::PlagueSpread { from, to, .. } => {
                vec![*from, *to]
            }
            EventKind::PlagueOutbreak { settlement, .. }
            | EventKind::PlagueToll { settlement, .. } => {
                vec![*settlement]
            }
            EventKind::HardTimes { settlements, .. } => settlements.clone(),
            _ => Vec::new(),
        }
    }

    /// Whether the event is minor background noise (growth and shrinkage).
    pub fn is_minor(&self) -> bool {
        matches!(
            self,
            EventKind::SettlementGrew { .. }
                | EventKind::SettlementShrank { .. }
                | EventKind::TradeSeason { .. }
                | EventKind::TradeRouteOpened { .. }
                | EventKind::TradeRouteClosed { .. }
                | EventKind::OpinionsShifted { .. }
                | EventKind::BlightSpread { .. }
                | EventKind::PlagueToll { .. }
        )
    }
}
