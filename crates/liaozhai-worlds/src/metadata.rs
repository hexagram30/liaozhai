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
    pub fn new(
        slug: impl Into<String>,
        name: impl Into<String>,
        short_description: impl Into<String>,
    ) -> Self {
        Self {
            id: WorldId::new(),
            slug: slug.into(),
            name: name.into(),
            short_description: short_description.into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_getters() {
        let meta = WorldMetadata::new(
            "studio-dusk",
            "The Studio at Dusk",
            "A small interior, warmly lit.",
        );
        assert_eq!(meta.slug(), "studio-dusk");
        assert_eq!(meta.name(), "The Studio at Dusk");
        assert_eq!(meta.short_description(), "A small interior, warmly lit.");
        assert_eq!(meta.id(), meta.id());
    }
}
