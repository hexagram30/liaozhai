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

    run_accept_loop(listener, ctx, semaphore, shutdown, drain_timeout).await;

    Ok(())
}

/// The core accept loop, factored out for testability.
pub(crate) async fn run_accept_loop(
    listener: TcpListener,
    ctx: Arc<SessionContext>,
    semaphore: Arc<Semaphore>,
    shutdown: CancellationToken,
    drain_timeout: Duration,
) {
    let mut join_set = JoinSet::new();

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

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;

    use liaozhai_auth::params::Argon2Params;
    use liaozhai_auth::rate_limiter::AuthRateLimiter;
    use liaozhai_auth::store::AccountStore;
    use liaozhai_net::context::SessionContext;
    use liaozhai_worlds::metadata::WorldMetadata;
    use liaozhai_worlds::registry::WorldRegistry;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Semaphore;
    use tokio_util::sync::CancellationToken;

    use super::*;

    const TEST_PASSWORD: &str = "secret";

    async fn setup(
        max_connections: usize,
        drain_secs: u64,
    ) -> (
        TcpListener,
        Arc<SessionContext>,
        Arc<Semaphore>,
        CancellationToken,
        Duration,
        tempfile::TempDir,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let params = Argon2Params::test_fast();
        let store = AccountStore::open(&db_path, &params).unwrap();
        store.create_account("alice", TEST_PASSWORD).await.unwrap();

        let shutdown = CancellationToken::new();

        let ctx = Arc::new(SessionContext {
            account_store: Arc::new(store),
            world_registry: Arc::new(WorldRegistry::new(vec![WorldMetadata::new(
                "test",
                "Test World",
                "A test.",
            )])),
            rate_limiter: Arc::new(AuthRateLimiter::new(Duration::from_secs(60), 10, 10_000)),
            max_login_attempts: 3,
            shutdown: shutdown.clone(),
        });

        let semaphore = Arc::new(Semaphore::new(max_connections));
        let drain_timeout = Duration::from_secs(drain_secs);

        (listener, ctx, semaphore, shutdown, drain_timeout, dir)
    }

    async fn read_all(client: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn addr_of(listener: &TcpListener) -> SocketAddr {
        listener.local_addr().unwrap()
    }

    #[tokio::test]
    async fn max_connections_rejects_excess() {
        let (listener, ctx, semaphore, shutdown, drain_timeout, _dir) = setup(2, 5).await;
        let addr = addr_of(&listener);

        let shutdown_clone = shutdown.clone();
        let server = tokio::spawn(async move {
            run_accept_loop(listener, ctx, semaphore, shutdown_clone, drain_timeout).await;
        });

        // Open 2 connections (at capacity)
        let mut c1 = TcpStream::connect(addr).await.unwrap();
        let mut c2 = TcpStream::connect(addr).await.unwrap();
        // Give the accept loop time to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // 3rd connection should be rejected
        let mut c3 = TcpStream::connect(addr).await.unwrap();
        let response = read_all(&mut c3).await;
        assert!(response.contains("Server is full"));

        // Clean up
        shutdown.cancel();
        drop(c1);
        drop(c2);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn semaphore_released_on_disconnect() {
        let (listener, ctx, semaphore, shutdown, drain_timeout, _dir) = setup(1, 5).await;
        let addr = addr_of(&listener);

        let shutdown_clone = shutdown.clone();
        let server = tokio::spawn(async move {
            run_accept_loop(listener, ctx, semaphore, shutdown_clone, drain_timeout).await;
        });

        // Open connection 1, then close it via quit
        let mut c1 = TcpStream::connect(addr).await.unwrap();
        let mut buf = vec![0u8; 4096];
        let _ = c1.read(&mut buf).await; // read banner
        c1.write_all(b"quit\r\n").await.unwrap();
        let _ = c1.read(&mut buf).await; // read goodbye
        drop(c1);
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Connection 2 should succeed (permit was released)
        let mut c2 = TcpStream::connect(addr).await.unwrap();
        let n = c2.read(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);
        assert!(response.contains("Liaozhai MUX"));

        shutdown.cancel();
        drop(c2);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_signal_drains_active_connections() {
        let (listener, ctx, semaphore, shutdown, drain_timeout, _dir) = setup(10, 5).await;
        let addr = addr_of(&listener);

        let shutdown_clone = shutdown.clone();
        let server = tokio::spawn(async move {
            run_accept_loop(listener, ctx, semaphore, shutdown_clone, drain_timeout).await;
        });

        let mut c1 = TcpStream::connect(addr).await.unwrap();
        let mut c2 = TcpStream::connect(addr).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        shutdown.cancel();

        let r1 = read_all(&mut c1).await;
        let r2 = read_all(&mut c2).await;
        assert!(r1.contains("studio closes"));
        assert!(r2.contains("studio closes"));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_drain_timeout_abandons_stuck_clients() {
        // Use very short drain timeout
        let (listener, ctx, semaphore, shutdown, _drain_timeout, _dir) = setup(10, 0).await;
        let addr = addr_of(&listener);
        let short_drain = Duration::from_millis(200);

        let shutdown_clone = shutdown.clone();
        let server = tokio::spawn(async move {
            run_accept_loop(listener, ctx, semaphore, shutdown_clone, short_drain).await;
        });

        // Open a connection but don't read from it (stuck client)
        let _c1 = TcpStream::connect(addr).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        shutdown.cancel();

        // Server should exit after the short drain timeout
        let result = tokio::time::timeout(Duration::from_secs(3), server).await;
        assert!(
            result.is_ok(),
            "server should have exited after drain timeout"
        );
    }
}
