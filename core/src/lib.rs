//! Deterministic procedural world generation.
//!
//! This crate holds all generation logic and has no UI or rendering
//! dependencies, so it builds for `wasm32-unknown-unknown` and can be unit
//! tested on its own.
//!
//! All randomness comes from a single [`WorldSeed`]. Each pipeline [`Stage`]
//! draws from its own ChaCha stream derived from that seed, so the same seed
//! and parameters always produce the same world.

pub mod heightmap;
pub mod rng;

pub use heightmap::{generate_heightmap, Heightmap, HeightmapParams};
pub use rng::{Stage, WorldSeed};
