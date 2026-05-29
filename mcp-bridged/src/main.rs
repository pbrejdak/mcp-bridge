//! `mcp-bridge` command-line entry. Subcommand structure per
//! [`docs/DAEMON.md`] §10.
//!
//! Implemented today: `daemon` (runs the pair endpoint).
//! All other subcommands return a not-implemented error pointing at
//! the roadmap.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use mcp_bridged::{config::Config, daemon, ipc, observability};

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
    let json_output = cli.json;

    match cli.command {
        Command::Daemon(args) => run_daemon(args).await,
        Command::Status => run_status(json_output).await,
        Command::Pair { .. } => not_implemented("pair"),
        Command::List => not_implemented("list"),
        Command::Show { .. } => not_implemented("show"),
        Command::Revoke { .. } => not_implemented("revoke"),
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

async fn run_status(json_output: bool) -> Result<()> {
    let config = Config::defaults().context("resolving default config")?;
    let socket = config.ipc_socket_path();

    #[cfg(not(unix))]
    {
        let _ = (json_output, socket);
        bail!("`mcp-bridge status` is not yet implemented on this platform");
    }

    #[cfg(unix)]
    {
        if !socket.exists() {
            bail!(
                "daemon does not appear to be running ({} is missing). \
                 Start it with `mcp-bridge daemon`.",
                socket.display()
            );
        }

        let request = ipc::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!("status-1"),
            method: ipc::method_names::DAEMON_STATUS.to_owned(),
            params: None,
        };
        let response = ipc::call_unix(&socket, &request)
            .await
            .map_err(|e| anyhow!("could not reach daemon over IPC: {e}"))?;

        if let Some(err) = response.error {
            bail!("daemon returned error {}: {}", err.code, err.message);
        }
        let result = response
            .result
            .ok_or_else(|| anyhow!("response carried neither result nor error"))?;

        if json_output {
            println!("{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }

        let status: ipc::DaemonStatus =
            serde_json::from_value(result).context("decoding daemon.status response")?;
        println!("mcp-bridged v{}", status.version);
        println!("uptime         {}s", status.uptime_seconds);
        println!("pair endpoint  {}", status.pair_endpoint);
        println!("pin count      {}", status.pin_count);
        Ok(())
    }
}

fn not_implemented(subcommand: &str) -> Result<()> {
    bail!("subcommand `{subcommand}` is not implemented yet — see docs/ROADMAP.md Phase 1");
}
