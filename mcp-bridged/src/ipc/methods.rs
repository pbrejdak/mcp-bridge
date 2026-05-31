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
use crate::identity::{DisplayName, Ed25519Pubkey, Keystore};
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
    pub const PAIR_INVITE_START: &str = "pair.invite_start";
    pub const SERVERS_REVOKE: &str = "servers.revoke";
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
    /// Resolver Ed25519 pubkey carried in every invite.
    pub resolver_pubkey: Ed25519Pubkey,
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
}

/// Result body of [`method_names::DAEMON_STATUS`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version: String,
    pub uptime_seconds: u64,
    pub pin_count: usize,
    pub pair_endpoint: String,
}

/// Request body for [`method_names::SERVERS_REVOKE`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersRevokeParams {
    /// Logical_id of the pin to revoke.
    pub pin_id: LogicalId,
}

/// Result body for [`method_names::SERVERS_REVOKE`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersRevokeResult {
    pub pin_id: LogicalId,
    /// True when the pin existed and was transitioned to Revoked. False
    /// when the pin existed but was already Revoked (no-op).
    pub revoked: bool,
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
            match revoke_pin(ctx, params.pin_id).await {
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

/// Failure modes from [`revoke_pin`] reused by the dispatcher arm.
#[derive(Debug)]
enum RevokeError {
    UnknownPin,
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
        ctx.resolver_pubkey,
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
            resolver_pubkey: *Keypair::generate().pubkey(),
            display_name: DisplayName::new("Test Bridge").unwrap(),
            invites,
            keystore,
            registry_path,
            sentinel: Sentinel::random(),
            adapters: Arc::new(Vec::new()),
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
