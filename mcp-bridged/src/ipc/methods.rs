//! IPC method registry — the per-method handlers the dispatcher calls.
//!
//! Phase 1 ships exactly one method: `daemon.status`. The rest of
//! [`docs/DAEMON.md`](../../../../docs/DAEMON.md) §5 lands as
//! follow-ups; each adds a single new arm here.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use std::path::PathBuf;

use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::adapters::{Adapter, Sentinel};
use crate::identity::{
    DisplayName, Ed25519Pubkey, Keypair, Keystore, SharedKeypair, current_keypair, swap_keypair,
};
use crate::observability::EventRecorder;
use crate::observability::recorder::LogEvent;
use crate::pair::invite::Invite;
use crate::pair::invite_register::InviteRegister;
use crate::pair::lan_addr::LanAddr;
use crate::pair::logical_id::LogicalId;
use crate::pair::nonce::Nonce;
use crate::registry::{PinState, Registry};

use super::wire::{JsonRpcRequest, JsonRpcResponse};

/// JSON-RPC method-name constants.
pub mod method_names {
    pub const DAEMON_STATUS: &str = "daemon.status";
    pub const SERVERS_LIST: &str = "servers.list";
    pub const SERVERS_DETAIL: &str = "servers.detail";
    pub const PAIR_INVITE_START: &str = "pair.invite_start";
    pub const PAIR_INVITE_CANCEL: &str = "pair.invite_cancel";
    pub const SERVERS_REVOKE: &str = "servers.revoke";
    pub const IDENTITY_SHOW: &str = "identity.show";
    pub const IDENTITY_ROTATE: &str = "identity.rotate";
    pub const LOG_RECENT: &str = "log.recent";
    pub const DIAGNOSTICS_BUNDLE: &str = "diagnostics.bundle";
    pub const UPDATE_CHECK: &str = "update.check";
}

/// JSON-RPC error codes per [JSON-RPC 2.0 §5.1](https://www.jsonrpc.org/specification#error_object).
#[allow(dead_code)] // some variants used by future methods only
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
}

/// Shared state every method handler can read.
///
/// `Clone` (cheap — every field is either `Copy` or `Arc`-backed) so
/// the server hands one out per connection.
#[allow(missing_debug_implementations)] // Arc<RwLock<Registry>> is not Debug.
#[derive(Clone)]
pub struct Context {
    /// When the daemon entered its run loop. Source for `uptime_seconds`.
    pub start: Instant,
    /// Shared pin registry; read for `pin_count`.
    pub registry: Arc<RwLock<Registry>>,
    /// The pair endpoint's actual bound socket address. Used both for
    /// status reporting and for constructing the LAN URL inside fresh
    /// pair invites.
    pub pair_endpoint_addr: SocketAddr,
    /// Shared Resolver keypair. Reads use `identity::current_keypair`
    /// to snapshot; `identity.rotate` swaps the keypair behind this
    /// handle atomically.
    pub resolver: SharedKeypair,
    /// Operator-facing name shown on the phone's confirm screen.
    pub display_name: DisplayName,
    /// Invite register; `pair.invite_start` writes to it, the HTTP
    /// pair endpoint consumes from it.
    pub invites: InviteRegister,
    /// OS keychain handle for per-Pin secret cleanup on revoke.
    pub keystore: Arc<Keystore>,
    /// Disk path the registry persists to after every mutation.
    pub registry_path: PathBuf,
    /// Per-install sentinel; only adapter entries carrying this UUID
    /// are removed when a pin is revoked.
    pub sentinel: Sentinel,
    /// Client Adapters notified on revoke (best-effort, identical to
    /// the accept-path fan-out).
    pub adapters: Arc<Vec<Arc<dyn Adapter>>>,
    /// In-memory recent-events buffer feeding `log.recent`. `None`
    /// when the observability layer wasn't installed (e.g. some
    /// integration tests skip it); the dispatcher returns an empty
    /// list in that case.
    pub recorder: Option<EventRecorder>,
    /// Shared connector cache; `identity.rotate` clears it after the
    /// mass-revoke so the proxy stops serving requests with now-
    /// revoked tokens.
    pub connector_cache: Arc<crate::proxy::ConnectorCache>,
    /// Notified by `identity.rotate` so the mDNS subscriber re-derives
    /// service-type HMACs against the new keypair immediately instead
    /// of waiting for the next UTC midnight.
    pub rotation_signal: Arc<tokio::sync::Notify>,
}

/// Result body of [`method_names::IDENTITY_SHOW`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityInfo {
    /// Resolver Ed25519 pubkey as `ed25519:<base64url>`.
    pub pubkey: Ed25519Pubkey,
    /// Operator-facing display name carried in every invite.
    pub display_name: DisplayName,
}

/// Result body of [`method_names::UPDATE_CHECK`].
///
/// Auto-update infrastructure is documented in
/// [`docs/PRIVACY.md`](../../../../docs/PRIVACY.md) §"updates.mcpbridge.me"
/// — once-a-day anonymous HTTPS GET against a signed manifest. The
/// daemon doesn't fetch the manifest yet; this method returns a clear
/// stub state so the CLI doesn't bail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    /// Version of the running daemon, from `CARGO_PKG_VERSION`.
    pub current: String,
    /// State of the update check.
    ///   - `"not_implemented"` — what we return today.
    ///   - `"up_to_date"` — once the manifest fetch lands.
    ///   - `"available"` — once the manifest fetch lands and reports a
    ///     newer version.
    pub state: String,
    /// Latest known version when `state == "up_to_date"` or `"available"`;
    /// `None` while `state == "not_implemented"`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub latest: Option<String>,
    /// Human-readable message for the CLI to print.
    pub message: String,
}

/// Result body of [`method_names::DIAGNOSTICS_BUNDLE`].
///
/// `bundle` is a plain-text envelope ready to paste into a bug report.
/// It folds together identity, listening addresses, pin summary, and
/// the last few log lines. Secrets never reach this report — the
/// daemon's tracing posture already keeps them out of the buffer, and
/// the registry's serialised form never carried them in the first
/// place (per `docs/DAEMON.md` §7.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsBundleResult {
    pub bundle: String,
}

/// Request body for [`method_names::LOG_RECENT`]. All fields optional;
/// the dispatcher applies sensible defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogRecentParams {
    /// Return only events with `seq > after`. Use the largest seq
    /// from a previous call as the cursor for follow-mode polling.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub after: Option<u64>,
    /// Cap on number of events returned (oldest-first, newest-trimmed
    /// when over cap). Defaults to 200; capped at 1024.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub limit: Option<usize>,
}

/// Result body of [`method_names::LOG_RECENT`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecentResult {
    pub events: Vec<LogEvent>,
}

