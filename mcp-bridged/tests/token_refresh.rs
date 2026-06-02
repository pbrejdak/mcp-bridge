//! End-to-end test for [`pair::token_refresh::WellKnownPathRefresher`].
//!
//! Spins up a stub HTTPS backend with the same self-signed cert
//! generator the daemon uses, mounts the well-known refresh path
//! `/.well-known/mcp-bridge/refresh-token`, computes the cert's
//! SHA-256, and asserts the refresher returns the new bearer.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use axum_server::tls_rustls::RustlsConfig;
use mcp_bridged::identity::{SelfSignedCert, generate_self_signed_cert};
use mcp_bridged::pair::backend_url::BackendUrl;
use mcp_bridged::pair::bearer_token::BearerToken;
use mcp_bridged::pair::cert_fingerprint::CertFingerprint;
use mcp_bridged::pair::endpoint::ensure_crypto_provider;
use mcp_bridged::pair::token_refresh::{
    BearerTokenRefresher, RefreshError, WELL_KNOWN_REFRESH_PATH, WellKnownPathRefresher,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[derive(Serialize)]
struct TokenBody {
    token: &'static str,
}

async fn refresh_endpoint(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // The test backend rejects callers without the expected current
    // bearer — same way a real Origin app would authenticate the
    // control call.
    if auth == "Bearer current-token" {
        (
            StatusCode::OK,
            Json(serde_json::to_value(TokenBody { token: "new-token-99" }).unwrap()),
        )
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "wrong bearer"})),
        )
    }
}

async fn start_backend(
    cert: &SelfSignedCert,
) -> (SocketAddr, CancellationToken, tokio::task::JoinHandle<()>) {
    ensure_crypto_provider();
    let cancel = CancellationToken::new();
    let app: Router = Router::new().route(WELL_KNOWN_REFRESH_PATH, get(refresh_endpoint));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let std_listener = listener.into_std().unwrap();
    std_listener.set_nonblocking(true).unwrap();

    let tls = RustlsConfig::from_der(vec![cert.cert_der.clone()], cert.key_der.to_vec())
        .await
        .unwrap();

    let handle = axum_server::Handle::new();
    let handle_for_shutdown = handle.clone();
    let cancel_for_shutdown = cancel.clone();
    tokio::spawn(async move {
        cancel_for_shutdown.cancelled().await;
        handle_for_shutdown.graceful_shutdown(None);
    });

    let task = tokio::spawn(async move {
        axum_server::from_tcp_rustls(std_listener, tls)
            .handle(handle)
            .serve(app.into_make_service())
            .await
            .expect("backend server");
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, cancel, task)
}

fn cert_fp(cert: &SelfSignedCert) -> CertFingerprint {
    CertFingerprint::from_bytes(Sha256::digest(&cert.cert_der).into())
}

#[tokio::test]
async fn well_known_refresher_returns_new_bearer_on_200() {
    let cert = generate_self_signed_cert(IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
    let fp = cert_fp(&cert);
    let (addr, cancel, task) = start_backend(&cert).await;

    let refresher = WellKnownPathRefresher;
    let backend_url = BackendUrl::new(format!("https://{addr}/")).unwrap();
    let current = BearerToken::new("current-token").unwrap();

    let new_token = refresher
        .refresh(&backend_url, fp, &current)
        .await
        .expect("refresh succeeds");
    assert_eq!(new_token.expose(), "new-token-99");

    cancel.cancel();
    let _ = task.await;
}

#[tokio::test]
async fn well_known_refresher_propagates_401_from_backend() {
    let cert = generate_self_signed_cert(IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
    let fp = cert_fp(&cert);
    let (addr, cancel, task) = start_backend(&cert).await;

    let refresher = WellKnownPathRefresher;
    let backend_url = BackendUrl::new(format!("https://{addr}/")).unwrap();
    let wrong = BearerToken::new("wrong-bearer").unwrap();

    let err = match refresher.refresh(&backend_url, fp, &wrong).await {
        Ok(_) => panic!("must fail when bearer is wrong"),
        Err(e) => e,
    };
    assert!(matches!(err, RefreshError::Status { code: 401 }));

    cancel.cancel();
    let _ = task.await;
}

#[tokio::test]
async fn well_known_refresher_rejects_cert_fingerprint_mismatch() {
    let cert = generate_self_signed_cert(IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
    let wrong_fp = CertFingerprint::from_bytes([0u8; 32]);
    let (addr, cancel, task) = start_backend(&cert).await;

    let refresher = WellKnownPathRefresher;
    let backend_url = BackendUrl::new(format!("https://{addr}/")).unwrap();
    let current = BearerToken::new("current-token").unwrap();

    let err = match refresher.refresh(&backend_url, wrong_fp, &current).await {
        Ok(_) => panic!("must fail on fingerprint mismatch"),
        Err(e) => e,
    };
    assert!(
        matches!(err, RefreshError::Send(_)),
        "fingerprint mismatch shows up as a Send error from rustls handshake; got {err:?}",
    );

    cancel.cancel();
    let _ = task.await;
}

// Suppress unused-import warning — the imports are conditionally used.
#[allow(dead_code)]
fn _silence_unused(s: Arc<()>) {
    drop(s);
}
