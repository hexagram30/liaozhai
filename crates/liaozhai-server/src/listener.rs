//! TCP listener and connection acceptance loop with graceful shutdown.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::config::AppConfig;
use anyhow::{Context, Result};
use liaozhai_auth::rate_limiter::AuthRateLimiter;
use liaozhai_auth::store::AccountStore;
use liaozhai_core::constants;
use liaozhai_net::context::SessionContext;
use liaozhai_net::output::LineWriter;
use liaozhai_worlds::registry::WorldRegistry;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Run the TCP listener, accepting connections and spawning per-connection tasks.
///
/// Handles SIGINT/SIGTERM for graceful shutdown and enforces `max_connections`
/// via a semaphore.
///
/// # Errors
///
/// Returns an error if binding the TCP listener or opening the account store fails.
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
        cfg.auth.rate_limiter_max_entries,
    ));

    let world_registry = Arc::new(
        WorldRegistry::load_from_toml(Path::new(&cfg.worlds.registry_path))
            .context("loading world registry")?,
    );
    info!(
        world_count = world_registry.len(),
        path = %cfg.worlds.registry_path,
        "world registry loaded"
    );

    let shutdown = CancellationToken::new();

    let ctx = Arc::new(SessionContext {
        account_store,
        world_registry,
        rate_limiter,
        max_login_attempts: cfg.auth.max_login_attempts,
        shutdown: shutdown.clone(),
    });

    let semaphore = Arc::new(Semaphore::new(cfg.server.max_connections));
    let drain_timeout = Duration::from_secs(cfg.server.shutdown_drain_secs);

    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding TCP listener to {addr}"))?;

    info!(%addr, "listening for connections");

    // Spawn signal handler
    let shutdown_signal = shutdown.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        info!("shutdown requested");
        shutdown_signal.cancel();
    });

    let mut join_set = JoinSet::new();

    // Accept loop
    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                break;
            }
            accept_result = listener.accept() => {
                let Ok((stream, peer)) = accept_result else {
                    error!(error = %accept_result.unwrap_err(), "failed to accept connection");
                    continue;
                };

                let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                    let (_, write_half) = stream.into_split();
                    let mut writer = LineWriter::new(write_half);
                    let _ = writer
                        .write_raw(constants::SERVER_FULL_MSG.as_bytes())
                        .await;
                    let _ = writer.shutdown().await;
                    info!(%peer, "rejected: server at max_connections");
                    continue;
                };

                let ctx = Arc::clone(&ctx);
                join_set.spawn(async move {
                    let _permit = permit;
                    if let Err(e) =
                        liaozhai_net::connection::handle_connection(stream, peer, ctx).await
                    {
                        error!(%peer, error = %e, "connection error");
                    }
                });
            }
        }
    }

    info!("closed listener");

    // Drain active connections
    let drain_result = tokio::time::timeout(drain_timeout, async {
        let mut drained = 0u32;
        while join_set.join_next().await.is_some() {
            drained += 1;
        }
        drained
    })
    .await;

    if let Ok(drained) = drain_result {
        info!(drained, "drained connections");
    } else {
        let remaining = join_set.len();
        warn!(remaining, "drain timeout, abandoning connections");
        join_set.shutdown().await;
    }

    info!("shutdown complete");
    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut sigterm = signal(SignalKind::terminate()).expect("sigterm handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("sigint handler");
    tokio::select! {
        _ = sigint.recv() => {},
        _ = sigterm.recv() => {},
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() {
    tokio::signal::ctrl_c().await.expect("ctrl_c handler");
}
