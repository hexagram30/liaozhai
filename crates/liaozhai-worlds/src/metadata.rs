//! World metadata types.
//!
//! M1: Type definitions only. TOML deserialization lands in M5.

use liaozhai_core::id::WorldId;
use serde::{Deserialize, Serialize};

/// Metadata for a single world in the registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldMetadata {
    id: WorldId,
    slug: String,
    name: String,
    short_description: String,
}

impl WorldMetadata {
    pub fn new(slug: String, name: String, short_description: String) -> Self {
        Self {
            id: WorldId::new(),
            slug,
            name,
            short_description,
        }
    }

    pub fn id(&self) -> WorldId {
        self.id
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn short_description(&self) -> &str {
        &self.short_description
    }
}
