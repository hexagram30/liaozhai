//! World registry — in-memory list of worlds loaded from TOML at startup.
//!
//! `WorldRegistry::load_from_toml` reads a `worlds.toml` file, validates it
//! (fail-fast on parse errors, missing/empty fields, duplicate slugs, or
//! empty registry), and produces a registry of [`WorldMetadata`] in
//! declaration order.

use std::collections::HashSet;
use std::path::Path;

use liaozhai_core::error::Error;
use serde::Deserialize;
use tracing::debug;

use crate::metadata::{WorldEntryDto, WorldMetadata};

/// An in-memory registry of available worlds.
#[derive(Debug, Clone, Default)]
pub struct WorldRegistry {
    worlds: Vec<WorldMetadata>,
}

impl WorldRegistry {
    /// Construct a registry from a list of worlds.
    pub fn new(worlds: Vec<WorldMetadata>) -> Self {
        Self { worlds }
    }

    /// Load and validate a world registry from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, the TOML is malformed,
    /// any entry has missing/empty fields, slugs are duplicated, or the
    /// registry is empty.
    pub fn load_from_toml(path: &Path) -> liaozhai_core::error::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::World(format!("reading {}: {e}", path.display())))?;
        let worlds = parse_worlds_toml(&content)
            .map_err(|e| Error::World(format!("parsing {}: {e}", path.display())))?;
        if worlds.is_empty() {
            return Err(Error::World(format!(
                "world registry is empty: {}",
                path.display()
            )));
        }
        for world in &worlds {
            debug!(slug = %world.slug(), name = %world.name(), "world loaded");
        }
        Ok(Self::new(worlds))
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

#[derive(Debug, Deserialize)]
struct WorldsFile {
    world: Vec<WorldEntryDto>,
}

