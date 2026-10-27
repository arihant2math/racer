//! Procedural track-side asset kit for the Downtown district: road sweeps
//! with markings and decals, racing furniture, street furniture, a modular
//! façade kit with a rule-based block assembler, procedural PBR textures,
//! validation, and the Bevy side (materials, spawning, lighting presets,
//! the showcase street).
//!
//! Generators are pure functions of their inputs and a seed: the same
//! inputs produce bit-identical meshes and textures.

pub mod building;
pub mod district;
pub mod kit;
pub mod mesh;
pub mod noise;
pub mod path;
pub mod props;
pub mod road;
pub mod texture;
pub mod validate;
pub mod batch;
pub mod lighting;
pub mod materials;
pub mod showcase;
pub mod catalogue;
pub mod export;
