//! `mcp-bridge` command-line entry. Subcommand structure per
//! [`docs/DAEMON.md`] §10. Bodies are not implemented at this revision —
//! see [`docs/ROADMAP.md`] Phase 1.

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

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
}

#[derive(Subcommand, Debug)]
enum IdentityCommand {
    /// Rotate the Resolver keypair (invalidates every paired phone).
    Rotate,
    /// Show the current Resolver identity (pubkey + display name).
    Show,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Phase 1 scaffold: subcommand shapes are real; no behaviour wired yet.
    match cli.command {
        Command::Daemon(args) => not_implemented(if args.install {
            "daemon --install"
        } else if args.uninstall {
            "daemon --uninstall"
        } else {
            "daemon"
        }),
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

fn not_implemented(subcommand: &str) -> Result<()> {
    bail!("subcommand `{subcommand}` is not implemented yet — see docs/ROADMAP.md Phase 1");
}
