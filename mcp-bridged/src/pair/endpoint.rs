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
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum_server::tls_rustls::RustlsConfig;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::identity::{Keypair, SelfSignedCert};
use crate::pair::accept::accept_direction_b;
use crate::pair::backend_verifier::BackendVerifier;
use crate::pair::invite_register::InviteRegister;
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
        });

        let app: Router = Router::new()
            .route("/pair", post(handle_pair))
            .with_state(state);

        let handle = axum_server::Handle::new();
        let handle_for_shutdown = handle.clone();
        tokio::spawn(async move {
            cancel.cancelled().await;
            handle_for_shutdown.graceful_shutdown(None);
        });

        axum_server::from_tcp_rustls(self.listener, tls)
            .handle(handle)
            .serve(app.into_make_service())
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

    // Insert + persist under a single write lock so concurrent pair
    // accepts can't interleave their saves.
    let mut reg = state.registry.write().await;
    reg.insert(pin);
    let save_result = reg.save(&state.registry_path).await;
    drop(reg);

    match save_result {
        Ok(()) => {
            info!(logical_id = %lid, "pair accepted and persisted");
            StatusCode::NO_CONTENT
        }
        Err(e) => {
            // The pin is in memory but not on disk. The user thinks they
            // succeeded but a restart loses the pairing. Surface 5xx so
            // the client knows to retry.
            error!(error = ?e, logical_id = %lid, "registry save failed after pair accept");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
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
