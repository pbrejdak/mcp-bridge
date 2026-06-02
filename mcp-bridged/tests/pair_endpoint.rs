//! Integration test for the HTTPS pair endpoint.
//!
//! Spins up a real `PairEndpoint` on `127.0.0.1:0`, builds a signed
//! Direction-B pair payload, seals it to the Resolver's pubkey, POSTs
//! it via `reqwest` over HTTPS (with the self-signed cert accepted),
//! and asserts the response code.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mcp_bridged::adapters::{Adapter, ClaudeDesktopAdapter, Sentinel};
use mcp_bridged::announce::AnnounceRateLimiter;
use mcp_bridged::identity::keystore::install_mock_backend;
use mcp_bridged::identity::{
    DisplayName, Ed25519Pubkey, Keypair, Keystore, Signature, generate_self_signed_cert,
};
use mcp_bridged::pair::Invite;
use mcp_bridged::pair::auth::{Auth, AuthType};
use mcp_bridged::pair::backend_url::BackendUrl;
use mcp_bridged::pair::backend_verifier::{AlwaysAccept, AlwaysFingerprintMismatch};
use mcp_bridged::pair::bearer_token::BearerToken;
use mcp_bridged::pair::cert_fingerprint::CertFingerprint;
use mcp_bridged::pair::endpoint::PairEndpoint;
use mcp_bridged::pair::invite::{Direction, SpecVersion};
use mcp_bridged::pair::invite_register::InviteRegister;
use mcp_bridged::pair::lan_addr::LanAddr;
use mcp_bridged::pair::logical_id::LogicalId;
use mcp_bridged::pair::nonce::Nonce;
use mcp_bridged::pair::payload::{BackendInfo, OriginInfo, PairPayload, Scope};
use mcp_bridged::pair::seal;
use mcp_bridged::registry::Registry;
use tempfile::TempDir;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// Build a signed pair payload addressed to `resolver`, paired with the
/// invite that should be registered for it.
fn build_signed_pair(resolver: &Keypair, nonce_byte: u8, lan_addr: &str) -> (Invite, PairPayload) {
    let origin = Keypair::generate();
    let nonce = Nonce::from_bytes([nonce_byte; 16]);

    let invite = Invite::new(
        *resolver.pubkey(),
        DisplayName::new("Patryk's MacBook Pro").unwrap(),
        LanAddr::new(lan_addr).unwrap(),
        nonce,
    );

    let mut payload = PairPayload {
        spec: SpecVersion::McpPairV0_1,
        direction: Direction::ResolverOffered,
        origin: OriginInfo {
            name: DisplayName::new("BodyLog").unwrap(),
            pubkey: *origin.pubkey(),
            logical_id: LogicalId::new("bodylog-7f3a").unwrap(),
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
        scope: vec![Scope::Tools, Scope::Resources],
        nonce,
        target_resolver_pubkey: Some(*resolver.pubkey()),
        sig: Signature::from_bytes([0u8; 64]),
    };
    let canonical = payload.canonical_signing_bytes().unwrap();
    payload.sig = origin.sign(&canonical);
    (invite, payload)
}

fn seal_payload(payload: &PairPayload, receiver: &Ed25519Pubkey) -> Vec<u8> {
    let plaintext = serde_json::to_vec(payload).unwrap();
    seal::seal_to(&plaintext, receiver).unwrap()
}

/// Returned bundle from [`start_endpoint`]. Grows when the endpoint
/// grows; tests pull what they need by field name.
struct EndpointHandles {
    local_addr: SocketAddr,
    resolver: Arc<Keypair>,
    invites: InviteRegister,
    registry: Arc<RwLock<Registry>>,
    registry_path: PathBuf,
    keystore: Arc<Keystore>,
    sentinel: Sentinel,
    loopback_addr: SocketAddr,
    _tmp: TempDir,
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

/// Start the endpoint on `127.0.0.1:0` with an isolated tmpdir-backed
/// registry and mock-backed keystore. Returns shared handles tests use
/// to drive and inspect the running server.
async fn start_endpoint(
    backend_verifier: Arc<dyn mcp_bridged::pair::backend_verifier::BackendVerifier>,
) -> EndpointHandles {
    start_endpoint_with_adapters(backend_verifier, Arc::new(Vec::new())).await
}

/// Variant of [`start_endpoint`] that lets a test supply adapters to be
/// invoked after a successful pair accept.
async fn start_endpoint_with_adapters(
    backend_verifier: Arc<dyn mcp_bridged::pair::backend_verifier::BackendVerifier>,
    adapters: Arc<Vec<Arc<dyn Adapter>>>,
) -> EndpointHandles {
    install_mock_backend();

    let resolver = Arc::new(Keypair::generate());
    let cert = generate_self_signed_cert(IpAddr::V4(Ipv4Addr::LOCALHOST)).expect("cert generation");
    let cancel = CancellationToken::new();
    let invites = InviteRegister::spawn(cancel.clone());

    let tmp = tempfile::tempdir().unwrap();
    let registry_path = tmp.path().join("registry.json");
    let registry = Arc::new(RwLock::new(Registry::new()));

    // Unique service name per call so parallel tests don't share state
    // in the mock backend.
    let unique = format!("test-{}", uuid_like());
    let keystore = Arc::new(Keystore::for_service(&unique).expect("keystore"));

    let sentinel = Sentinel::random();
    // A made-up loopback address — tests don't actually serve traffic
    // on it; they just assert the URL the adapter writes refers to it.
    let loopback_addr: SocketAddr = "127.0.0.1:18765".parse().unwrap();

    let endpoint = PairEndpoint {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        cert,
        resolver: resolver.clone(),
        invites: invites.clone(),
        backend_verifier,
        registry: registry.clone(),
        registry_path: registry_path.clone(),
        keystore: keystore.clone(),
        loopback_addr,
        sentinel,
        adapters,
        announce_rate_limiter: Arc::new(AnnounceRateLimiter::http_default()),
        connector_cache: Arc::new(mcp_bridged::proxy::ConnectorCache::new()),
        token_refresher: Arc::new(mcp_bridged::pair::token_refresh::WellKnownPathRefresher),
    };
    let bound = endpoint.bind().expect("bind to 127.0.0.1:0");
    let local_addr = bound.local_addr;

    let cancel_for_task = cancel.clone();
    let task = tokio::spawn(async move {
        bound.serve(cancel_for_task).await.expect("server");
    });

    // Give the listener a tick to start accepting before the client connects.
    tokio::time::sleep(Duration::from_millis(50)).await;

    EndpointHandles {
        local_addr,
        resolver,
        invites,
        registry,
        registry_path,
        keystore,
        sentinel,
        loopback_addr,
        _tmp: tmp,
        cancel,
        task,
    }
}

/// Cheap unique-ish suffix for service names. Not a real UUID; just two
/// values from a counter + the test thread's id.
fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let counter = N.fetch_add(1, Ordering::Relaxed);
    let tid = format!("{:?}", std::thread::current().id());
    format!("{counter}-{tid}")
}

fn lan_addr_for(local: SocketAddr) -> String {
    let ip = local.ip();
    let port = local.port();
    format!("https://{ip}:{port}/pair")
}

fn https_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("client")
}

