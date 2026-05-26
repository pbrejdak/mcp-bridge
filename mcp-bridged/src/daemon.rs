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

use thiserror::Error;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::config::Config;
use crate::identity::tls_cert::GenerateError as CertGenerateError;
use crate::identity::{Keypair, generate_self_signed_cert};
use crate::pair::backend_verifier::BackendVerifier;
use crate::pair::endpoint::{PairEndpoint, ServeError};
use crate::pair::invite_register::InviteRegister;
use crate::pair::rustls_backend_verifier::RustlsBackendVerifier;

/// Run the daemon until `cancel` is signalled.
///
/// The caller is responsible for wiring `cancel` to a signal handler
/// (see [`wait_for_shutdown_signal`]) in production. Tests pass a
/// CancellationToken they control directly.
pub async fn run(config: Config, cancel: CancellationToken) -> Result<(), DaemonError> {
    info!(
        bind = %config.bind_addr,
        data_dir = ?config.data_dir,
        "daemon starting"
    );

    // Ephemeral identity for Phase 1 — keystore persistence is a follow-up.
    let resolver = Arc::new(Keypair::generate());
    info!(pubkey = %resolver.pubkey(), "generated ephemeral Resolver identity");

    let cert = generate_self_signed_cert(config.bind_addr.ip())?;

    let invites = InviteRegister::spawn(cancel.clone());

    let verifier: Arc<dyn BackendVerifier> = Arc::new(RustlsBackendVerifier::new());

    let endpoint = PairEndpoint {
        bind_addr: config.bind_addr,
        cert,
        resolver,
        invites,
        backend_verifier: verifier,
    };
    let bound = endpoint.bind()?;
    info!(listening = %bound.local_addr, "pair endpoint bound");

    bound.serve(cancel).await?;

    info!("daemon shut down");
    Ok(())
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
    #[error("could not generate self-signed TLS certificate: {0}")]
    Cert(#[from] CertGenerateError),
    #[error("pair endpoint serve loop failed: {0}")]
    Serve(#[from] ServeError),
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

        // Bind to ephemeral port so the test never races with a real one.
        let config = Config::defaults()
            .unwrap()
            .with_bind_addr("127.0.0.1:0".parse().unwrap());
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
