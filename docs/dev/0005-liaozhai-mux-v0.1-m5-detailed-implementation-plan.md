# Liaozhai MUX v0.1 — M5 Detailed Implementation Plan

## Context

M4 is complete with real SQLite auth. M5 replaces `WorldRegistry::placeholder()` with TOML loading from `cfg.worlds.registry_path`. Smallest milestone — one file format, one loader, validation, and test migration. No new crates, no protocol changes.

**Source documents:**
- `workbench/m5-implementation-plan.md` (design decisions)
- `docs/design/05-active/0011-v0.1-implementation-plan.md` (M5 acceptance criteria)

---

## Implementation Order

1. Add `toml` + `tracing` + `tempfile` deps to `liaozhai-worlds/Cargo.toml`
2. TOML deserialization DTO in `metadata.rs`
3. `WorldRegistry::new(Vec<WorldMetadata>)` constructor, `load_from_toml`, `parse_worlds_toml` + all validation + tests
4. Remove `placeholder()` and its tests
5. Migrate test helpers in `session.rs` and `connection.rs`
6. Update `listener.rs` to call `load_from_toml`
7. Create `data/worlds.toml` example file
8. Update module docs

---

## Step 1: Deps

### `crates/liaozhai-worlds/Cargo.toml` (MODIFY) — add:
```toml
toml.workspace = true
tracing.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`toml` for parsing, `tracing` for per-world debug logging in `load_from_toml`.

---

## Step 2: TOML Deserialization

### `crates/liaozhai-worlds/src/metadata.rs` (MODIFY)

The existing `WorldMetadata` already has `#[derive(Deserialize)]` but its fields don't match the TOML shape (`short_description` vs `short`). Add a private DTO:

```rust
/// Intermediate struct for TOML deserialization.
/// Decouples the file format from the runtime type.
#[derive(Debug, Deserialize)]
pub(crate) struct WorldEntryDto {
    pub slug: String,
    pub name: String,
    pub short: String,
}
```

The DTO lives in `metadata.rs` (co-located with the type it converts to). `registry.rs` imports it.

Conversion: `WorldMetadata::new(dto.slug, dto.name, dto.short)` — reuses existing constructor.

---

## Step 3: Registry Loader + Validation

### `crates/liaozhai-worlds/src/registry.rs` (MODIFY)

**Replace `WorldRegistry::new()` (parameterless)** with `new(worlds: Vec<WorldMetadata>)`:

```rust
pub fn new(worlds: Vec<WorldMetadata>) -> Self {
    Self { worlds }
}
```

Keep `Default` impl (returns empty). The plan says to keep it for `#[serde(default)]` patterns.

**Add `load_from_toml`:**

```rust
pub fn load_from_toml(path: &Path) -> liaozhai_core::error::Result<Self> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| Error::World(format!("reading {}: {e}", path.display())))?;
    let worlds = parse_worlds_toml(&content)
        .map_err(|e| Error::World(format!("parsing {}: {e}", path.display())))?;
    if worlds.is_empty() {
        return Err(Error::World(format!("world registry is empty: {}", path.display())));
    }
    for world in &worlds {
        debug!(slug = %world.slug(), name = %world.name(), "world loaded");
    }
    Ok(Self::new(worlds))
}
```

**Add `parse_worlds_toml` (module-private):**

```rust
#[derive(Debug, Deserialize)]
struct WorldsFile {
    world: Vec<WorldEntryDto>,
}

fn parse_worlds_toml(content: &str) -> Result<Vec<WorldMetadata>, String> {
    let file: WorldsFile = toml::from_str(content)
        .map_err(|e| format!("{e}"))?;

    let mut slugs = std::collections::HashSet::new();
    let mut worlds = Vec::with_capacity(file.world.len());

    for (i, entry) in file.world.into_iter().enumerate() {
        let pos = i + 1;
        // Trim whitespace before validation and storage. Operators may
        // accidentally introduce leading/trailing spaces; normalizing
        // prevents subtle duplicate-slug misses (e.g., "abc" vs "abc ").
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
```

**Why trim before storing.** Trimming once at the deserializer boundary normalizes operator input — `slug = "studio-dusk"` and `slug = " studio-dusk "` produce the same internal value. Without trimming, those would parse as different slugs and the duplicate-detection check would silently miss the case. The cost is negligible (three trims per world entry on startup); the benefit is robustness against accidental whitespace.

**Validation rules (all fail-fast):**
- TOML parse errors
- Missing/empty `slug`, `name`, `short` on any entry
- Duplicate `slug` (case-sensitive)
- File read errors
- Empty registry (checked in `load_from_toml` after parse)

**Unit tests for `parse_worlds_toml` (~9 tests):**
- `parses_three_worlds_from_valid_toml`
- `rejects_missing_slug` / `rejects_missing_name` / `rejects_missing_short`
- `rejects_empty_slug` / `rejects_empty_name` / `rejects_empty_short`
- `rejects_duplicate_slug`
- `preserves_declaration_order`

