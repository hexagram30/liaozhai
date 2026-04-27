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

/// Authentication configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub db_path: String,
    pub max_login_attempts: u32,
    pub argon2_memory_cost: u32,
    pub argon2_time_cost: u32,
    pub argon2_parallelism: u32,
    pub rate_limit_window_secs: u64,
    pub rate_limit_max_failures: usize,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            db_path: "./data/accounts.db".to_owned(),
            max_login_attempts: constants::DEFAULT_MAX_LOGIN_ATTEMPTS,
            argon2_memory_cost: constants::DEFAULT_ARGON2_MEMORY_COST,
            argon2_time_cost: constants::DEFAULT_ARGON2_TIME_COST,
            argon2_parallelism: constants::DEFAULT_ARGON2_PARALLELISM,
            rate_limit_window_secs: constants::DEFAULT_RATE_LIMIT_WINDOW_SECS,
            rate_limit_max_failures: constants::DEFAULT_RATE_LIMIT_MAX_FAILURES,
        }
    }
}

impl AuthConfig {
    pub fn argon2_params(&self) -> liaozhai_auth::params::Argon2Params {
        liaozhai_auth::params::Argon2Params::new(
            self.argon2_memory_cost,
            self.argon2_time_cost,
            self.argon2_parallelism,
        )
    }

    pub fn rate_limit_window(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.rate_limit_window_secs)
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

/// CLI overrides applied on top of the config file.
#[derive(Debug, Default)]
pub struct Overrides {
    pub port: Option<u16>,
    pub bind_address: Option<String>,
}

/// Load configuration from an optional TOML file, with CLI overrides applied.
///
/// Precedence: CLI flags > TOML file > compiled defaults.
///
/// # Errors
///
/// Returns an error if the config file exists but cannot be read or parsed.
pub fn load(config_path: Option<&Path>, overrides: Overrides) -> Result<AppConfig> {
    let mut cfg = match config_path {
        Some(path) => {
            let contents = std::fs::read_to_string(path)
                .with_context(|| format!("reading config file: {}", path.display()))?;
            toml::from_str::<AppConfig>(&contents)
                .with_context(|| format!("parsing config file: {}", path.display()))?
        }
        None => AppConfig::default(),
    };

    if let Some(port) = overrides.port {
        cfg.server.port = port;
    }
    if let Some(bind) = overrides.bind_address {
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
        // Module path on every line. Useful during development; consider
        // false in production if logs feel cluttered.
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
        let cfg = load(None, Overrides::default()).unwrap();
        assert_eq!(cfg.server.port, constants::DEFAULT_PORT);
    }

    #[test]
    fn cli_port_override() {
        let cfg = load(
            None,
            Overrides {
                port: Some(5555),
                ..Overrides::default()
            },
        )
        .unwrap();
        assert_eq!(cfg.server.port, 5555);
    }

    #[test]
    fn cli_bind_override() {
        let cfg = load(
            None,
            Overrides {
                bind_address: Some("0.0.0.0".into()),
                ..Overrides::default()
            },
        )
        .unwrap();
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
