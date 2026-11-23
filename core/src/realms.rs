//! Stage: realms. The political map at year 0, before history begins.
//!
//! Every capital founds a realm. Realms claim land outward from their
//! capitals over the travel-cost grid (the cheapest claim wins), so borders
//! settle along mountains, deserts, broad rivers and straits; land beyond
//! every realm's reach stays wild. Each realm takes its government, names,
//! ruling house and heraldry from its people.

use rand_chacha::rand_core::RngCore;

use crate::biome::Biomes;
use crate::config::WorldConfig;
use crate::peoples::{PeopleTrait, Peoples};
use crate::rng::{entity_rng, stage};
use crate::settlements::{Rank, Settlements};
use crate::travel::{spread, UNOWNED};

/// Marks a cell or settlement outside every realm.
pub const NO_REALM: u16 = UNOWNED;

/// Size of the realm colour palette. Greedy colouring of a map's adjacency
/// graph rarely needs more than five or six.
pub const PALETTE_SIZE: u8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Government {
    Kingdom,
    Clanholds,
    Khaganate,
    Thalassocracy,
    Principality,
    Khanate,
    League,
}

impl Government {
    pub fn from_trait(trait_: PeopleTrait) -> Self {
        match trait_ {
            PeopleTrait::RiverFolk => Government::Kingdom,
            PeopleTrait::HillClans => Government::Clanholds,
            PeopleTrait::SteppeRiders => Government::Khaganate,
            PeopleTrait::SeaRaiders => Government::Thalassocracy,
            PeopleTrait::ForestDwellers => Government::Principality,
            PeopleTrait::DesertNomads => Government::Khanate,
            PeopleTrait::MarshFolk => Government::League,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Government::Kingdom => "Kingdom",
            Government::Clanholds => "Clanholds",
            Government::Khaganate => "Khaganate",
            Government::Thalassocracy => "Thalassocracy",
            Government::Principality => "Principality",
            Government::Khanate => "Khanate",
            Government::League => "League",
        }
    }
}

/// Heraldic colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tincture {
    Or,
    Argent,
    Gules,
    Azure,
    Vert,
    Sable,
    Purpure,
}