**Unit tests for `WorldRegistry::new` (~3 tests):**
- `new_with_worlds_preserves_order`
- `len_and_is_empty_match`
- `get_by_position_valid` (migrated from placeholder-based test)

**Integration tests for `load_from_toml` (~4 tests, using tempfile):**
- `loads_valid_file`
- `errors_on_missing_file`
- `errors_on_invalid_toml`
- `errors_on_empty_registry`

**Add one extra test in `liaozhai-net::connection::tests`:**

- `world_list_loaded_from_toml` — write a `tempfile::TempDir`-hosted `worlds.toml` with two custom-named worlds (e.g., "Test World A", "Test World B"), construct a `WorldRegistry` via `load_from_toml`, run a connection through to world selection, and assert the welcome screen contains the custom names. This closes the gap between parser-level unit tests and end-to-end visual behavior — verifies the TOML → registry → session → output pipeline.

Add the test to the existing `connection.rs` test module. The setup helper can grow a `setup_with_toml_worlds(toml_content: &str)` variant that the new test uses; the default `setup()` keeps using the inline `WorldRegistry::new(vec![...])` for speed.

---

## Step 4: Remove `placeholder()`

Delete `WorldRegistry::placeholder()` method and its tests (`placeholder_has_three_worlds`, `placeholder_world_names`). The `get_by_position` tests migrate to use `WorldRegistry::new(vec![...])`.

---

## Step 5: Migrate Test Helpers

### `crates/liaozhai-net/src/session.rs` tests (MODIFY)

Replace `test_registry()`:
```rust
fn test_registry() -> Arc<WorldRegistry> {
    Arc::new(WorldRegistry::new(vec![
        WorldMetadata::new("studio-dusk", "The Studio at Dusk", "A small interior, warmly lit."),
        WorldMetadata::new("mountain-trail", "The Mountain Trail", "A path winding into mist."),
        WorldMetadata::new("library-echoes", "The Library of Echoes", "A reading room of recursive proportions."),
    ]))
}
```

Replace the `format_world_list_matches_demo` test's `WorldRegistry::placeholder()` with `WorldRegistry::new(vec![...])` inline (or call `test_registry()`).

### `crates/liaozhai-net/src/connection.rs` tests (MODIFY)

In `setup()`, replace `Arc::new(WorldRegistry::placeholder())` with the same inline construction.

---

## Step 6: Update Listener

### `crates/liaozhai-server/src/listener.rs` (MODIFY)

Replace:
```rust
// TODO(M5): replace with TOML-loaded registry from cfg.worlds.registry_path.
let world_registry = Arc::new(WorldRegistry::placeholder());
```

With:
```rust
let world_registry = Arc::new(
    WorldRegistry::load_from_toml(Path::new(&cfg.worlds.registry_path))
        .context("loading world registry")?
);
```

---

## Step 7: Example worlds.toml

### `worlds.example.toml` (CREATE) — at the repo root

```toml
# Liaozhai MUX — example world registry.
# Copy to ./data/worlds.toml (or wherever cfg.worlds.registry_path points)
# and customize. Worlds are displayed to clients in declaration order.

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
```

**Why `worlds.example.toml` at the root, not `data/worlds.toml`.** The existing `.gitignore` excludes `data/` entirely (added in M1 to keep `accounts.db` out of the repo). Committing `data/worlds.toml` directly would be silently excluded from git. The cleaner pattern — and consistent with M4's `liaozhai.example.toml` — is an example file at the root that operators copy or symlink to their actual `cfg.worlds.registry_path` location.

**Operator workflow on a fresh clone:**

```bash
mkdir -p data
cp worlds.example.toml data/worlds.toml
# customize as needed
cargo run --bin liaozhai-server -- run
```

The README (or quick-start docs) should reference this step. Add a one-line mention if the README doesn't already cover it.

---

## Step 8: Update Module Docs

### `crates/liaozhai-worlds/src/registry.rs` (MODIFY) — replace the module-level doc:

Old (M1):
```rust
//! In-memory world registry.
//!
//! M1: Placeholder. TOML-backed loading lands in M5.
```

New:
```rust
//! World registry — in-memory list of worlds loaded from TOML at startup.
//!
//! `WorldRegistry::load_from_toml` reads a `worlds.toml` file, validates it
//! (fail-fast on parse errors, missing/empty fields, duplicate slugs, or
//! empty registry), and produces a registry of [`WorldMetadata`] in
//! declaration order.
```

### `crates/liaozhai-worlds/src/metadata.rs` (MODIFY) — replace the module-level doc:

Old (M1):
```rust
//! World metadata types.
//!
//! M1: Type definitions only. TOML deserialization lands in M5.
```

New:
```rust
//! World metadata types and TOML deserialization.
//!
//! [`WorldMetadata`] is the runtime type carried by [`super::registry::WorldRegistry`].
//! [`WorldEntryDto`] (private) is the deserialization shape for `worlds.toml`;
//! it decouples the file format from the runtime type so the public API of
//! `WorldMetadata` can evolve independently of the wire format.
```

