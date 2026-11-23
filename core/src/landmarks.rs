//! Stage: landmarks. Places with story hooks tied to the geography around
//! them: dragon dens on remote peaks, ruins of an elder age on good empty
//! land, leyline nexuses where the lines of power cross, cities drowned on
//! the continental shelf, and the origin of the Blight.
//!
//! Every landmark gets a name in the local people's language and a hook
//! built from what is really nearby (the closest town, the realm, the river,
//! the people, the deposits), so it can be lifted straight into a story.
//! `MagicLevel` scales the magical kinds; at `None` only mundane landmarks
//! appear.

use rand_chacha::rand_core::RngCore;

use crate::climate::elevation_m;
use crate::config::{MagicLevel, WorldConfig};
use crate::habitability::Habitability;
use crate::heightmap::Heightmap;
use crate::hydrology::Hydrology;
use crate::peoples::{Peoples, NO_PEOPLE};
use crate::realms::{Realms, NO_REALM};
use crate::resources::{Resource, Resources};
use crate::rng::{entity_rng, stage, stage_rng};
use crate::settlements::Settlements;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LandmarkKind {
    DragonDen,
    ElderRuin,
    LeylineNexus,
    DrownedCity,
}

impl LandmarkKind {
    pub fn is_magical(self) -> bool {
        matches!(self, LandmarkKind::DragonDen | LandmarkKind::LeylineNexus)
    }

