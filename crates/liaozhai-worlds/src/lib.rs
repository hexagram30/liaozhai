//! World registry for Liaozhai MUX.
//!
//! [`registry::WorldRegistry`] holds a list of [`metadata::WorldMetadata`]
//! loaded from a TOML file at startup. v0.1 worlds are display-only
//! (slug, name, short description); per-world ECS state arrives in v0.2+.

pub mod metadata;
pub mod registry;
