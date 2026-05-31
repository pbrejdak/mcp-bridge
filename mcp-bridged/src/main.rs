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
    /// Generate a pairing invite and wait for a phone to complete it.
    Pair,
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
        Command::Pair => run_pair(json_output).await,
        Command::List => run_list(json_output).await,
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

    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("status-1"),
        method: ipc::method_names::DAEMON_STATUS.to_owned(),
        params: None,
    };
    let response = call_with_friendly_error(&socket, &request).await?;

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

async fn run_list(json_output: bool) -> Result<()> {
    let config = Config::defaults().context("resolving default config")?;
    let socket = config.ipc_socket_path();

    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("list-1"),
        method: ipc::method_names::SERVERS_LIST.to_owned(),
        params: None,
    };
    let response = call_with_friendly_error(&socket, &request).await?;

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

    let entries: Vec<ipc::ServerListEntry> =
        serde_json::from_value(result).context("decoding servers.list response")?;
    render_servers_table(&entries);
    Ok(())
}

/// `mcp-bridge pair` — generate an invite, print the SAS/URL the user
/// reads to their phone, then poll the registry until the phone
/// completes the POST (or the 60-second invite lifetime expires).
async fn run_pair(json_output: bool) -> Result<()> {
    use std::collections::HashSet;
    use std::time::Duration;

    use mcp_bridged::pair::Invite;
    use mcp_bridged::pair::logical_id::LogicalId;

    let config = Config::defaults().context("resolving default config")?;
    let socket = config.ipc_socket_path();

    // Snapshot the existing pin set so we can spot the new arrival.
    let before: HashSet<LogicalId> = list_pins(&socket).await?.into_iter().map(|e| e.pin_id).collect();

    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("invite-1"),
        method: ipc::method_names::PAIR_INVITE_START.to_owned(),
        params: None,
    };
    let response = call_with_friendly_error(&socket, &request).await?;
    if let Some(err) = response.error {
        bail!("daemon returned error {}: {}", err.code, err.message);
    }
    let result = response
        .result
        .ok_or_else(|| anyhow!("response carried neither result nor error"))?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }

    let invite: Invite =
        serde_json::from_value(result.clone()).context("decoding pair.invite_start response")?;

    if !json_output {
        println!();
        println!("Pair this Bridge");
        println!();
        println!("  Bridge name : {}", invite.resolver.display_name.as_str());
        println!("  LAN address : {}", invite.resolver.lan_addr.as_str());
        println!("  SAS phrase  : {}", invite.resolver.sas.as_str());
        println!();
        println!("On the phone, scan a QR built from this JSON (or paste it directly):");
        println!();
        println!("{}", serde_json::to_string(&invite)?);
        println!();
        println!("Waiting up to 60s for the phone to complete the pair (Ctrl-C to cancel)...");
    }

    // Poll until either (a) a new pin appears with a LID we didn't see
    // before — that's the just-paired phone — or (b) the invite
    // lifetime expires. The InviteRegister cleans the nonce on its own.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(65);
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => {
                bail!("pair invite expired before any phone completed it");
            }
            _ = interval.tick() => {}
        }
        let pins = list_pins(&socket).await?;
        if let Some(new_pin) = pins.into_iter().find(|p| !before.contains(&p.pin_id)) {
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "paired": {
                            "pin_id": new_pin.pin_id,
                            "name": new_pin.name,
                            "backend_url": new_pin.backend_url,
                        }
                    }))?
                );
            } else {
                println!();
                println!("Paired with: {} ({})", new_pin.name, new_pin.pin_id);
                println!("Backend URL: {}", new_pin.backend_url);
            }
            return Ok(());
        }
    }
}

/// Helper used by `run_pair`: fetch the current `servers.list` snapshot.
async fn list_pins(socket: &std::path::Path) -> Result<Vec<ipc::ServerListEntry>> {
    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("list-poll"),
        method: ipc::method_names::SERVERS_LIST.to_owned(),
        params: None,
    };
    let response = call_with_friendly_error(socket, &request).await?;
    if let Some(err) = response.error {
        bail!("daemon returned error {}: {}", err.code, err.message);
    }
    let result = response
        .result
        .ok_or_else(|| anyhow!("response carried neither result nor error"))?;
    serde_json::from_value(result).context("decoding servers.list response")
}

/// Wrap [`ipc::call_local`] with a CLI-friendly error message when the
/// daemon isn't running. On Unix we can fast-path with `socket.exists()`;
/// on Windows the named pipe isn't a filesystem object, so we let the
/// connect attempt fail and translate the I/O error to a hint.
async fn call_with_friendly_error(
    socket: &std::path::Path,
    request: &ipc::JsonRpcRequest,
) -> Result<ipc::JsonRpcResponse> {
    #[cfg(unix)]
    {
        if !socket.exists() {
            bail!(
                "daemon does not appear to be running ({} is missing). \
                 Start it with `mcp-bridge daemon`.",
                socket.display()
            );
        }
    }
    ipc::call_local(socket, request)
        .await
        .map_err(|e| anyhow!(
            "could not reach daemon over IPC: {e}\n\
             Is the daemon running? Start it with `mcp-bridge daemon`."
        ))
}

fn render_servers_table(entries: &[ipc::ServerListEntry]) {
    if entries.is_empty() {
        println!("No paired servers.");
        println!("Pair one with `mcp-bridge pair <token>` (or via the Bridge Console).");
        return;
    }

    let id_width = entries
        .iter()
        .map(|e| e.pin_id.as_str().len())
        .max()
        .unwrap_or(0)
        .max("PIN ID".len());
    let name_width = entries
        .iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(0)
        .max("NAME".len());
    let state_width = entries
        .iter()
        .map(|e| state_label(e.state).len())
        .max()
        .unwrap_or(0)
        .max("STATE".len());

    println!(
        "{:<id_width$}  {:<name_width$}  {:<state_width$}  PAIRED (unix s)",
        "PIN ID", "NAME", "STATE",
    );
    for entry in entries {
        println!(
            "{:<id_width$}  {:<name_width$}  {:<state_width$}  {}",
            entry.pin_id.as_str(),
            entry.name,
            state_label(entry.state),
            entry.created_at,
        );
    }
}

fn state_label(state: mcp_bridged::registry::PinState) -> &'static str {
    use mcp_bridged::registry::PinState;
    match state {
        PinState::Reachable => "reachable",
        PinState::Unreachable => "unreachable",
        PinState::Revoked => "revoked",
    }
}

fn not_implemented(subcommand: &str) -> Result<()> {
    bail!("subcommand `{subcommand}` is not implemented yet — see docs/ROADMAP.md Phase 1");
}
