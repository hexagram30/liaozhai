//! Foundation crate for Liaozhai MUX.
//!
//! Provides shared types ([`id::AccountId`], [`id::WorldId`],
//! [`id::ConnectionId`]), the project-wide [`error::Error`] enum, and
//! workspace-wide [`constants`]. Every other workspace crate depends
//! on this one. Contains no I/O, no async code, no platform assumptions.

pub mod constants;
pub mod error;
pub mod id;
