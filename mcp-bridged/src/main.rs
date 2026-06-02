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
use mcp_bridged::{config::Config, daemon, install, ipc, observability};

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
    Rotate {
        /// Skip the confirmation prompt. Required for non-interactive
        /// use (CI, scripts).
        #[arg(long)]
        yes: bool,
    },
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
        Command::Show { pin } => run_show(pin, json_output).await,
        Command::Revoke { pin, consumer } => run_revoke(pin, consumer, json_output).await,
        Command::Logs { follow } => run_logs(follow, json_output).await,
        Command::Diagnostics => run_diagnostics(json_output).await,
        Command::Update => run_update(json_output).await,
        Command::Identity(IdentityCommand::Rotate { yes }) => {
            run_identity_rotate(yes, json_output).await
        }
        Command::Identity(IdentityCommand::Show) => run_identity_show(json_output).await,
    }
}

async fn run_daemon(args: DaemonArgs) -> Result<()> {
    let mut config = Config::defaults().context("resolving default config")?;
    if let Some(addr) = args.bind {
        config.bind_addr = addr;
    }
    if let Some(dir) = args.data_dir {
        config.data_dir = dir;
    }

    if args.install {
        let report = install::install(&config).context("installing launch unit")?;
        if report.replaced_existing {
            println!("Replaced existing launch unit at {:?}.", report.unit_path);
        } else {
            println!("Installed launch unit at {:?}.", report.unit_path);
        }
        println!("The daemon is now scheduled to start at user login.");
        return Ok(());
    }
    if args.uninstall {
        let report = install::uninstall(&config).context("uninstalling launch unit")?;
        if report.removed {
            println!("Removed launch unit at {:?}.", report.unit_path);
        } else {
            println!(
                "No launch unit was installed at {:?}; nothing to remove.",
                report.unit_path
            );
        }
        return Ok(());
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

/// `mcp-bridge update` — check whether a newer Bridge release is
/// available. Today the daemon returns a not-implemented stub; the
/// real manifest fetch lands when auto-update infrastructure is set
/// up (see `docs/PRIVACY.md` for the egress posture).
async fn run_update(json_output: bool) -> Result<()> {
    let config = Config::defaults().context("resolving default config")?;
    let socket = config.ipc_socket_path();

    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("update-1"),
        method: ipc::method_names::UPDATE_CHECK.to_owned(),
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

    let parsed: ipc::UpdateCheckResult =
        serde_json::from_value(result).context("decoding update.check response")?;
    println!("current  {}", parsed.current);
    if let Some(latest) = &parsed.latest {
        println!("latest   {latest}");
    }
    println!("state    {}", parsed.state);
    println!();
    println!("{}", parsed.message);
    Ok(())
}

/// `mcp-bridge diagnostics` — print a redacted plain-text bundle of
/// daemon state (identity, listening addresses, pin summary, last 50
/// log lines). Safe to paste into a bug report.
async fn run_diagnostics(json_output: bool) -> Result<()> {
    let config = Config::defaults().context("resolving default config")?;
    let socket = config.ipc_socket_path();

    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("diagnostics-1"),
        method: ipc::method_names::DIAGNOSTICS_BUNDLE.to_owned(),
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
    } else {
        let parsed: ipc::DiagnosticsBundleResult =
            serde_json::from_value(result).context("decoding diagnostics.bundle response")?;
        print!("{}", parsed.bundle);
    }
    Ok(())
}

/// `mcp-bridge logs [--follow]` — print recent daemon log entries.
/// Without --follow: prints the last 200 lines and exits. With:
/// polls every second using the largest seq seen so far as the
/// cursor, printing new lines as they arrive (Ctrl-C to stop).
async fn run_logs(follow: bool, json_output: bool) -> Result<()> {
    let config = Config::defaults().context("resolving default config")?;
    let socket = config.ipc_socket_path();

    let initial = fetch_logs(&socket, None).await?;
    let mut cursor = print_events(&initial.events, json_output)?;

    if !follow {
        return Ok(());
    }

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            ctrl = tokio::signal::ctrl_c() => {
                ctrl.map_err(|e| anyhow!("Ctrl-C handler: {e}"))?;
                return Ok(());
            }
            _ = interval.tick() => {}
        }
        let next = fetch_logs(&socket, cursor).await?;
        if let Some(last) = print_events(&next.events, json_output)? {
            cursor = Some(last);
        }
    }
}

