//! Shared connection context, bundling per-connection dependencies.

use std::sync::Arc;

use liaozhai_auth::rate_limiter::AuthRateLimiter;
use liaozhai_auth::store::AccountStore;
use liaozhai_worlds::registry::WorldRegistry;
use tokio_util::sync::CancellationToken;

/// Dependencies shared across all connections, passed to each connection handler.
///
/// Constructed once at server startup (in `listener.rs`) and shared via `Arc`.
#[derive(Debug, Clone)]
pub struct SessionContext {
    pub account_store: Arc<AccountStore>,
    pub world_registry: Arc<WorldRegistry>,
    pub rate_limiter: Arc<AuthRateLimiter>,
    pub max_login_attempts: u32,
    pub shutdown: CancellationToken,
}
