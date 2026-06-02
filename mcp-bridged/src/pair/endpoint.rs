//! HTTPS pair endpoint — terminates Direction-B sealed POSTs.
//!
//! Architecturally:
//!
//! ```text
//! phone POST https://<lan_addr>/pair  (sealed body)
//!              │
//!              ▼
//!     axum-rustls HTTPS server (this module)
//!              │ Bytes
//!              ▼
//!     pair::accept_direction_b
//!              │ PairPayload (validated) | AcceptError
//!              ▼
//!     204 No Content                       400 Bad Request (no body)
//! ```
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §5.3 and §4.5 rules 6-10:
//! responses leak no error detail to network observers — every failure is
//! a bare `400 Bad Request`. The redaction layer in `observability/` will
//! later capture richer reasons in local logs only.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Once};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum_server::tls_rustls::RustlsConfig;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::adapters::{Adapter, AdapterEntry, Sentinel};
use crate::announce::{AnnounceRateLimiter, apply_announce, parse_sealed_announce};
use crate::identity::{Keypair, Keystore, SelfSignedCert};
use crate::pair::accept::accept_direction_b;
use crate::pair::backend_verifier::BackendVerifier;
use crate::pair::invite_register::InviteRegister;
use crate::pair::loopback_key::LoopbackKey;
use crate::pair::token_refresh::BearerTokenRefresher;
use crate::proxy::ConnectorCache;
use crate::registry::{Registry, ServerPin};

/// Builder for the pair endpoint.
#[allow(missing_debug_implementations)] // Arc<dyn BackendVerifier> is not Debug.
pub struct PairEndpoint {
    pub bind_addr: SocketAddr,
    pub cert: SelfSignedCert,
    pub resolver: Arc<Keypair>,
    pub invites: InviteRegister,
    pub backend_verifier: Arc<dyn BackendVerifier>,
    /// In-memory pin registry. Writes happen here on every successful
    /// pair; reads from elsewhere (proxy hot path in a follow-up commit).
    pub registry: Arc<RwLock<Registry>>,
    /// Disk path where the registry persists after each accept.
    pub registry_path: PathBuf,
    /// OS keychain handle. The handler writes the per-Pin bearer token
    /// and loopback key here after each successful accept.
    pub keystore: Arc<Keystore>,
    /// Public address Consumers connect to (the loopback listener's
    /// bind address). Used to build the per-Pin loopback URL handed
    /// to adapters.
    pub loopback_addr: SocketAddr,
    /// Per-install sentinel UUID stamped onto every adapter-written
    /// entry so revoke / uninstall touches only Bridge-managed entries.
    pub sentinel: Sentinel,
    /// Client Adapters to notify on successful pair. Failures here are
    /// logged but do not fail the pair — the registry is the source of
    /// truth, adapter writes are downstream conveniences.
    pub adapters: Arc<Vec<Arc<dyn Adapter>>>,
    /// Per-IP + per-LID rate limiter applied to the announce endpoint
    /// (SPEC §5.6).
    pub announce_rate_limiter: Arc<AnnounceRateLimiter>,
    /// Process-wide proxy connector cache; invalidated on accepted
    /// announces that signal bearer-token rotation so the next request
    /// rebuilds with the new token (SPEC §5.7).
    pub connector_cache: Arc<ConnectorCache>,
    /// Concrete implementation of the SPEC §5.7 control call that
    /// fetches a new bearer token from the Origin's backend.
    pub token_refresher: Arc<dyn BearerTokenRefresher>,
}

/// Install the `ring` rustls crypto provider as the process default.
///
/// rustls 0.23 ships with neither `ring` nor `aws-lc-rs` enabled by
/// default; a provider must be installed before any TLS work happens.
/// Idempotent — safe to call from every entry point in the daemon.
pub fn ensure_crypto_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("rustls ring provider installs at process startup");
    });
}