/// Helper for `run_logs`: one log.recent call.
async fn fetch_logs(
    socket: &std::path::Path,
    after: Option<u64>,
) -> Result<ipc::LogRecentResult> {
    let params = serde_json::to_value(ipc::LogRecentParams { after, limit: None })?;
    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("logs-1"),
        method: ipc::method_names::LOG_RECENT.to_owned(),
        params: Some(params),
    };
    let response = call_with_friendly_error(socket, &request).await?;
    if let Some(err) = response.error {
        bail!("daemon returned error {}: {}", err.code, err.message);
    }
    let result = response
        .result
        .ok_or_else(|| anyhow!("response carried neither result nor error"))?;
    serde_json::from_value(result).context("decoding log.recent response")
}

/// Render `events` to stdout. Returns the largest seq printed, if any,
/// so the caller can advance the polling cursor.
fn print_events(
    events: &[mcp_bridged::observability::recorder::LogEvent],
    json_output: bool,
) -> Result<Option<u64>> {
    if json_output {
        for e in events {
            println!("{}", serde_json::to_string(e)?);
        }
    } else {
        for e in events {
            println!("[{:>5}] {} {} | {}", e.level, e.timestamp, e.target, e.message);
        }
    }
    Ok(events.last().map(|e| e.seq))
}

/// `mcp-bridge identity rotate` — destructive. Generates a new
/// Resolver keypair, persists it to the keychain, and revokes every
/// existing pin. The running daemon still uses the OLD keypair until
/// restart, so the CLI prints a clear restart-required notice on
/// success.
async fn run_identity_rotate(yes: bool, json_output: bool) -> Result<()> {
    use std::io::Write;

    if !yes {
        eprintln!(
            "WARNING: this generates a new Resolver keypair and REVOKES every paired phone."
        );
        eprintln!("Each phone will need to re-pair from scratch.");
        eprint!("Type `rotate` to confirm: ");
        std::io::stderr().flush().ok();
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .context("reading confirmation")?;
        if answer.trim() != "rotate" {
            bail!("rotate aborted");
        }
    }

    let config = Config::defaults().context("resolving default config")?;
    let socket = config.ipc_socket_path();

    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("rotate-1"),
        method: ipc::method_names::IDENTITY_ROTATE.to_owned(),
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

    let parsed: ipc::IdentityRotateResult =
        serde_json::from_value(result).context("decoding identity.rotate response")?;
    println!("Identity rotated.");
    println!("  new pubkey       {}", parsed.new_pubkey);
    println!("  revoked pins     {}", parsed.revoked_pins);
    if parsed.restart_required {
        println!();
        println!("RESTART REQUIRED: the running daemon still uses the previous keypair.");
        println!("Restart it (e.g. `mcp-bridge daemon --uninstall && mcp-bridge daemon --install`)");
        println!("for the rotated identity to take effect on the wire.");
    }
    Ok(())
}

/// `mcp-bridge identity show` — call identity.show and print the
/// Resolver pubkey + display name. Useful for verifying out-of-band
/// what the phone is about to pin to.
async fn run_identity_show(json_output: bool) -> Result<()> {
    let config = Config::defaults().context("resolving default config")?;
    let socket = config.ipc_socket_path();

    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("identity-show-1"),
        method: ipc::method_names::IDENTITY_SHOW.to_owned(),
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

    let info: ipc::IdentityInfo =
        serde_json::from_value(result).context("decoding identity.show response")?;
    println!("display name  {}", info.display_name.as_str());
    println!("pubkey        {}", info.pubkey);
    Ok(())
}

