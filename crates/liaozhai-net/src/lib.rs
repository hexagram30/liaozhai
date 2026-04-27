//! Network protocol and connection handling for Liaozhai MUX.
//!
//! [`codec::TelnetLineCodec`] strips telnet IAC sequences and produces
//! line-oriented input; [`connection::handle_connection`] drives a
//! single TCP session through the session state machine
//! (banner → auth → world selection → goodbye); [`output::LineWriter`]
//! provides atomic line-and-CRLF writes; [`context::SessionContext`]
//! bundles the per-connection dependencies.

pub mod codec;
pub mod connection;
pub mod context;
pub mod output;
pub(crate) mod session;
