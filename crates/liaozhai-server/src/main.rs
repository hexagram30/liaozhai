//! Liaozhai MUX server binary.
//!
//! Entry point: parses CLI arguments, loads configuration, starts the
//! tokio runtime, and delegates to the listener.

mod config;
mod listener;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

/// Liaozhai MUX — Multi-User eXegesis server.
#[derive(Debug, Parser)]
#[command(name = "liaozhai-server")]
#[command(about = "Liaozhai MUX server \u{2014} a text-world engine")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the server.
    Run {
        /// Path to the TOML configuration file.
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,

        /// TCP port to listen on (overrides config file).
        #[arg(long)]
        port: Option<u16>,

        /// Bind address (overrides config file).
        #[arg(long)]
        bind: Option<String>,
    },

    /// Account management (M4 placeholder).
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
}

#[derive(Debug, Subcommand)]
enum AccountAction {
    /// Create a new account (M4 placeholder).
    Create {
        /// The username for the new account.
        username: String,
    },
    /// List all accounts (M4 placeholder).
    List,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Run { config, port, bind } => {
            let cfg =
                config::load(config.as_deref(), port, bind).context("loading configuration")?;

            config::init_tracing(&cfg.logging);

            info!(
                version = liaozhai_core::constants::VERSION,
                "starting Liaozhai MUX"
            );

            let runtime = tokio::runtime::Runtime::new().context("creating tokio runtime")?;

            runtime.block_on(listener::run(&cfg))?;
        }
        Command::Account { action } => match action {
            AccountAction::Create { username } => {
                #[expect(clippy::print_stderr)]
                {
                    eprintln!("Account creation not yet implemented (M4). Username: {username}");
                }
            }
            AccountAction::List => {
                #[expect(clippy::print_stderr)]
                {
                    eprintln!("Account listing not yet implemented (M4).");
                }
            }
        },
    }

    Ok(())
}
