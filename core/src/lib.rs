//! Deterministic procedural world generation.
//!
//! This crate holds all generation logic and has no UI or rendering
//! dependencies, so it builds for `wasm32-unknown-unknown` and can be unit
//! tested on its own.
//!
//! A [`WorldConfig`] holds every parameter, including the seed. Each stage
//! takes `&WorldConfig` and draws randomness only from [`rng::stage_rng`] or
//! [`rng::entity_rng`], which hash the seed with a stage name. So the same
//! config always produces the same world, and toggling a later stage never
//! changes an earlier one.
//!
//! Pipeline: tectonics and noise (independent) → blended heightmap.

pub mod config;
pub mod fractal;
pub mod heightmap;
pub mod rng;
pub mod tectonics;
pub mod world;

pub use config::{WorldConfig, GENERATOR_VERSION, MAX_PLATES, MIN_PLATES};
pub use heightmap::Heightmap;
pub use tectonics::{Boundary, BoundaryKind, Plate, PlateKind, TectonicMap, NO_BOUNDARY};
pub use world::{StagesRun, World};
