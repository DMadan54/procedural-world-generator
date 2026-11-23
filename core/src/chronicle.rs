//! The event log as a readable chronicle: chapters by century, each with a
//! short summary, then a paragraph per year in which something worth
//! telling happened. Every sentence names things as they were called at
//! the time, and follows cause chains so the text says why ("after the
//! rains failed around Ziak").
//!
//! Wording varies by event: each event picks one of several phrasings with
//! `entity_rng(seed, "chronicle", event index)`, so the same world always
//! reads the same, but neighbouring entries don't all sound alike.
//!
//! Every [`EventKind`] has a template: [`Writer::sentence`] matches them
//! exhaustively, with no catch-all arm, so a new event kind won't compile
//! until it can be told.

use std::collections::BTreeMap;

use rand_chacha::rand_core::RngCore;

use crate::history::{Cause, Event, EventKind, Grievance, HistoryState, Ruler};
use crate::realms::NO_REALM;
use crate::rng::{entity_rng, stage};
use crate::world::World;

/// Whose story to tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Focus {
    World,
    Realm(u16),
    People(u16),
    /// A ruling house, by name (without "House").
    House(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChronicleOptions {
    pub focus: Focus,
    pub from: u32,
    pub to: u32,
    /// Keep the routine entries (growth, trade, border creep).
    pub everything: bool,
    /// Add the appendix for writers: gazetteer, ruling houses, names.
    pub for_writers: bool,
}

impl ChronicleOptions {
    pub fn whole(world: &World) -> Self {
        Self {
            focus: Focus::World,
            from: 0,
            to: world.history.years,
            everything: false,
            for_writers: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chronicle {
    pub title: String,
    pub chapters: Vec<Chapter>,
    pub appendix: Vec<Section>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub title: String,
    pub from: u32,
    pub to: u32,
    pub summary: String,
    pub years: Vec<YearEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct YearEntry {
    pub year: u32,
    pub text: String,
}

/// An appendix section: a heading and its entries.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub entries: Vec<String>,
}

/// Years per chapter.
const CHAPTER_YEARS: u32 = 100;

pub fn chronicle(world: &World, options: &ChronicleOptions) -> Chronicle {
    let events = &world.history.events;
    let mut writer = Writer {
        world,
        events,
        state: world.history.initial.clone(),
    };
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut tally = Tally::default();
    let mut paragraph: Vec<String> = Vec::new();
    let mut paragraph_year = 0;
    let to = options.to.min(world.history.years);
    let flush = |chapters: &mut Vec<Chapter>, paragraph: &mut Vec<String>, year: u32| {
        if !paragraph.is_empty() {
            if let Some(chapter) = chapters.last_mut() {
                chapter.years.push(YearEntry {
                    year,
                    text: paragraph.join(" "),
                });
            }
            paragraph.clear();
        }
    };
    for (index, event) in events.iter().enumerate() {
        if event.year > to {
            break;
        }
        let shown = event.year >= options.from
            && (options.everything || !routine(&event.kind))
            && writer.concerns(&event.kind, &options.focus);
        if shown {
            let chapter_start = (event.year.max(1) - 1) / CHAPTER_YEARS * CHAPTER_YEARS;
            if chapters.last().is_none_or(|c| c.from != chapter_start) {
                flush(&mut chapters, &mut paragraph, paragraph_year);
                if let Some(chapter) = chapters.last_mut() {
                    chapter.summary = tally.summary(&writer);
                }
                tally = Tally::default();
                chapters.push(Chapter {
                    title: chapter_title(chapter_start, world.history.years),
                    from: chapter_start,
                    to: (chapter_start + CHAPTER_YEARS).min(world.history.years),
                    summary: String::new(),
                    years: Vec::new(),
                });
            }
            if event.year != paragraph_year {
                flush(&mut chapters, &mut paragraph, paragraph_year);
                paragraph_year = event.year;
            }
            let variant = entity_rng(world.config.seed, stage::CHRONICLE, index as u64).next_u32();
            paragraph.push(capitalise(&writer.sentence(index, event, variant)));
            tally.count(&writer, &event.kind);
        }
        writer.state.apply(event);
    }
    flush(&mut chapters, &mut paragraph, paragraph_year);
    if let Some(chapter) = chapters.last_mut() {
        chapter.summary = tally.summary(&writer);
    }
    let title = match &options.focus {
        Focus::World => format!("A Chronicle of {}", world_name(world)),
        Focus::Realm(r) => format!("A Chronicle of {}", writer.realm(*r)),
        Focus::People(p) => format!(
            "A Chronicle of the {}",
            world
                .peoples
                .peoples
                .get(*p as usize)
                .map_or("Peoples", |p| p.name.as_str())
        ),
        Focus::House(h) => format!("A Chronicle of House {h}"),
    };
    let appendix = if options.for_writers {
        appendix(world, &writer.state)
    } else {
        Vec::new()
    };
    Chronicle {
        title,
        chapters,
        appendix,
    }
}

/// The world's name, in the tongue of its largest people.
pub fn world_name(world: &World) -> String {
    world
        .peoples
        .peoples
        .iter()
        .max_by(|a, b| a.cells.cmp(&b.cells).then(b.id.cmp(&a.id)))
        .map(|p| p.language.place_name(800_000))
        .unwrap_or_else(|| "the Known World".to_owned())
}

fn chapter_title(start: u32, years: u32) -> String {
    let n = start / CHAPTER_YEARS + 1;
    let ordinal = match n {
        1 => "First".to_owned(),
        2 => "Second".to_owned(),
        3 => "Third".to_owned(),
        4 => "Fourth".to_owned(),
        5 => "Fifth".to_owned(),
        6 => "Sixth".to_owned(),
        7 => "Seventh".to_owned(),
        8 => "Eighth".to_owned(),
        9 => "Ninth".to_owned(),
        10 => "Tenth".to_owned(),
        11 => "Eleventh".to_owned(),
        12 => "Twelfth".to_owned(),
        n => format!("{n}th"),
    };
    let end = (start + CHAPTER_YEARS).min(years);
    format!("The {ordinal} Century (years {}–{end})", start + 1)
}

/// The everyday churn a chronicler leaves out.
fn routine(kind: &EventKind) -> bool {
    kind.is_minor() || matches!(kind, EventKind::BorderChanged { .. })
}

/// What happened in a chapter, for its summary.
#[derive(Default)]
struct Tally {
    rose: Vec<String>,
    fell: Vec<String>,
    /// War index → battles fought in it this chapter.
    battles: BTreeMap<u32, u32>,
    wars: Vec<(u32, u16, u16, Grievance)>,
    disasters: Vec<String>,
    famines: u32,
    plagues: Vec<String>,
}

impl Tally {
    fn count(&mut self, w: &Writer, kind: &EventKind) {
        match kind {
            EventKind::RealmSplit { name, .. } => self.rose.push(format!("the {name}")),
            EventKind::RealmFell { realm, .. } | EventKind::RealmAnnexed { realm, .. } => {
                self.fell.push(w.realm(*realm))
            }
            EventKind::WarDeclared {
                war,
                attacker,
                defender,
                grievance,
            } => self
                .wars
                .push((*war, *attacker, *defender, grievance.clone())),
            EventKind::Battle { war, .. } => *self.battles.entry(*war).or_default() += 1,
            EventKind::VolcanoErupted { name, .. } => {
                self.disasters.push(format!("{name} erupted"))
            }
            EventKind::RiverAvulsion { name, .. } => self
                .disasters
                .push(format!("the {name} changed its course")),
            EventKind::Famine { .. } => self.famines += 1,
            EventKind::PlagueOutbreak { name, .. } => self.plagues.push(name.clone()),
            _ => {}
        }
    }

    fn summary(&self, w: &Writer) -> String {
        let mut parts = Vec::new();
        match (self.rose.is_empty(), self.fell.is_empty()) {
            (false, false) => parts.push(format!(
                "{} rose, and {} fell.",
                capitalise(&list(&self.rose)),
                list(&self.fell)
            )),
            (false, true) => parts.push(format!("{} rose.", capitalise(&list(&self.rose)))),
            (true, false) => parts.push(format!("{} fell.", capitalise(&list(&self.fell)))),
            (true, true) => {}
        }
        let greatest = self
            .wars
            .iter()
            .max_by_key(|(war, ..)| (self.battles.get(war).copied().unwrap_or(0), u32::MAX - war));
        if let Some((war, attacker, defender, grievance)) = greatest {
            let battles = self.battles.get(war).copied().unwrap_or(0);
            let fought = match battles {
                0 => String::new(),
                1 => ", decided in a single battle".to_owned(),
                n => format!(", fought over {n} battles"),
            };
            parts.push(format!(
                "The greatest war was {} against {} over {}{fought}.",
                w.realm(*attacker),
                w.realm(*defender),
                w.grievance(grievance)
            ));
        }
        let mut sorrows: Vec<String> = self.disasters.clone();
        sorrows.extend(
            self.plagues
                .iter()
                .map(|p| format!("{p} swept the trade roads")),
        );
        if self.famines > 0 {
            sorrows.push(match self.famines {
                1 => "one town starved".to_owned(),
                n => format!("famine struck {n} times"),
            });
        }
        if !sorrows.is_empty() {
            parts.push(format!("{}.", capitalise(&list(&sorrows))));
        }
        if parts.is_empty() {
            "A quiet age, little troubled by war or ruin.".to_owned()
        } else {
            parts.join(" ")
        }
    }
}

/// "a", "a and b", "a, b and c".
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// Picks one phrasing.
fn pick(variant: u32, options: Vec<String>) -> String {
    let n = options.len().max(1);
    options
        .into_iter()
        .nth(variant as usize % n)
        .unwrap_or_default()
}

struct Writer<'a> {
    world: &'a World,
    events: &'a [Event],
    /// The state just before the event being written.
    state: HistoryState,
}

impl Writer<'_> {
    fn realm(&self, r: u16) -> String {
        if r == NO_REALM {
            return "the wilds".to_owned();
        }
        self.state.realms.get(r as usize).map_or_else(
            || "a realm since forgotten".to_owned(),
            |r| format!("the {}", r.name),
        )
    }

    fn town(&self, s: u32) -> String {
        self.state
            .settlements
            .get(s as usize)
            .map_or_else(|| "a town since forgotten".to_owned(), |s| s.name.clone())
    }

    fn landmark(&self, l: u32) -> String {
        self.state
            .landmarks
            .get(l as usize)
            .map_or_else(|| "a place since forgotten".to_owned(), |l| l.name.clone())
    }

    fn people(&self, p: u16) -> String {
        self.world
            .peoples
            .peoples
            .get(p as usize)
            .map_or_else(|| "a people since forgotten".to_owned(), |p| p.name.clone())
    }

    fn ruler(r: &Ruler) -> String {
        format!("{} of House {}", r.name, r.house)
    }

    /// The nearest living town to a cell.
    fn near(&self, cell: u32) -> String {
        let w = self.state.width.max(1);
        self.state
            .settlements
            .iter()
            .filter(|s| s.alive)
            .min_by_key(|s| {
                let (a, b) = (s.cell as usize, cell as usize);
                (
                    (a % w).abs_diff(b % w).pow(2) + (a / w).abs_diff(b / w).pow(2),
                    s.cell,
                )
            })
            .map_or_else(|| "the wilds".to_owned(), |s| s.name.clone())
    }

    fn grievance(&self, g: &Grievance) -> String {
        match g {
            Grievance::DisputedBorder { cell } => {
                format!("the borderlands near {}", self.near(*cell))
            }
            Grievance::ContestedTradeRoute { from, to } => {
                format!(
                    "the trade road from {} to {}",
                    self.town(*from),
                    self.town(*to)
                )
            }
            Grievance::SuccessionClaim { house } => format!("House {house}'s claim to the throne"),
            Grievance::CulturalSplit => "old differences of custom and faith".to_owned(),
            Grievance::WeddingSlight { house } => {
                format!("a slight to House {house} at a royal wedding")
            }
            Grievance::OldWar { .. } => "an old war never forgiven".to_owned(),
        }
    }

    /// Why something happened, as a trailing clause (empty if there's
    /// nothing worth saying). Follows event causes up to `depth` links.
    fn reason(&self, cause: Cause, depth: u32) -> String {
        match cause {
            Cause::Event(i) => self.event_reason(i as usize, depth),
            Cause::OldAge => " on the death of the old ruler".to_owned(),
            Cause::UntimelyDeath => " after an untimely death".to_owned(),
            Cause::Conquest => " by conquest".to_owned(),
            Cause::Overextension => ", grown too large to hold together".to_owned(),
            Cause::Hardship => " in hard years".to_owned(),
            Cause::Prosperity => " in years of plenty".to_owned(),
            Cause::Friendship => " after long friendship".to_owned(),
            Cause::Estrangement => " as old goodwill soured".to_owned(),
            Cause::Protection => ", seeking a strong protector".to_owned(),
            Cause::Independence => ", grown strong enough to stand alone".to_owned(),
            Cause::Disturbed => ", stirred by the towns crowding below".to_owned(),
            Cause::Delving => ", where someone delved too deep".to_owned(),
            Cause::Tectonics => String::new(),
            Cause::Weather => String::new(),
            _ => String::new(),
        }
    }

    fn event_reason(&self, i: usize, depth: u32) -> String {
        let Some(event) = self.events.get(i) else {
            return String::new();
        };
        match &event.kind {
            EventKind::Drought { cell, .. } => {
                format!(" after the rains failed around {}", self.near(*cell))
            }
            EventKind::HardTimes { cause, .. } if depth > 0 => self.reason(*cause, depth - 1),
            EventKind::HardTimes { .. } => " in hard years".to_owned(),
            EventKind::VolcanoErupted { name, .. } => format!(" after {name} erupted"),
            EventKind::Earthquake { cell, .. } => {
                format!(" after the great earthquake near {}", self.near(*cell))
            }
            EventKind::Siege { settlement, .. } => {
                format!(" after the siege of {}", self.town(*settlement))
            }
            EventKind::Famine { settlement, .. } => {
                format!(", fleeing the famine in {}", self.town(*settlement))
            }
            EventKind::Flood { name, .. } => format!(" after the {name} flooded"),
            EventKind::RiverAvulsion { name, .. } => format!(" when the {name} changed its course"),
            EventKind::DragonRaid {
                landmark,
                settlement,
                ..
            } => format!(
                " after the dragon of {} burned {}",
                self.landmark(*landmark),
                self.town(*settlement)
            ),
            EventKind::PlagueToll { settlement, .. } => {
                format!(" as plague emptied {}", self.town(*settlement))
            }
            EventKind::RuinDiscovered {
                landmark, realm, ..
            } => format!(
                " after {} delved into {}",
                self.realm(*realm),
                self.landmark(*landmark)
            ),
            EventKind::NexusClaimed {
                landmark, realm, ..
            } => {
                format!(
                    " after {} took {}",
                    self.realm(*realm),
                    self.landmark(*landmark)
                )
            }
            EventKind::BlightSpread { .. } => " as the Blight spread".to_owned(),
            EventKind::Battle { name, .. } => format!(" after the {name}"),
            EventKind::WarDeclared { grievance, .. } => {
                format!(" in the war over {}", self.grievance(grievance))
            }
            EventKind::TradeSeason { .. } => " as goods flowed through it".to_owned(),
            EventKind::RealmSplit { name, .. } => format!(" after the {name} broke away"),
            _ => String::new(),
        }
    }

    /// Whether an event belongs in the story being told.
    fn concerns(&self, kind: &EventKind, focus: &Focus) -> bool {
        let owner = |s: u32| self.state.settlement_realm(s);
        let realms: Vec<u16> = {
            let mut r = kind.realms();
            r.extend(kind.settlements().into_iter().map(owner));
            if let Some(l) = kind.landmark() {
                if let Some(lm) = self.state.landmarks.get(l as usize) {
                    r.push(self.state.owner[lm.cell as usize]);
                }
            }
            match kind {
                EventKind::Battle { war, .. }
                | EventKind::Siege { war, .. }
                | EventKind::WarJoined { war, .. }
                | EventKind::PeaceSigned { war, .. } => {
                    if let Some(w) = self.state.wars.get(*war as usize) {
                        r.extend(w.attackers.iter().chain(&w.defenders));
                    }
                }
                _ => {}
            }
            r
        };
        match focus {
            Focus::World => true,
            Focus::Realm(id) => realms.contains(id),
            Focus::People(p) => {
                realms.iter().any(|&r| {
                    self.state
                        .realms
                        .get(r as usize)
                        .is_some_and(|x| x.people == *p)
                }) || kind.settlements().iter().any(|&s| {
                    self.state
                        .settlements
                        .get(s as usize)
                        .is_some_and(|x| x.people == *p)
                }) || matches!(kind, EventKind::SettlementFounded { people, .. } if people == p)
            }
            Focus::House(house) => {
                let ruled = realms.iter().any(|&r| {
                    self.state
                        .realms
                        .get(r as usize)
                        .and_then(|x| x.ruler.as_ref())
                        .is_some_and(|x| &x.house == house)
                });
                ruled
                    || match kind {
                        EventKind::RealmFounded { ruler, .. }
                        | EventKind::RulerSucceeded { ruler, .. }
                        | EventKind::RealmSplit { ruler, .. } => &ruler.house == house,
                        EventKind::MarriageArranged {
                            bride_house,
                            groom_house,
                            ..
                        } => bride_house == house || groom_house == house,
                        _ => false,
                    }
            }
        }
    }

    /// One sentence for one event, in one of its phrasings.
    fn sentence(&self, _index: usize, event: &Event, v: u32) -> String {
        let year = event.year;
        let s = match &event.kind {
            EventKind::RealmFounded { realm, ruler, .. } => pick(
                v,
                vec![
                    format!("{} stood under {}.", self.realm(*realm), Self::ruler(ruler)),
                    format!("{} ruled {}.", Self::ruler(ruler), self.realm(*realm)),
                ],
            ),
            EventKind::RulerSucceeded { realm, ruler, cause } => {
                let why = self.reason(*cause, 2);
                pick(
                    v,
                    vec![
                        format!("{} took the throne of {}{why}.", Self::ruler(ruler), self.realm(*realm)),
                        format!("In {}, {} was crowned{why}.", self.realm(*realm), Self::ruler(ruler)),
                        format!("The crown of {} passed to {}{why}.", self.realm(*realm), Self::ruler(ruler)),
                    ],
                )
            }
            EventKind::BorderChanged { from, to, cells, .. } => pick(
                v,
                vec![
                    format!("{} leagues passed from {} to {}.", cells.len(), self.realm(*from), self.realm(*to)),
                    format!("{} pushed its border into the lands of {}.", capitalise(&self.realm(*to)), self.realm(*from)),
                ],
            ),
            EventKind::SettlementFounded { name, people, .. } => pick(
                v,
                vec![
                    format!("Settlers of the {} founded {name}.", self.people(*people)),
                    format!("The village of {name} was founded by the {}.", self.people(*people)),
                    format!("{name} was raised on new land by {} hands.", self.people(*people)),
                ],
            ),
            EventKind::SettlementGrew {
                settlement,
                population,
                ..
            } => pick(
                v,
                vec![
                    format!("{} grew to {population} souls.", self.town(*settlement)),
                    format!("{} prospered and swelled to {population}.", self.town(*settlement)),
                ],
            ),
            EventKind::SettlementShrank {
                settlement,
                population,
                ..
            } => pick(
                v,
                vec![
                    format!("{} dwindled to {population} souls.", self.town(*settlement)),
                    format!("Hard years left {} with {population} people.", self.town(*settlement)),
                ],
            ),
            EventKind::SettlementRenamed { settlement, name, cause } => pick(
                v,
                vec![
                    format!("{} was renamed {name}{}.", self.town(*settlement), self.reason(*cause, 1)),
                    format!("Its new masters called {} {name}.", self.town(*settlement)),
                ],
            ),
            EventKind::RealmFell { realm, cause } => pick(
                v,
                vec![
                    format!("{} fell{}.", self.realm(*realm), self.reason(*cause, 1)),
                    format!("{} was no more{}.", self.realm(*realm), self.reason(*cause, 1)),
                ],
            ),
            EventKind::RealmSplit {
                realm, name, ruler, cause, ..
            } => pick(
                v,
                vec![
                    format!(
                        "The {name} broke away from {} under {}{}.",
                        self.realm(*realm),
                        Self::ruler(ruler),
                        self.reason(*cause, 1)
                    ),
                    format!(
                        "{} split apart{}, and {} proclaimed the {name}.",
                        self.realm(*realm),
                        self.reason(*cause, 1),
                        Self::ruler(ruler)
                    ),
                ],
            ),
            EventKind::TradeSeason { flows, .. } => {
                let goods: f32 = flows.iter().map(|f| f.shipped).sum();
                pick(
                    v,
                    vec![
                        format!("Caravans and ships carried {goods:.0} loads of goods between the towns."),
                        format!("Merchants moved {goods:.0} loads along the roads and sea lanes."),
                    ],
                )
            }
            EventKind::TradeRouteOpened { from, to, .. } => pick(
                v,
                vec![
                    format!("Goods began to flow from {} to {}.", self.town(*from), self.town(*to)),
                    format!("Merchants opened a trade road from {} to {}.", self.town(*from), self.town(*to)),
                ],
            ),
            EventKind::TradeRouteClosed { from, to, .. } => pick(
                v,
                vec![
                    format!("The trade from {} to {} ceased.", self.town(*from), self.town(*to)),
                    format!("No more goods came from {} to {}.", self.town(*from), self.town(*to)),
                ],
            ),
            EventKind::TradeHub { settlement, .. } => pick(
                v,
                vec![
                    format!("{} became a great market where many roads met.", self.town(*settlement)),
                    format!("So much passed through {} that it grew into a hub of trade.", self.town(*settlement)),
                ],
            ),
            EventKind::Shortage { settlement, resource, .. } => pick(
                v,
                vec![
                    format!("{} could not get enough {}.", self.town(*settlement), resource.name().to_lowercase()),
                    format!("{} went short of {}.", self.town(*settlement), resource.name().to_lowercase()),
                ],
            ),
            EventKind::OpinionsShifted { changes } => pick(
                v,
                vec![
                    format!("Envoys came and went; {} courts warmed or cooled to one another.", changes.len()),
                    "Old friendships cooled and new ones warmed between the courts.".to_owned(),
                ],
            ),
            EventKind::AllianceFormed { a, b, cause } => pick(
                v,
                vec![
                    format!("{} and {} swore alliance{}.", self.realm(*a), self.realm(*b), self.reason(*cause, 1)),
                    format!("{} bound itself to {} in alliance{}.", self.realm(*a), self.realm(*b), self.reason(*cause, 1)),
                ],
            ),
            EventKind::AllianceBroken { a, b, cause } => pick(
                v,
                vec![
                    format!("The alliance of {} and {} broke{}.", self.realm(*a), self.realm(*b), self.reason(*cause, 1)),
                    format!("{} and {} went their own ways{}.", self.realm(*a), self.realm(*b), self.reason(*cause, 1)),
                ],
            ),
            EventKind::RivalryDeclared { a, b, grievance } => pick(
                v,
                vec![
                    format!(
                        "Bad blood rose between {} and {} over {}.",
                        self.realm(*a),
                        self.realm(*b),
                        self.grievance(grievance)
                    ),
                    format!(
                        "{} and {} became rivals over {}.",
                        self.realm(*a),
                        self.realm(*b),
                        self.grievance(grievance)
                    ),
                ],
            ),
            EventKind::TradePactSigned { a, b, .. } => pick(
                v,
                vec![
                    format!("{} and {} sealed a pact of trade.", self.realm(*a), self.realm(*b)),
                    format!("Merchants of {} and {} were promised free passage.", self.realm(*a), self.realm(*b)),
                ],
            ),
            EventKind::MarriageArranged {
                a,
                b,
                bride,
                bride_house,
                groom,
                groom_house,
                ..
            } => pick(
                v,
                vec![
                    format!(
                        "{bride} of House {bride_house} was wed to {groom} of House {groom_house}, joining {} and {}.",
                        self.realm(*a),
                        self.realm(*b)
                    ),
                    format!(
                        "A royal wedding bound {} to {}: {bride} of House {bride_house} married {groom} of House {groom_house}.",
                        self.realm(*a),
                        self.realm(*b)
                    ),
                ],
            ),
            EventKind::VassalSworn { vassal, overlord, cause } => pick(
                v,
                vec![
                    format!("{} bent the knee to {}{}.", self.realm(*vassal), self.realm(*overlord), self.reason(*cause, 1)),
                    format!("{} became a vassal of {}{}.", self.realm(*vassal), self.realm(*overlord), self.reason(*cause, 1)),
                ],
            ),
            EventKind::VassalFreed { vassal, overlord, cause } => pick(
                v,
                vec![
                    format!("{} threw off the rule of {}{}.", self.realm(*vassal), self.realm(*overlord), self.reason(*cause, 1)),
                    format!("{} was freed from {}{}.", self.realm(*vassal), self.realm(*overlord), self.reason(*cause, 1)),
                ],
            ),
            EventKind::WarDeclared {
                attacker,
                defender,
                grievance,
                ..
            } => pick(
                v,
                vec![
                    format!(
                        "{} went to war with {} over {}.",
                        self.realm(*attacker),
                        self.realm(*defender),
                        self.grievance(grievance)
                    ),
                    format!(
                        "Over {}, {} declared war on {}.",
                        self.grievance(grievance),
                        self.realm(*attacker),
                        self.realm(*defender)
                    ),
                    format!(
                        "War came between {} and {}, over {}.",
                        self.realm(*attacker),
                        self.realm(*defender),
                        self.grievance(grievance)
                    ),
                ],
            ),
            EventKind::WarJoined {
                realm,
                on_attacking_side,
                war,
                ..
            } => {
                let side = self
                    .state
                    .wars
                    .get(*war as usize)
                    .map(|w| if *on_attacking_side { w.attacker } else { w.defender })
                    .unwrap_or(NO_REALM);
                pick(
                    v,
                    vec![
                        format!("{} took up arms beside {}.", self.realm(*realm), self.realm(side)),
                        format!("{} answered the call of its ally {}.", self.realm(*realm), self.realm(side)),
                    ],
                )
            }
            EventKind::Battle {
                war,
                name,
                attacker_won,
                attacker_losses,
                defender_losses,
                ..
            } => {
                let (winner, loser) = self
                    .state
                    .wars
                    .get(*war as usize)
                    .map(|w| if *attacker_won { (w.attacker, w.defender) } else { (w.defender, w.attacker) })
                    .unwrap_or((NO_REALM, NO_REALM));
                let dead = attacker_losses + defender_losses;
                pick(
                    v,
                    vec![
                        format!("At the {name}, {} defeated {}; {dead} fell.", self.realm(winner), self.realm(loser)),
                        format!("{} won the {name}, and {dead} men did not come home.", capitalise(&self.realm(winner))),
                        format!("The {name} went to {}.", self.realm(winner)),
                    ],
                )
            }
            EventKind::Siege { settlement, fell, .. } => {
                if *fell {
                    pick(
                        v,
                        vec![
                            format!("{} fell after a siege.", self.town(*settlement)),
                            format!("The walls of {} were breached.", self.town(*settlement)),
                        ],
                    )
                } else {
                    pick(
                        v,
                        vec![
                            format!("{} withstood a siege.", self.town(*settlement)),
                            format!("The besiegers of {} gave up and marched away.", self.town(*settlement)),
                        ],
                    )
                }
            }
            EventKind::PeaceSigned {
                attacker,
                defender,
                terms,
                ..
            } => {
                let loser = if terms.victor == *attacker { *defender } else { *attacker };
                // What the loser gave up, naming it once.
                let mut gave = Vec::new();
                if terms.tribute >= 1.0 {
                    gave.push(format!("paid {:.0} in tribute", terms.tribute));
                }
                if terms.vassalage {
                    gave.push(format!("swore fealty to {}", self.realm(terms.victor)));
                }
                let extra = if gave.is_empty() {
                    String::new()
                } else {
                    format!("; {} {}", self.realm(loser), gave.join(" and "))
                };
                pick(
                    v,
                    vec![
                        format!(
                            "Peace was made between {} and {}, to the advantage of {}{extra}.",
                            self.realm(*attacker),
                            self.realm(*defender),
                            self.realm(terms.victor)
                        ),
                        format!(
                            "The war between {} and {} ended with {} the victor{extra}.",
                            self.realm(*attacker),
                            self.realm(*defender),
                            self.realm(terms.victor)
                        ),
                    ],
                )
            }
            EventKind::RealmAnnexed { realm, by, .. } => pick(
                v,
                vec![
                    format!("{} was swallowed whole by {}.", self.realm(*realm), self.realm(*by)),
                    format!("{} annexed {}.", capitalise(&self.realm(*by)), self.realm(*realm)),
                ],
            ),
            EventKind::DragonAwakened { landmark, cause } => pick(
                v,
                vec![
                    format!("The dragon of {} woke{}.", self.landmark(*landmark), self.reason(*cause, 1)),
                    format!("Smoke rose from {}: its dragon was awake{}.", self.landmark(*landmark), self.reason(*cause, 1)),
                ],
            ),
            EventKind::DragonRaid {
                landmark,
                settlement,
                deaths,
            } => pick(
                v,
                vec![
                    format!("The dragon of {} burned {}; {deaths} died.", self.landmark(*landmark), self.town(*settlement)),
                    format!("Fire fell on {} from the dragon of {}, and {deaths} perished.", self.town(*settlement), self.landmark(*landmark)),
                ],
            ),
            EventKind::DragonSlept { landmark } => pick(
                v,
                vec![
                    format!("The dragon of {} returned to its hoard and slept.", self.landmark(*landmark)),
                    format!("The fires over {} went out; the dragon slept again.", self.landmark(*landmark)),
                ],
            ),
            EventKind::DragonSlain {
                landmark,
                realm,
                hero,
                cause,
                ..
            } => pick(
                v,
                vec![
                    format!(
                        "{hero} of {} slew the dragon of {}{} and carried off its hoard.",
                        self.realm(*realm),
                        self.landmark(*landmark),
                        self.reason(*cause, 1)
                    ),
                    format!(
                        "{}, {hero} of {} climbed to {} and killed its dragon.",
                        capitalise(self.reason(*cause, 1).trim_start_matches([' ', ','])),
                        self.realm(*realm),
                        self.landmark(*landmark)
                    ),
                ],
            ),
            EventKind::RuinDiscovered { landmark, realm, .. } => pick(
                v,
                vec![
                    format!("Delvers of {} found treasure in {}.", self.realm(*realm), self.landmark(*landmark)),
                    format!("{} broke into the vaults of {}.", capitalise(&self.realm(*realm)), self.landmark(*landmark)),
                ],
            ),
            EventKind::NexusClaimed { landmark, realm, .. } => pick(
                v,
                vec![
                    format!("{} took {} and the power that pools there.", capitalise(&self.realm(*realm)), self.landmark(*landmark)),
                    format!("The hedge-witches of {} came to {}.", self.realm(*realm), self.landmark(*landmark)),
                ],
            ),
            EventKind::BlightSpread { cells, cause } => {
                let first = matches!(cause, Cause::Delving)
                    || matches!(cause, Cause::Event(i) if !matches!(self.events.get(*i as usize).map(|e| &e.kind), Some(EventKind::BlightSpread { .. })));
                if first {
                    pick(
                        v,
                        vec![
                            format!("The Blight rose{} and took the land around it.", self.reason(*cause, 1)),
                            format!("A grey sickness crept out of the earth{}: the Blight had come.", self.reason(*cause, 1)),
                        ],
                    )
                } else {
                    format!("The Blight spread over {} more leagues.", cells.len())
                }
            }
            EventKind::BlightPurged { cells, realm, hero, .. } => pick(
                v,
                vec![
                    format!("{hero} of {} burned the Blight from {} leagues.", self.realm(*realm), cells.len()),
                    format!("Fire-bearers of {} under {hero} drove the Blight back.", self.realm(*realm)),
                ],
            ),
            EventKind::BlightReachedSettlement {
                settlement,
                deaths,
                abandoned,
                ..
            } => {
                if *abandoned {
                    format!(
                        "The Blight reached {}; {deaths} died and the rest fled, leaving it to ruin.",
                        self.town(*settlement)
                    )
                } else {
                    format!("The Blight reached {}, and {deaths} died.", self.town(*settlement))
                }
            }
            EventKind::VolcanoErupted { name, cell, .. } => pick(
                v,
                vec![
                    format!("{name} erupted above {}.", self.near(*cell)),
                    format!("The earth split and {name} poured out fire and ash near {}.", self.near(*cell)),
                ],
            ),
            EventKind::Earthquake { cell, magnitude, .. } => pick(
                v,
                vec![
                    format!("The ground shook near {}.", self.near(*cell)),
                    format!("A great earthquake{} struck near {}.", if *magnitude > 7.0 { ", the worst in memory," } else { "" }, self.near(*cell)),
                ],
            ),
            EventKind::HardTimes { settlements, cause, .. } => {
                let siege = matches!(cause, Cause::Event(i) if matches!(self.events.get(*i as usize).map(|e| &e.kind), Some(EventKind::Siege { .. })));
                if siege && settlements.len() == 1 {
                    return format!("The fields around {} lay trampled and burned.", self.town(settlements[0]));
                }
                let why = self.reason(*cause, 0);
                match settlements.len() {
                    1 => format!("Hard years came to {}{why}.", self.town(settlements[0])),
                    n => format!("Hard years came to {n} towns{why}."),
                }
            }
            EventKind::SettlementStruck {
                settlement,
                deaths,
                abandoned,
                cause,
                ..
            } => {
                let why = self.reason(*cause, 0);
                if *abandoned {
                    format!("{} was destroyed{why}; {deaths} died and it was abandoned.", self.town(*settlement))
                } else {
                    format!("{} was stricken{why}, and {deaths} died.", self.town(*settlement))
                }
            }
            EventKind::Drought { cell, .. } => pick(
                v,
                vec![
                    format!("The rains failed around {}.", self.near(*cell)),
                    format!("Drought parched the fields about {}.", self.near(*cell)),
                ],
            ),
            EventKind::Famine {
                settlement,
                deaths,
                cause,
            } => pick(
                v,
                vec![
                    format!("Famine came to {}{}; {deaths} starved.", self.town(*settlement), self.reason(*cause, 2)),
                    format!("{deaths} starved in {}{}.", self.town(*settlement), self.reason(*cause, 2)),
                ],
            ),
            EventKind::Migration {
                from,
                to,
                people,
                cause,
            } => pick(
                v,
                vec![
                    format!("{people} people walked from {} to {}{}.", self.town(*from), self.town(*to), self.reason(*cause, 0)),
                    format!("{} took in {people} refugees from {}.", self.town(*to), self.town(*from)),
                ],
            ),
            EventKind::Flood { name, .. } => pick(
                v,
                vec![
                    format!("The {name} burst its banks."),
                    format!("A great flood came down the {name}."),
                ],
            ),
            EventKind::RiverAvulsion { name, .. } => pick(
                v,
                vec![
                    format!("The {name} broke its levees and cut a new course to the sea."),
                    format!("When the waters fell, the {name} no longer ran in its old bed."),
                ],
            ),
            EventKind::SettlementStranded { settlement, .. } => pick(
                v,
                vec![
                    format!("{} was left on a dry riverbed.", self.town(*settlement)),
                    format!("The river abandoned {}, and its boats rotted on the mud.", self.town(*settlement)),
                ],
            ),
            EventKind::PlagueOutbreak { settlement, name, .. } => pick(
                v,
                vec![
                    format!("{} broke out in {}.", capitalise(name), self.town(*settlement)),
                    format!("Sailors brought {name} to {}.", self.town(*settlement)),
                ],
            ),
            EventKind::PlagueSpread { from, to, .. } => pick(
                v,
                vec![
                    format!("The plague travelled the trade road from {} to {}.", self.town(*from), self.town(*to)),
                    format!("Merchants from {} carried the plague to {}.", self.town(*from), self.town(*to)),
                ],
            ),
            EventKind::PlagueToll {
                settlement,
                deaths,
                infected,
            } => {
                if *infected == 0.0 {
                    format!("The plague passed from {}.", self.town(*settlement))
                } else {
                    format!("Plague killed {deaths} in {}.", self.town(*settlement))
                }
            }
        };
        let _ = year;
        s
    }
}

/// The appendix for writers.
fn appendix(world: &World, end: &HistoryState) -> Vec<Section> {
    let mut sections = Vec::new();

    // Gazetteer: every town that ever stood, then the landmarks.
    let founded: BTreeMap<u32, u32> = world
        .history
        .events
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::SettlementFounded { settlement, .. } => Some((settlement, e.year)),
            _ => None,
        })
        .collect();
    let mut towns: Vec<String> = end
        .settlements
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let people = world
                .peoples
                .peoples
                .get(s.people as usize)
                .map_or("", |p| p.name.as_str());
            let since = founded.get(&(i as u32)).map_or_else(
                || "since before the reckoning".to_owned(),
                |y| format!("founded in {y}"),
            );
            let state = if s.alive {
                format!("{} of {} people", s.rank.name(), s.population)
            } else {
                "abandoned".to_owned()
            };
            format!(
                "**{}** ({}): a {people} {state}, {since}.",
                s.name,
                syllables(&s.name)
            )
        })
        .collect();
    towns.sort();
    sections.push(Section {
        title: "Gazetteer: towns".to_owned(),
        entries: towns,
    });
    sections.push(Section {
        title: "Gazetteer: landmarks".to_owned(),
        entries: end
            .landmarks
            .iter()
            .map(|l| format!("**{}** — {}", capitalise(&l.name), l.hook))
            .collect(),
    });

    // Ruling houses, each with its line of rulers.
    let mut houses: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut names = world.history.initial.clone();
    for e in &world.history.events {
        let entry = match &e.kind {
            EventKind::RealmFounded { realm, ruler, .. }
            | EventKind::RulerSucceeded { realm, ruler, .. } => Some((
                ruler.house.clone(),
                format!(
                    "{} ({}, {})",
                    ruler.name,
                    e.year,
                    names
                        .realms
                        .get(*realm as usize)
                        .map_or("", |r| r.name.as_str())
                ),
            )),
            EventKind::RealmSplit { name, ruler, .. } => Some((
                ruler.house.clone(),
                format!("{} ({}, founder of {name})", ruler.name, e.year),
            )),
            _ => None,
        };
        if let Some((house, ruler)) = entry {
            houses.entry(house).or_default().push(ruler);
        }
        names.apply(e);
    }
    sections.push(Section {
        title: "Ruling houses".to_owned(),
        entries: houses
            .into_iter()
            .map(|(house, line)| format!("**House {house}**: {}", line.join(" → ")))
            .collect(),
    });

    // Names in each tongue, broken into syllables to say them by.
    sections.push(Section {
        title: "Names by people".to_owned(),
        entries: world
            .peoples
            .peoples
            .iter()
            .map(|p| {
                let places: Vec<String> = end
                    .settlements
                    .iter()
                    .filter(|s| s.people == p.id)
                    .take(6)
                    .map(|s| syllables(&s.name))
                    .collect();
                let persons: Vec<String> = (0..6)
                    .map(|k| syllables(&p.language.personal_name(7_000_000 + k)))
                    .collect();
                format!(
                    "**The {}** ({}): places {}; people {}.",
                    p.name,
                    syllables(&p.name),
                    places.join(", "),
                    persons.join(", ")
                )
            })
            .collect(),
    });
    sections
}

