//! Loopback Listener — inbound `127.0.0.1` HTTP face per
//! [`docs/SPEC.md`](../../../../docs/SPEC.md) §6.
//!
//! Address shape: `http://127.0.0.1:<port>/<logical_id>?key=<256-bit base64url>`
//!
//! Request flow per SPEC §6.2 (rule order matters):
//!   1. **Bind check** — refuses to start unless the bind address is on
//!      a loopback interface.
//!   2. **Host header check** — every request must carry
//!      `Host: 127.0.0.1:<port>` or `Host: localhost:<port>`. Anything
//!      else → `421 Misdirected Request` (DNS-rebinding defense).
//!   3. **Path lookup** — first path segment is the `LogicalId`; the
//!      rest is forwarded to the backend.
//!   4. **Pin lookup** — read the in-memory registry. Unknown LID →
//!      `404`. `PinState::Revoked` → `410`. `PinState::Unreachable` is
//!      not enforced here; the connector failing is what reports it.
//!   5. **Key validation** — constant-time compare against the
//!      loopback key stored in the keychain. Missing or wrong → `401`
//!      with no body (no error detail leaked).
//!   6. **Forward** via a cached `OriginConnector`. Connector errors →
//!      `502 Bad Gateway`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderName, StatusCode, Uri};
use axum::response::Response;
use axum::routing::any;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::identity::{Keystore, KeystoreError};
use crate::pair::logical_id::LogicalId;
use crate::registry::{PinState, Registry, ServerPin};

use super::connector::{ConnectorError, ForwardRequest, OriginConnector};

/// Builder for the loopback listener.
#[allow(missing_debug_implementations)] // Arc<Keystore> is not Debug.
pub struct LoopbackListener {
    /// Where the loopback listener binds. MUST be on a loopback interface;
    /// [`serve`](Self::serve) refuses to start otherwise.
    pub bind_addr: SocketAddr,
    pub registry: Arc<RwLock<Registry>>,
    pub keystore: Arc<Keystore>,
}

impl LoopbackListener {
    /// Bind, serve, and drain on `cancel`. Errors at bind time if
    /// `bind_addr` is not on a loopback interface.
    pub async fn serve(self, cancel: CancellationToken) -> Result<(), ListenerError> {
        if !self.bind_addr.ip().is_loopback() {
            return Err(ListenerError::NotLoopback {
                addr: self.bind_addr,
            });
        }

        let state = Arc::new(AppState {
            bind_addr: self.bind_addr,
            registry: self.registry,
            keystore: self.keystore,
            connectors: Mutex::new(HashMap::new()),
        });

        let app: Router = Router::new().fallback(any(handle)).with_state(state);

        let listener =
            TcpListener::bind(self.bind_addr)
                .await
                .map_err(|e| ListenerError::Bind {
                    addr: self.bind_addr,
                    source: e,
                })?;
        info!(addr = %self.bind_addr, "loopback listener bound");

        axum::serve(listener, app.into_make_service())
            .with_graceful_shutdown(async move {
                cancel.cancelled().await;
                info!("loopback listener shutting down");
            })
            .await
            .map_err(ListenerError::Serve)
    }
}

/// Shared state every request handler reads.
#[allow(missing_debug_implementations)] // Arc<Keystore> + Mutex are not Debug.
struct AppState {
    bind_addr: SocketAddr,
    registry: Arc<RwLock<Registry>>,
    keystore: Arc<Keystore>,
    connectors: Mutex<HashMap<LogicalId, Arc<OriginConnector>>>,
}