fn parse_worlds_toml(content: &str) -> Result<Vec<WorldMetadata>, String> {
    let file: WorldsFile = toml::from_str(content).map_err(|e| format!("{e}"))?;

    let mut slugs = HashSet::new();
    let mut worlds = Vec::with_capacity(file.world.len());

    for (i, entry) in file.world.into_iter().enumerate() {
        let pos = i + 1;
        let slug = entry.slug.trim().to_owned();
        let name = entry.name.trim().to_owned();
        let short = entry.short.trim().to_owned();

        if slug.is_empty() {
            return Err(format!("entry {pos}: slug is empty"));
        }
        if name.is_empty() {
            return Err(format!("entry {pos}: name is empty"));
        }
        if short.is_empty() {
            return Err(format!("entry {pos}: short description is empty"));
        }
        if !slugs.insert(slug.clone()) {
            return Err(format!("entry {pos}: duplicate slug '{slug}'"));
        }
        worlds.push(WorldMetadata::new(slug, name, short));
    }

    Ok(worlds)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_TOML: &str = r#"
        [[world]]
        slug = "studio-dusk"
        name = "The Studio at Dusk"
        short = "A small interior, warmly lit."

        [[world]]
        slug = "mountain-trail"
        name = "The Mountain Trail"
        short = "A path winding into mist."

        [[world]]
        slug = "library-echoes"
        name = "The Library of Echoes"
        short = "A reading room of recursive proportions."
    "#;

    // --- parse_worlds_toml ---

    #[test]
    fn parses_three_worlds_from_valid_toml() {
        let worlds = parse_worlds_toml(VALID_TOML).unwrap();
        assert_eq!(worlds.len(), 3);
        assert_eq!(worlds[0].slug(), "studio-dusk");
        assert_eq!(worlds[1].slug(), "mountain-trail");
        assert_eq!(worlds[2].slug(), "library-echoes");
    }

    #[test]
    fn preserves_declaration_order() {
        let worlds = parse_worlds_toml(VALID_TOML).unwrap();
        let names: Vec<&str> = worlds.iter().map(|w| w.name()).collect();
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
    fn rejects_missing_slug() {
        let toml = r#"
            [[world]]
            name = "Test"
            short = "Desc"
        "#;
        assert!(parse_worlds_toml(toml).is_err());
    }

    #[test]
    fn rejects_missing_name() {
        let toml = r#"
            [[world]]
            slug = "test"
            short = "Desc"
        "#;
        assert!(parse_worlds_toml(toml).is_err());
    }

    #[test]
    fn rejects_missing_short() {
        let toml = r#"
            [[world]]
            slug = "test"
            name = "Test"
        "#;
        assert!(parse_worlds_toml(toml).is_err());
    }

    #[test]
    fn rejects_empty_slug() {
        let toml = r#"
            [[world]]
            slug = ""
            name = "Test"
            short = "Desc"
        "#;
        let err = parse_worlds_toml(toml).unwrap_err();
        assert!(err.contains("slug is empty"));
    }

    #[test]
    fn rejects_empty_name() {
        let toml = r#"
            [[world]]
            slug = "test"
            name = ""
            short = "Desc"
        "#;
        let err = parse_worlds_toml(toml).unwrap_err();
        assert!(err.contains("name is empty"));
    }

    #[test]
    fn rejects_empty_short() {
        let toml = r#"
            [[world]]
            slug = "test"
            name = "Test"
            short = ""
        "#;
        let err = parse_worlds_toml(toml).unwrap_err();
        assert!(err.contains("short description is empty"));
    }

    #[test]
    fn rejects_duplicate_slug() {
        let toml = r#"
            [[world]]
            slug = "same"
            name = "First"
            short = "Desc"

            [[world]]
            slug = "same"
            name = "Second"
            short = "Desc"
        "#;
        let err = parse_worlds_toml(toml).unwrap_err();
        assert!(err.contains("duplicate slug 'same'"));
    }

    // --- WorldRegistry::new ---

    #[test]
    fn new_with_worlds_preserves_order() {
        let worlds = vec![
            WorldMetadata::new("a", "Alpha", "First"),
            WorldMetadata::new("b", "Beta", "Second"),
        ];
        let reg = WorldRegistry::new(worlds);
        assert_eq!(reg.worlds()[0].slug(), "a");
        assert_eq!(reg.worlds()[1].slug(), "b");
    }

    #[test]
    fn len_and_is_empty_match() {
        let reg = WorldRegistry::new(vec![WorldMetadata::new("a", "Alpha", "First")]);
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());

        let empty = WorldRegistry::default();
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
    }

    #[test]
    fn get_by_position_valid() {
        let reg = WorldRegistry::new(vec![
            WorldMetadata::new("a", "Alpha", "First"),
            WorldMetadata::new("b", "Beta", "Second"),
        ]);
        assert_eq!(reg.get_by_position(1).unwrap().name(), "Alpha");
        assert_eq!(reg.get_by_position(2).unwrap().name(), "Beta");
        assert!(reg.get_by_position(0).is_none());
        assert!(reg.get_by_position(3).is_none());
    }

    // --- load_from_toml ---

    #[test]
    fn loads_valid_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("worlds.toml");
        std::fs::write(&path, VALID_TOML).unwrap();

        let reg = WorldRegistry::load_from_toml(&path).unwrap();
        assert_eq!(reg.len(), 3);
        assert_eq!(reg.worlds()[0].name(), "The Studio at Dusk");
    }

    #[test]
    fn errors_on_missing_file() {
        let path = std::path::Path::new("/nonexistent/worlds.toml");
        let err = WorldRegistry::load_from_toml(path).unwrap_err();
        assert!(err.to_string().contains("reading"));
    }

    #[test]
    fn errors_on_invalid_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("worlds.toml");
        std::fs::write(&path, "not valid toml {{{{").unwrap();

        let err = WorldRegistry::load_from_toml(&path).unwrap_err();
        assert!(err.to_string().contains("parsing"));
    }

    #[test]
    fn errors_on_empty_registry() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("worlds.toml");
        std::fs::write(&path, "world = []").unwrap();

        let err = WorldRegistry::load_from_toml(&path).unwrap_err();
        assert!(err.to_string().contains("empty"));
    }
}