    pub fn name(self) -> &'static str {
        match self {
            LandmarkKind::DragonDen => "dragon den",
            LandmarkKind::ElderRuin => "elder ruin",
            LandmarkKind::LeylineNexus => "leyline nexus",
            LandmarkKind::DrownedCity => "drowned city",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Landmark {
    pub id: u32,
    pub kind: LandmarkKind,
    pub cell: u32,
    pub name: String,
    /// One or two sentences naming real nearby places.
    pub hook: String,
}

/// A line of power across the map, drawn faintly on the antique map.
#[derive(Clone, Debug, PartialEq)]
pub struct Leyline {
    pub points: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Landmarks {
    pub width: usize,
    pub height: usize,
    pub landmarks: Vec<Landmark>,
    pub leylines: Vec<Leyline>,
    /// Per river (index into `Hydrology::rivers`): its name, in the language
    /// of the people at its mouth.
    pub river_names: Vec<String>,
    /// The landmark where the Blight begins, if magic allows one.
    pub blight_origin: Option<u32>,
}

/// Everything landmarks are placed and described from.
pub struct LandmarkInputs<'a> {
    pub config: &'a WorldConfig,
    pub terrain: &'a Heightmap,
    pub hydrology: &'a Hydrology,
    pub habitability: &'a Habitability,
    pub peoples: &'a Peoples,
    pub settlements: &'a Settlements,
    pub resources: &'a Resources,
    pub realms: &'a Realms,
}

/// A uniform draw in `0.0..1.0` from 24 random bits (exact in `f32`).
fn unit_f32(rng: &mut impl RngCore) -> f32 {
    (rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32
}

fn magic_scale(level: MagicLevel) -> f32 {
    match level {
        MagicLevel::None => 0.0,
        MagicLevel::Rare => 0.5,
        MagicLevel::Common => 1.0,
        MagicLevel::Saturated => 2.0,
    }
}

/// Runs the landmarks stage.
pub fn generate_landmarks(input: &LandmarkInputs) -> Landmarks {
    let (w, h) = (input.terrain.width, input.terrain.height);
    let p = &input.config.landmarks;
    let reference = w as f32 / 512.0;
    let area = reference * reference;
    let magic = magic_scale(input.config.magic.level);
    let rng = &mut stage_rng(input.config.seed, stage::LANDMARKS);
    if input.peoples.peoples.is_empty() || input.settlements.settlements.is_empty() {
        // Nobody to name anything or tell its story.
        return Landmarks {
            width: w,
            height: h,
            landmarks: Vec::new(),
            leylines: Vec::new(),
            river_names: Vec::new(),
            blight_origin: None,
        };
    }
    let ctx = Describer::new(input);
    let mut out = Landmarks {
        width: w,
        height: h,
        landmarks: Vec::new(),
        leylines: Vec::new(),
        river_names: ctx.river_names.clone(),
        blight_origin: None,
    };
    let sea = input.config.terrain.sea_level;

    // Dragon dens: high, remote peaks, better near gems and ore.
    let count = (p.dragon_dens as f32 * area * magic).round() as usize;
    let remote = 25.0 * reference;
    let dens = pick(w, h, count, 40.0 * reference, |i| {
        let height = input.terrain.data[i];
        let elevation = elevation_m(height, &input.config.terrain);
        if height < sea || elevation < 1500.0 || ctx.nearest_settlement(i).1 < remote {
            return 0.0;
        }
        let riches = ctx.deposits_near(i, 15.0 * reference, &[Resource::Gems, Resource::Ore]);
        elevation / 1000.0 + riches as f32
    });
    for cell in dens {
        ctx.add(&mut out, LandmarkKind::DragonDen, cell);
    }

    // Elder ruins: good land nobody has settled.
    let count = (p.elder_ruins as f32 * area).round() as usize;
    let empty = 20.0 * reference;
    let jitter: Vec<f32> = (0..w * h).map(|_| unit_f32(rng)).collect();
    let ruins = pick(w, h, count, 40.0 * reference, |i| {
        let hab = input.habitability.value[i];
        if input.terrain.data[i] < sea || hab < 0.5 || ctx.nearest_settlement(i).1 < empty {
            return 0.0;
        }
        hab + 0.3 * jitter[i]
    });
    for cell in ruins {
        ctx.add(&mut out, LandmarkKind::ElderRuin, cell);
    }

    // Leylines bent through shared focus points; three or more meet at each.
    let foci = (p.nexuses as f32 * area * magic).round() as usize;
    let mut focus_cells = Vec::new();
    for _ in 0..foci * 40 {
        if focus_cells.len() >= foci {
            break;
        }
        let cell = (rng.next_u64() % (w * h) as u64) as usize;
        let far = focus_cells.iter().all(|&f: &usize| {
            let (dx, dy) = (
                (f % w) as f32 - (cell % w) as f32,
                (f / w) as f32 - (cell / w) as f32,
            );
            dx * dx + dy * dy > (80.0 * reference).powi(2)
        });
        if input.terrain.data[cell] >= sea && far {
            focus_cells.push(cell);
        }
    }
    for (k, &focus) in focus_cells.iter().enumerate() {
        for line in 0..3 {
            let angle = (line as f32 + unit_f32(rng) * 0.6) * std::f32::consts::PI / 3.0;
            out.leylines
                .push(leyline(focus, angle, w, h, unit_f32(rng) - 0.5, k));
        }
        ctx.add(&mut out, LandmarkKind::LeylineNexus, focus);
    }

    // Drowned cities: just below the waves, close to the coast.
    let count = (p.drowned_cities as f32 * area).round() as usize;
    let drowned = pick(w, h, count, 50.0 * reference, |i| {
        let depth = sea - input.terrain.data[i];
        if depth <= 0.0 || depth > 0.02 || !ctx.land_within(i, 3) {
            return 0.0;
        }
        1.0 / (1.0 + ctx.nearest_settlement(i).1 / reference)
    });
    for cell in drowned {
        ctx.add(&mut out, LandmarkKind::DrownedCity, cell);
    }

    // The Blight starts at a nexus if there is one, else an elder ruin.
    if magic > 0.0 {
        let origin = out
            .landmarks
            .iter()
            .filter(|l| l.kind == LandmarkKind::LeylineNexus)
            .chain(
                out.landmarks
                    .iter()
                    .filter(|l| l.kind == LandmarkKind::ElderRuin),
            )
            .map(|l| l.id)
            .next();
        out.blight_origin = origin;
    }
    out
}

/// Greedy best-first picks with spacing. Ties go to the lower cell.
fn pick(
    w: usize,
    h: usize,
    count: usize,
    spacing: f32,
    score: impl Fn(usize) -> f32,
) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    let mut candidates: Vec<(f32, usize)> = (0..w * h)
        .map(|i| (score(i), i))
        .filter(|&(s, _)| s > 0.0)
        .collect();
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut chosen: Vec<usize> = Vec::new();
    for (_, cell) in candidates {
        if chosen.len() >= count {
            break;
        }
        let clear = chosen.iter().all(|&c| {
            let (dx, dy) = (
                (c % w) as f32 - (cell % w) as f32,
                (c / w) as f32 - (cell / w) as f32,
            );
            dx * dx + dy * dy >= spacing * spacing
        });
        if clear {
            chosen.push(cell);
        }
    }
    chosen
}

/// A gently bent line through `focus` at `angle`, clipped to the map.
fn leyline(focus: usize, angle: f32, w: usize, h: usize, bend: f32, _seed: usize) -> Leyline {
    let (fx, fy) = ((focus % w) as f32, (focus / w) as f32);
    let (dx, dy) = (angle.cos(), angle.sin());
    let reach = (w.max(h) as f32) * 1.5;
    let mut points = Vec::new();
    for k in 0..=96 {
        let t = (k as f32 / 96.0 - 0.5) * 2.0 * reach;
        let sway = bend * 0.15 * reach * (std::f32::consts::PI * t / reach).sin();
        let (x, y) = (fx + dx * t - dy * sway, fy + dy * t + dx * sway);
        if x >= 0.0 && y >= 0.0 && x < w as f32 && y < h as f32 {
            points.push([x, y]);
        }
    }
    Leyline { points }
}

/// Names landmarks and writes their hooks from what's around them.
struct Describer<'a> {
    input: &'a LandmarkInputs<'a>,
    river_names: Vec<String>,
    /// River cells: (cell, river index).
    river_cells: Vec<(usize, usize)>,
}

impl<'a> Describer<'a> {
    fn new(input: &'a LandmarkInputs<'a>) -> Self {
        let hy = input.hydrology;
        let w = input.terrain.width;
        let mut river_cells = Vec::new();
        let river_names = hy
            .rivers
            .iter()
            .map(|river| {
                let [mx, my] = river.mouth;
                let mouth =
                    (my as usize).min(input.terrain.height - 1) * w + (mx as usize).min(w - 1);
                let people = Self::people_near(input, mouth);
                input.peoples.peoples[people as usize]
                    .language
                    .place_name(4_000_000 + river.id as u64)
            })
            .collect();
        for (r, river) in hy.rivers.iter().enumerate() {
            for &s in &river.segments {
                for point in &hy.segments[s as usize].points {
                    river_cells.push((point[1] as usize * w + point[0] as usize, r));
                }
            }
        }
        Self {
            input,
            river_names,
            river_cells,
        }
    }

