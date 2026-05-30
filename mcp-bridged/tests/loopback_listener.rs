//! End-to-end test for the loopback listener.
//!
//! Stub HTTPS backend → pre-populated registry + keystore → loopback
//! listener bound on 127.0.0.1:0 → HTTP client posts to the loopback
//! URL → assert the backend received the proxied request with our
//! bearer token attached.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use futures_util::StreamExt;
use axum_server::tls_rustls::RustlsConfig;
use mcp_bridged::identity::keystore::install_mock_backend;
use mcp_bridged::identity::{Keystore, SelfSignedCert, generate_self_signed_cert};
use mcp_bridged::pair::backend_url::BackendUrl;
use mcp_bridged::pair::bearer_token::BearerToken;
use mcp_bridged::pair::cert_fingerprint::CertFingerprint;
use mcp_bridged::pair::endpoint::ensure_crypto_provider;
use mcp_bridged::pair::logical_id::LogicalId;
use mcp_bridged::pair::loopback_key::LoopbackKey;
use mcp_bridged::pair::payload::Scope;
use mcp_bridged::proxy::LoopbackListener;
use mcp_bridged::registry::{PinState, Registry, ServerPin};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
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
            body_text: std::str::from_utf8(&body)
                .ok()
                .map(std::borrow::ToOwned::to_owned),
        }),
    )
}

/// SSE handler: emits three `data: N` events spaced apart so a buffered
/// proxy would visibly delay the first chunk vs. a streaming one.
async fn sse() -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);
    tokio::spawn(async move {
        for n in 1..=3 {
            let line = format!("data: {n}\n\n");
            if tx.send(Ok(Bytes::from(line))).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    });
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    (
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(stream),
    )
        .into_response()
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
        .route("/sse", any(sse))
        .route("/", any(echo))
        .route("/{*rest}", any(echo))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind backend");
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
            .expect("backend server");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, cancel, task)
}

fn cert_fp(cert: &SelfSignedCert) -> CertFingerprint {
    CertFingerprint::from_bytes(Sha256::digest(&cert.cert_der).into())
}

/// Test scaffold: a stub HTTPS backend + pre-populated registry +
/// keystore + a bound loopback listener.
struct LoopbackHarness {
    listener_addr: SocketAddr,
    lid: LogicalId,
    loopback_key: LoopbackKey,
    bearer_value: String,
    backend_cancel: CancellationToken,
    backend_task: tokio::task::JoinHandle<()>,
    listener_cancel: CancellationToken,
    listener_task: tokio::task::JoinHandle<()>,
}