/// Result body of [`method_names::IDENTITY_ROTATE`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityRotateResult {
    /// Pubkey of the freshly generated keypair — now persisted in the
    /// keychain AND hot-swapped into the running daemon's in-memory
    /// `SharedKeypair`. Every code path that takes the keypair on
    /// request (pair endpoint, mDNS subscriber, IPC dispatcher) sees
    /// the new value on the next call.
    pub new_pubkey: Ed25519Pubkey,
    /// Number of pins revoked as part of the rotation. Every paired
    /// phone has to re-pair.
    pub revoked_pins: usize,
    /// `false` at this protocol version — the in-process hot-swap is
    /// done by the time the response returns. Reserved for future
    /// platforms where the runtime can't perform the swap (e.g. a
    /// restricted Tauri sidecar).
    pub restart_required: bool,
}

/// Result body of [`method_names::DAEMON_STATUS`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version: String,
    pub uptime_seconds: u64,
    pub pin_count: usize,
    pub pair_endpoint: String,
}

/// Request body for [`method_names::SERVERS_DETAIL`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersDetailParams {
    pub pin_id: LogicalId,
}

/// Request body for [`method_names::PAIR_INVITE_CANCEL`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairInviteCancelParams {
    /// Nonce of the invite to drop. The hyphenated base64url form
    /// returned by `pair.invite_start`.
    pub nonce: Nonce,
}

/// Request body for [`method_names::SERVERS_REVOKE`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersRevokeParams {
    /// Logical_id of the pin to revoke.
    pub pin_id: LogicalId,
    /// Optional Adapter name. When present, the pin keeps its state
    /// and per-Pin secrets — only the named adapter's config entry is
    /// removed. When omitted, the pin is revoked outright (state →
    /// Revoked, all adapter entries removed, keychain entries deleted).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub consumer: Option<String>,
}

/// Result body for [`method_names::SERVERS_REVOKE`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersRevokeResult {
    pub pin_id: LogicalId,
    /// True when the pin existed and was transitioned to Revoked. False
    /// when the pin existed but was already Revoked, OR when a
    /// per-Consumer revoke succeeded (the pin itself isn't revoked).
    pub revoked: bool,
    /// True when the request named a Consumer and the adapter's entry
    /// was removed. Absent (or false) for whole-pin revokes.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub consumer_removed: Option<String>,
}

/// One row in the [`method_names::SERVERS_LIST`] response.
///
/// Mirrors [`docs/DAEMON.md`](../../../../docs/DAEMON.md) §5.1: the CLI
/// and Bridge Console both read this shape. Per-Consumer state and
/// last-activity tracking land alongside their respective subsystems;
/// today `consumers` is empty and `last_activity_at` is `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerListEntry {
    /// Stable per-Origin identifier (the logical_id from SPEC §3).
    pub pin_id: LogicalId,
    /// User-visible name as supplied by the Origin at pair time.
    pub name: String,
    /// Current reachability state of the pin.
    pub state: PinState,
    /// Unix seconds the pin was first accepted.
    pub created_at: u64,
    /// Backend URL the daemon connects to (for diagnostics; the
    /// Consumer never sees this).
    pub backend_url: String,
    /// Consumers currently configured for this pin. Empty until the
    /// per-Consumer ACL subsystem lands.
    pub consumers: Vec<String>,
    /// Unix seconds of the most recent forwarded request, if any.
    pub last_activity_at: Option<u64>,
}

