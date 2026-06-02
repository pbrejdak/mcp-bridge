//! Daemon entry point.
//!
//! Owns the top-level [`CancellationToken`], wires services together,
//! waits for shutdown.
//!
//! Today this is essentially the daemon main loop: it generates an
//! ephemeral identity, spawns the [`InviteRegister`], binds the
//! [`PairEndpoint`], serves until cancelled, and drains. Persistence
//! (keystore-backed identity, registry on disk) lands in follow-up
//! commits. Real restart-policy supervision per
//! [`docs/DAEMON.md`](../../../docs/DAEMON.md) §4 lands when the daemon
//! has more than its current two services to manage.

use std::sync::Arc;
use std::time::Instant;

use thiserror::Error;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::adapters::{Adapter, ClaudeDesktopAdapter};
use crate::announce::{AnnounceRateLimiter, MdnsSdSubscriber, mdns};
use crate::config::Config;
use crate::identity::tls_cert::GenerateError as CertGenerateError;
use crate::identity::{Keystore, KeystoreError, generate_self_signed_cert};
use crate::ipc;
use crate::pair::backend_verifier::BackendVerifier;
use crate::pair::endpoint::{PairEndpoint, ServeError};
use crate::pair::invite_register::InviteRegister;
use crate::pair::rustls_backend_verifier::RustlsBackendVerifier;
use crate::proxy::LoopbackListener;
use crate::proxy::listener::ListenerError;
use crate::registry::{Registry, RegistryError};

/// Run the daemon until `cancel` is signalled.
///
/// The caller is responsible for wiring `cancel` to a signal handler
/// (see [`wait_for_shutdown_signal`]) in production. Tests pass a
/// CancellationToken they control directly.
#[allow(clippy::too_many_lines)] // Top-level wiring; one statement per subsystem by design.
pub async fn run(config: Config, cancel: CancellationToken) -> Result<(), DaemonError> {
    info!(
        bind = %config.bind_addr,
        data_dir = ?config.data_dir,
        "daemon starting"
    );

    // Persistent identity in the OS keychain. First launch generates and
    // saves; every subsequent launch loads the same keypair so already-
    // paired phones keep working without re-pairing.
    let keystore = Arc::new(Keystore::new()?);
    let kp = keystore.load_or_generate_resolver_keypair()?;
    let pubkey_str = kp.pubkey().to_string();
    let resolver = Arc::new(kp);
    info!(pubkey = %pubkey_str, "Resolver identity loaded");

    let cert = generate_self_signed_cert(config.bind_addr.ip())?;

    let registry_path = config.registry_path();
    let registry = Registry::load_or_empty(&registry_path).await?;
    info!(pin_count = registry.len(), path = ?registry_path, "registry loaded");
    let registry = Arc::new(RwLock::new(registry));

    let invites = InviteRegister::spawn(cancel.clone());

    let verifier: Arc<dyn BackendVerifier> = Arc::new(RustlsBackendVerifier::new());

    let sentinel = keystore.load_or_generate_sentinel()?;
    info!(sentinel = %sentinel, "per-install sentinel loaded");

    let adapters: Arc<Vec<Arc<dyn Adapter>>> = Arc::new(vec![Arc::new(ClaudeDesktopAdapter::new())]);

    let resolver_pubkey = *resolver.pubkey();
    let resolver_for_mdns = resolver.clone();
    let invites_for_ipc = invites.clone();
    let registry_path_for_ipc = registry_path.clone();
    let registry_path_for_mdns = registry_path.clone();
    let adapters_for_ipc = adapters.clone();

    // Shared between the proxy listener and the bearer-token rotation
    // path: announce handlers invalidate entries here so the proxy's
    // next request rebuilds the connector with the rotated bearer
    // (SPEC §5.7).
    let connector_cache = Arc::new(crate::proxy::ConnectorCache::new());
    let connector_cache_for_endpoint = connector_cache.clone();
    let connector_cache_for_mdns = connector_cache.clone();
    let token_refresher: Arc<dyn crate::pair::token_refresh::BearerTokenRefresher> =
        Arc::new(crate::pair::token_refresh::WellKnownPathRefresher);
    let token_refresher_for_mdns = token_refresher.clone();

    let endpoint = PairEndpoint {
        bind_addr: config.bind_addr,
        cert,
        resolver,
        invites,
        backend_verifier: verifier,
        registry: registry.clone(),
        registry_path,
        keystore: keystore.clone(),
        loopback_addr: config.loopback_addr,
        sentinel,
        adapters,
        announce_rate_limiter: Arc::new(AnnounceRateLimiter::http_default()),
        connector_cache: connector_cache_for_endpoint,
        token_refresher,
    };
    let bound = endpoint.bind()?;
    info!(listening = %bound.local_addr, "pair endpoint bound");

    let ipc_ctx = ipc::Context {
        start: Instant::now(),
        registry: registry.clone(),
        pair_endpoint_addr: bound.local_addr,
        resolver_pubkey,
        display_name: config.display_name.clone(),
        invites: invites_for_ipc,
        keystore: keystore.clone(),
        registry_path: registry_path_for_ipc,
        sentinel,
        adapters: adapters_for_ipc,
        recorder: crate::observability::recorder(),
    };
    let ipc_socket_path = config.ipc_socket_path();
    let ipc_cancel = cancel.clone();
    let ipc_task = tokio::spawn(async move {
        if let Err(e) = ipc::serve(ipc_socket_path, ipc_ctx, ipc_cancel).await {
            error!(error = ?e, "IPC server exited with error");
        }
    });

    let loopback = LoopbackListener {
        bind_addr: config.loopback_addr,
        registry: registry.clone(),
        keystore: keystore.clone(),
        connectors: connector_cache.clone(),
    };
    let loopback_cancel = cancel.clone();
    let loopback_task = tokio::spawn(async move {
        if let Err(e) = loopback.serve(loopback_cancel).await {
            error!(error = ?e, "loopback listener exited with error");
        }
    });

    // mDNS subscriber. Best-effort: a sandbox / lockdown that refuses
    // the multicast bind shouldn't take down the rest of the daemon.
    let mdns_task = match MdnsSdSubscriber::new() {
        Ok(subscriber) => {
            let subscriber_arc: Arc<dyn crate::announce::MdnsSubscriber> = Arc::new(subscriber);
            let registry_for_mdns = registry.clone();
            let rate_limiter = Arc::new(AnnounceRateLimiter::mdns_default());
            let keystore_for_mdns = keystore.clone();
            let mdns_cancel = cancel.clone();
            Some(tokio::spawn(async move {
                if let Err(e) = mdns::serve_mdns(
                    subscriber_arc,
                    resolver_for_mdns,
                    registry_for_mdns,
                    registry_path_for_mdns,
                    rate_limiter,
                    keystore_for_mdns,
                    connector_cache_for_mdns,
                    token_refresher_for_mdns,
                    mdns_cancel,
                )
                .await
                {
                    error!(error = ?e, "mDNS subscriber exited with error");
                }
            }))
        }
        Err(e) => {
            tracing::warn!(error = ?e, "mDNS subscriber unavailable; cellular Origins will need HTTP announces only");
            None
        }
    };

    let serve_result = bound.serve(cancel).await;
    let _ = ipc_task.await;
    let _ = loopback_task.await;
    if let Some(task) = mdns_task {
        let _ = task.await;
    }

    info!("daemon shut down");
    serve_result.map_err(DaemonError::Serve)
}

