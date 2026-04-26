//! Shared types, IDs, error enum, and constants for Liaozhai MUX.
//!
//! This crate is the foundation layer. Every other crate in the workspace
//! depends on it. It contains no I/O and no async code.

pub mod constants;
pub mod error;
pub mod id;
