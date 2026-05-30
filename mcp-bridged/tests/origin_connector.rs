//! Integration test for [`OriginConnector`].
//!
//! Spins up a minimal axum HTTPS server using the same self-signed
//! cert generator the daemon's pair endpoint uses, computes the cert's
//! SHA-256, and exercises the connector against it:
//!
//! - matching fingerprint + valid bearer → 200 with echoed metadata
//! - wrong fingerprint → ConnectorError::Send (rustls handshake fails)
//! - unreachable backend → ConnectorError::Send

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::any;
use axum::{Json, Router};
use axum_server::tls_rustls::RustlsConfig;
use mcp_bridged::identity::{SelfSignedCert, generate_self_signed_cert};
use mcp_bridged::pair::backend_url::BackendUrl;
use mcp_bridged::pair::bearer_token::BearerToken;
use mcp_bridged::pair::cert_fingerprint::CertFingerprint;
use mcp_bridged::pair::endpoint::ensure_crypto_provider;
use mcp_bridged::proxy::{ConnectorError, ForwardRequest, OriginConnector};
use reqwest::Method;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct EchoState {
    expected_bearer: String,
}

#[derive(Serialize)]
struct EchoResult {
    method: String,
    path: String,
    bearer_header: Option<String>,
    body_len: usize,
    body_text: Option<String>,
}

async fn echo(
    State(state): State<Arc<EchoState>>,
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<EchoResult>) {
    let bearer_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(std::borrow::ToOwned::to_owned);
    let status = if bearer_header.as_deref() == Some(state.expected_bearer.as_str()) {
        StatusCode::OK
    } else {
        StatusCode::UNAUTHORIZED
    };
    (
        status,
        Json(EchoResult {
            method: method.to_string(),
            path: uri.path().to_owned(),
            bearer_header,
            body_len: body.len(),
            body_text: std::str::from_utf8(&body)
                .ok()
                .map(std::borrow::ToOwned::to_owned),
        }),
    )
}

async fn start_https_backend(
    cert: &SelfSignedCert,
    expected_bearer: &str,
) -> (SocketAddr, CancellationToken, tokio::task::JoinHandle<()>) {
    ensure_crypto_provider();

    let cancel = CancellationToken::new();
    let state = Arc::new(EchoState {
        expected_bearer: format!("Bearer {expected_bearer}"),
    });

    let app: Router = Router::new()
        .route("/", any(echo))
        .route("/{*rest}", any(echo))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let std_listener = listener.into_std().expect("into_std");
    std_listener.set_nonblocking(true).expect("nonblocking");

    let tls = RustlsConfig::from_der(vec![cert.cert_der.clone()], cert.key_der.to_vec())
        .await
        .expect("tls config");

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
            .expect("server");
    });

    // Let the listener begin accepting.
    tokio::time::sleep(Duration::from_millis(50)).await;

    (addr, cancel, task)
}

fn cert_fp(cert: &SelfSignedCert) -> CertFingerprint {
    CertFingerprint::from_bytes(Sha256::digest(&cert.cert_der).into())
}

#[tokio::test]
async fn matching_fingerprint_and_bearer_forwards_successfully() {
    let cert = generate_self_signed_cert(IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
    let fp = cert_fp(&cert);
    let bearer = "token-xyz";
    let (addr, cancel, task) = start_https_backend(&cert, bearer).await;

    let backend_url = BackendUrl::new(format!("https://{addr}/")).unwrap();
    let connector =
        OriginConnector::new(backend_url, fp, BearerToken::new(bearer).unwrap()).unwrap();

    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    let req = ForwardRequest {
        method: Method::POST,
        path_and_query: "/tools/call".to_owned(),
        headers,
        body: br#"{"name":"echo","args":{"x":1}}"#.to_vec(),
    };

    let resp = connector
        .forward(req)
        .await
        .unwrap_or_else(|e| panic!("forward failed: {e}"));
    assert_eq!(resp.status, reqwest::StatusCode::OK);

    let body_bytes = axum::body::to_bytes(Body::from_stream(resp.body), usize::MAX)
        .await
        .expect("drain body");
    let parsed: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(parsed["method"], "POST");
    assert_eq!(parsed["path"], "/tools/call");
    assert_eq!(parsed["bearer_header"], format!("Bearer {bearer}"));
    assert_eq!(parsed["body_text"], r#"{"name":"echo","args":{"x":1}}"#);

    cancel.cancel();
    let _ = task.await;
}

#[tokio::test]
async fn wrong_fingerprint_fails_at_handshake() {
    let cert = generate_self_signed_cert(IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
    let wrong_fp = CertFingerprint::from_bytes([0u8; 32]);
    let (addr, cancel, task) = start_https_backend(&cert, "anything").await;

    let backend_url = BackendUrl::new(format!("https://{addr}/")).unwrap();
    let connector =
        OriginConnector::new(backend_url, wrong_fp, BearerToken::new("anything").unwrap()).unwrap();

    let req = ForwardRequest {
        method: Method::GET,
        path_and_query: "/health".to_owned(),
        headers: HeaderMap::new(),
        body: Vec::new(),
    };

    let err = match connector.forward(req).await {
        Ok(_) => panic!("forward must fail"),
        Err(e) => e,
    };
    assert!(matches!(err, ConnectorError::Send(_)));

    cancel.cancel();
    let _ = task.await;
}

#[tokio::test]
async fn unreachable_backend_returns_send_error() {
    // Bind then drop to get a port that's almost certainly closed.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let backend_url = BackendUrl::new(format!("https://{addr}/")).unwrap();
    let connector = OriginConnector::new(
        backend_url,
        CertFingerprint::from_bytes([0u8; 32]),
        BearerToken::new("anything").unwrap(),
    )
    .unwrap();

    let req = ForwardRequest {
        method: Method::GET,
        path_and_query: "/".to_owned(),
        headers: HeaderMap::new(),
        body: Vec::new(),
    };
    let err = match connector.forward(req).await {
        Ok(_) => panic!("forward must fail"),
        Err(e) => e,
    };
    assert!(matches!(err, ConnectorError::Send(_)));
}
