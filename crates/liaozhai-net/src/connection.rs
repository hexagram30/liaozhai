//! Per-connection handling.
//!
//! M4: state-machine-driven I/O loop with real authentication,
//! per-connection retry counter, per-IP rate limiting, and IAC ECHO.

use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::StreamExt;
use liaozhai_core::constants;
use liaozhai_core::id::ConnectionId;
use tokio::net::TcpStream;
use tokio_util::codec::FramedRead;
use tracing::{debug, info, trace, warn};

use crate::codec::{CodecItem, TelnetCodecError, TelnetLineCodec};
use crate::context::SessionContext;
use crate::output::LineWriter;
use crate::session::{Session, SessionState, Transition};

/// Handle a single inbound TCP connection.
///
/// # Errors
///
/// Returns an error if a fatal I/O error occurs.
pub async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    ctx: Arc<SessionContext>,
) -> liaozhai_core::error::Result<()> {
    handle_connection_with_codec(stream, peer, TelnetLineCodec::new(), ctx).await
}

/// Like [`handle_connection`], but accepts a pre-configured codec.
///
/// # Errors
///
/// Returns an error if a fatal I/O error occurs.
#[expect(clippy::too_many_lines)]
pub async fn handle_connection_with_codec(
    stream: TcpStream,
    peer: SocketAddr,
    codec: TelnetLineCodec,
    ctx: Arc<SessionContext>,
) -> liaozhai_core::error::Result<()> {
    let conn_id = ConnectionId::new();
    info!(%conn_id, %peer, "connection accepted");

    // Check IP rate limit immediately on connect
    if ctx.rate_limiter.is_throttled(peer.ip()) {
        let (_, write_half) = stream.into_split();
        let mut writer = LineWriter::new(write_half);
        // Connection is rate-limited; notice write failure is non-actionable.
        let _ = writer
            .write_raw(constants::AUTH_RATE_LIMITED_MSG.as_bytes())
            .await;
        let _ = writer.shutdown().await;
        info!(%conn_id, %peer, "rate-limited, disconnected immediately");
        return Ok(());
    }

    let (read_half, write_half) = stream.into_split();
    let mut lines = FramedRead::new(read_half, codec);
    let mut writer = LineWriter::new(write_half);

    let mut session = Session::new(Arc::clone(&ctx.world_registry));
    let initial_output = format!("{}{}", constants::BANNER, Session::initial_prompt());
    if let Err(e) = writer.write_raw(initial_output.as_bytes()).await {
        warn!(%conn_id, %peer, error = %e, "failed to send banner");
        return Err(e.into());
    }

    let mut auth_failures: u32 = 0;
    let mut line_count: u64 = 0;
    loop {
        match lines.next().await {
            Some(Ok(CodecItem::Line(line))) => {
                line_count += 1;
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
                        let mut frame = Vec::with_capacity(output.len() + 3);
                        if matches!(&next, SessionState::Authenticating { username: Some(_) }) {
                            frame.extend_from_slice(constants::IAC_WILL_ECHO);
                        } else if matches!(&next, SessionState::WorldSelection { .. }) {
                            frame.extend_from_slice(constants::IAC_WONT_ECHO);
                        }
                        frame.extend_from_slice(output.as_bytes());
                        if let Err(e) = writer.write_raw(&frame).await {
                            warn!(%conn_id, %peer, error = %e, "failed to write output");
                            break;
                        }
                        debug!(
                            %conn_id, %peer,
                            from = ?session.state(), to = ?next,
                            "state transition"
                        );
                        session.apply(next);
                    }
                    Transition::AuthPending { username, password } => {
                        // IAC ECHO: password consumed, restore client echo immediately
                        if let Err(e) = writer.write_raw(constants::IAC_WONT_ECHO).await {
                            warn!(%conn_id, %peer, error = %e, "failed to write IAC WONT ECHO");
                            break;
                        }

                        // Async credential verification
                        let auth_result = match ctx
                            .account_store
                            .verify_credentials(&username, &password)
                            .await
                        {
                            Ok(result) => result,
                            Err(e) => {
                                warn!(%conn_id, %peer, error = %e, "auth store error");
                                // Connection terminated due to internal error;
                                // notice write failure is non-actionable.
                                let _ = writer
                                    .write_raw(constants::AUTH_INTERNAL_ERROR_MSG.as_bytes())
                                    .await;
                                break;
                            }
                        };

                        if let Some(ref account) = auth_result {
                            ctx.rate_limiter.reset(peer.ip());
                            if let Err(e) = ctx.account_store.record_login(account.id()).await {
                                warn!(%conn_id, %peer, error = %e, "failed to record login");
                            }
                            info!(%conn_id, %peer, username = %username, "authentication successful");
                        } else {
                            auth_failures += 1;
                            ctx.rate_limiter.record_failure(peer.ip());
                            warn!(
                                %conn_id, %peer,
                                username = %username,
                                failures = auth_failures,
                                "authentication failed"
                            );

                            if auth_failures >= ctx.max_login_attempts {
                                // Retry limit exceeded; notice write failure is non-actionable.
                                let _ = writer
                                    .write_raw(constants::AUTH_MAX_RETRIES_MSG.as_bytes())
                                    .await;
                                break;
                            }
                        }

                        let completion = session.complete_auth(auth_result);
                        match completion {
                            Transition::Advance { next, output } => {
                                // The inner Advance from complete_auth only ever targets
                                // WorldSelection or Authenticating { username: None }. It
                                // never targets the password sub-state, so no IAC ECHO
                                // handling is needed here. If complete_auth ever gains a
                                // new variant that targets a different state, revisit.
                                if let Err(e) = writer.write_raw(output.as_bytes()).await {
                                    warn!(%conn_id, %peer, error = %e, "failed to write output");
                                    break;
                                }
                                debug!(
                                    %conn_id, %peer,
                                    from = ?session.state(), to = ?next,
                                    "state transition"
                                );
                                session.apply(next);
                            }
                            other => {
                                warn!(%conn_id, %peer, transition = ?other, "unexpected transition from complete_auth");
                                break;
                            }
                        }
                    }
                    Transition::Disconnect { goodbye } => {
                        // IAC ECHO: if disconnecting from password state, restore echo first
                        if session.is_password_input() {
                            let _ = writer.write_raw(constants::IAC_WONT_ECHO).await;
                        }
                        debug!(
                            %conn_id, %peer,
                            from = ?session.state(),
                            "session ending"
                        );
                        // Session is ending; notice write failure is non-actionable.
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
                // Connection terminated; notice write failure is non-actionable.
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
    use std::time::Duration;

    use liaozhai_auth::params::Argon2Params;
    use liaozhai_auth::rate_limiter::AuthRateLimiter;
    use liaozhai_auth::store::AccountStore;
    use liaozhai_worlds::metadata::WorldMetadata;
    use liaozhai_worlds::registry::WorldRegistry;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    const TEST_PASSWORD: &str = "secret";

    async fn setup() -> (
        TcpListener,
        SocketAddr,
        Arc<SessionContext>,
        tempfile::TempDir,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let params = Argon2Params::test_fast();
        let store = AccountStore::open(&db_path, &params).unwrap();
        store.create_account("alice", TEST_PASSWORD).await.unwrap();

        let ctx = Arc::new(SessionContext {
            account_store: Arc::new(store),
            world_registry: Arc::new(WorldRegistry::new(vec![
                WorldMetadata::new(
                    "studio-dusk",
                    "The Studio at Dusk",
                    "A small interior, warmly lit.",
                ),
                WorldMetadata::new(
                    "mountain-trail",
                    "The Mountain Trail",
                    "A path winding into mist.",
                ),
                WorldMetadata::new(
                    "library-echoes",
                    "The Library of Echoes",
                    "A reading room of recursive proportions.",
                ),
            ])),
            rate_limiter: Arc::new(AuthRateLimiter::new(Duration::from_secs(60), 10)),
            max_login_attempts: 3,
        });

        (listener, addr, ctx, dir)
    }

    async fn read_until_str(client: &mut TcpStream, marker: &str) -> String {
        let mut buf = vec![0u8; 16384];
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

    async fn read_raw_until(client: &mut TcpStream, marker: &[u8]) -> Vec<u8> {
        let mut buf = vec![0u8; 16384];
        let mut received = Vec::new();
        loop {
            let n = client.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            received.extend_from_slice(&buf[..n]);
            if received.windows(marker.len()).any(|w| w == marker) {
                break;
            }
        }
        received
    }

    #[tokio::test]
    async fn full_v01_demo() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();

        let banner = read_until_str(&mut client, "Username: ").await;
        assert!(banner.contains("Liaozhai MUX"));

        client.write_all(b"alice\r\n").await.unwrap();
        let prompt = read_until_str(&mut client, "Password: ").await;
        assert!(prompt.contains("Password: "));

        client
            .write_all(format!("{TEST_PASSWORD}\r\n").as_bytes())
            .await
            .unwrap();
        let world_list = read_until_str(&mut client, "Select a world").await;
        assert!(world_list.contains("Welcome, alice"));
        assert!(world_list.contains("Available worlds:"));
        assert!(world_list.contains("Select a world (1-3, or 'quit'):"));

        client.write_all(b"1\r\n").await.unwrap();
        let goodbye = read_until_str(&mut client, "Disconnecting").await;
        assert!(goodbye.contains("The Studio at Dusk"));

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn failed_login_re_prompts_for_username() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;

        client.write_all(b"wrongpass\r\n").await.unwrap();
        let response = read_until_str(&mut client, "Username: ").await;
        assert!(response.contains("Authentication failed"));
        assert!(response.contains("Username: "));

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn three_failed_logins_disconnects() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        for i in 0..3 {
            client.write_all(b"alice\r\n").await.unwrap();
            let _ = read_until_str(&mut client, "Password: ").await;
            client.write_all(b"wrong\r\n").await.unwrap();

            if i < 2 {
                let _ = read_until_str(&mut client, "Username: ").await;
            }
        }

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Too many failed attempts. Disconnecting."));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn unknown_user_returns_auth_failed() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        client.write_all(b"nobody\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;
        client.write_all(b"whatever\r\n").await.unwrap();
        let response = read_until_str(&mut client, "Username: ").await;
        assert!(response.contains("Authentication failed"));

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn iac_will_echo_before_password_prompt() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        client.write_all(b"alice\r\n").await.unwrap();
        // Read raw bytes to check for IAC WILL ECHO before "Password: "
        let raw = read_raw_until(&mut client, b"Password: ").await;
        assert!(
            raw.windows(3).any(|w| w == constants::IAC_WILL_ECHO),
            "expected IAC WILL ECHO before password prompt"
        );

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn iac_wont_echo_after_password() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;

        client
            .write_all(format!("{TEST_PASSWORD}\r\n").as_bytes())
            .await
            .unwrap();
        // Read raw bytes to check for IAC WONT ECHO before welcome
        let raw = read_raw_until(&mut client, b"Welcome").await;
        assert!(
            raw.windows(3).any(|w| w == constants::IAC_WONT_ECHO),
            "expected IAC WONT ECHO after password consumption"
        );

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn quit_at_username_state() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
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
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;
        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;

        client.write_all(b"quit\r\n").await.unwrap();
        // Should get IAC WONT ECHO + goodbye
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Until the next strange tale."));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn quit_at_world_selection_state() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;
        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;
        client
            .write_all(format!("{TEST_PASSWORD}\r\n").as_bytes())
            .await
            .unwrap();
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
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        client.write_all(b"\r\n").await.unwrap();
        let error = read_until_str(&mut client, "Username: ").await;
        assert!(error.contains("cannot be empty"));

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn invalid_world_selection_re_prompts() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;
        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;
        client
            .write_all(format!("{TEST_PASSWORD}\r\n").as_bytes())
            .await
            .unwrap();
        let _ = read_until_str(&mut client, "Select a world").await;

        client.write_all(b"4\r\n").await.unwrap();
        let error = read_until_str(&mut client, "Select a world").await;
        assert!(error.contains("between 1 and 3"));

        client.write_all(b"2\r\n").await.unwrap();
        let goodbye = read_until_str(&mut client, "Disconnecting").await;
        assert!(goodbye.contains("The Mountain Trail"));

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn client_disconnect_without_quit() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;
        drop(client);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn iac_bytes_stripped_during_session() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
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
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;

        let long_data = "x".repeat(5000);
        client.write_all(long_data.as_bytes()).await.unwrap();
        let notice = read_until_str(&mut client, "Line too long").await;
        assert!(notice.contains("Line too long; ignored."));

        client.write_all(b"\r\n").await.unwrap();
        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn buffer_overflow_disconnects_client() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            let codec = TelnetLineCodec::with_limits(4096, 256);
            handle_connection_with_codec(stream, peer, codec, c)
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

    #[tokio::test]
    async fn rate_limited_connection_rejected_immediately() {
        let (listener, addr, ctx, _dir) = setup().await;

        // Trigger the rate limiter for the loopback IP.
        let loopback: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        for _ in 0..15 {
            ctx.rate_limiter.record_failure(loopback);
        }

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Too many failed attempts from your IP"));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn successful_login_records_last_login_at() {
        let (listener, addr, ctx, _dir) = setup().await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;
        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;
        client
            .write_all(format!("{TEST_PASSWORD}\r\n").as_bytes())
            .await
            .unwrap();
        let _ = read_until_str(&mut client, "Select a world").await;
        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        server.await.unwrap();

        let accounts = ctx.account_store.list_accounts().await.unwrap();
        let alice = accounts.iter().find(|a| a.username() == "alice").unwrap();
        assert!(alice.last_login_at().is_some());
    }

    async fn setup_with_toml_worlds(
        toml_content: &str,
    ) -> (
        TcpListener,
        SocketAddr,
        Arc<SessionContext>,
        tempfile::TempDir,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let worlds_path = dir.path().join("worlds.toml");
        std::fs::write(&worlds_path, toml_content).unwrap();

        let params = Argon2Params::test_fast();
        let store = AccountStore::open(&db_path, &params).unwrap();
        store.create_account("alice", TEST_PASSWORD).await.unwrap();

        let registry = WorldRegistry::load_from_toml(&worlds_path).unwrap();

        let ctx = Arc::new(SessionContext {
            account_store: Arc::new(store),
            world_registry: Arc::new(registry),
            rate_limiter: Arc::new(AuthRateLimiter::new(Duration::from_secs(60), 10)),
            max_login_attempts: 3,
        });

        (listener, addr, ctx, dir)
    }

    #[tokio::test]
    async fn world_list_loaded_from_toml() {
        let toml = r#"
            [[world]]
            slug = "test-a"
            name = "Test World Alpha"
            short = "The first test world."

            [[world]]
            slug = "test-b"
            name = "Test World Bravo"
            short = "The second test world."
        "#;

        let (listener, addr, ctx, _dir) = setup_with_toml_worlds(toml).await;

        let c = ctx.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer, c).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "Username: ").await;
        client.write_all(b"alice\r\n").await.unwrap();
        let _ = read_until_str(&mut client, "Password: ").await;
        client
            .write_all(format!("{TEST_PASSWORD}\r\n").as_bytes())
            .await
            .unwrap();
        let world_list = read_until_str(&mut client, "Select a world").await;

        assert!(world_list.contains("Test World Alpha"));
        assert!(world_list.contains("Test World Bravo"));
        assert!(world_list.contains("Select a world (1-2, or 'quit'):"));

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        server.await.unwrap();
    }
}
