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
    fn banner_contains_project_name() {
        assert!(BANNER.contains("Liaozhai MUX"));
        assert!(BANNER.contains("Multi-User eXegesis"));
    }
}
