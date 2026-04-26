//! Per-connection handling.
//!
//! M2: codec-based line I/O with echo loop and session terminators.

use std::net::SocketAddr;

use futures_util::StreamExt;
use liaozhai_core::constants;
use liaozhai_core::id::ConnectionId;
use tokio::net::TcpStream;
use tokio_util::codec::FramedRead;
use tracing::{debug, info, trace, warn};

use crate::codec::{CodecItem, TelnetCodecError, TelnetLineCodec};
use crate::output::LineWriter;

/// Handle a single inbound TCP connection.
///
/// Sends the server banner, then enters an echo loop: each line received
/// from the client is written back with a CRLF terminator. Session-ending
/// commands (`quit`, `exit`, `bye`, `disconnect`) close the connection
/// gracefully.
///
/// # Errors
///
/// Returns an error if a fatal I/O error occurs. Non-fatal codec errors
/// (e.g., line too long) are handled inline by sending a notice to the
/// client and continuing.
pub async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
) -> liaozhai_core::error::Result<()> {
    handle_connection_with_codec(stream, peer, TelnetLineCodec::new()).await
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
) -> liaozhai_core::error::Result<()> {
    let conn_id = ConnectionId::new();
    info!(%conn_id, %peer, "connection accepted");

    let (read_half, write_half) = stream.into_split();
    let mut lines = FramedRead::new(read_half, codec);
    let mut writer = LineWriter::new(write_half);

    if let Err(e) = writer.write_raw(constants::BANNER.as_bytes()).await {
        warn!(%conn_id, %peer, error = %e, "failed to send banner");
        return Err(e.into());
    }

    let mut line_count: u64 = 0;
    loop {
        match lines.next().await {
            Some(Ok(CodecItem::Line(line))) => {
                line_count += 1;
                trace!(%conn_id, %peer, line_count, line = %line, "received line");

                if is_session_terminator(&line) {
                    debug!(%conn_id, %peer, "session terminator received");
                    // Session is ending; if the goodbye write fails, the client
                    // is already gone and there's nothing actionable.
                    let _ = writer.write_raw(constants::GOODBYE_MSG.as_bytes()).await;
                    break;
                }

                if let Err(e) = writer.write_line(&line).await {
                    warn!(%conn_id, %peer, error = %e, "failed to echo line");
                    break;
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

fn is_session_terminator(line: &str) -> bool {
    matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "quit" | "exit" | "bye" | "disconnect"
    )
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    async fn setup() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        (listener, addr)
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

    // --- Session terminator unit tests ---

    #[test]
    fn session_terminator_quit() {
        assert!(is_session_terminator("quit"));
    }

    #[test]
    fn session_terminator_exit() {
        assert!(is_session_terminator("exit"));
    }

    #[test]
    fn session_terminator_bye() {
        assert!(is_session_terminator("bye"));
    }

    #[test]
    fn session_terminator_disconnect() {
        assert!(is_session_terminator("disconnect"));
    }

    #[test]
    fn session_terminator_case_insensitive() {
        assert!(is_session_terminator("QUIT"));
        assert!(is_session_terminator("Exit"));
        assert!(is_session_terminator("BYE"));
    }

    #[test]
    fn session_terminator_whitespace() {
        assert!(is_session_terminator("  quit  "));
    }

    #[test]
    fn session_terminator_rejects_other() {
        assert!(!is_session_terminator("hello"));
        assert!(!is_session_terminator(""));
        assert!(!is_session_terminator("quitting"));
    }

    // --- Integration tests ---

    #[tokio::test]
    async fn banner_then_echo() {
        let (listener, addr) = setup().await;

        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();

        // Read banner
        let banner = read_until_str(&mut client, "eXegesis").await;
        assert!(banner.contains("Liaozhai MUX"));

        // Send a line, read echo
        client.write_all(b"hello\r\n").await.unwrap();
        let echo = read_until_str(&mut client, "hello").await;
        assert!(echo.contains("hello"));

        // Quit
        client.write_all(b"quit\r\n").await.unwrap();
        let goodbye = read_until_str(&mut client, "strange tale").await;
        assert!(goodbye.contains("Until the next strange tale."));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn quit_ends_session() {
        let (listener, addr) = setup().await;

        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "eXegesis").await;

        client.write_all(b"quit\r\n").await.unwrap();

        // Should reach EOF after goodbye
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Until the next strange tale."));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn exit_ends_session() {
        let (listener, addr) = setup().await;

        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "eXegesis").await;

        client.write_all(b"EXIT\r\n").await.unwrap();

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Until the next strange tale."));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn multiple_lines_echoed() {
        let (listener, addr) = setup().await;

        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "eXegesis").await;

        for word in &["alpha", "beta", "gamma"] {
            client
                .write_all(format!("{word}\r\n").as_bytes())
                .await
                .unwrap();
            let echo = read_until_str(&mut client, word).await;
            assert!(echo.contains(word));
        }

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn client_disconnect_without_quit() {
        let (listener, addr) = setup().await;

        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "eXegesis").await;

        // Close without quit
        drop(client);

        // Server task should complete cleanly
        server.await.unwrap();
    }

    #[tokio::test]
    async fn iac_bytes_stripped_in_echo() {
        let (listener, addr) = setup().await;

        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "eXegesis").await;

        // Send IAC WILL ECHO followed by "hello"
        client.write_all(b"\xFF\xFB\x01hello\r\n").await.unwrap();
        let echo = read_until_str(&mut client, "hello").await;
        assert!(echo.contains("hello"));
        // Verify no 0xFF bytes leaked through (they would render as U+FFFD after lossy decode).
        assert!(!echo.contains('\u{FFFD}'));

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn line_too_long_continues() {
        let (listener, addr) = setup().await;

        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            handle_connection(stream, peer).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "eXegesis").await;

        // Send >4096 bytes without a terminator to trigger LineTooLong.
        // The codec only checks length when no terminator is found, so the
        // data must arrive without a line terminator in the same read.
        let long_data = "x".repeat(5000);
        client.write_all(long_data.as_bytes()).await.unwrap();

        let notice = read_until_str(&mut client, "Line too long").await;
        assert!(notice.contains("Line too long; ignored."));

        // Send a terminator to end the discarded overflow, then a normal line.
        client.write_all(b"\r\n").await.unwrap();
        client.write_all(b"ok\r\n").await.unwrap();
        let echo = read_until_str(&mut client, "ok\r\n").await;
        assert!(echo.contains("ok"));

        client.write_all(b"quit\r\n").await.unwrap();
        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn buffer_overflow_disconnects_client() {
        let (listener, addr) = setup().await;

        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            let codec = TelnetLineCodec::with_limits(4096, 256);
            handle_connection_with_codec(stream, peer, codec)
                .await
                .unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_str(&mut client, "eXegesis").await;

        // Send 300 bytes with no terminator to exceed the 256-byte buffer cap.
        let payload = vec![b'x'; 300];
        client.write_all(&payload).await.unwrap();

        let mut rest = Vec::new();
        client.read_to_end(&mut rest).await.unwrap();
        let rest_str = String::from_utf8_lossy(&rest);
        assert!(rest_str.contains("Buffer overflow; disconnecting."));

        server.await.unwrap();
    }
}
