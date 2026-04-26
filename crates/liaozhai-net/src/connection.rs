//! Per-connection handling.
//!
//! M3: state-machine-driven I/O loop (banner → auth → world selection → goodbye).

use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::StreamExt;
use liaozhai_core::constants;
use liaozhai_core::id::ConnectionId;
use liaozhai_worlds::registry::WorldRegistry;
use tokio::net::TcpStream;
use tokio_util::codec::FramedRead;
use tracing::{debug, info, trace, warn};

use crate::codec::{CodecItem, TelnetCodecError, TelnetLineCodec};
use crate::output::LineWriter;
use crate::session::{Session, Transition};

/// Handle a single inbound TCP connection.
///
/// Sends the server banner, then drives the session state machine through
/// authentication and world selection.
///
/// # Errors
///
/// Returns an error if a fatal I/O error occurs.
pub async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    registry: Arc<WorldRegistry>,
) -> liaozhai_core::error::Result<()> {
    handle_connection_with_codec(stream, peer, TelnetLineCodec::new(), registry).await
}

/// Like [`handle_connection`], but accepts a pre-configured codec.
///
/// Useful for tests that need custom line/buffer limits.
///
/// # Errors
///
/// Returns an error if a fatal I/O error occurs.
pub async fn handle_connection_with_codec(
    stream: TcpStream,
    peer: SocketAddr,
    codec: TelnetLineCodec,
    registry: Arc<WorldRegistry>,
) -> liaozhai_core::error::Result<()> {
    let conn_id = ConnectionId::new();
    info!(%conn_id, %peer, "connection accepted");

    let (read_half, write_half) = stream.into_split();
    let mut lines = FramedRead::new(read_half, codec);
    let mut writer = LineWriter::new(write_half);

    let mut session = Session::new(registry);
    let initial_output = format!("{}{}", constants::BANNER, Session::initial_prompt());
    if let Err(e) = writer.write_raw(initial_output.as_bytes()).await {
        warn!(%conn_id, %peer, error = %e, "failed to send banner");
        return Err(e.into());
    }

    let mut line_count: u64 = 0;
    loop {
        match lines.next().await {
            Some(Ok(CodecItem::Line(line))) => {
                line_count += 1;
                // TODO(M4): redact password from trace logging when IAC ECHO
                // negotiation lands. For now, redact based on session state.
                let logged_line = if session.is_password_input() {
                    "<redacted>"
                } else {
                    line.as_str()
                };
                trace!(%conn_id, %peer, line_count, line = %logged_line, "received line");

                let transition = session.handle_input(&line);

                match transition {
                    Transition::Stay { output } => {
                        if let Err(e) = writer.write_raw(output.as_bytes()).await {
                            warn!(%conn_id, %peer, error = %e, "failed to write output");
                            break;
                        }
                    }
                    Transition::Advance { next, output } => {
                        if let Err(e) = writer.write_raw(output.as_bytes()).await {
                            warn!(%conn_id, %peer, error = %e, "failed to write output");
                            break;
                        }
                        debug!(
                            %conn_id,
                            %peer,
                            from = ?session.state(),
                            to = ?next,
                            "state transition"
                        );
                        session.apply(next);
                    }
                    Transition::Disconnect { goodbye } => {
                        debug!(
                            %conn_id,
                            %peer,
                            from = ?session.state(),
                            "session ending"
                        );
                        // Session is ending; if the goodbye write fails, the client
                        // is already gone and there's nothing actionable.
                        let _ = writer.write_raw(goodbye.as_bytes()).await;
                        break;
                    }
                }
            }

            Some(Ok(CodecItem::LineTooLong)) => {
                warn!(%conn_id, %peer, "line exceeded maximum length");
                if let Err(e) = writer
                    .write_raw(constants::LINE_TOO_LONG_MSG.as_bytes())
                    .await
                {
                    warn!(%conn_id, %peer, error = %e, "failed to send line-too-long notice");
                    break;
                }
            }

            Some(Err(TelnetCodecError::BufferOverflow { max })) => {
                warn!(%conn_id, %peer, max_size = max, "per-connection buffer overflow");
                // Connection is being terminated by the server; if the notice
                // write fails, the client is already gone.
                let _ = writer
                    .write_raw(constants::BUFFER_OVERFLOW_MSG.as_bytes())
                    .await;
                break;
            }

            Some(Err(TelnetCodecError::Io(e))) => {
                warn!(%conn_id, %peer, error = %e, "I/O error reading from client");
                break;
            }

            None => {
                debug!(%conn_id, %peer, "client disconnected (EOF)");
                break;
            }
        }
    }

    if let Err(e) = writer.shutdown().await {
        warn!(%conn_id, %peer, error = %e, "failed to shut down write half");
    }

    info!(%conn_id, %peer, line_count, "connection closed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use liaozhai_worlds::registry::WorldRegistry;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    async fn setup() -> (TcpListener, SocketAddr, Arc<WorldRegistry>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let registry = Arc::new(WorldRegistry::placeholder());
        (listener, addr, registry)
    }

    async fn read_until_str(client: &mut TcpStream, marker: &str) -> String {
        let mut buf = vec![0u8; 8192];
        let mut received = String::new();
        loop {
            let n = client.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            received.push_str(&String::from_utf8_lossy(&buf[..n]));
            if received.contains(marker) {
                break;
            }
        }
        received
    }

    #[tokio::test]
    async fn full_v01_demo() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, reg).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();

        let banner = read_until_str(&mut client, "Username: ").await;
        assert!(banner.contains("Liaozhai MUX"));
        assert!(banner.contains("Username: "));

        client.write_all(b"alice\r\n").await.unwrap();
        let prompt = read_until_str(&mut client, "Password: ").await;
        assert!(prompt.contains("Password: "));

        client.write_all(b"secret\r\n").await.unwrap();
        let world_list = read_until_str(&mut client, "Select a world").await;
        assert!(world_list.contains("Welcome, alice"));
        assert!(world_list.contains("Available worlds:"));
        assert!(world_list.contains("The Studio at Dusk"));
        assert!(world_list.contains("The Mountain Trail"));
        assert!(world_list.contains("The Library of Echoes"));
        assert!(world_list.contains("Select a world (1-3, or 'quit'):"));

        client.write_all(b"1\r\n").await.unwrap();
        let goodbye = read_until_str(&mut client, "Disconnecting").await;
        assert!(goodbye.contains("The Studio at Dusk"));
        assert!(goodbye.contains("Disconnecting"));

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn quit_at_username_state() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, reg).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        client.write_all(b"quit\r\n").await.unwrap();

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Until the next strange tale."));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn quit_at_password_state() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, reg).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;

        client.write_all(b"quit\r\n").await.unwrap();

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Until the next strange tale."));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn quit_at_world_selection_state() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, reg).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;
        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;
        client.write_all(b"secret\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Select a world").await;

        client.write_all(b"quit\r\n").await.unwrap();

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Until the next strange tale."));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn invalid_username_re_prompts() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, reg).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        client.write_all(b"\r\n").await.unwrap();
        let error = read_until_str(&mut client, "Username: ").await;
        assert!(error.contains("cannot be empty"));

        client.write_all(b"alice\r\n").await.unwrap();
        let prompt = read_until_str(&mut client, "Password: ").await;
        assert!(prompt.contains("Password: "));

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn invalid_world_selection_re_prompts() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, reg).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;
        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;
        client.write_all(b"secret\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Select a world").await;

        client.write_all(b"4\r\n").await.unwrap();
        let error = read_until_str(&mut client, "Select a world").await;
        assert!(error.contains("between 1 and 3"));

        client.write_all(b"abc\r\n").await.unwrap();
        let error = read_until_str(&mut client, "Select a world").await;
        assert!(error.contains("Please enter a number"));

        client.write_all(b"2\r\n").await.unwrap();
        let goodbye = read_until_str(&mut client, "Disconnecting").await;
        assert!(goodbye.contains("The Mountain Trail"));

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn exit_alias_at_world_selection() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, reg).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;
        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;
        client.write_all(b"secret\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Select a world").await;

        client.write_all(b"EXIT\r\n").await.unwrap();

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Until the next strange tale."));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn client_disconnect_without_quit() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, reg).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        drop(client);

        server.await.unwrap();
    }

    #[tokio::test]
    async fn iac_bytes_stripped_during_session() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, reg).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        client.write_all(b"\xFF\xFB\x01alice\r\n").await.unwrap();
        let prompt = read_until_str(&mut client, "Password: ").await;
        assert!(prompt.contains("Password: "));

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn line_too_long_during_session() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, reg).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        let long_data = "x".repeat(5000);
        client.write_all(long_data.as_bytes()).await.unwrap();

        let notice = read_until_str(&mut client, "Line too long").await;
        assert!(notice.contains("Line too long; ignored."));

        client.write_all(b"\r\n").await.unwrap();
        client.write_all(b"alice\r\n").await.unwrap();
        let prompt = read_until_str(&mut client, "Password: ").await;
        assert!(prompt.contains("Password: "));

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn buffer_overflow_disconnects_client() {
        let (listener, addr, registry) = setup().await;

        let reg = registry.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            let codec = TelnetLineCodec::with_limits(4096, 256);
            handle_connection_with_codec(stream, peer, codec, reg)
                .await
                .unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        let payload = vec![b'x'; 300];
        client.write_all(&payload).await.unwrap();

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Buffer overflow; disconnecting."));

        server.await.unwrap();
    }
}