/// A name broken into syllables ("Vun·vrum·lum"): a break before a lone
/// consonant between vowels, and between two consonants between vowels.
/// Common digraphs stay together.
pub fn syllables(word: &str) -> String {
    const DIGRAPHS: [&str; 7] = ["th", "ch", "sh", "kh", "gh", "ph", "zh"];
    let chars: Vec<char> = word.chars().collect();
    let vowel = |c: char| "aeiouyAEIOUY".contains(c);
    // Group letters into units (digraphs count as one consonant).
    let mut units: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() {
            let pair: String = chars[i..i + 2].iter().collect::<String>().to_lowercase();
            if DIGRAPHS.contains(&pair.as_str()) {
                units.push(chars[i..i + 2].iter().collect());
                i += 2;
                continue;
            }
        }
        units.push(chars[i].to_string());
        i += 1;
    }
    let is_v = |u: &String| u.chars().count() == 1 && vowel(u.chars().next().unwrap_or('x'));
    // Between each pair of vowels: one consonant starts the next syllable
    // (V·CV); in a longer run the first consonant closes this one (VC·CCV).
    let vowels: Vec<usize> = (0..units.len()).filter(|&k| is_v(&units[k])).collect();
    let breaks: Vec<usize> = vowels
        .windows(2)
        .filter_map(|pair| match pair[1] - pair[0] - 1 {
            0 => None,
            1 => Some(pair[0] + 1),
            _ => Some(pair[0] + 2),
        })
        .collect();
    let mut out = String::new();
    for (k, unit) in units.iter().enumerate() {
        if breaks.contains(&k) {
            out.push('·');
        }
        out.push_str(unit);
    }
    out
}