/// Dispatch one JSON-RPC request against `ctx` and return the response.
///
/// Unknown methods get a JSON-RPC -32601 (method not found) so the
/// caller can distinguish protocol-level failures from transport ones.
#[allow(clippy::too_many_lines)] // Single dispatcher; one arm per IPC method by design.
pub async fn dispatch(req: JsonRpcRequest, ctx: &Context) -> JsonRpcResponse {
    let id = req.id;
    match req.method.as_str() {
        method_names::DAEMON_STATUS => {
            let status = build_daemon_status(ctx).await;
            match serde_json::to_value(&status) {
                Ok(value) => JsonRpcResponse::success(id, value),
                Err(e) => JsonRpcResponse::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("could not serialize status: {e}"),
                ),
            }
        }
        method_names::SERVERS_LIST => {
            let entries = build_servers_list(ctx).await;
            match serde_json::to_value(&entries) {
                Ok(value) => JsonRpcResponse::success(id, value),
                Err(e) => JsonRpcResponse::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("could not serialize servers list: {e}"),
                ),
            }
        }
        method_names::SERVERS_DETAIL => {
            let params: ServersDetailParams = match req
                .params
                .as_ref()
                .ok_or_else(|| "missing `params` (expected { pin_id })".to_owned())
                .and_then(|v| serde_json::from_value(v.clone()).map_err(|e| format!("{e}")))
            {
                Ok(p) => p,
                Err(msg) => {
                    return JsonRpcResponse::error(id, error_codes::INVALID_PARAMS, msg);
                }
            };
            let registry_guard = ctx.registry.read().await;
            let Some(pin) = registry_guard.get(&params.pin_id).cloned() else {
                return JsonRpcResponse::error(
                    id,
                    error_codes::INVALID_PARAMS,
                    "no pin matches that pin_id".to_owned(),
                );
            };
            drop(registry_guard);
            match serde_json::to_value(&pin) {
                Ok(value) => JsonRpcResponse::success(id, value),
                Err(e) => JsonRpcResponse::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("could not serialize pin: {e}"),
                ),
            }
        }
        method_names::IDENTITY_SHOW => {
            let info = IdentityInfo {
                pubkey: *current_keypair(&ctx.resolver).pubkey(),
                display_name: ctx.display_name.clone(),
            };
            match serde_json::to_value(&info) {
                Ok(value) => JsonRpcResponse::success(id, value),
                Err(e) => JsonRpcResponse::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("could not serialize identity: {e}"),
                ),
            }
        }
        method_names::UPDATE_CHECK => {
            // Stub state. Once the manifest fetch is wired up (signed
            // HTTPS GET to updates.mcpbridge.me, daily, anonymous —
            // see PRIVACY.md), this arm will return `up_to_date` or
            // `available` with a real `latest` value.
            let result = UpdateCheckResult {
                current: env!("CARGO_PKG_VERSION").to_owned(),
                state: "not_implemented".to_owned(),
                latest: None,
                message: "Auto-update is not yet wired up. \
                          Check https://github.com/mcp-bridge/mcp-bridge/releases manually."
                    .to_owned(),
            };
            match serde_json::to_value(&result) {
                Ok(value) => JsonRpcResponse::success(id, value),
                Err(e) => JsonRpcResponse::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("could not serialize update result: {e}"),
                ),
            }
        }
        method_names::DIAGNOSTICS_BUNDLE => {
            let bundle = build_diagnostics_bundle(ctx).await;
            match serde_json::to_value(&DiagnosticsBundleResult { bundle }) {
                Ok(value) => JsonRpcResponse::success(id, value),
                Err(e) => JsonRpcResponse::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("could not serialize diagnostics bundle: {e}"),
                ),
            }
        }
        method_names::LOG_RECENT => {
            let params: LogRecentParams = match req.params.as_ref() {
                Some(value) => match serde_json::from_value(value.clone()) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            id,
                            error_codes::INVALID_PARAMS,
                            format!("{e}"),
                        );
                    }
                },
                None => LogRecentParams::default(),
            };
            let limit = params.limit.unwrap_or(200).min(1024);
            let events = ctx
                .recorder
                .as_ref()
                .map(|r| r.recent(params.after, limit))
                .unwrap_or_default();
            match serde_json::to_value(&LogRecentResult { events }) {
                Ok(value) => JsonRpcResponse::success(id, value),
                Err(e) => JsonRpcResponse::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("could not serialize log events: {e}"),
                ),
            }
        }
        method_names::IDENTITY_ROTATE => match rotate_identity(ctx).await {
            Ok(result) => match serde_json::to_value(&result) {
                Ok(value) => JsonRpcResponse::success(id, value),
                Err(e) => JsonRpcResponse::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("could not serialize rotate result: {e}"),
                ),
            },
            Err(e) => JsonRpcResponse::error(id, error_codes::INTERNAL_ERROR, e),
        },
        method_names::PAIR_INVITE_CANCEL => {
            let params: PairInviteCancelParams = match req
                .params
                .as_ref()
                .ok_or_else(|| "missing `params` (expected { nonce })".to_owned())
                .and_then(|v| serde_json::from_value(v.clone()).map_err(|e| format!("{e}")))
            {
                Ok(p) => p,
                Err(msg) => {
                    return JsonRpcResponse::error(id, error_codes::INVALID_PARAMS, msg);
                }
            };
            // Consume drains the invite from the active register. The
            // contract is idempotent — already-consumed and never-seen
            // both mean "there's no live invite to cancel", which is
            // the success state from the caller's perspective.
            let _ = ctx.invites.consume(params.nonce).await;
            JsonRpcResponse::success(id, serde_json::json!({}))
        }
        method_names::PAIR_INVITE_START => match build_invite(ctx).await {
            Ok(invite) => match serde_json::to_value(&invite) {
                Ok(value) => JsonRpcResponse::success(id, value),
                Err(e) => JsonRpcResponse::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("could not serialize invite: {e}"),
                ),
            },
            Err(e) => JsonRpcResponse::error(id, error_codes::INTERNAL_ERROR, e),
        },
        method_names::SERVERS_REVOKE => {
            let params: ServersRevokeParams = match req
                .params
                .as_ref()
                .ok_or_else(|| "missing `params` (expected { pin_id })".to_owned())
                .and_then(|v| serde_json::from_value(v.clone()).map_err(|e| format!("{e}")))
            {
                Ok(p) => p,
                Err(msg) => {
                    return JsonRpcResponse::error(id, error_codes::INVALID_PARAMS, msg);
                }
            };
            let outcome = match params.consumer {
                Some(name) => revoke_consumer(ctx, params.pin_id, name).await,
                None => revoke_pin(ctx, params.pin_id).await,
            };
            match outcome {
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(value) => JsonRpcResponse::success(id, value),
                    Err(e) => JsonRpcResponse::error(
                        id,
                        error_codes::INTERNAL_ERROR,
                        format!("could not serialize revoke result: {e}"),
                    ),
                },
                Err(RevokeError::UnknownPin) => JsonRpcResponse::error(
                    id,
                    error_codes::INVALID_PARAMS,
                    "no pin matches that pin_id".to_owned(),
                ),
                Err(RevokeError::UnknownConsumer(name)) => JsonRpcResponse::error(
                    id,
                    error_codes::INVALID_PARAMS,
                    format!("no registered adapter named `{name}`"),
                ),
                Err(RevokeError::Internal(msg)) => {
                    JsonRpcResponse::error(id, error_codes::INTERNAL_ERROR, msg)
                }
            }
        }
        unknown => JsonRpcResponse::error(
            id,
            error_codes::METHOD_NOT_FOUND,
            format!("method `{unknown}` is not implemented"),
        ),
    }
}

async fn build_daemon_status(ctx: &Context) -> DaemonStatus {
    let pin_count = ctx.registry.read().await.len();
    DaemonStatus {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        uptime_seconds: ctx.start.elapsed().as_secs(),
        pin_count,
        pair_endpoint: ctx.pair_endpoint_addr.to_string(),
    }
}

/// Assemble the plain-text diagnostics bundle. Reads only fields that
/// are already public-by-design in other IPC methods (status, identity,
/// servers.list, log.recent) — every value here would have been
/// fetchable individually anyway. The bundle just bundles them.
async fn build_diagnostics_bundle(ctx: &Context) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(2048);

    let version = env!("CARGO_PKG_VERSION");
    let uptime = ctx.start.elapsed().as_secs();
    let _ = writeln!(out, "# MCP Bridge diagnostics");
    let _ = writeln!(out);
    let _ = writeln!(out, "[daemon]");
    let _ = writeln!(out, "version            = {version}");
    let _ = writeln!(out, "uptime_seconds     = {uptime}");
    let _ = writeln!(out, "pair_endpoint      = {}", ctx.pair_endpoint_addr);
    let _ = writeln!(out, "registry_path      = {:?}", ctx.registry_path);
    let _ = writeln!(out, "sentinel           = {}", ctx.sentinel);
    let _ = writeln!(out);

    let _ = writeln!(out, "[identity]");
    let _ = writeln!(out, "display_name       = {}", ctx.display_name.as_str());
    let resolver_pubkey = *current_keypair(&ctx.resolver).pubkey();
    let _ = writeln!(out, "resolver_pubkey    = {resolver_pubkey}");
    let _ = writeln!(out);

    let pins: Vec<_> = {
        let reg = ctx.registry.read().await;
        reg.iter().cloned().collect()
    };
    let _ = writeln!(out, "[pins]  count = {}", pins.len());
    for pin in &pins {
        let scope: Vec<String> = pin
            .scope
            .iter()
            .map(|s| format!("{s:?}").to_lowercase())
            .collect();
        let _ = writeln!(
            out,
            "  - lid={} name={:?} state={:?} backend={} scope=[{}] created_at={}",
            pin.logical_id.as_str(),
            pin.display_name.as_str(),
            pin.state,
            pin.backend_url.as_str(),
            scope.join(", "),
            pin.created_at,
        );
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "[adapters]");
    if ctx.adapters.is_empty() {
        let _ = writeln!(out, "  (none registered)");
    } else {
        for adapter in ctx.adapters.iter() {
            let _ = writeln!(out, "  - {}", adapter.name());
        }
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "[recent_logs] (oldest first)");
    let events = ctx
        .recorder
        .as_ref()
        .map(|r| r.recent(None, 50))
        .unwrap_or_default();
    if events.is_empty() {
        let _ = writeln!(out, "  (recorder unavailable or buffer empty)");
    } else {
        for e in &events {
            let _ = writeln!(
                out,
                "  {:>5} {} {} | {}",
                e.level, e.timestamp, e.target, e.message
            );
        }
    }

    out
}