impl PairEndpoint {
    /// Bind the TCP socket and return a [`BoundEndpoint`] that exposes
    /// the chosen `local_addr` (useful when binding to port 0). The
    /// caller can then register invites referring to that exact address
    /// before starting to serve.
    pub fn bind(self) -> Result<BoundEndpoint, ServeError> {
        ensure_crypto_provider();
        let listener = std::net::TcpListener::bind(self.bind_addr).map_err(ServeError::Bind)?;
        listener.set_nonblocking(true).map_err(ServeError::Bind)?;
        let local_addr = listener.local_addr().map_err(ServeError::Bind)?;
        Ok(BoundEndpoint {
            listener,
            local_addr,
            cert_der: self.cert.cert_der.clone(),
            key_der: self.cert.key_der.to_vec(),
            resolver: self.resolver,
            invites: self.invites,
            backend_verifier: self.backend_verifier,
            registry: self.registry,
            registry_path: self.registry_path,
            keystore: self.keystore,
            loopback_addr: self.loopback_addr,
            sentinel: self.sentinel,
            adapters: self.adapters,
            announce_rate_limiter: self.announce_rate_limiter,
            connector_cache: self.connector_cache,
            token_refresher: self.token_refresher,
        })
    }
}

/// A bound (but not yet serving) pair endpoint. Drop or [`serve`](Self::serve).
#[allow(missing_debug_implementations)] // Arc<dyn BackendVerifier> is not Debug.
pub struct BoundEndpoint {
    listener: std::net::TcpListener,
    /// The socket the endpoint is bound to. When `PairEndpoint::bind_addr`
    /// asks for port 0, this reflects the kernel-assigned port.
    pub local_addr: SocketAddr,
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
    resolver: Arc<Keypair>,
    invites: InviteRegister,
    backend_verifier: Arc<dyn BackendVerifier>,
    registry: Arc<RwLock<Registry>>,
    registry_path: PathBuf,
    keystore: Arc<Keystore>,
    loopback_addr: SocketAddr,
    sentinel: Sentinel,
    adapters: Arc<Vec<Arc<dyn Adapter>>>,
    announce_rate_limiter: Arc<AnnounceRateLimiter>,
    connector_cache: Arc<ConnectorCache>,
    token_refresher: Arc<dyn BearerTokenRefresher>,
}

impl BoundEndpoint {
    /// Serve until `cancel` is signalled. Returns when the server has
    /// finished its graceful-shutdown drain.
    pub async fn serve(self, cancel: CancellationToken) -> Result<(), ServeError> {
        let tls = RustlsConfig::from_der(vec![self.cert_der], self.key_der)
            .await
            .map_err(ServeError::Tls)?;

        let state = Arc::new(AppState {
            local_addr: self.local_addr,
            resolver: self.resolver,
            invites: self.invites,
            backend_verifier: self.backend_verifier,
            registry: self.registry,
            registry_path: self.registry_path,
            keystore: self.keystore,
            loopback_addr: self.loopback_addr,
            sentinel: self.sentinel,
            adapters: self.adapters,
            announce_rate_limiter: self.announce_rate_limiter,
            connector_cache: self.connector_cache,
            token_refresher: self.token_refresher,
        });

        let app: Router = Router::new()
            .route("/pair", post(handle_pair))
            .route("/announce", post(handle_announce))
            .with_state(state);

        let handle = axum_server::Handle::new();
        let handle_for_shutdown = handle.clone();
        tokio::spawn(async move {
            cancel.cancelled().await;
            handle_for_shutdown.graceful_shutdown(None);
        });

        axum_server::from_tcp_rustls(self.listener, tls)
            .handle(handle)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .map_err(ServeError::Serve)
    }
}