impl LoopbackHarness {
    async fn start() -> Self {
        install_mock_backend();

        // 1. Stub HTTPS backend with its own self-signed cert.
        let backend_cert = generate_self_signed_cert(IpAddr::V4(Ipv4Addr::LOCALHOST)).unwrap();
        let backend_fp = cert_fp(&backend_cert);
        let bearer_value = "token-loopback-test";
        let (backend_addr, backend_cancel, backend_task) =
            start_https_backend(&backend_cert, bearer_value).await;

        // 2. Pre-populate registry + keystore as if a pair had succeeded.
        let lid = LogicalId::new("bodylog").unwrap();
        let pin = ServerPin {
            logical_id: lid.clone(),
            origin_pubkey: mcp_bridged::identity::Ed25519Pubkey::from_bytes([0u8; 32]),
            display_name: mcp_bridged::identity::DisplayName::new("BodyLog").unwrap(),
            backend_url: BackendUrl::new(format!("https://{backend_addr}/")).unwrap(),
            backend_fp,
            scope: vec![Scope::Tools],
            state: PinState::Reachable,
            last_seen_seq: 0,
            auth_rotated_at: None,
            cert_rotated_at: None,
            created_at: 0,
        };
        let mut reg = Registry::new();
        reg.insert(pin);
        let registry = Arc::new(RwLock::new(reg));

        // Unique service so parallel tests don't collide.
        let svc = format!(
            "loopback-{}",
            std::time::Instant::now().elapsed().as_nanos()
        );
        let keystore = Arc::new(Keystore::for_service(&svc).unwrap());
        let loopback_key = LoopbackKey::random();
        keystore.save_loopback_key(&lid, &loopback_key).unwrap();
        let bearer = BearerToken::new(bearer_value).unwrap();
        keystore.save_bearer_token(&lid, &bearer).unwrap();

        // 3. Loopback listener on an ephemeral port. Listener picks the
        // port at bind time, so we use 127.0.0.1:0 and discover the
        // chosen port via a probe TcpListener first.
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = probe.local_addr().unwrap();
        drop(probe);

        let listener_cancel = CancellationToken::new();
        let listener = LoopbackListener {
            bind_addr: listener_addr,
            registry: registry.clone(),
            keystore: keystore.clone(),
        };
        let cancel_for_task = listener_cancel.clone();
        let listener_task = tokio::spawn(async move {
            listener.serve(cancel_for_task).await.expect("listener");
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        Self {
            listener_addr,
            lid,
            loopback_key,
            bearer_value: bearer_value.to_owned(),
            backend_cancel,
            backend_task,
            listener_cancel,
            listener_task,
        }
    }

    fn loopback_url(&self, path_suffix: &str) -> String {
        let key = self.loopback_key.encoded();
        let lid = self.lid.as_str();
        let addr = self.listener_addr;
        let sep = if path_suffix.contains('?') { "&" } else { "?" };
        format!("http://{addr}/{lid}{path_suffix}{sep}key={key}")
    }

    async fn shutdown(self) {
        self.listener_cancel.cancel();
        let _ = self.listener_task.await;
        self.backend_cancel.cancel();
        let _ = self.backend_task.await;
    }
}

#[tokio::test]
async fn happy_path_proxies_to_backend() {
    let h = LoopbackHarness::start().await;
    let url = h.loopback_url("/tools/call");

    let client = reqwest::Client::builder().build().unwrap();
    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .body(r#"{"name":"echo","args":{"x":1}}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body = resp.bytes().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["method"], "POST");
    assert_eq!(parsed["path"], "/tools/call");
    assert_eq!(
        parsed["bearer_header"],
        format!("Bearer {}", h.bearer_value)
    );
    assert_eq!(parsed["body_text"], r#"{"name":"echo","args":{"x":1}}"#);

    h.shutdown().await;
}

#[tokio::test]
async fn missing_key_returns_401() {
    let h = LoopbackHarness::start().await;
    let url = format!("http://{}/{}/anything", h.listener_addr, h.lid.as_str());

    let client = reqwest::Client::builder().build().unwrap();
    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(
        resp.bytes().await.unwrap().is_empty(),
        "401 must have no body"
    );

    h.shutdown().await;
}

#[tokio::test]
async fn wrong_key_returns_401() {
    let h = LoopbackHarness::start().await;
    let wrong = LoopbackKey::from_bytes([0xEEu8; 32]).encoded();
    let url = format!(
        "http://{}/{}/anything?key={wrong}",
        h.listener_addr,
        h.lid.as_str()
    );

    let client = reqwest::Client::builder().build().unwrap();
    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    h.shutdown().await;
}

#[tokio::test]
async fn unknown_lid_returns_404() {
    let h = LoopbackHarness::start().await;
    let key = h.loopback_key.encoded();
    let url = format!(
        "http://{}/unknown-server/anything?key={key}",
        h.listener_addr
    );

    let client = reqwest::Client::builder().build().unwrap();
    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    h.shutdown().await;
}

#[tokio::test]
async fn sse_response_streams_chunk_by_chunk() {
    // Proves the listener doesn't buffer the response body. The
    // backend emits three SSE events 120ms apart and the test asserts
    // the first one arrives at the client well before the third is
    // even sent — impossible if the proxy were holding the whole body.
    let h = LoopbackHarness::start().await;
    let url = h.loopback_url("/sse");

    let client = reqwest::Client::builder().build().unwrap();
    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    assert!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .contains("text/event-stream"),
    );

    let started = tokio::time::Instant::now();
    let mut chunks: Vec<String> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("stream chunk");
        chunks.push(String::from_utf8(chunk.to_vec()).unwrap());
        // Bail as soon as we've collected the first event — a buffering
        // proxy would never reach this line before the backend has sent
        // every chunk.
        if chunks.iter().any(|s| s.contains("data: 1")) {
            break;
        }
    }
    let first_chunk_elapsed = started.elapsed();
    assert!(
        chunks.iter().any(|s| s.contains("data: 1")),
        "first SSE event must arrive before stream ends; got {chunks:?}",
    );
    // The backend sleeps 120ms BETWEEN events; the third event lands at
    // T ≈ 360ms. If the listener buffered, we'd see no chunks before T.
    // Give a generous wall-clock budget to absorb scheduler / TLS jitter.
    assert!(
        first_chunk_elapsed < Duration::from_millis(300),
        "first chunk took {first_chunk_elapsed:?}, expected streaming behaviour",
    );

    h.shutdown().await;
}

#[tokio::test]
async fn bad_host_header_returns_421() {
    let h = LoopbackHarness::start().await;
    let key = h.loopback_key.encoded();
    let lid = h.lid.as_str();
    let url = format!("http://{}/{lid}/anything?key={key}", h.listener_addr);

    let client = reqwest::Client::builder().build().unwrap();
    let resp = client
        .get(&url)
        .header("host", "evil.example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::MISDIRECTED_REQUEST);

    h.shutdown().await;
}