/// Run the daemon and wire its shutdown to the host process's SIGINT /
/// SIGTERM (Unix) or Ctrl-C (Windows).
///
/// The binary entry point calls this; tests that need explicit
/// cancellation control use [`run`] directly.
pub async fn run_with_signal_handler(config: Config) -> Result<(), DaemonError> {
    let cancel = CancellationToken::new();
    let cancel_for_signals = cancel.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        cancel_for_signals.cancel();
    });
    run(config, cancel).await
}

/// Block until the host process receives SIGINT or SIGTERM (Unix) or
/// Ctrl-C / Ctrl-Break (Windows). Returns when the first such signal
/// is observed.
///
/// Callers typically spawn this in a task that cancels a
/// [`CancellationToken`] on completion.
pub async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("SIGTERM handler installs at process startup");
        let mut sigint =
            signal(SignalKind::interrupt()).expect("SIGINT handler installs at process startup");
        tokio::select! {
            _ = sigterm.recv() => info!("received SIGTERM"),
            _ = sigint.recv() => info!("received SIGINT"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        info!("received Ctrl-C");
    }
}

/// Failure modes for [`run`].
#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("could not load Resolver identity from keychain: {0}")]
    Keystore(#[from] KeystoreError),
    #[error("could not generate self-signed TLS certificate: {0}")]
    Cert(#[from] CertGenerateError),
    #[error("could not load Server Registry: {0}")]
    Registry(#[from] RegistryError),
    #[error("pair endpoint serve loop failed: {0}")]
    Serve(#[from] ServeError),
    #[error("loopback listener failed: {0}")]
    Loopback(#[from] ListenerError),
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Smoke test — the daemon boots, listens, then shuts down cleanly
    /// when its CancellationToken fires. No real pair POSTs.
    #[tokio::test]
    async fn run_boots_and_shuts_down_cleanly() {
        crate::observability::init();
        // Critical: keep the daemon's Keystore::new() out of the real
        // OS keychain. Process-global; idempotent.
        crate::identity::keystore::install_mock_backend();

        // Bind to ephemeral port and an isolated data dir so the test
        // never races with another and never touches the real registry.
        let tmp = tempfile::tempdir().unwrap();
        let config = Config::defaults()
            .unwrap()
            .with_bind_addr("127.0.0.1:0".parse().unwrap())
            .with_loopback_addr("127.0.0.1:0".parse().unwrap())
            .with_data_dir(tmp.path());
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        let handle = tokio::spawn(async move { run(config, cancel_clone).await });

        // Give the listener a moment to bind.
        tokio::time::sleep(Duration::from_millis(100)).await;

        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("daemon should shut down within 2s")
            .expect("daemon task should not panic");

        result.expect("daemon should exit cleanly");
    }
}