/// Per-request handler implementing SPEC §6.2.
async fn handle(State(state): State<Arc<AppState>>, req: Request) -> Response {
    // -- 1. Host header check -----------------------------------------
    if !host_header_matches_bind(&req, state.bind_addr) {
        return empty_response(StatusCode::MISDIRECTED_REQUEST);
    }

    // -- 2. Parse path → LogicalId + remainder ------------------------
    let Some((lid, suffix)) = parse_path(req.uri()) else {
        return empty_response(StatusCode::NOT_FOUND);
    };

    // -- 3. Pin lookup ------------------------------------------------
    let pin = {
        let registry_guard = state.registry.read().await;
        registry_guard.get(&lid).cloned()
    };
    let Some(pin) = pin else {
        return empty_response(StatusCode::NOT_FOUND);
    };
    match pin.state {
        PinState::Revoked => return empty_response(StatusCode::GONE),
        PinState::Reachable | PinState::Unreachable => {}
    }

    // -- 4. Loopback key check ----------------------------------------
    let Some(supplied) = query_value(req.uri(), "key") else {
        return empty_response(StatusCode::UNAUTHORIZED);
    };
    let expected = match state.keystore.load_loopback_key(&lid) {
        Ok(Some(k)) => k,
        Ok(None) => {
            // Pin exists in registry but no loopback key in keychain — a
            // partial-state bug. Treat as 401 so we never leak that the
            // pin exists with the wrong key.
            warn!(logical_id = %lid, "registry has pin but keychain has no loopback key");
            return empty_response(StatusCode::UNAUTHORIZED);
        }
        Err(e) => {
            error!(error = ?e, "keystore load_loopback_key failed");
            return empty_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    if !expected.matches_encoded(&supplied) {
        return empty_response(StatusCode::UNAUTHORIZED);
    }

    // -- 5. Forward via cached connector ------------------------------
    let connector = match get_or_build_connector(&state, &pin) {
        Ok(c) => c,
        Err(GetConnectorError::Keystore(e)) => {
            error!(error = ?e, "keystore load_bearer_token failed");
            return empty_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
        Err(GetConnectorError::MissingBearer) => {
            // Pin present + loopback key matched, but no bearer token.
            // Same partial-state class as above; surface as 502.
            error!(logical_id = %lid, "no bearer token in keychain for pin");
            return empty_response(StatusCode::BAD_GATEWAY);
        }
        Err(GetConnectorError::Build(e)) => {
            error!(error = ?e, "could not build OriginConnector");
            return empty_response(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let (parts, body) = req.into_parts();
    let body_bytes = match axum::body::to_bytes(body, MAX_INBOUND_BODY).await {
        Ok(b) => b.to_vec(),
        Err(e) => {
            warn!(error = ?e, "could not read inbound request body");
            return empty_response(StatusCode::PAYLOAD_TOO_LARGE);
        }
    };

    let forward_req = ForwardRequest {
        method: parts.method.clone(),
        path_and_query: suffix,
        headers: parts.headers,
        body: body_bytes,
    };

    match connector.forward(forward_req).await {
        Ok(resp) => to_axum_response(resp),
        Err(ConnectorError::Send(e)) => {
            warn!(error = ?e, logical_id = %lid, "forward to backend failed");
            empty_response(StatusCode::BAD_GATEWAY)
        }
        Err(e) => {
            error!(error = ?e, "OriginConnector returned unexpected error");
            empty_response(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Cap on the request body we will buffer before forwarding. Mirrors
/// reasonable MCP request shapes (tool arguments + small JSON).
const MAX_INBOUND_BODY: usize = 4 * 1024 * 1024;

fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .expect("empty response always constructible")
}

fn to_axum_response(resp: super::connector::ForwardResponse) -> Response {
    let mut builder = Response::builder().status(resp.status.as_u16());
    for (name, value) in &resp.headers {
        builder = builder.header(name, value);
    }
    builder
        .body(Body::from(Bytes::from(resp.body)))
        .unwrap_or_else(|_| empty_response(StatusCode::INTERNAL_SERVER_ERROR))
}

/// Check the request's `Host` header against our bind address. Accepts
/// the literal IP+port we bound to plus `localhost:<port>` for the
/// loopback case.
fn host_header_matches_bind(req: &Request, bind_addr: SocketAddr) -> bool {
    let Some(host) = req
        .headers()
        .get(HeaderName::from_static("host"))
        .and_then(|h| h.to_str().ok())
    else {
        return false;
    };
    let port = bind_addr.port();
    let ip = bind_addr.ip();
    let acceptable = [format!("{ip}:{port}"), format!("localhost:{port}")];
    acceptable
        .iter()
        .any(|candidate| host.eq_ignore_ascii_case(candidate))
}

/// Split a URI path into `(LogicalId, path_and_query_for_backend)`.
/// `/bodylog` → (`bodylog`, `/`). `/bodylog/tools/call?x=1` →
/// (`bodylog`, `/tools/call?x=1`).
fn parse_path(uri: &Uri) -> Option<(LogicalId, String)> {
    let path = uri.path();
    let mut segments = path.trim_start_matches('/').splitn(2, '/');
    let first = segments.next()?;
    if first.is_empty() {
        return None;
    }
    let lid = LogicalId::new(first).ok()?;
    let rest = segments.next().unwrap_or("");
    let suffix = match (rest.is_empty(), uri.query()) {
        (true, None) => "/".to_owned(),
        (true, Some(q)) => format!("/?{q}"),
        (false, None) => format!("/{rest}"),
        (false, Some(q)) => format!("/{rest}?{q}"),
    };
    Some((lid, suffix))
}

/// Look up a query parameter by exact key. Manual parse — using
/// `axum::extract::Query` would 400 on malformed queries, but we want
/// to map "no `key`" to a clean 401 instead.
fn query_value(uri: &Uri, key: &str) -> Option<String> {
    let query = uri.query()?;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let k = it.next()?;
        let v = it.next().unwrap_or("");
        if k == key {
            return Some(urlencoding_decode(v).unwrap_or_else(|| v.to_owned()));
        }
    }
    None
}

/// Minimal percent-decoding (decode_utf8 wrapper). Returns None on
/// non-UTF-8 input so the caller falls back to the raw form (which the
/// constant-time compare will then reject).
fn urlencoding_decode(s: &str) -> Option<String> {
    // For base64url the only special chars are `-` and `_`, neither
    // percent-encoded in practice. Most clients won't escape the
    // payload at all. This shim covers the rare case.
    percent_encoded(s).ok()
}

fn percent_encoded(s: &str) -> Result<String, std::string::FromUtf8Error> {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                #[allow(clippy::cast_possible_truncation)]
                let byte = ((hi << 4) | lo) as u8;
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out)
}

/// Look up (or lazy-build + cache) the connector for one pin.
fn get_or_build_connector(
    state: &Arc<AppState>,
    pin: &ServerPin,
) -> Result<Arc<OriginConnector>, GetConnectorError> {
    {
        let cache = state.connectors.lock().expect("connectors mutex");
        if let Some(c) = cache.get(&pin.logical_id) {
            return Ok(c.clone());
        }
    }

    let bearer = state
        .keystore
        .load_bearer_token(&pin.logical_id)
        .map_err(GetConnectorError::Keystore)?
        .ok_or(GetConnectorError::MissingBearer)?;

    let connector = Arc::new(
        OriginConnector::new(pin.backend_url.clone(), pin.backend_fp, bearer)
            .map_err(GetConnectorError::Build)?,
    );

    let mut cache = state.connectors.lock().expect("connectors mutex");
    let entry = cache.entry(pin.logical_id.clone()).or_insert(connector);
    Ok(entry.clone())
}

enum GetConnectorError {
    Keystore(KeystoreError),
    MissingBearer,
    Build(ConnectorError),
}

/// Failure modes for [`LoopbackListener::serve`].
#[derive(Debug, Error)]
pub enum ListenerError {
    #[error("loopback bind address {addr} is not a loopback IP; refusing to start")]
    NotLoopback { addr: SocketAddr },
    #[error("could not bind TCP socket at {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("axum serve loop failed: {0}")]
    Serve(#[source] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_path_extracts_lid_and_empty_suffix() {
        let uri: Uri = "/bodylog".parse().unwrap();
        let (lid, suffix) = parse_path(&uri).unwrap();
        assert_eq!(lid.as_str(), "bodylog");
        assert_eq!(suffix, "/");
    }

    #[test]
    fn parse_path_extracts_lid_and_subpath() {
        let uri: Uri = "/bodylog/tools/call".parse().unwrap();
        let (lid, suffix) = parse_path(&uri).unwrap();
        assert_eq!(lid.as_str(), "bodylog");
        assert_eq!(suffix, "/tools/call");
    }

    #[test]
    fn parse_path_preserves_query_string() {
        let uri: Uri = "/bodylog?key=ABCDEF".parse().unwrap();
        let (lid, suffix) = parse_path(&uri).unwrap();
        assert_eq!(lid.as_str(), "bodylog");
        assert_eq!(suffix, "/?key=ABCDEF");
    }

    #[test]
    fn parse_path_rejects_empty_lid() {
        let uri: Uri = "/".parse().unwrap();
        assert!(parse_path(&uri).is_none());
    }

    #[test]
    fn parse_path_rejects_invalid_lid_chars() {
        let uri: Uri = "/BodyLog/whatever".parse().unwrap();
        // Capital letters are not in the LID alphabet.
        assert!(parse_path(&uri).is_none());
    }

    #[test]
    fn query_value_finds_present_key() {
        let uri: Uri = "/x?key=ABCDEF&other=1".parse().unwrap();
        assert_eq!(query_value(&uri, "key"), Some("ABCDEF".to_owned()));
    }

    #[test]
    fn query_value_returns_none_when_absent() {
        let uri: Uri = "/x?other=1".parse().unwrap();
        assert!(query_value(&uri, "key").is_none());
    }

    #[test]
    fn query_value_returns_none_with_no_query_string() {
        let uri: Uri = "/x".parse().unwrap();
        assert!(query_value(&uri, "key").is_none());
    }
}