    fn people_near(input: &LandmarkInputs, cell: usize) -> u16 {
        let owner = input
            .peoples
            .homeland
            .get(cell)
            .copied()
            .unwrap_or(NO_PEOPLE);
        if owner != NO_PEOPLE {
            return owner;
        }
        let w = input.terrain.width;
        input
            .settlements
            .settlements
            .iter()
            .min_by(|a, b| {
                let d = |s: usize| {
                    let (dx, dy) = (
                        (s % w) as f32 - (cell % w) as f32,
                        (s / w) as f32 - (cell / w) as f32,
                    );
                    dx * dx + dy * dy
                };
                d(a.cell).total_cmp(&d(b.cell)).then(a.id.cmp(&b.id))
            })
            .map_or(0, |s| s.people)
    }

    fn dist(&self, a: usize, b: usize) -> f32 {
        let w = self.input.terrain.width;
        (((a % w) as f32 - (b % w) as f32).powi(2) + ((a / w) as f32 - (b / w) as f32).powi(2))
            .sqrt()
    }

    /// The nearest settlement and its distance.
    fn nearest_settlement(&self, cell: usize) -> (Option<u32>, f32) {
        self.input
            .settlements
            .settlements
            .iter()
            .map(|s| (Some(s.id), self.dist(s.cell, cell)))
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .unwrap_or((None, f32::INFINITY))
    }

    fn deposits_near(&self, cell: usize, radius: f32, kinds: &[Resource]) -> usize {
        self.input
            .resources
            .deposits
            .iter()
            .filter(|d| kinds.contains(&d.resource) && self.dist(d.cell as usize, cell) <= radius)
            .count()
    }

    fn land_within(&self, cell: usize, r: usize) -> bool {
        let (w, h) = (self.input.terrain.width, self.input.terrain.height);
        let sea = self.input.config.terrain.sea_level;
        let (x, y) = (cell % w, cell / w);
        (y.saturating_sub(r)..(y + r + 1).min(h)).any(|ny| {
            (x.saturating_sub(r)..(x + r + 1).min(w))
                .any(|nx| self.input.terrain.data[ny * w + nx] >= sea)
        })
    }

