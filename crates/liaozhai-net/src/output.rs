//! Write-side helpers for connection output.

use std::io;

use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;

/// A line-oriented writer wrapping the TCP write half.
#[derive(Debug)]
pub struct LineWriter {
    inner: OwnedWriteHalf,
}

impl LineWriter {
    pub fn new(write_half: OwnedWriteHalf) -> Self {
        Self { inner: write_half }
    }

    /// Write a string followed by CRLF.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the write fails.
    pub async fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.inner.write_all(line.as_bytes()).await?;
        self.inner.write_all(b"\r\n").await
    }

    /// Write raw bytes directly (no CRLF appended).
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the write fails.
    pub async fn write_raw(&mut self, data: &[u8]) -> io::Result<()> {
        self.inner.write_all(data).await
    }

    /// Flush buffered output.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the flush fails.
    pub async fn flush(&mut self) -> io::Result<()> {
        self.inner.flush().await
    }

    /// Shut down the write half of the connection.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the shutdown fails.
    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.inner.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt;
    use tokio::net::{TcpListener, TcpStream};

    use super::*;

    #[tokio::test]
    async fn write_line_appends_crlf() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (_, write_half) = stream.into_split();
            let mut writer = LineWriter::new(write_half);
            writer.write_line("hello").await.unwrap();
            writer.shutdown().await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello\r\n");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn write_raw_sends_exact_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (_, write_half) = stream.into_split();
            let mut writer = LineWriter::new(write_half);
            writer.write_raw(b"raw\n").await.unwrap();
            writer.shutdown().await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"raw\n");

        server.await.unwrap();
    }
}
