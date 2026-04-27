//! TCP listener and connection acceptance loop.

use std::path::Path;
use std::sync::Arc;

use crate::config::AppConfig;
use anyhow::{Context, Result};
use liaozhai_auth::rate_limiter::AuthRateLimiter;
use liaozhai_auth::store::AccountStore;
use liaozhai_net::context::SessionContext;
use liaozhai_worlds::registry::WorldRegistry;
use tokio::net::TcpListener;
use tracing::{error, info};

/// Run the TCP listener, accepting connections and spawning per-connection tasks.
///
/// # Errors
///
/// Returns an error if binding the TCP listener fails.
pub async fn run(cfg: &AppConfig) -> Result<()> {
    let addr = format!("{}:{}", cfg.server.bind_address, cfg.server.port);

    // Open (or create) the account store
    let params = cfg.auth.argon2_params();
    let account_store = AccountStore::open(Path::new(&cfg.auth.db_path), &params)
        .context("opening account store")?;
    let account_store = Arc::new(account_store);
    info!(db_path = %cfg.auth.db_path, "account store opened");

    // Create rate limiter
    let rate_limiter = Arc::new(AuthRateLimiter::new(
        cfg.auth.rate_limit_window(),
        cfg.auth.rate_limit_max_failures,
    ));

    // TODO(M5): replace with TOML-loaded registry from cfg.worlds.registry_path.
    let world_registry = Arc::new(WorldRegistry::placeholder());
    info!(world_count = world_registry.len(), "world registry loaded");

    let ctx = Arc::new(SessionContext {
        account_store,
        world_registry,
        rate_limiter,
        max_login_attempts: cfg.auth.max_login_attempts,
    });

    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding TCP listener to {addr}"))?;

    info!(%addr, "listening for connections");

    // TODO(M6): enforce cfg.server.max_connections via a tokio::sync::Semaphore.
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let ctx = Arc::clone(&ctx);
                tokio::spawn(async move {
                    if let Err(e) =
                        liaozhai_net::connection::handle_connection(stream, peer, ctx).await
                    {
                        error!(%peer, error = %e, "connection error");
                    }
                });
            }
            Err(e) => {
                error!(error = %e, "failed to accept connection");
            }
        }
    }
}