#[tokio::test]
async fn happy_path_returns_204_and_persists_everything() {
    let h = start_endpoint(Arc::new(AlwaysAccept)).await;

    let lan_addr = lan_addr_for(h.local_addr);
    let (invite, payload) = build_signed_pair(&h.resolver, 1, &lan_addr);
    h.invites.register(invite).await.unwrap();
    let sealed = seal_payload(&payload, h.resolver.pubkey());

    let client = https_client();
    let url = format!("https://{}/pair", h.local_addr);
    let resp = client.post(&url).body(sealed).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // In-memory registry has the pin.
    {
        let reg = h.registry.read().await;
        assert_eq!(reg.len(), 1);
        assert!(reg.get(&payload.origin.logical_id).is_some());
    }

    // On-disk registry has the pin.
    assert!(h.registry_path.exists(), "registry.json must be on disk");
    let on_disk = mcp_bridged::registry::Registry::load_or_empty(&h.registry_path)
        .await
        .unwrap();
    assert_eq!(on_disk.len(), 1);

    // Per-Pin secrets are in the keystore.
    let lid = &payload.origin.logical_id;
    let stored_loopback = h
        .keystore
        .load_loopback_key(lid)
        .unwrap()
        .expect("loopback key persisted");
    // 32-byte key → 43 base64url-no-pad chars. We can't see the bytes
    // (as_bytes is crate-internal); checking the encoded length proves
    // the persisted form is the full secret.
    assert_eq!(stored_loopback.encoded().len(), 43);
    let stored_bearer = h
        .keystore
        .load_bearer_token(lid)
        .unwrap()
        .expect("bearer token persisted");
    assert_eq!(stored_bearer.expose(), "token-abc");

    h.cancel.cancel();
    let _ = h.task.await;
}

