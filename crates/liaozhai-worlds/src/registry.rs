//! In-memory world registry.
//!
//! M1: Placeholder. TOML-backed loading lands in M5.

use crate::metadata::WorldMetadata;

/// An in-memory registry of available worlds.
#[derive(Debug, Clone, Default)]
pub struct WorldRegistry {
    worlds: Vec<WorldMetadata>,
}

impl WorldRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.worlds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty()
    }

    pub fn worlds(&self) -> &[WorldMetadata] {
        &self.worlds
    }

    /// Create a registry populated with placeholder demo worlds.
    ///
    /// Returns three hardcoded worlds for use before M5's TOML loading.
    // TODO(M5): replace with TOML loading from worlds.toml
    pub fn placeholder() -> Self {
        Self {
            worlds: vec![
                WorldMetadata::new(
                    "studio-dusk",
                    "The Studio at Dusk",
                    "A small interior, warmly lit.",
                ),
                WorldMetadata::new(
                    "mountain-trail",
                    "The Mountain Trail",
                    "A path winding into mist.",
                ),
                WorldMetadata::new(
                    "library-echoes",
                    "The Library of Echoes",
                    "A reading room of recursive proportions.",
                ),
            ],
        }
    }

    /// Get a world by its 1-based display position.
    ///
    /// Returns `None` if the position is out of range.
    pub fn get_by_position(&self, one_based_index: usize) -> Option<&WorldMetadata> {
        if one_based_index == 0 {
            return None;
        }
        self.worlds.get(one_based_index - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry() {
        let reg = WorldRegistry::new();
        assert_eq!(reg.len(), 0);
        assert!(reg.is_empty());
        assert!(reg.worlds().is_empty());
    }

    #[test]
    fn placeholder_has_three_worlds() {
        let reg = WorldRegistry::placeholder();
        assert_eq!(reg.len(), 3);
        assert!(!reg.is_empty());
    }

    #[test]
    fn placeholder_world_names() {
        let reg = WorldRegistry::placeholder();
        let names: Vec<&str> = reg.worlds().iter().map(|w| w.name()).collect();
        assert_eq!(
            names,
            vec![
                "The Studio at Dusk",
                "The Mountain Trail",
                "The Library of Echoes"
            ]
        );
    }

    #[test]
    fn get_by_position_valid() {
        let reg = WorldRegistry::placeholder();
        assert_eq!(reg.get_by_position(1).unwrap().name(), "The Studio at Dusk");
        assert_eq!(
            reg.get_by_position(3).unwrap().name(),
            "The Library of Echoes"
        );
    }

    #[test]
    fn get_by_position_zero_returns_none() {
        let reg = WorldRegistry::placeholder();
        assert!(reg.get_by_position(0).is_none());
    }

    #[test]
    fn get_by_position_out_of_range_returns_none() {
        let reg = WorldRegistry::placeholder();
        assert!(reg.get_by_position(4).is_none());
    }
}
