//! Account management and authentication for Liaozhai MUX.
//!
//! [`store::AccountStore`] holds accounts in `SQLite` with argon2id-hashed
//! passwords; [`rate_limiter::AuthRateLimiter`] tracks failed-login
//! attempts per IP for sliding-window throttling; [`params::Argon2Params`]
//! encapsulates the hashing parameters loaded from configuration.

pub mod account;
pub mod params;
pub mod rate_limiter;
pub mod store;
