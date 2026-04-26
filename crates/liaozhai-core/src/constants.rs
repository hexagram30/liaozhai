//! Shared constants: version, banner, network defaults.

/// Project version, sourced from `Cargo.toml` at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Connection banner shown to clients on accept.
///
/// Uses `concat!` + `env!` so the version is embedded at compile time
/// with no runtime allocation.
pub const BANNER: &str = concat!(
    "\r\n  Liaozhai MUX \u{804a}\u{9f4b} \u{2014} v",
    env!("CARGO_PKG_VERSION"),
    "\r\n  Multi-User eXegesis\r\n\r\n",
);

/// Default TCP port.
pub const DEFAULT_PORT: u16 = 4444;

/// Default bind address.
pub const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1";

/// Default maximum concurrent connections.
pub const DEFAULT_MAX_CONNECTIONS: usize = 100;

/// Default logging filter.
pub const DEFAULT_LOG_FILTER: &str = "info";

/// Maximum length of a single input line, in bytes (post-IAC-stripping).
pub const MAX_LINE_LENGTH: usize = 4096;

/// Maximum per-connection buffer size before forced disconnect, in bytes.
pub const MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024; // 10 MB

/// Message sent to client when a line exceeds `MAX_LINE_LENGTH`.
pub const LINE_TOO_LONG_MSG: &str = "Line too long; ignored.\r\n";

/// Message sent to client when buffer exceeds `MAX_BUFFER_SIZE`.
pub const BUFFER_OVERFLOW_MSG: &str = "Buffer overflow; disconnecting.\r\n";

/// Goodbye message sent on session-terminating command.
pub const GOODBYE_MSG: &str = "Until the next strange tale.\r\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn banner_contains_version() {
        assert!(BANNER.contains(VERSION));
    }

    #[test]
    fn max_line_length_is_4096() {
        assert_eq!(MAX_LINE_LENGTH, 4096);
    }

    #[test]
    fn max_buffer_size_is_10mb() {
        assert_eq!(MAX_BUFFER_SIZE, 10 * 1024 * 1024);
    }

    #[test]
    fn banner_contains_project_name() {
        assert!(BANNER.contains("Liaozhai MUX"));
        assert!(BANNER.contains("Multi-User eXegesis"));
    }
}