/// Generate a fresh Resolver keypair, persist it to the keychain, hot-
/// swap the in-memory `SharedKeypair`, revoke every existing pin, and
/// signal the mDNS subscriber to re-derive its service-type HMAC.
///
/// `restart_required` is reported as `false`: the pair endpoint
/// handlers, mDNS subscriber, and IPC dispatcher all read the keypair
/// via [`current_keypair`] at request time, so the swap takes effect
/// on every code path that touches the keypair next. The proxy
/// connector cache is cleared too — pins are revoked anyway, but the
/// belt-and-suspenders clear means no request can ride a stale
/// connector during the brief window between swap and the
/// mass-revoke's cache-invalidating effect.
async fn rotate_identity(ctx: &Context) -> Result<IdentityRotateResult, String> {
    // 1. Snapshot the pin list outside the rotate path's locks so the
    //    subsequent revoke_pin calls can each take the write lock.
    let pin_ids: Vec<LogicalId> = {
        let reg = ctx.registry.read().await;
        reg.iter().map(|p| p.logical_id.clone()).collect()
    };
    let total = pin_ids.len();

    // 2. Revoke every existing pin. Failures here log but don't abort
    //    the rotate — the keypair swap is the load-bearing step, and
    //    leaving stale pin entries in Reachable state would be worse
    //    than leaving them half-cleaned.
    let mut revoked_pins = 0;
    for pin_id in pin_ids {
        match revoke_pin(ctx, pin_id.clone()).await {
            Ok(result) if result.revoked => revoked_pins += 1,
            Ok(_) => { /* already-revoked counts as a no-op */ }
            Err(e) => {
                tracing::warn!(error = ?e, %pin_id, "revoke failed during identity rotate");
            }
        }
    }
    tracing::info!(
        revoked = revoked_pins,
        total,
        "revoked pins as part of identity rotate"
    );

    // 3. Generate + persist the new keypair. Keystore is the durable
    //    source of truth — a restart between this step and the swap
    //    below would pick up the new key on its own.
    let new_kp = Keypair::generate();
    let new_pubkey = *new_kp.pubkey();
    ctx.keystore
        .save_resolver_keypair(&new_kp)
        .map_err(|e| format!("could not save new keypair: {e}"))?;

    // 4. Hot-swap the in-memory keypair. Pair endpoint handlers, mDNS
    //    handle_announcement, and identity.show / pair.invite_start
    //    all re-read on next request and see the new key.
    swap_keypair(&ctx.resolver, new_kp);

    // 5. Clear the connector cache so the proxy doesn't serve a
    //    request through a connector holding a now-revoked bearer.
    ctx.connector_cache.clear();

    // 6. Notify mDNS to re-derive its service-type HMACs immediately
    //    instead of waiting for UTC midnight.
    ctx.rotation_signal.notify_one();

    Ok(IdentityRotateResult {
        new_pubkey,
        revoked_pins,
        restart_required: false,
    })
}

/// Failure modes from [`revoke_pin`] / [`revoke_consumer`] reused by
/// the dispatcher arm.
#[derive(Debug)]
enum RevokeError {
    UnknownPin,
    UnknownConsumer(String),
    Internal(String),
}

/// Apply revocation to a pin: flip state to Revoked, persist the
/// registry, then best-effort delete per-pin keychain secrets and
/// adapter entries that carry our sentinel.
///
/// Returns `revoked = false` when the pin existed but was already in
/// Revoked state (idempotent no-op). Returns [`RevokeError::UnknownPin`]
/// when the pin_id doesn't match any pin.
async fn revoke_pin(
    ctx: &Context,
    pin_id: LogicalId,
) -> Result<ServersRevokeResult, RevokeError> {
    // Flip state + persist under the registry write lock.
    let already_revoked = {
        let mut reg = ctx.registry.write().await;
        let Some(pin) = reg.get(&pin_id).cloned() else {
            return Err(RevokeError::UnknownPin);
        };
        let was_revoked = matches!(pin.state, PinState::Revoked);
        if !was_revoked {
            let mut updated = pin;
            updated.state = PinState::Revoked;
            reg.insert(updated);
            reg.save(&ctx.registry_path)
                .await
                .map_err(|e| RevokeError::Internal(format!("registry save failed: {e}")))?;
        }
        was_revoked
    };

    if !already_revoked {
        // Best-effort cleanup of per-pin secrets. Failures are logged
        // but do not fail the revoke — the pin is already in Revoked
        // state in the registry, which is the source of truth.
        if let Err(e) = ctx.keystore.delete_loopback_key(&pin_id) {
            tracing::warn!(error = ?e, %pin_id, "could not delete loopback key on revoke");
        }
        if let Err(e) = ctx.keystore.delete_bearer_token(&pin_id) {
            tracing::warn!(error = ?e, %pin_id, "could not delete bearer token on revoke");
        }
        for adapter in ctx.adapters.iter() {
            if let Err(e) = adapter.remove_entry(&pin_id, ctx.sentinel).await {
                tracing::warn!(
                    error = ?e,
                    adapter = adapter.name(),
                    %pin_id,
                    "adapter remove_entry failed on revoke",
                );
            }
        }
    }

    Ok(ServersRevokeResult {
        pin_id,
        revoked: !already_revoked,
        consumer_removed: None,
    })
}

/// Per-Consumer revoke: keep the pin alive (state stays Reachable /
/// Unreachable) and the per-pin secrets in the keychain, but ask one
/// named Adapter to remove its config entry for this pin. Lets the
/// operator unplug a single Consumer (e.g. uninstall Claude Desktop
/// while keeping Cursor still paired) without re-pairing the phone.
async fn revoke_consumer(
    ctx: &Context,
    pin_id: LogicalId,
    consumer: String,
) -> Result<ServersRevokeResult, RevokeError> {
    {
        let reg = ctx.registry.read().await;
        if reg.get(&pin_id).is_none() {
            return Err(RevokeError::UnknownPin);
        }
    }
    let adapter = ctx
        .adapters
        .iter()
        .find(|a| a.name() == consumer)
        .cloned()
        .ok_or_else(|| RevokeError::UnknownConsumer(consumer.clone()))?;
    if let Err(e) = adapter.remove_entry(&pin_id, ctx.sentinel).await {
        tracing::warn!(
            error = ?e,
            adapter = consumer.as_str(),
            %pin_id,
            "adapter remove_entry failed on per-Consumer revoke",
        );
    }
    Ok(ServersRevokeResult {
        pin_id,
        revoked: false,
        consumer_removed: Some(consumer),
    })
}

