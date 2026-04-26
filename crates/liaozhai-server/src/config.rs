//! Configuration loading: TOML file + CLI overrides + defaults.

use std::path::Path;

use anyhow::{Context, Result};
use liaozhai_core::constants;
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

/// Top-level server configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub worlds: WorldsConfig,
    pub logging: LoggingConfig,
}

/// Network listener configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind_address: String,
    pub port: u16,
    pub max_connections: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: constants::DEFAULT_BIND_ADDRESS.to_owned(),
            port: constants::DEFAULT_PORT,
            max_connections: constants::DEFAULT_MAX_CONNECTIONS,
        }
    }
}

/// Authentication configuration (M4 placeholder).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub db_path: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            db_path: "./data/accounts.db".to_owned(),
        }
    }
}

/// World registry configuration (M5 placeholder).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WorldsConfig {
    pub registry_path: String,
}

impl Default for WorldsConfig {
    fn default() -> Self {
        Self {
            registry_path: "./data/worlds.toml".to_owned(),
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub default_filter: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            default_filter: constants::DEFAULT_LOG_FILTER.to_owned(),
        }
    }
}

/// Load configuration from an optional TOML file, with CLI overrides applied.
///
/// Precedence: CLI flags > TOML file > compiled defaults.
///
/// # Errors
///
/// Returns an error if the config file exists but cannot be read or parsed.
pub fn load(
    config_path: Option<&Path>,
    port_override: Option<u16>,
    bind_override: Option<String>,
) -> Result<AppConfig> {
    let mut cfg = match config_path {
        Some(path) => {
            let contents = std::fs::read_to_string(path)
                .with_context(|| format!("reading config file: {}", path.display()))?;
            toml::from_str::<AppConfig>(&contents)
                .with_context(|| format!("parsing config file: {}", path.display()))?
        }
        None => AppConfig::default(),
    };

    if let Some(port) = port_override {
        cfg.server.port = port;
    }
    if let Some(bind) = bind_override {
        cfg.server.bind_address = bind;
    }

    Ok(cfg)
}

/// Initialize the `tracing` subscriber.
///
/// Respects `RUST_LOG` if set; otherwise uses the config file's default filter.
pub fn init_tracing(logging: &LoggingConfig) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&logging.default_filter));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.server.port, 4444);
        assert_eq!(cfg.server.bind_address, "127.0.0.1");
        assert_eq!(cfg.server.max_connections, 100);
        assert_eq!(cfg.logging.default_filter, "info");
    }

    #[test]
    fn load_without_file_returns_defaults() {
        let cfg = load(None, None, None).unwrap();
        assert_eq!(cfg.server.port, constants::DEFAULT_PORT);
    }

    #[test]
    fn cli_port_override() {
        let cfg = load(None, Some(5555), None).unwrap();
        assert_eq!(cfg.server.port, 5555);
    }

    #[test]
    fn cli_bind_override() {
        let cfg = load(None, None, Some("0.0.0.0".into())).unwrap();
        assert_eq!(cfg.server.bind_address, "0.0.0.0");
    }

    #[test]
    fn toml_deserialization() {
        let toml_str = r#"
            [server]
            port = 9999
            bind_address = "10.0.0.1"

            [logging]
            default_filter = "debug"
        "#;
        let cfg: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.server.port, 9999);
        assert_eq!(cfg.server.bind_address, "10.0.0.1");
        assert_eq!(cfg.logging.default_filter, "debug");
        // Unspecified sections get defaults
        assert_eq!(cfg.auth.db_path, "./data/accounts.db");
    }
}