#[tokio::test]
async fn unknown_nonce_returns_400() {
    let h = start_endpoint(Arc::new(AlwaysAccept)).await;

    let lan_addr = lan_addr_for(h.local_addr);
    let (_invite_not_registered, payload) = build_signed_pair(&h.resolver, 2, &lan_addr);
    let sealed = seal_payload(&payload, h.resolver.pubkey());

    let client = https_client();
    let url = format!("https://{}/pair", h.local_addr);
    let resp = client.post(&url).body(sealed).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    // SPEC §5.3: no body leaked.
    assert!(resp.content_length() == Some(0) || resp.bytes().await.unwrap().is_empty());

    h.cancel.cancel();
    let _ = h.task.await;
}

#[tokio::test]
async fn backend_fingerprint_mismatch_returns_400() {
    let h = start_endpoint(Arc::new(AlwaysFingerprintMismatch)).await;

    let lan_addr = lan_addr_for(h.local_addr);
    let (invite, payload) = build_signed_pair(&h.resolver, 3, &lan_addr);
    h.invites.register(invite).await.unwrap();
    let sealed = seal_payload(&payload, h.resolver.pubkey());

    let client = https_client();
    let url = format!("https://{}/pair", h.local_addr);
    let resp = client.post(&url).body(sealed).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    h.cancel.cancel();
    let _ = h.task.await;
}

#[tokio::test]
async fn happy_path_writes_adapter_entry_with_sentinel_and_loopback_url() {
    // ClaudeDesktopAdapter pointed at a tempdir-backed config file.
    // After a successful pair, the file must contain a `mcpServers/<lid>`
    // entry with url = loopback URL + sentinel = our per-install UUID.
    let adapter_tmp = tempfile::tempdir().unwrap();
    let config_path = adapter_tmp.path().join("claude_desktop_config.json");
    let adapter = Arc::new(ClaudeDesktopAdapter::with_config_path(config_path.clone()));
    let adapters: Arc<Vec<Arc<dyn Adapter>>> = Arc::new(vec![adapter]);

    let h = start_endpoint_with_adapters(Arc::new(AlwaysAccept), adapters).await;

    let lan_addr = lan_addr_for(h.local_addr);
    let (invite, payload) = build_signed_pair(&h.resolver, 4, &lan_addr);
    h.invites.register(invite).await.unwrap();
    let sealed = seal_payload(&payload, h.resolver.pubkey());

    let client = https_client();
    let url = format!("https://{}/pair", h.local_addr);
    let resp = client.post(&url).body(sealed).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // Adapter file exists.
    assert!(config_path.exists(), "adapter config file must be written");
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    let lid = payload.origin.logical_id.as_str();
    let entry = json
        .get("mcpServers")
        .and_then(|m| m.get(lid))
        .expect("entry must be present under mcpServers/<lid>");

    let entry_url = entry
        .get("url")
        .and_then(|v| v.as_str())
        .expect("entry must carry a url");
    let prefix = format!("http://{}/{}?key=", h.loopback_addr, lid);
    assert!(
        entry_url.starts_with(&prefix),
        "url {entry_url} must start with {prefix}",
    );
    // 32-byte key → 43 base64url-no-pad chars.
    let key_suffix = entry_url.strip_prefix(&prefix).unwrap();
    assert_eq!(key_suffix.len(), 43, "url must end with 43-char key");

    let sentinel_str = entry
        .get("_mcp_bridge_managed")
        .and_then(|v| v.as_str())
        .expect("entry must carry the sentinel marker");
    assert_eq!(sentinel_str, h.sentinel.to_string());

    h.cancel.cancel();
    let _ = h.task.await;
}

#[tokio::test]
async fn garbage_body_returns_400() {
    let h = start_endpoint(Arc::new(AlwaysAccept)).await;

    let client = https_client();
    let url = format!("https://{}/pair", h.local_addr);
    let resp = client
        .post(&url)
        .body(b"random non-sealed bytes that aren't even the right length".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    h.cancel.cancel();
    let _ = h.task.await;
}
