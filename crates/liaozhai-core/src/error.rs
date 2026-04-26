//! Project-wide error types.

/// The project-wide error enum.
///
/// Library crates in this workspace return `Error` from fallible operations.
/// The server binary wraps these in `anyhow::Error` for top-level handling.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A configuration file could not be parsed.
    #[error("configuration error: {0}")]
    Config(String),

    /// An authentication error occurred.
    #[error("authentication error: {0}")]
    Auth(String),

    /// A network protocol error occurred.
    #[error("network error: {0}")]
    Net(String),

    /// A world registry error occurred.
    #[error("world error: {0}")]
    World(String),

    /// A protocol-level error from the telnet codec.
    #[error("codec error: {0}")]
    Codec(String),
}

/// Convenience alias used throughout the workspace libraries.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn error_display_formatting() {
        let e = Error::Config("missing field".into());
        assert_eq!(e.to_string(), "configuration error: missing field");
    }

    #[test]
    fn codec_error_display() {
        let e = Error::Codec("line too long".into());
        assert_eq!(e.to_string(), "codec error: line too long");
    }

    #[test]
    fn io_error_converts_via_from() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "gone");
        let e: Error = io_err.into();
        assert!(matches!(e, Error::Io(_)));
        assert!(e.to_string().contains("gone"));
    }
}