/// Per-request state shared with axum handlers.
struct AppState {
    local_addr: SocketAddr,
    resolver: Arc<Keypair>,
    invites: InviteRegister,
    backend_verifier: Arc<dyn BackendVerifier>,
    registry: Arc<RwLock<Registry>>,
    registry_path: PathBuf,
    keystore: Arc<Keystore>,
    loopback_addr: SocketAddr,
    sentinel: Sentinel,
    adapters: Arc<Vec<Arc<dyn Adapter>>>,
    announce_rate_limiter: Arc<AnnounceRateLimiter>,
    connector_cache: Arc<ConnectorCache>,
    token_refresher: Arc<dyn BearerTokenRefresher>,
}

async fn handle_pair(State(state): State<Arc<AppState>>, body: Bytes) -> StatusCode {
    let Ok(payload) = accept_direction_b(
        &body,
        state.resolver.as_ref(),
        &state.invites,
        state.backend_verifier.as_ref(),
        state.local_addr,
    )
    .await
    else {
        return StatusCode::BAD_REQUEST;
    };

    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let pin = ServerPin::from_accepted_payload(&payload, created_at);
    let lid = pin.logical_id.clone();

    // Persist the per-Pin secrets to the keychain BEFORE writing the
    // registry. The registry references these secrets indirectly (by
    // LID); if the keychain write fails the registry entry would point
    // at material that does not exist.
    let loopback_key = LoopbackKey::random();
    if let Err(e) = state.keystore.save_loopback_key(&lid, &loopback_key) {
        error!(error = ?e, logical_id = %lid, "keystore loopback-key save failed");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    if let Some(token) = payload.auth.value.as_ref() {
        if let Err(e) = state.keystore.save_bearer_token(&lid, token) {
            error!(error = ?e, logical_id = %lid, "keystore bearer-token save failed");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }

    // Insert + persist under a single write lock so concurrent pair
    // accepts can't interleave their saves.
    let display_name = pin.display_name.clone();
    let mut reg = state.registry.write().await;
    reg.insert(pin);
    let save_result = reg.save(&state.registry_path).await;
    drop(reg);

    if let Err(e) = save_result {
        // The pin is in memory but not on disk. The user thinks they
        // succeeded but a restart loses the pairing. Surface 5xx so
        // the client knows to retry.
        error!(error = ?e, logical_id = %lid, "registry save failed after pair accept");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    let url = format!(
        "http://{}/{}?key={}",
        state.loopback_addr,
        lid,
        loopback_key.encoded(),
    );
    let entry = AdapterEntry {
        logical_id: lid.clone(),
        display_name,
        url,
        sentinel: state.sentinel,
    };
    for adapter in state.adapters.iter() {
        if let Err(e) = adapter.write_entry(&entry).await {
            warn!(
                error = ?e,
                adapter = adapter.name(),
                logical_id = %lid,
                "adapter write_entry failed (pair still succeeded)",
            );
        }
    }

    info!(logical_id = %lid, "pair accepted and persisted");
    StatusCode::NO_CONTENT
}

/// SPEC §5.3 HTTP-POST announce carrier. The body is a libsodium
/// `crypto_box`-sealed announce payload addressed to the Resolver's
/// pubkey. Success returns 204 No Content; every failure returns a
/// bare `400 Bad Request` with no body (matching the pair endpoint's
/// information-hiding posture).
///
/// Pre-signature rate-limit drops happen in two stages per SPEC §5.6:
/// first per source IP (cheap), then per LID after unseal but before
/// the Ed25519 verify.
async fn handle_announce(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> StatusCode {
    if !state.announce_rate_limiter.check_ip(peer.ip()) {
        tracing::debug!(peer = %peer.ip(), "announce rejected: per-IP rate limit");
        return StatusCode::BAD_REQUEST;
    }

    let payload = match parse_sealed_announce(&body, state.resolver.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!(error = ?e, "announce rejected at parse");
            return StatusCode::BAD_REQUEST;
        }
    };

    if !state.announce_rate_limiter.check_lid(&payload.lid) {
        tracing::debug!(
            logical_id = %payload.lid,
            "announce rejected: per-LID rate limit",
        );
        return StatusCode::BAD_REQUEST;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let accepted = match apply_announce(payload, &state.registry, now).await {
        Ok(a) => a,
        Err(e) => {
            tracing::debug!(error = ?e, "announce rejected");
            return StatusCode::BAD_REQUEST;
        }
    };

    // accept_announce mutated the in-memory registry under its write
    // lock and returned. Persist the new state to disk; failure here is
    // the same partial-state class as a successful pair that can't be
    // saved — surface 5xx so the Origin retries on the next interval.
    let registry_guard = state.registry.read().await;
    let save_result = registry_guard.save(&state.registry_path).await;
    drop(registry_guard);
    if let Err(e) = save_result {
        error!(error = ?e, logical_id = %accepted.logical_id, "registry save failed after announce accept");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    if accepted.auth_rotated {
        // SPEC §5.7: re-fetch the bearer token, persist it, invalidate
        // the proxy's pooled connector. Best-effort — failures here
        // log warn but the announce itself stays accepted (the seq +
        // any fp ratchet already committed under the registry write).
        let registry = state.registry.clone();
        let keystore = state.keystore.clone();
        let cache = state.connector_cache.clone();
        let refresher = state.token_refresher.clone();
        let lid = accepted.logical_id.clone();
        tokio::spawn(async move {
            run_token_rotation(lid, registry, keystore, cache, refresher).await;
        });
    }

    info!(
        logical_id = %accepted.logical_id,
        seq = accepted.new_seq,
        fp_changed = accepted.fp_changed,
        auth_rotated = accepted.auth_rotated,
        "announce accepted",
    );
    StatusCode::NO_CONTENT
}

/// Execute the SPEC §5.7 bearer-token rotation for one pin:
///   1. Read the current pin (for backend URL + fp) and bearer token.
///   2. Call the refresher's control call against the pinned backend.
///   3. Save the returned bearer to the keychain.
///   4. Invalidate the proxy connector cache so the next request
///      rebuilds with the fresh token.
///
/// Public so the mDNS-bridge task in [`crate::announce::mdns`] can
/// share the implementation. Failures at any step log warn and return —
/// the rotation will retry on the next announce that asserts
/// `auth_rotated=true`.
pub(crate) async fn run_token_rotation(
    pin_id: crate::pair::logical_id::LogicalId,
    registry: Arc<RwLock<Registry>>,
    keystore: Arc<Keystore>,
    cache: Arc<ConnectorCache>,
    refresher: Arc<dyn BearerTokenRefresher>,
) {
    let pin = {
        let reg = registry.read().await;
        let Some(p) = reg.get(&pin_id).cloned() else {
            warn!(%pin_id, "token rotation skipped: pin not in registry");
            return;
        };
        p
    };
    let current = match keystore.load_bearer_token(&pin_id) {
        Ok(Some(t)) => t,
        Ok(None) => {
            warn!(%pin_id, "token rotation skipped: no current bearer in keychain");
            return;
        }
        Err(e) => {
            warn!(error = ?e, %pin_id, "token rotation skipped: keystore load failed");
            return;
        }
    };
    let new_bearer = match refresher
        .refresh(&pin.backend_url, pin.backend_fp, &current)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            warn!(error = ?e, %pin_id, "bearer-token refresh failed");
            return;
        }
    };
    if let Err(e) = keystore.save_bearer_token(&pin_id, &new_bearer) {
        warn!(error = ?e, %pin_id, "could not persist refreshed bearer token");
        return;
    }
    cache.invalidate(&pin_id);
    info!(%pin_id, "bearer token refreshed and connector cache invalidated");
}

/// Failure modes for [`PairEndpoint::bind`] and [`BoundEndpoint::serve`].
#[derive(Debug, Error)]
pub enum ServeError {
    #[error("could not bind TCP socket: {0}")]
    Bind(std::io::Error),
    #[error("could not configure rustls: {0}")]
    Tls(std::io::Error),
    #[error("server task exited with an I/O error: {0}")]
    Serve(std::io::Error),
}
