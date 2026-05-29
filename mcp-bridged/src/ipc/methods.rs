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

use crate::registry::Registry;

use super::wire::{JsonRpcRequest, JsonRpcResponse};

/// JSON-RPC method-name constants.
pub mod method_names {
    pub const DAEMON_STATUS: &str = "daemon.status";
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx() -> Context {
        Context {
            start: Instant::now(),
            registry: Arc::new(RwLock::new(Registry::new())),
            pair_endpoint_addr: "127.0.0.1:8765".parse().unwrap(),
        }
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
