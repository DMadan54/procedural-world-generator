//! Seeded randomness. This is the only source of randomness in the generator.

use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// The single seed every part of a world is derived from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorldSeed(pub u64);

/// A pipeline stage. Each stage gets its own independent RNG stream, so adding
/// a stage, or changing how many numbers one stage draws, never shifts the
/// output of the others.
///
/// Stream ids are part of the world format: never renumber an existing stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    Heightmap,
}

impl Stage {
    fn stream_id(self) -> u64 {
        match self {
            Stage::Heightmap => 1,
        }
    }
}

impl WorldSeed {
    /// The RNG for one pipeline stage of this world.
    pub fn stage_rng(self, stage: Stage) -> ChaCha8Rng {
        let mut rng = ChaCha8Rng::seed_from_u64(self.0);
        rng.set_stream(stage.stream_id());
        rng
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::RngCore;

    #[test]
    fn same_seed_same_stream() {
        let mut a = WorldSeed(7).stage_rng(Stage::Heightmap);
        let mut b = WorldSeed(7).stage_rng(Stage::Heightmap);
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_differ() {
        let mut a = WorldSeed(7).stage_rng(Stage::Heightmap);
        let mut b = WorldSeed(8).stage_rng(Stage::Heightmap);
        assert_ne!(a.next_u64(), b.next_u64());
    }
}
