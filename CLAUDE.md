# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

See ./README.md

## Build Commands

`make help` provides the full list of commands, but here's a summary:

```bash
make build            # Debug build
make test             # Run all tests
make lint             # Clippy + format check
make format           # Auto-format with rustfmt
make check            # Build + lint + test
make check-all        # Build + lint + coverage
make coverage         # Text coverage report (cargo llvm-cov)
make docs             # Generate API docs -> target/doc/minilogue-xd/index.html
```

Running a single test:

```bash
cargo test test_name
cargo test module::tests::specific_test
```

## Architecture

See `./docs/design/01-draft/0005-oxurack-system-architecture.md`

## Writing Code

### Sound Design

**`assets/ai/SOUND_DESIGN.md`** -- Minilogue XD sound design skill: patch architecture, parameter relationships, classic sound recipes (pads, bass, leads, Berlin School, ambient, acid), sequencing patterns, genre quick reference, and idiomatic library API usage. **Read this when designing sounds, creating patches, or brainstorming electronica.**

### Rust Quality Guidelines

1. **`assets/ai/ai-rust/skills/claude/SKILL.md`** -- advanced Rust programming skill (**use this**)
2. **`assets/ai/ai-rust/guides/*.md`** -- comprehensive Rust guidelines referenced by the skill
3. **`assets/ai/CLAUDE-CODE-COVERAGE.md`** -- test coverage guide (95%+ target)

**Important:** `assets/ai/ai-rust` may be a symlink. If it doesn't resolve, clone it:

```bash
git clone https://github.com/oxur/ai-rust assets/ai/ai-rust
```

### Key Conventions

- Target 95%+ test coverage; never accept broken or ignored tests
- Always run `make format` after changes and `make lint` before testing
- Validate implementations against real `.mnlgxdlib`/`.mnlgxdprog` files and the workbench Python implementations, not just the Korg spec (which has known errata)
- The `workbench/` directory (gitignored) contains cloned reference implementations for cross-validation

### User Guide

The official Korg product user guide (PDF) for the Minilogue has been converted to Markdown, with images included -- and most importantly, images have all been analysed and annotated with captions for easy AI-reading, here:

- `./docs/korg-user-guide/book.md`

### Known Spec Errata

- Bank select and program number encoding differs from spec in practice -- validate against real files
- `.mnlgxdpreset` files from firmware v2.10+ use 448-byte program blobs, not 1024-byte; both formats must be supported

## Project Plan

The detailed project plan with milestone breakdowns lives at:
`docs/design/02-under-review/0001-minilogue-xd-rust-library-project-plan.md`

## Git Remotes

Pushes go to: macpro, github, codeberg (via `make push`)