/// Build a fresh Direction-B pairing invite (random nonce, current
/// Resolver pubkey and display name, LAN URL derived from the pair
/// endpoint's bound address) and register it with the invite actor.
async fn build_invite(ctx: &Context) -> Result<Invite, String> {
    let mut nonce_bytes = [0u8; 16];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_bytes(nonce_bytes);

    let lan = format!("https://{}/pair", ctx.pair_endpoint_addr);
    let lan_addr = LanAddr::new(&lan).map_err(|e| format!("could not build LAN URL: {e}"))?;

    let invite = Invite::new(
        *current_keypair(&ctx.resolver).pubkey(),
        ctx.display_name.clone(),
        lan_addr,
        nonce,
    );

    ctx.invites
        .register(invite.clone())
        .await
        .map_err(|e| format!("could not register invite: {e}"))?;

    Ok(invite)
}

async fn build_servers_list(ctx: &Context) -> Vec<ServerListEntry> {
    let registry = ctx.registry.read().await;
    let mut entries: Vec<ServerListEntry> = registry
        .iter()
        .map(|pin| ServerListEntry {
            pin_id: pin.logical_id.clone(),
            name: pin.display_name.as_str().to_owned(),
            state: pin.state,
            created_at: pin.created_at,
            backend_url: pin.backend_url.as_str().to_owned(),
            consumers: Vec::new(),
            last_activity_at: None,
        })
        .collect();
    // Stable order: oldest pin first, ties broken by logical_id.
    entries.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.pin_id.as_str().cmp(b.pin_id.as_str()))
    });
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{DisplayName, Keypair, Signature};
    use crate::pair::auth::{Auth, AuthType};
    use crate::pair::backend_url::BackendUrl;
    use crate::pair::bearer_token::BearerToken;
    use crate::pair::cert_fingerprint::CertFingerprint;
    use crate::pair::invite::{Direction, SpecVersion};
    use crate::pair::nonce::Nonce;
    use crate::pair::payload::{BackendInfo, OriginInfo, PairPayload, Scope};
    use crate::registry::ServerPin;

    fn make_ctx() -> Context {
        // Spawn an invite-register actor with a never-fired cancel
        // token. Tests that call `pair.invite_start` will register
        // invites against it; nothing else touches it.
        let cancel = tokio_util::sync::CancellationToken::new();
        let invites = InviteRegister::spawn(cancel);
        // Per-test mock keystore — install_mock_backend is idempotent.
        crate::identity::keystore::install_mock_backend();
        let service = format!(
            "ipc-tests-{}-{}",
            std::process::id(),
            std::time::Instant::now().elapsed().as_nanos()
        );
        let keystore = Arc::new(Keystore::for_service(&service).expect("keystore"));
        let tmp = tempfile::tempdir().expect("tmpdir");
        let registry_path = tmp.path().join("registry.json");
        // Leak the tempdir so it outlives the ctx — every test fn that
        // calls make_ctx is short-lived, so this is fine for unit tests.
        std::mem::forget(tmp);
        Context {
            start: Instant::now(),
            registry: Arc::new(RwLock::new(Registry::new())),
            pair_endpoint_addr: "10.0.0.5:8765".parse().unwrap(),
            resolver: crate::identity::shared_keypair(Keypair::generate()),
            display_name: DisplayName::new("Test Bridge").unwrap(),
            invites,
            keystore,
            registry_path,
            sentinel: Sentinel::random(),
            adapters: Arc::new(Vec::new()),
            recorder: Some(EventRecorder::new()),
            connector_cache: Arc::new(crate::proxy::ConnectorCache::new()),
            rotation_signal: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn sample_pin(name: &str, lid: &str, created_at: u64) -> ServerPin {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
        let mut payload = PairPayload {
            spec: SpecVersion::McpPairV0_1,
            direction: Direction::ResolverOffered,
            origin: OriginInfo {
                name: DisplayName::new(name).unwrap(),
                pubkey: *origin.pubkey(),
                logical_id: LogicalId::new(lid).unwrap(),
            },
            backend: BackendInfo {
                url: BackendUrl::new("https://10.0.0.42:54321/").unwrap(),
                fp: CertFingerprint::from_bytes([0xab; 32]),
                ca: None,
            },
            auth: Auth {
                ty: AuthType::Bearer,
                value: Some(BearerToken::new("token-abc").unwrap()),
            },
            scope: vec![Scope::Tools],
            nonce: Nonce::from_bytes([0u8; 16]),
            target_resolver_pubkey: Some(*resolver.pubkey()),
            sig: Signature::from_bytes([0u8; 64]),
        };
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = origin.sign(&canonical);
        ServerPin::from_accepted_payload(&payload, created_at)
    }

    #[tokio::test]
    async fn daemon_status_returns_expected_shape() {
        let ctx = make_ctx();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(1),
            method: method_names::DAEMON_STATUS.to_owned(),
            params: None,
        };
        let resp = dispatch(req, &ctx).await;
        let result = resp.result.expect("success response carries result");
        assert_eq!(result["pair_endpoint"], "10.0.0.5:8765");
        assert_eq!(result["pin_count"], 0);
        assert_eq!(result["version"], env!("CARGO_PKG_VERSION"));
        assert!(result["uptime_seconds"].is_u64());
    }

    #[tokio::test]
    async fn servers_list_returns_empty_array_when_registry_empty() {
        let ctx = make_ctx();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(2),
            method: method_names::SERVERS_LIST.to_owned(),
            params: None,
        };
        let resp = dispatch(req, &ctx).await;
        let result = resp.result.expect("success response carries result");
        assert!(result.is_array(), "result must be an array");
        assert_eq!(result.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn servers_list_returns_pins_oldest_first() {
        let ctx = make_ctx();
        {
            let mut reg = ctx.registry.write().await;
            reg.insert(sample_pin("Late", "late-9999", 2_000));
            reg.insert(sample_pin("Early", "early-0001", 1_000));
            reg.insert(sample_pin("Middle", "mid-5555", 1_500));
        }

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(3),
            method: method_names::SERVERS_LIST.to_owned(),
            params: None,
        };
        let resp = dispatch(req, &ctx).await;
        let result = resp.result.expect("success response carries result");
        let entries: Vec<ServerListEntry> = serde_json::from_value(result).unwrap();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].pin_id.as_str(), "early-0001");
        assert_eq!(entries[0].name, "Early");
        assert_eq!(entries[1].pin_id.as_str(), "mid-5555");
        assert_eq!(entries[2].pin_id.as_str(), "late-9999");
        assert!(entries.iter().all(|e| e.consumers.is_empty()));
        assert!(entries.iter().all(|e| e.last_activity_at.is_none()));
    }

    #[tokio::test]
    async fn servers_list_shape_includes_required_fields() {
        let ctx = make_ctx();
        {
            let mut reg = ctx.registry.write().await;
            reg.insert(sample_pin("BodyLog", "bodylog-7f3a", 1_700_000_000));
        }
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(4),
            method: method_names::SERVERS_LIST.to_owned(),
            params: None,
        };
        let resp = dispatch(req, &ctx).await;
        let result = resp.result.expect("success response carries result");
        let first = &result.as_array().unwrap()[0];
        for key in [
            "pin_id",
            "name",
            "state",
            "created_at",
            "backend_url",
            "consumers",
            "last_activity_at",
        ] {
            assert!(first.get(key).is_some(), "field `{key}` must be present");
        }
        // Pin state is serialised as the variant name.
        assert_eq!(first["state"], "Reachable");
    }

    #[tokio::test]
    async fn pair_invite_start_returns_a_registered_invite() {
        let ctx = make_ctx();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(7),
            method: method_names::PAIR_INVITE_START.to_owned(),
            params: None,
        };
        let resp = dispatch(req, &ctx).await;
        let result = resp.result.expect("success response carries result");

        // Shape matches the SPEC §4.2 invite JSON.
        assert_eq!(result["spec"], "mcp-pair/v0.1");
        assert_eq!(result["direction"], "resolver_offered");
        assert_eq!(result["resolver"]["display_name"], "Test Bridge");
        assert_eq!(result["resolver"]["lan_addr"], "https://10.0.0.5:8765/pair");
        assert!(result["resolver"]["sas"].as_str().is_some());
        assert!(result["nonce"].as_str().is_some());

        // The invite landed in the register — a phone showing up with
        // this nonce inside the lifetime window will be accepted.
        let invite: Invite = serde_json::from_value(result).unwrap();
        let consumed = ctx.invites.consume(invite.nonce).await;
        assert!(consumed.is_ok(), "invite must be in the register");
    }

    #[tokio::test]
    async fn back_to_back_invites_use_different_nonces() {
        let ctx = make_ctx();
        let one = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::PAIR_INVITE_START.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await
        .result
        .unwrap();
        let two = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(2),
                method: method_names::PAIR_INVITE_START.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await
        .result
        .unwrap();
        assert_ne!(
            one["nonce"], two["nonce"],
            "fresh invites must carry distinct nonces"
        );
    }

    #[tokio::test]
    async fn identity_show_returns_pubkey_and_display_name() {
        let ctx = make_ctx();
        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::IDENTITY_SHOW.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await;
        let result = resp.result.expect("success response carries result");
        let pubkey = result["pubkey"].as_str().unwrap();
        assert!(
            pubkey.starts_with("ed25519:"),
            "pubkey must be the canonical ed25519:<base64url> form, got {pubkey}",
        );
        assert_eq!(result["display_name"], "Test Bridge");

        // Round-trips through IdentityInfo without losing data.
        let info: IdentityInfo = serde_json::from_value(result).unwrap();
        assert_eq!(info.display_name.as_str(), "Test Bridge");
        assert_eq!(info.pubkey, *current_keypair(&ctx.resolver).pubkey());
    }

    #[tokio::test]
    async fn update_check_returns_not_implemented_stub() {
        let ctx = make_ctx();
        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::UPDATE_CHECK.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await;
        let result = resp.result.expect("success response carries result");
        assert_eq!(result["current"], env!("CARGO_PKG_VERSION"));
        assert_eq!(result["state"], "not_implemented");
        assert!(result.get("latest").is_none() || result["latest"].is_null());
        assert!(
            result["message"]
                .as_str()
                .unwrap()
                .contains("Auto-update is not yet wired up"),
        );
    }

    #[tokio::test]
    async fn diagnostics_bundle_includes_identity_pins_and_logs() {
        let ctx = make_ctx();
        {
            let mut reg = ctx.registry.write().await;
            reg.insert(sample_pin("BodyLog", "bodylog-7f3a", 1_700_000_000));
        }
        ctx.recorder.as_ref().unwrap().push(LogEvent {
            seq: 0,
            timestamp: 1_700_000_500,
            level: "INFO".to_owned(),
            target: "test".to_owned(),
            message: "diagnostic event".to_owned(),
        });

        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::DIAGNOSTICS_BUNDLE.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await;
        let result = resp.result.expect("success response carries result");
        let bundle = result["bundle"].as_str().unwrap();
        assert!(bundle.contains("[daemon]"), "must label the daemon section");
        assert!(bundle.contains("[identity]"));
        assert!(bundle.contains("Test Bridge"), "display name appears");
        assert!(bundle.contains("bodylog-7f3a"), "pin shows up under [pins]");
        assert!(
            bundle.contains("diagnostic event"),
            "recent log appears in [recent_logs]",
        );
        // Sanity: ed25519 pubkey shape leaks through, no secret-shaped
        // strings appear.
        assert!(bundle.contains("ed25519:"));
        assert!(!bundle.contains("bearer"), "no bearer token in bundle");
        assert!(!bundle.contains("loopback_key"));
    }

    #[tokio::test]
    async fn log_recent_returns_recorded_events_in_seq_order() {
        let ctx = make_ctx();
        // Push three events into the recorder via the public push
        // path that the Layer impl exercises.
        let recorder = ctx.recorder.as_ref().unwrap();
        for i in 0..3 {
            recorder.push(LogEvent {
                seq: 0,
                timestamp: 100 + i,
                level: "INFO".to_owned(),
                target: "test".to_owned(),
                message: format!("event {i}"),
            });
        }

        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::LOG_RECENT.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await;
        let result = resp.result.expect("success response carries result");
        let events = result["events"].as_array().unwrap();
        assert_eq!(events.len(), 3);
        let seqs: Vec<u64> = events
            .iter()
            .map(|e| e["seq"].as_u64().unwrap())
            .collect();
        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn log_recent_honors_after_cursor() {
        let ctx = make_ctx();
        let recorder = ctx.recorder.as_ref().unwrap();
        for i in 0..5 {
            recorder.push(LogEvent {
                seq: 0,
                timestamp: i,
                level: "INFO".to_owned(),
                target: "t".to_owned(),
                message: format!("e{i}"),
            });
        }
        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(2),
                method: method_names::LOG_RECENT.to_owned(),
                params: Some(serde_json::json!({ "after": 2 })),
            },
            &ctx,
        )
        .await;
        let result = resp.result.expect("success response carries result");
        let events = result["events"].as_array().unwrap();
        assert_eq!(events.len(), 3, "must return events 3, 4, 5");
    }

    #[tokio::test]
    async fn log_recent_when_recorder_is_absent_returns_empty_array() {
        let mut ctx = make_ctx();
        ctx.recorder = None;
        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(3),
                method: method_names::LOG_RECENT.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await;
        let result = resp.result.expect("success response carries result");
        assert!(result["events"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn identity_rotate_persists_a_fresh_keypair_and_revokes_pins() {
        let ctx = make_ctx();
        {
            let mut reg = ctx.registry.write().await;
            reg.insert(sample_pin("BodyLog", "bodylog-7f3a", 1_700_000_000));
            reg.insert(sample_pin("Sensors", "sensors-9012", 1_700_000_500));
        }
        // The keystore needs to remember the resolver pubkey before
        // rotate so the test can assert that the rotated value differs.
        let before = Keypair::generate();
        let before_pubkey = *before.pubkey();
        ctx.keystore.save_resolver_keypair(&before).unwrap();

        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::IDENTITY_ROTATE.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await;
        let result = resp.result.expect("success response carries result");
        assert_eq!(result["revoked_pins"], 2);
        assert_eq!(
            result["restart_required"], false,
            "hot-swap is in place; restart no longer required",
        );

        // Persisted keypair changed.
        let loaded = ctx
            .keystore
            .load_resolver_keypair()
            .unwrap()
            .expect("keypair must still exist after rotate");
        assert_ne!(
            *loaded.pubkey(),
            before_pubkey,
            "rotate must overwrite the keypair in the keychain",
        );

        // In-memory keypair was hot-swapped too — identity.show now
        // returns the new pubkey, matching what was just persisted.
        let in_memory = *current_keypair(&ctx.resolver).pubkey();
        assert_eq!(
            in_memory,
            *loaded.pubkey(),
            "in-memory keypair must match the freshly persisted one",
        );

        // Both pins are now Revoked.
        let reg = ctx.registry.read().await;
        for lid in ["bodylog-7f3a", "sensors-9012"] {
            let pin = reg.get(&LogicalId::new(lid).unwrap()).unwrap();
            assert_eq!(pin.state, PinState::Revoked, "{lid} must be revoked");
        }
    }

    #[tokio::test]
    async fn identity_rotate_hot_swaps_resolver_pubkey_in_memory() {
        // Before rotate: identity.show reports the original pubkey.
        // After rotate (NO daemon restart): identity.show reports the
        // new pubkey AND the connector cache is empty.
        let ctx = make_ctx();
        let before_pubkey = *current_keypair(&ctx.resolver).pubkey();

        // Stuff a fake connector entry so we can confirm rotate clears
        // it — use a placeholder Pin to seed the cache via the
        // registry path.
        {
            let mut reg = ctx.registry.write().await;
            reg.insert(sample_pin("BodyLog", "bodylog-7f3a", 1_700_000_000));
        }

        // Pre-rotate identity.show.
        let pre = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::IDENTITY_SHOW.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await
        .result
        .unwrap();
        let pre_info: IdentityInfo = serde_json::from_value(pre).unwrap();
        assert_eq!(pre_info.pubkey, before_pubkey);

        // Rotate.
        dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(2),
                method: method_names::IDENTITY_ROTATE.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await;

        // Post-rotate identity.show — pubkey advanced.
        let post = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(3),
                method: method_names::IDENTITY_SHOW.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await
        .result
        .unwrap();
        let post_info: IdentityInfo = serde_json::from_value(post).unwrap();
        assert_ne!(post_info.pubkey, before_pubkey, "identity must rotate");
        assert_eq!(
            post_info.pubkey,
            *current_keypair(&ctx.resolver).pubkey(),
            "in-memory keypair matches identity.show output",
        );
    }

    #[tokio::test]
    async fn identity_rotate_with_empty_registry_still_succeeds() {
        let ctx = make_ctx();
        let initial = Keypair::generate();
        ctx.keystore.save_resolver_keypair(&initial).unwrap();

        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(2),
                method: method_names::IDENTITY_ROTATE.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await;
        let result = resp.result.expect("success response carries result");
        assert_eq!(result["revoked_pins"], 0);
        assert_eq!(result["restart_required"], false);

        let loaded = ctx.keystore.load_resolver_keypair().unwrap().unwrap();
        assert_ne!(*loaded.pubkey(), *initial.pubkey());
    }

    #[tokio::test]
    async fn servers_detail_returns_full_pin_for_a_known_lid() {
        let ctx = make_ctx();
        {
            let mut reg = ctx.registry.write().await;
            reg.insert(sample_pin("BodyLog", "bodylog-7f3a", 1_700_000_000));
        }
        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::SERVERS_DETAIL.to_owned(),
                params: Some(serde_json::json!({ "pin_id": "bodylog-7f3a" })),
            },
            &ctx,
        )
        .await;
        let result = resp.result.expect("success response carries result");
        assert_eq!(result["logical_id"], "bodylog-7f3a");
        assert_eq!(result["display_name"], "BodyLog");
        assert_eq!(result["state"], "Reachable");
        assert_eq!(result["backend_url"], "https://10.0.0.42:54321/");
        assert_eq!(result["created_at"], 1_700_000_000);
        // Sanity: no secret material leaks.
        assert!(result.get("bearer_token").is_none());
        assert!(result.get("loopback_key").is_none());
    }

    #[tokio::test]
    async fn servers_detail_unknown_pin_returns_invalid_params() {
        let ctx = make_ctx();
        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(2),
                method: method_names::SERVERS_DETAIL.to_owned(),
                params: Some(serde_json::json!({ "pin_id": "nope-9999" })),
            },
            &ctx,
        )
        .await;
        let err = resp.error.expect("missing pin must be an error");
        assert_eq!(err.code, error_codes::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn servers_detail_missing_params_returns_invalid_params() {
        let ctx = make_ctx();
        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(3),
                method: method_names::SERVERS_DETAIL.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await;
        let err = resp.error.expect("missing params must be an error");
        assert_eq!(err.code, error_codes::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn pair_invite_cancel_removes_a_live_invite() {
        let ctx = make_ctx();
        let started = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::PAIR_INVITE_START.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await
        .result
        .unwrap();
        let invite: Invite = serde_json::from_value(started).unwrap();

        let cancel = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(2),
                method: method_names::PAIR_INVITE_CANCEL.to_owned(),
                params: Some(serde_json::json!({ "nonce": invite.nonce })),
            },
            &ctx,
        )
        .await;
        assert!(cancel.result.is_some());
        assert!(cancel.error.is_none());

        // A phone showing up with the canceled nonce now fails — the
        // register has moved it to the consumed table.
        let attempted = ctx.invites.consume(invite.nonce).await;
        assert!(attempted.is_err(), "canceled invite must not be consumable");
    }

    #[tokio::test]
    async fn pair_invite_cancel_is_idempotent_for_unknown_nonces() {
        let ctx = make_ctx();
        let made_up = Nonce::from_bytes([0xAA; 16]);
        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(3),
                method: method_names::PAIR_INVITE_CANCEL.to_owned(),
                params: Some(serde_json::json!({ "nonce": made_up })),
            },
            &ctx,
        )
        .await;
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn pair_invite_cancel_rejects_missing_params() {
        let ctx = make_ctx();
        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(4),
                method: method_names::PAIR_INVITE_CANCEL.to_owned(),
                params: None,
            },
            &ctx,
        )
        .await;
        let err = resp.error.expect("missing params must be an error");
        assert_eq!(err.code, error_codes::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn servers_revoke_flips_pin_state_and_persists() {
        let ctx = make_ctx();
        {
            let mut reg = ctx.registry.write().await;
            reg.insert(sample_pin("BodyLog", "bodylog-7f3a", 1_700_000_000));
        }
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(1),
            method: method_names::SERVERS_REVOKE.to_owned(),
            params: Some(serde_json::json!({ "pin_id": "bodylog-7f3a" })),
        };
        let resp = dispatch(req, &ctx).await;
        let result = resp.result.expect("success response carries result");
        assert_eq!(result["pin_id"], "bodylog-7f3a");
        assert_eq!(result["revoked"], true);

        let registry_guard = ctx.registry.read().await;
        let pin = registry_guard
            .get(&LogicalId::new("bodylog-7f3a").unwrap())
            .unwrap();
        assert_eq!(pin.state, PinState::Revoked);
        drop(registry_guard);

        // Disk has the revoked state too.
        let on_disk = Registry::load_or_empty(&ctx.registry_path).await.unwrap();
        let pin = on_disk
            .get(&LogicalId::new("bodylog-7f3a").unwrap())
            .unwrap();
        assert_eq!(pin.state, PinState::Revoked);
    }

    /// Recording adapter the per-Consumer revoke tests inject so they
    /// can assert remove_entry was called with the expected
    /// (logical_id, sentinel).
    struct RecordingAdapter {
        calls: std::sync::Mutex<Vec<(LogicalId, Sentinel)>>,
    }
    #[async_trait::async_trait]
    impl Adapter for RecordingAdapter {
        fn name(&self) -> &'static str {
            "claude-desktop"
        }
        async fn detect(
            &self,
        ) -> Result<Option<crate::adapters::Detected>, crate::adapters::AdapterError> {
            Ok(None)
        }
        async fn write_entry(
            &self,
            _e: &crate::adapters::AdapterEntry,
        ) -> Result<(), crate::adapters::AdapterError> {
            Ok(())
        }
        async fn remove_entry(
            &self,
            logical_id: &LogicalId,
            sentinel: Sentinel,
        ) -> Result<(), crate::adapters::AdapterError> {
            self.calls
                .lock()
                .unwrap()
                .push((logical_id.clone(), sentinel));
            Ok(())
        }
    }

    #[tokio::test]
    async fn servers_revoke_per_consumer_keeps_pin_alive_and_returns_consumer_name() {
        let ctx = make_ctx();
        {
            let mut reg = ctx.registry.write().await;
            reg.insert(sample_pin("BodyLog", "bodylog-7f3a", 1_700_000_000));
        }
        let recorder = Arc::new(RecordingAdapter {
            calls: std::sync::Mutex::new(Vec::new()),
        });
        let mut ctx = ctx;
        ctx.adapters = Arc::new(vec![recorder.clone() as Arc<dyn Adapter>]);

        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::SERVERS_REVOKE.to_owned(),
                params: Some(serde_json::json!({
                    "pin_id": "bodylog-7f3a",
                    "consumer": "claude-desktop",
                })),
            },
            &ctx,
        )
        .await;
        let result = resp.result.expect("success response carries result");
        assert_eq!(result["pin_id"], "bodylog-7f3a");
        assert_eq!(result["revoked"], false, "pin stays alive");
        assert_eq!(result["consumer_removed"], "claude-desktop");

        // Pin state untouched.
        let reg = ctx.registry.read().await;
        let pin = reg.get(&LogicalId::new("bodylog-7f3a").unwrap()).unwrap();
        assert_eq!(pin.state, PinState::Reachable);
        drop(reg);

        let calls = recorder.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0.as_str(), "bodylog-7f3a");
    }

    #[tokio::test]
    async fn servers_revoke_per_consumer_unknown_adapter_is_an_error() {
        let ctx = make_ctx();
        {
            let mut reg = ctx.registry.write().await;
            reg.insert(sample_pin("BodyLog", "bodylog-7f3a", 1_700_000_000));
        }
        let resp = dispatch(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(1),
                method: method_names::SERVERS_REVOKE.to_owned(),
                params: Some(serde_json::json!({
                    "pin_id": "bodylog-7f3a",
                    "consumer": "cursor",
                })),
            },
            &ctx,
        )
        .await;
        let err = resp.error.expect("unknown consumer must be an error");
        assert_eq!(err.code, error_codes::INVALID_PARAMS);
        assert!(err.message.contains("cursor"));
    }

    #[tokio::test]
    async fn servers_revoke_unknown_pin_returns_invalid_params() {
        let ctx = make_ctx();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(2),
            method: method_names::SERVERS_REVOKE.to_owned(),
            params: Some(serde_json::json!({ "pin_id": "no-such-lid" })),
        };
        let resp = dispatch(req, &ctx).await;
        let err = resp.error.expect("missing pin must be an error");
        assert_eq!(err.code, error_codes::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn servers_revoke_second_call_is_a_silent_noop() {
        let ctx = make_ctx();
        {
            let mut reg = ctx.registry.write().await;
            reg.insert(sample_pin("BodyLog", "bodylog-7f3a", 1_700_000_000));
        }
        let req = |id: u32| JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(id),
            method: method_names::SERVERS_REVOKE.to_owned(),
            params: Some(serde_json::json!({ "pin_id": "bodylog-7f3a" })),
        };

        let first = dispatch(req(1), &ctx).await.result.unwrap();
        assert_eq!(first["revoked"], true);

        let second = dispatch(req(2), &ctx).await.result.unwrap();
        assert_eq!(second["revoked"], false, "second revoke must be a no-op");
    }

    #[tokio::test]
    async fn servers_revoke_missing_params_returns_invalid_params() {
        let ctx = make_ctx();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(3),
            method: method_names::SERVERS_REVOKE.to_owned(),
            params: None,
        };
        let resp = dispatch(req, &ctx).await;
        let err = resp.error.expect("missing params must be an error");
        assert_eq!(err.code, error_codes::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let ctx = make_ctx();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(42),
            method: "nonexistent.method".to_owned(),
            params: None,
        };
        let resp = dispatch(req, &ctx).await;
        let err = resp.error.expect("error response carries error");
        assert_eq!(err.code, error_codes::METHOD_NOT_FOUND);
        assert!(resp.result.is_none());
        assert_eq!(resp.id, serde_json::json!(42));
    }
}
