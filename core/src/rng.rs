//! Seeded randomness. This is the only source of randomness in the generator.
//!
//! Every RNG is derived from the world seed plus a stage name (and, for
//! [`entity_rng`], an index) by hashing. Streams don't depend on each other or
//! on the order in which stages run, so adding or toggling a later feature can
//! never change the output of an earlier one.

use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Stage names used to derive RNGs.
///
/// These are part of the world format: renaming one changes that stage's
/// output for every seed. Add new names; never edit existing ones.
pub mod stage {
    pub const NOISE: &str = "noise";
    pub const TECTONIC_PLATES: &str = "tectonics.plates";
    pub const TECTONIC_DRIFT: &str = "tectonics.drift";
    pub const TECTONIC_WARP: &str = "tectonics.warp";
    pub const TECTONIC_CLUSTER: &str = "tectonics.cluster";
    pub const CLIMATE: &str = "climate";
    pub const EROSION: &str = "erosion";
    pub const BIOMES: &str = "biomes";
    pub const LANGUAGE: &str = "language";
    pub const PEOPLES: &str = "peoples";
    pub const PEOPLE_TRAITS: &str = "peoples.traits";
    pub const NAMES_PEOPLE: &str = "names.people";
    pub const NAMES_PLACE: &str = "names.place";
    pub const NAMES_PERSON: &str = "names.person";
}

const DOMAIN_STAGE: u64 = 0x5354_4147_4521_0001;
const DOMAIN_ENTITY: u64 = 0x454E_5449_5459_0002;
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// An independent RNG for one stage, derived from `seed` and the stage name.
pub fn stage_rng(seed: u64, stage: &str) -> ChaCha8Rng {
    ChaCha8Rng::from_seed(derive_key(&[DOMAIN_STAGE, seed, fnv1a(stage)]))
}

/// An independent RNG for entity `index` within a stage (one plate, one
/// river, one dragon...). An entity's randomness doesn't depend on how many
/// other entities there are or what order they're processed in.
pub fn entity_rng(seed: u64, stage: &str, index: u64) -> ChaCha8Rng {
    ChaCha8Rng::from_seed(derive_key(&[DOMAIN_ENTITY, seed, fnv1a(stage), index]))
}

/// FNV-1a: small, fixed, and identical on every platform and Rust version
/// (unlike `std`'s `DefaultHasher`, whose algorithm is unspecified).
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// SplitMix64 finaliser: a strong 64-bit bit mixer.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Absorbs the parts into a 32-byte ChaCha key.
fn derive_key(parts: &[u64]) -> [u8; 32] {
    let state = parts.iter().fold(
        GOLDEN,
        |state, &part| mix(state.wrapping_add(GOLDEN) ^ part),
    );
    let mut key = [0u8; 32];
    for (i, chunk) in key.as_chunks_mut::<8>().0.iter_mut().enumerate() {
        let word = mix(state.wrapping_add((i as u64 + 1).wrapping_mul(GOLDEN)));
        *chunk = word.to_le_bytes();
    }
    key
}

/// A uniform value in `0.0..1.0`.
pub(crate) fn unit_f64(rng: &mut impl RngCore) -> f64 {
    // Top 53 bits, the precision of an f64 mantissa.
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// A uniform value in `0.0..1.0`.
pub(crate) fn unit_f32(rng: &mut impl RngCore) -> f32 {
    // Top 24 bits, the precision of an f32 mantissa.
    (rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(mut rng: ChaCha8Rng) -> u64 {
        rng.next_u64()
    }

    #[test]
    fn same_inputs_same_stream() {
        let mut a = stage_rng(7, "noise");
        let mut b = stage_rng(7, "noise");
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn seeds_stages_and_entities_are_independent() {
        let base = first(stage_rng(7, "noise"));
        assert_ne!(base, first(stage_rng(8, "noise")));
        assert_ne!(base, first(stage_rng(7, "tectonics.plates")));
        assert_ne!(base, first(entity_rng(7, "noise", 0)));
        assert_ne!(
            first(entity_rng(7, "noise", 0)),
            first(entity_rng(7, "noise", 1))
        );
        assert_ne!(first(entity_rng(7, "a", 0)), first(entity_rng(7, "b", 0)));
    }

    /// Pins the derivation. If this fails, every existing seed now produces a
    /// different world: only change it together with `GENERATOR_VERSION`.
    #[test]
    fn derivation_is_stable() {
        assert_eq!(fnv1a(""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(fnv1a("a"), 0xAF63_DC4C_8601_EC8C);
        assert_eq!(first(stage_rng(42, stage::NOISE)), STAGE_42_NOISE);
        assert_eq!(
            first(entity_rng(42, stage::TECTONIC_DRIFT, 3)),
            ENTITY_42_DRIFT_3
        );
    }
    const STAGE_42_NOISE: u64 = 8582476738564353936;
    const ENTITY_42_DRIFT_3: u64 = 10992505513941621850;

    #[test]
    fn unit_values_are_in_range() {
        let mut rng = stage_rng(3, "test");
        for _ in 0..1000 {
            assert!((0.0..1.0).contains(&unit_f64(&mut rng)));
            assert!((0.0..1.0).contains(&unit_f32(&mut rng)));
        }
    }
}