/// The device on a realm's arms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charge {
    Lion,
    Eagle,
    Tower,
    Sword,
    Star,
    Moon,
    Tree,
    Wave,
    Ship,
    Horse,
    Serpent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heraldry {
    pub field: Tincture,
    pub charge: Charge,
    pub charge_tincture: Tincture,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dynasty {
    /// The house's name, without "House".
    pub house: String,
    /// The founder's personal name.
    pub founder: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Realm {
    pub id: u16,
    /// Full name, e.g. "Khaganate of Gra".
    pub name: String,
    /// The bare place name, e.g. "Gra".
    pub short_name: String,
    pub government: Government,
    /// Settlement id of the capital.
    pub capital: u32,
    pub people: u16,
    pub dynasty: Dynasty,
    /// Index into the realm palette; neighbours never share one.
    pub colour: u8,
    pub heraldry: Heraldry,
    /// Territory size in cells.
    pub cells: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Realms {
    pub width: usize,
    pub height: usize,
    pub realms: Vec<Realm>,
    /// Per cell: the owning realm, or [`NO_REALM`] (water and wilderness).
    pub owner: Vec<u16>,
    /// Per realm: the realms sharing a land border with it, sorted.
    pub neighbours: Vec<Vec<u16>>,
    /// Per settlement: its realm, or [`NO_REALM`].
    pub settlement_realm: Vec<u16>,
}

impl Realms {
    pub fn realm_at(&self, x: usize, y: usize) -> Option<&Realm> {
        self.realms.get(self.owner[y * self.width + x] as usize)
    }
}

/// Runs the realms stage.
pub fn generate_realms(
    config: &WorldConfig,
    biomes: &Biomes,
    peoples: &Peoples,
    settlements: &Settlements,
    travel_cost: &[f32],
) -> Realms {
    let (w, h) = (biomes.width, biomes.height);
    let capitals: Vec<u32> = settlements
        .settlements
        .iter()
        .filter(|s| s.rank == Rank::Capital)
        .map(|s| s.id)
        .collect();
    let sources: Vec<(usize, u16)> = capitals
        .iter()
        .enumerate()
        .map(|(id, &s)| (settlements.settlements[s as usize].cell, id as u16))
        .collect();
    let reach = spread(travel_cost, w, h, &sources, config.realms.max_reach);
    let owner: Vec<u16> = (0..w * h)
        .map(|i| {
            if biomes.biome[i].is_water() {
                NO_REALM
            } else {
                reach.owner[i]
            }
        })
        .collect();

    let neighbours = adjacency(&owner, w, h, capitals.len());
    let colours = colour_graph(&neighbours);
    let mut cells = vec![0u32; capitals.len()];
    for &o in &owner {
        if o != NO_REALM {
            cells[o as usize] += 1;
        }
    }

    let realms = capitals
        .iter()
        .enumerate()
        .map(|(id, &capital)| {
            let id = id as u16;
            let settlement = &settlements.settlements[capital as usize];
            let people = &peoples.peoples[settlement.people as usize];
            let government = Government::from_trait(people.trait_);
            let language = &people.language;
            // Realm and house names come from parts of the language's name
            // space that settlements and rulers don't use.
            let short_name = language.place_name(100_000 + id as u64);
            let name = match id % 3 {
                0 => format!("{} of {}", government.name(), short_name),
                1 => format!("{} {}", short_name, government.name()),
                _ => format!("{} of {}", government.name(), settlement.name),
            };
            let short_name = if id % 3 == 2 {
                settlement.name.clone()
            } else {
                short_name
            };
            Realm {
                id,
                name,
                short_name,
                government,
                capital,
                people: people.id,
                dynasty: Dynasty {
                    house: language.place_name(200_000 + id as u64),
                    founder: language.personal_name(id as u64 * 64),
                },
                colour: colours[id as usize],
                heraldry: heraldry(config.seed, id, people.trait_),
                cells: cells[id as usize],
            }
        })
        .collect();

    let settlement_realm = settlements
        .settlements
        .iter()
        .map(|s| owner[s.cell])
        .collect();
    Realms {
        width: w,
        height: h,
        realms,
        owner,
        neighbours,
        settlement_realm,
    }
}

/// Realms that share a land border (4-neighbour), as sorted lists.
fn adjacency(owner: &[u16], w: usize, h: usize, count: usize) -> Vec<Vec<u16>> {
    let mut pairs = std::collections::BTreeSet::new();
    for y in 0..h {
        for x in 0..w {
            let a = owner[y * w + x];
            if a == NO_REALM {
                continue;
            }
            for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                if nx < w && ny < h {
                    let b = owner[ny * w + nx];
                    if b != NO_REALM && b != a {
                        pairs.insert((a.min(b), a.max(b)));
                    }
                }
            }
        }
    }
    let mut neighbours = vec![Vec::new(); count];
    for (a, b) in pairs {
        neighbours[a as usize].push(b);
        neighbours[b as usize].push(a);
    }
    for list in &mut neighbours {
        list.sort_unstable();
    }
    neighbours
}

/// Greedy graph colouring in id order: each realm takes the lowest palette
/// index none of its already-coloured neighbours use.
fn colour_graph(neighbours: &[Vec<u16>]) -> Vec<u8> {
    let mut colours: Vec<Option<u8>> = vec![None; neighbours.len()];
    for id in 0..neighbours.len() {
        let taken: Vec<u8> = neighbours[id]
            .iter()
            .filter_map(|&n| colours[n as usize])
            .collect();
        let free = (0..PALETTE_SIZE)
            .find(|c| !taken.contains(c))
            .unwrap_or(id as u8 % PALETTE_SIZE);
        colours[id] = Some(free);
    }
    colours.into_iter().map(|c| c.unwrap_or(0)).collect()
}

fn heraldry(seed: u64, id: u16, trait_: PeopleTrait) -> Heraldry {
    use Charge::*;
    use Tincture::*;
    let rng = &mut entity_rng(seed, stage::REALM_HERALDRY, id as u64);
    let favoured: &[Charge] = match trait_ {
        PeopleTrait::SeaRaiders => &[Ship, Wave, Serpent],
        PeopleTrait::SteppeRiders => &[Horse, Eagle, Moon],
        PeopleTrait::ForestDwellers => &[Tree, Star, Eagle],
        PeopleTrait::HillClans => &[Tower, Eagle, Sword],
        PeopleTrait::DesertNomads => &[Moon, Star, Serpent],
        PeopleTrait::MarshFolk => &[Serpent, Wave, Moon],
        PeopleTrait::RiverFolk => &[Lion, Tower, Wave],
    };
    let all = [
        Lion, Eagle, Tower, Sword, Star, Moon, Tree, Wave, Ship, Horse, Serpent,
    ];
    let charge = if rng.next_u32().is_multiple_of(3) {
        all[(rng.next_u32() as usize) % all.len()]
    } else {
        favoured[(rng.next_u32() as usize) % favoured.len()]
    };
    // Heraldry's rule of tincture: metal on colour or colour on metal.
    let metals = [Or, Argent];
    let colours = [Gules, Azure, Vert, Sable, Purpure];
    let metal = metals[(rng.next_u32() as usize) % metals.len()];
    let colour = colours[(rng.next_u32() as usize) % colours.len()];
    let (field, charge_tincture) = if rng.next_u32().is_multiple_of(2) {
        (metal, colour)
    } else {
        (colour, metal)
    };
    Heraldry {
        field,
        charge,
        charge_tincture,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    fn small(seed: u64) -> WorldConfig {
        let mut config = WorldConfig {
            seed,
            width: 128,
            height: 128,
            ..Default::default()
        };
        config.erosion.droplets = 4000;
        config
    }

    #[test]
    fn deterministic() {
        assert_eq!(
            World::generate(&small(5)).realms,
            World::generate(&small(5)).realms
        );
    }

    #[test]
    fn capitals_lie_in_their_own_realms() {
        let world = World::generate(&small(42));
        for realm in &world.realms.realms {
            let capital = &world.settlements.settlements[realm.capital as usize];
            assert_eq!(world.realms.owner[capital.cell], realm.id, "{}", realm.name);
            assert_eq!(
                world.realms.settlement_realm[realm.capital as usize],
                realm.id
            );
        }
    }

    #[test]
    fn each_settlement_has_at_most_one_realm_matching_the_map() {
        let world = World::generate(&small(42));
        assert_eq!(
            world.realms.settlement_realm.len(),
            world.settlements.settlements.len()
        );
        for (s, &r) in world
            .settlements
            .settlements
            .iter()
            .zip(&world.realms.settlement_realm)
        {
            assert_eq!(r, world.realms.owner[s.cell]);
        }
    }

    #[test]
    fn realm_count_is_near_the_target() {
        for seed in [1, 2, 3, 4] {
            for target in [6, 10, 16] {
                let mut config = small(seed);
                config.politics.target_realms = target;
                let n = World::generate(&config).realms.realms.len() as f32;
                let t = target as f32;
                assert!(
                    (n - t).abs() <= 0.3 * t,
                    "seed {seed}: {n} realms for target {t}"
                );
            }
        }
    }

    #[test]
    fn neighbours_never_share_a_colour_and_adjacency_is_symmetric() {
        for seed in [1, 42, 99] {
            let world = World::generate(&small(seed));
            let r = &world.realms;
            for (a, list) in r.neighbours.iter().enumerate() {
                assert!(list.windows(2).all(|p| p[0] < p[1]), "sorted");
                for &b in list {
                    assert!(r.neighbours[b as usize].contains(&(a as u16)));
                    assert_ne!(r.realms[a].colour, r.realms[b as usize].colour);
                }
            }
        }
    }

    #[test]
    fn water_is_never_owned() {
        let world = World::generate(&small(42));
        for (i, &o) in world.realms.owner.iter().enumerate() {
            if world.biomes.biome[i].is_water() {
                assert_eq!(o, NO_REALM);
            }
        }
    }

    #[test]
    fn realm_settings_leave_earlier_stages_alone() {
        let base = World::generate(&small(42));
        let mut config = small(42);
        config.realms.max_reach = 40.0;
        let changed = World::generate(&config);
        assert_eq!(base.settlements, changed.settlements);
        assert_eq!(base.peoples, changed.peoples);
        assert_ne!(base.realms, changed.realms);
    }

    #[test]
    fn names_are_filled_in() {
        let world = World::generate(&small(42));
        for realm in &world.realms.realms {
            assert!(realm.name.contains(realm.government.name()));
            assert!(!realm.dynasty.house.is_empty() && !realm.dynasty.founder.is_empty());
        }
    }
}
