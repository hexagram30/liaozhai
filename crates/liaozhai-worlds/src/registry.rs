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
}
