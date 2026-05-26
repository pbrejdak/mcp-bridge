//! `mcp-bridge` command-line entry. Subcommand structure per
//! [`docs/DAEMON.md`] §10.
//!
//! Implemented today: `daemon` (runs the pair endpoint).
//! All other subcommands return a not-implemented error pointing at
//! the roadmap.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use mcp_bridged::{config::Config, daemon, observability};

#[derive(Parser, Debug)]
#[command(name = "mcp-bridge", version, about = "MCP Bridge daemon CLI", long_about = None)]
struct Cli {
    /// Output machine-readable JSON where applicable.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Long-running daemon mode (used by launchd / systemd / Scheduled Task).
    Daemon(DaemonArgs),
    /// Connect to the running daemon and drive a pair via the install token.
    Pair { token: String },
    /// List paired servers.
    List,
    /// Show details for one paired server.
    Show { pin: String },
    /// Revoke a pin, optionally per-Consumer.
    Revoke {
        pin: String,
        consumer: Option<String>,
    },
    /// Print daemon status.
    Status,
    /// Tail or follow daemon logs.
    Logs {
        #[arg(long)]
        follow: bool,
    },
    /// Print a redacted diagnostics bundle to stdout.
    Diagnostics,
    /// Check for updates; optionally apply on next launch.
    Update,
    /// Resolver identity (Ed25519) management.
    #[command(subcommand)]
    Identity(IdentityCommand),
}

#[derive(clap::Args, Debug)]
struct DaemonArgs {
    /// Register the platform launch unit and start.
    #[arg(long, conflicts_with = "uninstall")]
    install: bool,
    /// Reverse `--install`.
    #[arg(long)]
    uninstall: bool,
    /// Override the pair endpoint bind address (default: 127.0.0.1:8765).
    #[arg(long, env = "MCP_BRIDGE_BIND")]
    bind: Option<SocketAddr>,
    /// Override the daemon data directory (default: OS-specific via
    /// `directories::ProjectDirs`).
    #[arg(long, env = "MCP_BRIDGE_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum IdentityCommand {
    /// Rotate the Resolver keypair (invalidates every paired phone).
    Rotate,
    /// Show the current Resolver identity (pubkey + display name).
    Show,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    observability::init();
    let cli = Cli::parse();

    match cli.command {
        Command::Daemon(args) => run_daemon(args).await,
        Command::Pair { .. } => not_implemented("pair"),
        Command::List => not_implemented("list"),
        Command::Show { .. } => not_implemented("show"),
        Command::Revoke { .. } => not_implemented("revoke"),
        Command::Status => not_implemented("status"),
        Command::Logs { .. } => not_implemented("logs"),
        Command::Diagnostics => not_implemented("diagnostics"),
        Command::Update => not_implemented("update"),
        Command::Identity(IdentityCommand::Rotate) => not_implemented("identity rotate"),
        Command::Identity(IdentityCommand::Show) => not_implemented("identity show"),
    }
}

async fn run_daemon(args: DaemonArgs) -> Result<()> {
    if args.install {
        bail!("daemon --install is not implemented yet — see docs/ROADMAP.md Phase 2");
    }
    if args.uninstall {
        bail!("daemon --uninstall is not implemented yet — see docs/ROADMAP.md Phase 2");
    }
    let mut config = Config::defaults().context("resolving default config")?;
    if let Some(addr) = args.bind {
        config.bind_addr = addr;
    }
    if let Some(dir) = args.data_dir {
        config.data_dir = dir;
    }
    daemon::run_with_signal_handler(config)
        .await
        .context("daemon run loop")?;
    Ok(())
}

fn not_implemented(subcommand: &str) -> Result<()> {
    bail!("subcommand `{subcommand}` is not implemented yet — see docs/ROADMAP.md Phase 1");
}
