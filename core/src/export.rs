//! The world as one JSON document: config (with its share code), a summary
//! of the terrain, the peoples, settlements, realms, landmarks, resources
//! and roads, and the complete event log with the state it starts from.
//! Optional per-cell grids are stored compactly, as base64 of
//! little-endian bytes. The format is documented in
//! `docs/export-format.md`; bump [`SCHEMA_VERSION`] when it changes.
//!
//! The event log is enough to rebuild every year: [`WorldExport::history`]
//! replays it into a [`History`] identical to the one exported.

use serde::{Deserialize, Serialize};

use crate::config::WorldConfig;
use crate::history::{Event, History, HistoryState};
use crate::landmarks::LandmarkKind;
use crate::resources::Resource;
use crate::roads::LinkKind;
use crate::settlements::{Rank, SettlementFlags};
use crate::world::World;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldExport {
    pub schema_version: u32,
    pub generator_version: u32,
    /// Paste into the app's share box to regenerate this exact world.
    pub share_code: String,
    pub config: WorldConfig,
    pub terrain: TerrainSummary,
    /// Share of land in each biome, largest first.
    pub biomes: Vec<BiomeShare>,
    pub peoples: Vec<PeopleExport>,
    /// As founded at year 0; later towns appear in the event log.
    pub settlements: Vec<SettlementExport>,
    /// As at year 0.
    pub realms: Vec<RealmExport>,
    pub landmarks: Vec<LandmarkExport>,
    /// Per river (by id): its name.
    pub river_names: Vec<String>,
    pub deposits: Vec<DepositExport>,
    pub roads: Vec<RoadExport>,
    pub history: HistoryExport,
    /// Per-cell grids, if asked for.
    pub grids: Option<Grids>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainSummary {
    pub width: usize,
    pub height: usize,
    pub sea_level: f32,
    pub max_elevation_m: f32,
    pub land_fraction: f32,
    pub highest_cell: u32,
    pub highest_m: f32,
    pub lakes: usize,
    pub rivers: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiomeShare {
    pub biome: String,
    pub share: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PeopleExport {
    pub id: u16,
    pub name: String,
    pub trait_name: String,
    pub origin: u32,
    pub cells: u32,
    pub expansionism: f32,
    pub seafaring: f32,
    pub insularity: f32,
    /// `names::Language::generate(language_seed, language_id)` rebuilds the
    /// language.
    pub language_seed: u64,
    pub language_id: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettlementExport {
    pub id: u32,
    pub name: String,
    pub cell: u32,
    pub x: u32,
    pub y: u32,
    pub people: u16,
    pub rank: Rank,
    pub population: u32,
    pub port: bool,
    pub river_crossing: bool,
    pub mountain_pass: bool,
    pub oasis: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RealmExport {
    pub id: u16,
    pub name: String,
    pub short_name: String,
    pub government: String,
    pub capital: u32,
    pub people: u16,
    pub house: String,
    pub founder: String,
    pub arms: String,
    pub colour: u8,
    pub cells: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LandmarkExport {
    pub id: u32,
    pub kind: LandmarkKind,
    pub name: String,
    pub hook: String,
    pub cell: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DepositExport {
    pub cell: u32,
    pub resource: Resource,
    pub richness: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoadExport {
    pub a: u32,
    pub b: u32,
    /// "road", "river" or "sea".
    pub kind: String,
    pub cost: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryExport {
    pub years: u32,
    pub snapshot_interval: u32,
    /// The state before any event.
    pub initial: HistoryState,
    /// Every event, ordered by `(year, seq)`.
    pub events: Vec<Event>,
}

/// Per-cell grids, row by row from the top left, base64 of little-endian
/// values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grids {
    /// `f32` normalised height after erosion (sea level in `terrain`).
    pub height: String,
    /// `u8` biome, by index in the order of `biome_names`.
    pub biome: String,
    pub biome_names: Vec<String>,
    /// `u16` people whose homeland it is (65535 for none).
    pub homeland: String,
    /// `u16` realm owning it at year 0 (65535 for none).
    pub realm: String,
}

impl WorldExport {
    pub fn new(world: &World, include_grids: bool) -> Self {
        let config = &world.config;
        let (w, h) = (world.terrain.width, world.terrain.height);
        let sea = config.terrain.sea_level;
        let land = world.terrain.data.iter().filter(|&&v| v >= sea).count();
        let (highest_cell, highest) =
            world
                .terrain
                .data
                .iter()
                .enumerate()
                .fold(
                    (0, f32::MIN),
                    |best, (i, &v)| if v > best.1 { (i, v) } else { best },
                );
        let mut biomes: Vec<BiomeShare> = world
            .biomes
            .coverage()
            .into_iter()
            .map(|(b, share)| BiomeShare {
                biome: b.name().to_owned(),
                share,
            })
            .collect();
        biomes.sort_by(|a, b| b.share.total_cmp(&a.share).then(a.biome.cmp(&b.biome)));
        Self {
            schema_version: SCHEMA_VERSION,
            generator_version: config.generator_version,
            share_code: crate::share::encode(config),
            config: config.clone(),
            terrain: TerrainSummary {
                width: w,
                height: h,
                sea_level: sea,
                max_elevation_m: config.terrain.max_elevation_m,
                land_fraction: land as f32 / (w * h).max(1) as f32,
                highest_cell: highest_cell as u32,
                highest_m: crate::climate::elevation_m(highest, &config.terrain),
                lakes: world.hydrology.lakes.len(),
                rivers: world.hydrology.rivers.len(),
            },
            biomes,
            peoples: world
                .peoples
                .peoples
                .iter()
                .map(|p| PeopleExport {
                    id: p.id,
                    name: p.name.clone(),
                    trait_name: p.trait_.name().to_owned(),
                    origin: p.origin as u32,
                    cells: p.cells,
                    expansionism: p.expansionism,
                    seafaring: p.seafaring,
                    insularity: p.insularity,
                    language_seed: p.language.seed(),
                    language_id: p.language.id,
                })
                .collect(),
            settlements: world
                .settlements
                .settlements
                .iter()
                .map(|s| {
                    let SettlementFlags {
                        port,
                        river_crossing,
                        mountain_pass,
                        oasis,
                    } = s.flags;
                    SettlementExport {
                        id: s.id,
                        name: s.name.clone(),
                        cell: s.cell as u32,
                        x: (s.cell % w) as u32,
                        y: (s.cell / w) as u32,
                        people: s.people,
                        rank: s.rank,
                        population: s.population,
                        port,
                        river_crossing,
                        mountain_pass,
                        oasis,
                    }
                })
                .collect(),
            realms: world
                .realms
                .realms
                .iter()
                .map(|r| RealmExport {
                    id: r.id,
                    name: r.name.clone(),
                    short_name: r.short_name.clone(),
                    government: r.government.name().to_owned(),
                    capital: r.capital,
                    people: r.people,
                    house: r.dynasty.house.clone(),
                    founder: r.dynasty.founder.clone(),
                    arms: format!(
                        "{:?}, a {:?} {:?}",
                        r.heraldry.field, r.heraldry.charge, r.heraldry.charge_tincture
                    ),
                    colour: r.colour,
                    cells: r.cells,
                })
                .collect(),
            landmarks: world
                .landmarks
                .landmarks
                .iter()
                .map(|l| LandmarkExport {
                    id: l.id,
                    kind: l.kind,
                    name: l.name.clone(),
                    hook: l.hook.clone(),
                    cell: l.cell,
                })
                .collect(),
            river_names: world.landmarks.river_names.clone(),
            deposits: world
                .resources
                .deposits
                .iter()
                .map(|d| DepositExport {
                    cell: d.cell,
                    resource: d.resource,
                    richness: d.richness,
                })
                .collect(),
            roads: world
                .roads
                .links
                .iter()
                .map(|l| RoadExport {
                    a: l.a,
                    b: l.b,
                    kind: match l.kind {
                        LinkKind::Road => "road",
                        LinkKind::River => "river",
                        LinkKind::Sea => "sea",
                    }
                    .to_owned(),
                    cost: l.cost,
                })
                .collect(),
            history: HistoryExport {
                years: world.history.years,
                snapshot_interval: world.history.snapshot_interval(),
                initial: world.history.initial.clone(),
                events: world.history.events.clone(),
            },
            grids: include_grids.then(|| Grids {
                height: base64::encode(
                    &world
                        .terrain
                        .data
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<u8>>(),
                ),
                biome: base64::encode(
                    &world
                        .biomes
                        .biome
                        .iter()
                        .map(|b| b.index() as u8)
                        .collect::<Vec<u8>>(),
                ),
                biome_names: crate::biome::Biome::ALL
                    .iter()
                    .map(|b| b.name().to_owned())
                    .collect(),
                homeland: base64::encode(&u16_bytes(&world.peoples.homeland)),
                realm: base64::encode(&u16_bytes(&world.realms.owner)),
            }),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("export types always serialise")
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The history, rebuilt from the event log.
    pub fn history(&self) -> History {
        History::from_log(
            self.history.initial.clone(),
            self.history.events.clone(),
            self.history.years,
            self.history.snapshot_interval,
        )
    }
}

fn u16_bytes(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Standard base64 (RFC 4648, with padding), small enough to keep here
/// rather than take a dependency.
pub mod base64 {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    pub fn decode(text: &str) -> Option<Vec<u8>> {
        let value = |c: u8| ALPHABET.iter().position(|&a| a == c).map(|p| p as u32);
        let text = text.trim_end_matches('=').as_bytes();
        let mut out = Vec::with_capacity(text.len() * 3 / 4);
        for chunk in text.chunks(4) {
            let mut n = 0u32;
            for (i, &c) in chunk.iter().enumerate() {
                n |= value(c)? << (18 - 6 * i);
            }
            for i in 0..chunk.len().saturating_sub(1) {
                out.push((n >> (16 - 8 * i)) as u8);
            }
        }
        Some(out)
    }

    /// `#[serde(with = "...")]` helpers for per-cell vectors.
    pub mod u16s {
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(values: &[u16], s: S) -> Result<S::Ok, S::Error> {
            s.serialize_str(&super::encode(&super::super::u16_bytes(values)))
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u16>, D::Error> {
            let text = String::deserialize(d)?;
            let bytes =
                super::decode(&text).ok_or_else(|| serde::de::Error::custom("bad base64"))?;
            Ok(bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&b| u16::from_le_bytes(b))
                .collect())
        }
    }

    pub mod bools {
        use serde::{Deserialize, Deserializer, Serializer};

        /// One bit per value, lowest bit first, prefixed by the count.
        pub fn serialize<S: Serializer>(values: &[bool], s: S) -> Result<S::Ok, S::Error> {
            let mut bytes = (values.len() as u32).to_le_bytes().to_vec();
            bytes.extend(values.chunks(8).map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0u8, |byte, (i, &b)| byte | (u8::from(b) << i))
            }));
            s.serialize_str(&super::encode(&bytes))
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<bool>, D::Error> {
            let text = String::deserialize(d)?;
            let bytes =
                super::decode(&text).ok_or_else(|| serde::de::Error::custom("bad base64"))?;
            if bytes.len() < 4 {
                return Err(serde::de::Error::custom("bit grid too short"));
            }
            let n = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
            Ok((0..n)
                .map(|i| bytes.get(4 + i / 8).is_some_and(|b| b >> (i % 8) & 1 == 1))
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> WorldConfig {
        let mut config = WorldConfig {
            seed: 42,
            width: 128,
            height: 128,
            ..Default::default()
        };
        config.erosion.droplets = 4000;
        config.history.years = 600;
        config
    }

    #[test]
    fn base64_round_trips() {
        for len in 0..20 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(base64::decode(&base64::encode(&bytes)).unwrap(), bytes);
        }
        assert_eq!(base64::encode(b"Man"), "TWFu");
        assert_eq!(base64::encode(b"Ma"), "TWE=");
    }

    #[test]
    fn exported_event_log_reproduces_every_year() {
        let world = World::generate(&small());
        let json = WorldExport::new(&world, true).to_json();
        let back = WorldExport::from_json(&json).expect("parses");
        let history = back.history();
        for year in [0, 1, 37, 150, 299, 300, 451, 600] {
            assert_eq!(
                history.state_at(year),
                world.history.state_at(year),
                "year {year}"
            );
        }
        assert_eq!(history, world.history);
        assert_eq!(back.config, world.config);
        assert_eq!(
            crate::share::decode(&back.share_code).unwrap(),
            world.config
        );
    }

    #[test]
    fn grids_decode_to_the_world() {
        let world = World::generate(&small());
        let export = WorldExport::new(&world, true);
        let grids = export.grids.unwrap();
        let heights: Vec<f32> = base64::decode(&grids.height)
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&b| f32::from_le_bytes(b))
            .collect();
        assert_eq!(heights, world.terrain.data);
        assert!(WorldExport::new(&world, false).grids.is_none());
    }
}
