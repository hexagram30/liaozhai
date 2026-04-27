//! Liaozhai MUX server binary.
//!
//! Entry point: parses CLI arguments, loads configuration, starts the
//! tokio runtime, and delegates to the listener or account management.

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
    /// Path to the TOML configuration file.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the server.
    Run {
        /// TCP port to listen on (overrides config file).
        #[arg(long)]
        port: Option<u16>,

        /// Bind address (overrides config file).
        #[arg(long)]
        bind: Option<String>,
    },

    /// Account management.
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
}

#[derive(Debug, Subcommand)]
enum AccountAction {
    /// Create a new account.
    Create {
        /// The username for the new account.
        username: String,
    },
    /// List all accounts.
    List,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Run { port, bind } => {
            let cfg = config::load(
                cli.config.as_deref(),
                config::Overrides {
                    port,
                    bind_address: bind,
                },
            )
            .context("loading configuration")?;

            config::init_tracing(&cfg.logging);

            info!(
                version = liaozhai_core::constants::VERSION,
                "starting Liaozhai MUX"
            );

            let runtime = tokio::runtime::Runtime::new().context("creating tokio runtime")?;

            runtime.block_on(listener::run(&cfg))?;
        }
        Command::Account { action } => {
            let cfg = config::load(cli.config.as_deref(), config::Overrides::default())
                .context("loading configuration")?;

            let params = cfg.auth.argon2_params();
            let store = liaozhai_auth::store::AccountStore::open(
                std::path::Path::new(&cfg.auth.db_path),
                &params,
            )
            .context("opening account store")?;

            let runtime = tokio::runtime::Runtime::new().context("creating tokio runtime")?;

            match action {
                AccountAction::Create { username } => {
                    let username = username.trim().to_owned();
                    if username.is_empty() {
                        anyhow::bail!("Username cannot be empty.");
                    }

                    let password = read_password("Password: ")?;
                    let confirm = read_password("Confirm:  ")?;

                    if password != confirm {
                        anyhow::bail!("Passwords do not match.");
                    }
                    if password.is_empty() {
                        anyhow::bail!("Password cannot be empty.");
                    }

                    runtime.block_on(async {
                        store
                            .create_account(&username, &password)
                            .await
                            .context("creating account")?;

                        #[expect(clippy::print_stdout)]
                        {
                            println!("Account '{username}' created.");
                        }

                        Ok::<(), anyhow::Error>(())
                    })?;
                }
                AccountAction::List => {
                    runtime.block_on(async {
                        let accounts = store.list_accounts().await.context("listing accounts")?;

                        #[expect(clippy::print_stdout)]
                        {
                            println!(
                                "{:<38} {:<16} {:<21} LAST LOGIN",
                                "ID", "USERNAME", "CREATED"
                            );

                            for account in &accounts {
                                let created = format_timestamp(account.created_at());
                                let last_login = account
                                    .last_login_at()
                                    .map_or_else(|| "(never)".to_owned(), format_timestamp);
                                println!(
                                    "{:<38} {:<16} {:<21} {}",
                                    account.id(),
                                    account.username(),
                                    created,
                                    last_login,
                                );
                            }

                            if accounts.is_empty() {
                                println!("(no accounts)");
                            }
                        }

                        Ok::<(), anyhow::Error>(())
                    })?;
                }
            }
        }
    }

    Ok(())
}

/// Read a password, prompting on the TTY if available, falling back to stdin.
fn read_password(prompt: &str) -> Result<String> {
    use std::io::IsTerminal;

    #[expect(clippy::print_stderr)]
    if std::io::stdin().is_terminal() {
        rpassword::prompt_password(prompt).context("reading password")
    } else {
        eprint!("{prompt}");
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .context("reading password from stdin")?;
        Ok(line
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_owned())
    }
}

fn format_timestamp(epoch_secs: i64) -> String {
    let Ok(dt) = time::OffsetDateTime::from_unix_timestamp(epoch_secs) else {
        return format!("{epoch_secs}");
    };
    let format = time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]")
        .expect("valid format description");
    dt.format(&format)
        .unwrap_or_else(|_| format!("{epoch_secs}"))
}