---

## File Inventory

### CREATE (1 file)
| File | Purpose |
|------|---------|
| `worlds.example.toml` | Example world registry at repo root (template for operator-copied `data/worlds.toml`) |

### MODIFY (6 files)
| File | Change |
|------|--------|
| `crates/liaozhai-worlds/Cargo.toml` | Add toml, tracing, tempfile deps |
| `crates/liaozhai-worlds/src/metadata.rs` | Add `WorldEntryDto`; update module docs |
| `crates/liaozhai-worlds/src/registry.rs` | Replace `new()` + add `load_from_toml` + `parse_worlds_toml` + remove `placeholder()` + new tests; update module docs |
| `crates/liaozhai-net/src/session.rs` | Migrate `test_registry()` to use `new(vec![...])` |
| `crates/liaozhai-net/src/connection.rs` | Migrate `setup()` to use `new(vec![...])`; add `world_list_loaded_from_toml` integration test |
| `crates/liaozhai-server/src/listener.rs` | Replace `placeholder()` with `load_from_toml` |

---

## Test Coverage Target

- **`liaozhai-worlds::registry`**: 85%+. Pure logic — parse, validate, construct, load. Highly testable; the parser is unit-testable with synthetic TOML strings, the loader integration-testable with `tempfile`-backed paths.
- **`liaozhai-worlds::metadata`**: 80%+. The DTO is small; the conversion path is exercised through the parser tests.
- **`liaozhai-net::connection`**: maintained from M4 (65–75%). One new integration test exercises the TOML→connection pipeline.
- **`liaozhai-net::session`**: maintained from M3 (80–90%). M5 doesn't touch session logic; only the test fixture data construction changes.
- **Workspace overall (M5 cumulative)**: 80%+. ADR-0011's 80%-by-M6 target is reached this milestone or very close to it.

## Risks

**Risk: TOML parse errors can be terse.** A malformed file produces error messages like "expected newline at line 7 column 3" which doesn't always help operators find the issue. *Mitigation:* wrap parse errors with file path context via `Error::World(format!("parsing {}: {e}", path.display()))`. The `toml` crate's `serde::de::Error` already includes line/column, so the combined message is sufficient for v0.1.

**Risk: `WorldEntryDto` field changes break operator config files silently.** If a future milestone adds a required field (e.g., `tick_rate_ms`), existing `worlds.toml` files become invalid and the server refuses to start. *Mitigation:* new fields land as `Option<T>` in the DTO with sensible defaults applied during conversion to `WorldMetadata`. Don't make new fields required without a migration plan.

**Risk: test fixture migration touches multiple files.** The placeholder-removal forces edits to M3's session tests and M4's connection setup helper. *Mitigation:* the edits are mechanical (replace one constructor with another); no logic changes. Verify by running `cargo test --workspace` after the migration.

**Risk: trim-before-validate hides operator typos.** If an operator types `slug = " studio-dusk"` (leading space) and another `slug = "studio-dusk"`, trimming makes them duplicates and the loader rejects the file with "duplicate slug 'studio-dusk'". This is the desired behavior — it surfaces a real conflict. But a paranoid alternative would be: don't trim, error on whitespace-prefixed/suffixed slugs entirely. The plan goes with the trim-and-detect approach because the duplicate-detection rejection is informative and it tolerates accidental whitespace gracefully. Worth flagging in case operators want stricter behavior later.

## Definition of done for M5

- All acceptance criteria from `workbench/m5-implementation-plan.md` pass.
- `make check` (build + clippy + fmt + test) is green.
- The full v0.1 acceptance demo from ADR-0011 runs end-to-end with TOML-loaded worlds.
- `grep -rn "placeholder" crates/ --include="*.rs"` returns nothing — all references removed.
- A working `worlds.example.toml` is committed at the repo root.
- Code review surfaces no must-fix items remaining.
- Manual verification: edit `worlds.toml` to add a fourth world, restart the server, telnet in, see four worlds in the list.

## Verification

```bash
make check                          # build + clippy + fmt + test

# Verify placeholder is gone:
grep -rn "placeholder" crates/ --include="*.rs"   # should return nothing

# Set up worlds.toml on a fresh clone:
mkdir -p data
cp worlds.example.toml data/worlds.toml

# Run server:
cargo run --bin liaozhai-server -- run --port 4444
# telnet 127.0.0.1 4444 → login → see three worlds from data/worlds.toml

# Add a fourth world to data/worlds.toml, restart, verify it appears

# Error cases (expect clear startup error messages):
mv data/worlds.toml data/worlds.toml.bak    # missing file
cargo run --bin liaozhai-server -- run      # should fail with path in error
mv data/worlds.toml.bak data/worlds.toml

# Edit data/worlds.toml: add a duplicate slug → restart → fails with "duplicate slug 'X'"
# Edit data/worlds.toml: remove a required field → restart → fails with "entry N: ... is empty" (or TOML parse error)
# Edit data/worlds.toml: empty out all [[world]] entries → restart → fails with "world registry is empty"
```
