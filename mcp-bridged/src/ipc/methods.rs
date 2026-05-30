//! IPC method registry — the per-method handlers the dispatcher calls.
//!
//! Phase 1 ships exactly one method: `daemon.status`. The rest of
//! [`docs/DAEMON.md`](../../../../docs/DAEMON.md) §5 lands as
//! follow-ups; each adds a single new arm here.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::pair::logical_id::LogicalId;
use crate::registry::{PinState, Registry};

use super::wire::{JsonRpcRequest, JsonRpcResponse};

/// JSON-RPC method-name constants.
pub mod method_names {
    pub const DAEMON_STATUS: &str = "daemon.status";
    pub const SERVERS_LIST: &str = "servers.list";
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
    /// The pair endpoint's actual bound socket address.
    pub pair_endpoint_addr: SocketAddr,
}

/// Result body of [`method_names::DAEMON_STATUS`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version: String,
    pub uptime_seconds: u64,
    pub pin_count: usize,
    pub pair_endpoint: String,
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
        Context {
            start: Instant::now(),
            registry: Arc::new(RwLock::new(Registry::new())),
            pair_endpoint_addr: "127.0.0.1:8765".parse().unwrap(),
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
        assert_eq!(result["pair_endpoint"], "127.0.0.1:8765");
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