    fn nearest_river(&self, cell: usize, radius: f32) -> Option<&str> {
        self.river_cells
            .iter()
            .map(|&(c, r)| (self.dist(c, cell), r))
            .filter(|&(d, _)| d <= radius)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, r)| self.river_names[r].as_str())
    }

    fn add(&self, out: &mut Landmarks, kind: LandmarkKind, cell: usize) {
        let id = out.landmarks.len() as u32;
        let input = self.input;
        let reference = input.terrain.width as f32 / 512.0;
        let people_id = Self::people_near(input, cell);
        let people = &input.peoples.peoples[people_id as usize];
        let base = people.language.place_name(3_000_000 + id as u64);
        let (town, town_distance) = self.nearest_settlement(cell);
        let town = town.map_or_else(
            || "the coast".to_owned(),
            |t| input.settlements.settlements[t as usize].name.clone(),
        );
        let days = ((town_distance / reference / 6.0).round() as u32).max(1);
        let realm = {
            let owner = input.realms.owner[cell];
            let owner = if owner != NO_REALM {
                owner
            } else {
                // The realm of the nearest town.
                self.nearest_settlement(cell)
                    .0
                    .map_or(NO_REALM, |t| input.realms.settlement_realm[t as usize])
            };
            input
                .realms
                .realms
                .get(owner as usize)
                .map_or_else(|| "free lands".to_owned(), |r| r.name.clone())
        };
        let river = self.nearest_river(cell, 25.0 * reference);
        // Each landmark draws its own wording, so neighbours don't read alike.
        let mut rng = entity_rng(input.config.seed, stage::LANDMARKS, id as u64);
        let v = rng.next_u32() % 3;
        let folk = &people.name;
        let by_river = river.map_or_else(String::new, |r| format!(" by the River {r}"));
        let (name, hook) = match kind {
            LandmarkKind::DragonDen => {
                let name = match v {
                    0 => format!("{base} Crag"),
                    1 => format!("the Spire of {base}"),
                    _ => format!("the Burnt Peak of {base}"),
                };
                let gems = self.deposits_near(cell, 15.0 * reference, &[Resource::Gems]) > 0;
                let (lure, seam) = if gems {
                    ("gems", "gem")
                } else {
                    ("ore", "ore")
                };
                let hook = match v {
                    0 => format!("a high crag {days} days above {town} in the {realm}, where {folk} herders lose a flock every spring. {} glitter in the scree below its mouth.", capitalise(lure)),
                    1 => format!("the {folk} of {town} say a wyrm sleeps on a bed of {lure} beneath this peak, {days} days' climb from the nearest hearth in the {realm}."),
                    _ => format!("its slopes are scorched black above the {seam} seams; {folk} lords of the {realm} pay {town} to keep the beacon that warns when it stirs."),
                };
                (name, hook)
            }
            LandmarkKind::ElderRuin => {
                let name = match v {
                    0 => format!("the Ruins of {base}"),
                    1 => format!("the Broken Halls of {base}"),
                    _ => format!("the Sunken Court of {base}"),
                };
                let hook = match v {
                    0 => format!("halls of an elder people on good land{by_river} that the {folk} will not settle, {days} days' walk from {town}."),
                    1 => format!("toppled columns{by_river}, {days} days from {town}; {folk} shepherds swear the carvings change between visits."),
                    _ => format!("a ring of fallen walls in the {realm}{by_river}; whoever built it was gone before the {folk} came, and no one in {town} will speak its true name."),
                };
                (name, hook)
            }
            LandmarkKind::LeylineNexus => {
                let name = match v {
                    0 => format!("the {base} Stones"),
                    1 => format!("the Crossing of {base}"),
                    _ => format!("the {base} Ring"),
                };
                let hook = match v {
                    0 => format!("three leylines cross here on the marches of the {realm}; {folk} hedge-witches from {town} leave offerings at the standing stones."),
                    1 => format!("compasses spin and dreams run true where the lines meet, {days} days from {town}; {folk} seers and the lords of the {realm} have fought over it before."),
                    _ => format!("a ring of standing stones {days} days from {town} that hums at midsummer; {folk} elders bury their dead within sight of it."),
                };
                (name, hook)
            }
            LandmarkKind::DrownedCity => {
                let name = match v {
                    0 => format!("Drowned {base}"),
                    1 => format!("{base} Beneath the Waves"),
                    _ => format!("the Sea-Halls of {base}"),
                };
                let hook = match v {
                    0 => format!("towers beneath the shallows off {town}, drowned in the elder catastrophe; at low tide {folk} fishers hear bells."),
                    1 => format!("the nets of {town} come up with green bronze and carved stone; the {folk} say the sea took this city for its pride."),
                    _ => format!("on still days the roofs of a drowned city show beneath the water off {town}, and divers from the {realm} come back with gold, or not at all."),
                };
                (name, hook)
            }
        };
        let hook = format!("{}: {hook}", capitalise(&name));
        out.landmarks.push(Landmark {
            id,
            kind,
            cell: cell as u32,
            name,
            hook,
        });
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
    use super::*;
    use crate::world::World;

    fn small(seed: u64, magic: MagicLevel) -> WorldConfig {
        let mut config = WorldConfig {
            seed,
            width: 192,
            height: 192,
            ..Default::default()
        };
        config.erosion.droplets = 5000;
        config.history.years = 100;
        config.magic.level = magic;
        config
    }

    #[test]
    fn deterministic() {
        let c = small(5, MagicLevel::Common);
        assert_eq!(World::generate(&c).landmarks, World::generate(&c).landmarks);
    }

    #[test]
    fn no_magic_means_no_magical_landmarks() {
        let world = World::generate(&small(42, MagicLevel::None));
        assert!(world
            .landmarks
            .landmarks
            .iter()
            .all(|l| !l.kind.is_magical()));
        assert!(world.landmarks.leylines.is_empty());
        assert_eq!(world.landmarks.blight_origin, None);
    }

    #[test]
    fn dens_are_never_in_or_beside_settlements() {
        for seed in [1, 2, 42] {
            let world = World::generate(&small(seed, MagicLevel::Saturated));
            let w = world.landmarks.width;
            for den in world
                .landmarks
                .landmarks
                .iter()
                .filter(|l| l.kind == LandmarkKind::DragonDen)
            {
                for s in &world.settlements.settlements {
                    let (dx, dy) = (
                        (s.cell % w) as f32 - (den.cell as usize % w) as f32,
                        (s.cell / w) as f32 - (den.cell as usize / w) as f32,
                    );
                    assert!(
                        (dx * dx + dy * dy).sqrt() >= 25.0 * w as f32 / 512.0,
                        "{} is next to {}",
                        den.name,
                        s.name
                    );
                }
            }
        }
    }

    #[test]
    fn drowned_cities_are_underwater() {
        for seed in [1, 2, 42] {
            let world = World::generate(&small(seed, MagicLevel::None));
            let sea = world.config.terrain.sea_level;
            for city in world
                .landmarks
                .landmarks
                .iter()
                .filter(|l| l.kind == LandmarkKind::DrownedCity)
            {
                assert!(
                    world.terrain.data[city.cell as usize] < sea,
                    "{} is dry",
                    city.name
                );
            }
        }
    }

    #[test]
    fn hooks_name_real_places() {
        let world = World::generate(&small(42, MagicLevel::Saturated));
        assert!(!world.landmarks.landmarks.is_empty());
        let towns: Vec<&str> = world
            .settlements
            .settlements
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        let peoples: Vec<&str> = world
            .peoples
            .peoples
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        for l in &world.landmarks.landmarks {
            assert!(
                towns.iter().any(|t| l.hook.contains(t)),
                "{} names no town",
                l.hook
            );
            assert!(
                peoples.iter().any(|p| l.hook.contains(p)),
                "{} names no people",
                l.hook
            );
        }
    }

    #[test]
    fn landmark_settings_leave_earlier_stages_alone() {
        let base = World::generate(&small(42, MagicLevel::Common));
        let mut config = small(42, MagicLevel::Common);
        config.landmarks.elder_ruins = 20;
        let changed = World::generate(&config);
        assert_eq!(base.realms, changed.realms);
        assert_eq!(base.settlements, changed.settlements);
        assert_ne!(base.landmarks, changed.landmarks);
    }
}