/// `mcp-bridge show <pin>` — call servers.detail and render the full
/// (secret-free) pin record. Output goes one field per line for human
/// reading; `--json` passes the raw response through.
async fn run_show(pin: String, json_output: bool) -> Result<()> {
    let config = Config::defaults().context("resolving default config")?;
    let socket = config.ipc_socket_path();

    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("show-1"),
        method: ipc::method_names::SERVERS_DETAIL.to_owned(),
        params: Some(serde_json::json!({ "pin_id": pin })),
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

    let str_field = |k: &str| {
        result
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_owned()
    };
    let scope = result.get("scope").and_then(|v| v.as_array()).map_or_else(
        || "?".to_owned(),
        |arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    let opt_u64 = |k: &str| {
        result
            .get(k)
            .and_then(serde_json::Value::as_u64)
            .map_or_else(|| "-".to_owned(), |n| n.to_string())
    };

    println!("PIN ID         {}", str_field("logical_id"));
    println!("NAME           {}", str_field("display_name"));
    println!("STATE          {}", str_field("state"));
    println!("SCOPE          {scope}");
    println!("BACKEND URL    {}", str_field("backend_url"));
    println!("BACKEND FP     {}", str_field("backend_fp"));
    println!("ORIGIN PUBKEY  {}", str_field("origin_pubkey"));
    println!("PAIRED (unix)  {}", opt_u64("created_at"));
    println!("LAST SEEN SEQ  {}", opt_u64("last_seen_seq"));
    println!("AUTH ROTATED   {}", opt_u64("auth_rotated_at"));
    println!("CERT ROTATED   {}", opt_u64("cert_rotated_at"));
    Ok(())
}

/// `mcp-bridge revoke <pin>` — flip the pin to Revoked and tear down
/// the per-pin secrets + adapter entries. Per-Consumer revoke
/// (`consumer` argument) is a Phase 2 ACL feature and is rejected for
/// now with a clear message.
async fn run_revoke(
    pin: String,
    consumer: Option<String>,
    json_output: bool,
) -> Result<()> {
    let config = Config::defaults().context("resolving default config")?;
    let socket = config.ipc_socket_path();

    let mut params = serde_json::Map::new();
    params.insert("pin_id".to_owned(), serde_json::Value::String(pin.clone()));
    if let Some(ref c) = consumer {
        params.insert("consumer".to_owned(), serde_json::Value::String(c.clone()));
    }

    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("revoke-1"),
        method: ipc::method_names::SERVERS_REVOKE.to_owned(),
        params: Some(serde_json::Value::Object(params)),
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

    let pin_id = result["pin_id"].as_str().unwrap_or(&pin);
    if let Some(c) = consumer.as_ref() {
        let removed = result["consumer_removed"].as_str();
        if removed == Some(c.as_str()) {
            println!("Removed {pin_id} from {c}. Pin remains paired.");
        } else {
            println!("No change for {pin_id} ({c}).");
        }
    } else {
        let revoked = result["revoked"].as_bool().unwrap_or(false);
        if revoked {
            println!("Revoked {pin_id}.");
        } else {
            println!("{pin_id} was already revoked. (no-op)");
        }
    }
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
        let invite_json = serde_json::to_string(&invite)?;
        println!();
        println!("Pair this Bridge");
        println!();
        println!("  Bridge name : {}", invite.resolver.display_name.as_str());
        println!("  LAN address : {}", invite.resolver.lan_addr.as_str());
        println!("  SAS phrase  : {}", invite.resolver.sas.as_str());
        println!();
        println!("Scan the QR with the phone:");
        println!();
        println!("{}", render_qr(&invite_json));
        println!("Or paste the raw invite JSON into the phone:");
        println!();
        println!("{invite_json}");
        println!();
        println!("Waiting up to 60s for the phone to complete the pair (Ctrl-C to cancel)...");
    }

    // Poll until either (a) a new pin appears with a LID we didn't see
    // before — that's the just-paired phone — (b) the invite lifetime
    // expires, or (c) the user hits Ctrl-C. The ctrl_c arm sends a
    // pair.invite_cancel so the nonce is released immediately instead
    // of sitting in the register for the rest of its lifetime.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(65);
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => {
                cancel_invite(&socket, invite.nonce).await;
                bail!("pair invite expired before any phone completed it");
            }
            ctrl = tokio::signal::ctrl_c() => {
                if let Err(e) = ctrl {
                    return Err(anyhow!("failed to install Ctrl-C handler: {e}"));
                }
                cancel_invite(&socket, invite.nonce).await;
                bail!("pair aborted by user (Ctrl-C)");
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

/// Best-effort `pair.invite_cancel` call. Failure to reach the daemon
/// just means the invite will expire on its own in ≤60s, so we log
/// and move on rather than failing the abort path.
async fn cancel_invite(socket: &std::path::Path, nonce: mcp_bridged::pair::nonce::Nonce) {
    let request = ipc::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!("cancel-1"),
        method: ipc::method_names::PAIR_INVITE_CANCEL.to_owned(),
        params: Some(serde_json::json!({ "nonce": nonce })),
    };
    if let Err(e) = ipc::call_local(socket, &request).await {
        tracing::debug!(error = ?e, "pair.invite_cancel call failed (invite will expire on its own)");
    }
}

/// Render `payload` as a Unicode-block QR code suitable for terminal
/// display. Uses Dense1x2 blocks (one terminal cell = two QR modules
/// stacked) so the result fits comfortably on an 80-column screen for
/// invites of the size we generate.
fn render_qr(payload: &str) -> String {
    use qrcode::QrCode;
    use qrcode::render::unicode::Dense1x2;

    match QrCode::new(payload.as_bytes()) {
        Ok(code) => code
            .render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .quiet_zone(true)
            .build(),
        Err(e) => format!("(could not render QR: {e})"),
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