impl Chronicle {
    pub fn to_markdown(&self) -> String {
        let mut out = format!("# {}\n\n", self.title);
        for chapter in &self.chapters {
            out.push_str(&format!(
                "## {}\n\n*{}*\n\n",
                chapter.title, chapter.summary
            ));
            for entry in &chapter.years {
                out.push_str(&format!("**{}.** {}\n\n", entry.year, entry.text));
            }
        }
        if !self.appendix.is_empty() {
            out.push_str("# For writers\n\n");
            for section in &self.appendix {
                out.push_str(&format!("## {}\n\n", section.title));
                for entry in &section.entries {
                    out.push_str(&format!("- {entry}\n"));
                }
                out.push('\n');
            }
        }
        out
    }

    /// A self-contained page set in IM Fell on parchment.
    pub fn to_html(&self) -> String {
        let esc = |t: &str| {
            t.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        };
        // Bold (**x**) in appendix entries.
        let rich = |t: &str| {
            let mut out = String::new();
            for (i, part) in esc(t).split("**").enumerate() {
                if i % 2 == 1 {
                    out.push_str(&format!("<b>{part}</b>"));
                } else {
                    out.push_str(part);
                }
            }
            out
        };
        let mut body = format!("<h1>{}</h1>\n", esc(&self.title));
        for chapter in &self.chapters {
            body.push_str(&format!(
                "<h2>{}</h2>\n<p class=\"summary\">{}</p>\n",
                esc(&chapter.title),
                esc(&chapter.summary)
            ));
            for entry in &chapter.years {
                body.push_str(&format!(
                    "<p><span class=\"year\">{}</span> {}</p>\n",
                    entry.year,
                    esc(&entry.text)
                ));
            }
        }
        if !self.appendix.is_empty() {
            body.push_str("<h1>For writers</h1>\n");
            for section in &self.appendix {
                body.push_str(&format!("<h2>{}</h2>\n<ul>\n", esc(&section.title)));
                for entry in &section.entries {
                    body.push_str(&format!("<li>{}</li>\n", rich(entry)));
                }
                body.push_str("</ul>\n");
            }
        }
        format!(
            r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link href="https://fonts.googleapis.com/css2?family=IM+Fell+English:ital@0;1&family=IM+Fell+English+SC&display=swap" rel="stylesheet">
<style>
  :root {{ --paper: #ece0bf; --ink: #3d2e1f; --red: #9e2919; --rule: #b59a6a; }}
  body {{ margin: 0; background: #d9c89e; color: var(--ink);
         font: 19px/1.55 "IM Fell English", Georgia, serif; }}
  main {{ max-width: 46rem; margin: 2rem auto; padding: 3rem 3.5rem;
         background: var(--paper); box-shadow: 0 0 0 1px var(--rule), 0 0 0 6px var(--paper),
         0 0 0 7px var(--rule), 0 10px 40px rgba(60,40,10,.35); }}
  h1 {{ font-family: "IM Fell English SC", serif; font-weight: normal; text-align: center;
        font-size: 2.2rem; letter-spacing: .04em; margin: .2em 0 .8em; }}
  h2 {{ font-family: "IM Fell English SC", serif; font-weight: normal; color: var(--red);
        text-align: center; margin: 2.2em 0 .4em; }}
  h2::before, h2::after {{ content: " ❦ "; color: var(--rule); }}
  .summary {{ font-style: italic; text-align: center; margin: 0 2rem 1.6em; }}
  p {{ margin: 0 0 .9em; text-align: justify; hyphens: auto; }}
  .year {{ font-family: "IM Fell English SC", serif; color: var(--red); margin-right: .3em; }}
  li {{ margin-bottom: .4em; }}
  @media (max-width: 40rem) {{ main {{ margin: 0; padding: 1.5rem 1rem; }} }}
</style>
</head>
<body><main>
{body}</main></body>
</html>
"#,
            title = esc(&self.title)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldConfig;

    fn small() -> World {
        let mut config = WorldConfig {
            seed: 42,
            width: 128,
            height: 128,
            ..Default::default()
        };
        config.erosion.droplets = 4000;
        config.history.years = 400;
        World::generate(&config)
    }

    #[test]
    fn same_world_same_chronicle() {
        let world = small();
        let options = ChronicleOptions {
            for_writers: true,
            ..ChronicleOptions::whole(&world)
        };
        assert_eq!(chronicle(&world, &options), chronicle(&world, &options));
    }

    /// The chronicle of a fixed small world, word for word. If generation
    /// changes on purpose, rerun with `UPDATE_SNAPSHOTS=1` and review the
    /// diff of `core/tests/snapshots/chronicle_small.md`.
    #[test]
    fn matches_snapshot() {
        let world = small();
        let text = chronicle(&world, &ChronicleOptions::whole(&world)).to_markdown();
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/snapshots/chronicle_small.md"
        );
        if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
            std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
            std::fs::write(path, &text).unwrap();
        }
        let expected =
            std::fs::read_to_string(path).expect("snapshot missing: run with UPDATE_SNAPSHOTS=1");
        assert_eq!(text.replace("\r\n", "\n"), expected.replace("\r\n", "\n"));
    }

    #[test]
    fn every_event_can_be_told_without_placeholders() {
        for seed in [1, 42] {
            let mut config = WorldConfig {
                seed,
                width: 128,
                height: 128,
                ..Default::default()
            };
            config.erosion.droplets = 4000;
            config.history.years = 600;
            config.magic.level = crate::config::MagicLevel::Saturated;
            config.disasters.plagues.per_century = 3.0;
            config.disasters.floods.per_century = 6.0;
            let world = World::generate(&config);
            let options = ChronicleOptions {
                everything: true,
                for_writers: true,
                ..ChronicleOptions::whole(&world)
            };
            let c = chronicle(&world, &options);
            let told: usize = c.chapters.iter().map(|ch| ch.years.len()).sum();
            assert!(told > 0);
            for text in [c.to_markdown(), c.to_html()] {
                for bad in ["{", "}", "  ", " .", " ,", "..", "NaN", "inf"] {
                    let body = text.split("<style>").next().unwrap_or(&text).to_owned()
                        + text.split("</style>").nth(1).unwrap_or("");
                    let body = if text.starts_with("<!doctype") {
                        body
                    } else {
                        text.clone()
                    };
                    assert!(!body.contains(bad), "{bad:?} in chronicle: …{}…", {
                        let at = body.find(bad).unwrap();
                        &body[at.saturating_sub(60)..(at + 60).min(body.len())]
                    });
                }
            }
        }
    }

    #[test]
    fn focus_narrows_the_story() {
        let world = small();
        let all = chronicle(&world, &ChronicleOptions::whole(&world)).to_markdown();
        let one = chronicle(
            &world,
            &ChronicleOptions {
                focus: Focus::Realm(0),
                ..ChronicleOptions::whole(&world)
            },
        )
        .to_markdown();
        assert!(one.len() < all.len());
        let range = chronicle(
            &world,
            &ChronicleOptions {
                from: 101,
                to: 200,
                ..ChronicleOptions::whole(&world)
            },
        );
        assert!(range
            .chapters
            .iter()
            .all(|c| c.years.iter().all(|y| (101..=200).contains(&y.year))));
    }

    #[test]
    fn syllables_split_between_vowels() {
        assert_eq!(syllables("Vunvrumlum"), "Vun·vrum·lum");
        assert_eq!(syllables("Kozdraund"), "Koz·draund");
        assert_eq!(syllables("Othae"), "O·thae");
    }
}
