//! Per-connection handling.
//!
//! M1: send a banner, close the connection.
//! M2/M3: codec-based line I/O, state machine.

use std::net::SocketAddr;

use liaozhai_core::constants;
use liaozhai_core::id::ConnectionId;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tracing::{info, warn};

/// Handle a single inbound TCP connection.
///
/// Sends the server banner and immediately closes the connection.
///
/// # Errors
///
/// Returns an error if writing the banner or shutting down the stream fails.
pub async fn handle_connection(
    mut stream: TcpStream,
    peer: SocketAddr,
) -> liaozhai_core::error::Result<()> {
    let conn_id = ConnectionId::new();
    info!(%conn_id, %peer, "connection accepted");

    if let Err(e) = stream.write_all(constants::BANNER.as_bytes()).await {
        warn!(%conn_id, %peer, error = %e, "failed to send banner");
        return Err(e.into());
    }

    if let Err(e) = stream.shutdown().await {
        warn!(%conn_id, %peer, error = %e, "failed to shut down stream");
        return Err(e.into());
    }

    info!(%conn_id, %peer, "connection closed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    use super::*;

    #[tokio::test]
    async fn sends_banner_and_closes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();

        let received = String::from_utf8_lossy(&buf);
        assert!(received.contains("Liaozhai MUX"));
        assert!(received.contains("Multi-User eXegesis"));
        assert!(received.contains(constants::VERSION));

        server.await.unwrap();
    }
}
