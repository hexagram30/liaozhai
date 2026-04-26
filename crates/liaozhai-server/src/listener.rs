//! TCP listener and connection acceptance loop.

use std::sync::Arc;

use crate::config::AppConfig;
use anyhow::{Context, Result};
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

    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding TCP listener to {addr}"))?;

    info!(%addr, "listening for connections");

    // TODO(M5): replace with TOML-loaded registry from cfg.worlds.registry_path.
    let registry = Arc::new(WorldRegistry::placeholder());
    info!(world_count = registry.len(), "world registry loaded");

    // TODO(M6): enforce cfg.server.max_connections via a tokio::sync::Semaphore.
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let reg = registry.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        liaozhai_net::connection::handle_connection(stream, peer, reg).await
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
