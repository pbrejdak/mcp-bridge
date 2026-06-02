//! Integration test for the HTTPS /announce endpoint.
//!
//! Spins up a real PairEndpoint on `127.0.0.1:0`, pre-seeds the
//! registry with a Pin, builds a signed announce payload sealed to the
//! Resolver pubkey, POSTs it to `/announce` over HTTPS (with the self-
//! signed cert accepted), and asserts both the response code and the
//! registry mutation.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mcp_bridged::adapters::{Adapter, Sentinel};
use mcp_bridged::announce::AnnounceRateLimiter;
use mcp_bridged::announce::payload::{
    AnnounceBackend, AnnouncePayload, SpecVersion as AnnounceSpec,
};
use mcp_bridged::identity::keystore::install_mock_backend;
use mcp_bridged::identity::{
    DisplayName, Ed25519Pubkey, Keypair, Keystore, Signature, generate_self_signed_cert,
};
use mcp_bridged::pair::auth::{Auth, AuthType};
use mcp_bridged::pair::backend_url::BackendUrl;
use mcp_bridged::pair::backend_verifier::AlwaysAccept;
use mcp_bridged::pair::bearer_token::BearerToken;
use mcp_bridged::pair::cert_fingerprint::CertFingerprint;
use mcp_bridged::pair::endpoint::PairEndpoint;
use mcp_bridged::pair::invite::{Direction, SpecVersion};
use mcp_bridged::pair::invite_register::InviteRegister;
use mcp_bridged::pair::logical_id::LogicalId;
use mcp_bridged::pair::nonce::Nonce;
use mcp_bridged::pair::payload::{BackendInfo, OriginInfo, PairPayload, Scope};
use mcp_bridged::pair::seal;
use mcp_bridged::registry::{Registry, ServerPin};
use tempfile::TempDir;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

struct EndpointHandles {
    local_addr: SocketAddr,
    resolver: Arc<Keypair>,
    registry: Arc<RwLock<Registry>>,
    registry_path: PathBuf,
    _keystore: Arc<Keystore>,
    _tmp: TempDir,
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

async fn start_endpoint() -> EndpointHandles {
    install_mock_backend();

    let resolver = Arc::new(Keypair::generate());
    let cert = generate_self_signed_cert("127.0.0.1".parse().unwrap()).expect("cert");
    let cancel = CancellationToken::new();
    let invites = InviteRegister::spawn(cancel.clone());

    let tmp = tempfile::tempdir().unwrap();
    let registry_path = tmp.path().join("registry.json");
    let registry = Arc::new(RwLock::new(Registry::new()));

    let unique = format!("test-{}", uuid_like());
    let keystore = Arc::new(Keystore::for_service(&unique).expect("keystore"));

    let endpoint = PairEndpoint {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        cert,
        resolver: mcp_bridged::identity::shared_keypair_from_arc(resolver.clone()),
        invites,
        backend_verifier: Arc::new(AlwaysAccept),
        registry: registry.clone(),
        registry_path: registry_path.clone(),
        keystore: keystore.clone(),
        loopback_addr: "127.0.0.1:18765".parse().unwrap(),
        sentinel: Sentinel::random(),
        adapters: Arc::new(Vec::<Arc<dyn Adapter>>::new()),
        announce_rate_limiter: Arc::new(AnnounceRateLimiter::http_default()),
        connector_cache: Arc::new(mcp_bridged::proxy::ConnectorCache::new()),
        token_refresher: Arc::new(mcp_bridged::pair::token_refresh::WellKnownPathRefresher),
    };
    let bound = endpoint.bind().expect("bind");
    let local_addr = bound.local_addr;

    let cancel_for_task = cancel.clone();
    let task = tokio::spawn(async move {
        bound.serve(cancel_for_task).await.expect("server");
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    EndpointHandles {
        local_addr,
        resolver,
        registry,
        registry_path,
        _keystore: keystore,
        _tmp: tmp,
        cancel,
        task,
    }
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let counter = N.fetch_add(1, Ordering::Relaxed);
    let tid = format!("{:?}", std::thread::current().id());
    format!("{counter}-{tid}")
}

fn https_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("client")
}

/// Build a Pin for `lid` plus the origin keypair that signed it.
async fn seed_pin(handles: &EndpointHandles, lid: &str) -> Keypair {
    let origin = Keypair::generate();
    let mut payload = PairPayload {
        spec: SpecVersion::McpPairV0_1,
        direction: Direction::ResolverOffered,
        origin: OriginInfo {
            name: DisplayName::new("BodyLog").unwrap(),
            pubkey: *origin.pubkey(),
            logical_id: LogicalId::new(lid).unwrap(),
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
        scope: vec![Scope::Tools],
        nonce: Nonce::from_bytes([0u8; 16]),
        target_resolver_pubkey: Some(*handles.resolver.pubkey()),
        sig: Signature::from_bytes([0u8; 64]),
    };
    let canonical = payload.canonical_signing_bytes().unwrap();
    payload.sig = origin.sign(&canonical);

    let mut reg = handles.registry.write().await;
    reg.insert(ServerPin::from_accepted_payload(&payload, now_unix() - 100));
    origin
}

fn signed_announce(
    origin: &Keypair,
    lid: &str,
    seq: u64,
    fp: [u8; 32],
    cert_rotated_at: Option<u64>,
) -> AnnouncePayload {
    let mut payload = AnnouncePayload {
        spec: AnnounceSpec::McpAnnounceV0_1,
        lid: LogicalId::new(lid).unwrap(),
        backend: AnnounceBackend {
            url: BackendUrl::new("https://10.0.0.42:54321/").unwrap(),
            fp: CertFingerprint::from_bytes(fp),
        },
        auth_rotated_at: None,
        cert_rotated_at,
        seq,
        exp: now_unix(),
        sig: Signature::from_bytes([0u8; 64]),
    };
    let canonical = payload.canonical_signing_bytes().unwrap();
    payload.sig = origin.sign(&canonical);
    payload
}

fn seal_for(payload: &AnnouncePayload, receiver: &Ed25519Pubkey) -> Vec<u8> {
    let bytes = serde_json::to_vec(payload).unwrap();
    seal::seal_to(&bytes, receiver).unwrap()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::test]
async fn announce_happy_path_returns_204_and_advances_seq() {
    let h = start_endpoint().await;
    let origin = seed_pin(&h, "bodylog-7f3a").await;

    let payload = signed_announce(&origin, "bodylog-7f3a", 7, [0xab; 32], None);
    let sealed = seal_for(&payload, h.resolver.pubkey());

    let client = https_client();
    let url = format!("https://{}/announce", h.local_addr);
    let resp = client.post(&url).body(sealed).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // Registry shows the new seq.
    let reg = h.registry.read().await;
    let pin = reg.get(&LogicalId::new("bodylog-7f3a").unwrap()).unwrap();
    assert_eq!(pin.last_seen_seq, 7);
    drop(reg);

    // Disk has the new seq too — the handler persists after accept.
    assert!(h.registry_path.exists());
    let on_disk = Registry::load_or_empty(&h.registry_path).await.unwrap();
    let pin = on_disk
        .get(&LogicalId::new("bodylog-7f3a").unwrap())
        .unwrap();
    assert_eq!(pin.last_seen_seq, 7);

    h.cancel.cancel();
    let _ = h.task.await;
}

#[tokio::test]
async fn announce_unknown_lid_returns_400() {
    let h = start_endpoint().await;
    let stranger = Keypair::generate();

    let payload = signed_announce(&stranger, "no-such-lid-x", 1, [0xab; 32], None);
    let sealed = seal_for(&payload, h.resolver.pubkey());

    let client = https_client();
    let url = format!("https://{}/announce", h.local_addr);
    let resp = client.post(&url).body(sealed).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(resp.bytes().await.unwrap().is_empty(), "no body on 400");

    h.cancel.cancel();
    let _ = h.task.await;
}

#[tokio::test]
async fn announce_garbage_body_returns_400() {
    let h = start_endpoint().await;

    let client = https_client();
    let url = format!("https://{}/announce", h.local_addr);
    let resp = client
        .post(&url)
        .body(b"random non-sealed bytes".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    h.cancel.cancel();
    let _ = h.task.await;
}

#[tokio::test]
async fn announce_per_lid_rate_limit_drops_burst() {
    // SPEC §5.6 caps verifications at 1/sec/LID. Send two well-formed
    // announces in immediate succession; the first must be accepted
    // (consumes the LID token), the second must be rejected by the
    // pre-signature rate-limit drop.
    let h = start_endpoint().await;
    let origin = seed_pin(&h, "bodylog-7f3a").await;

    let p1 = signed_announce(&origin, "bodylog-7f3a", 1, [0xab; 32], None);
    let s1 = seal_for(&p1, h.resolver.pubkey());
    let p2 = signed_announce(&origin, "bodylog-7f3a", 2, [0xab; 32], None);
    let s2 = seal_for(&p2, h.resolver.pubkey());

    let client = https_client();
    let url = format!("https://{}/announce", h.local_addr);

    let r1 = client.post(&url).body(s1).send().await.unwrap();
    assert_eq!(r1.status(), reqwest::StatusCode::NO_CONTENT);

    let r2 = client.post(&url).body(s2).send().await.unwrap();
    assert_eq!(r2.status(), reqwest::StatusCode::BAD_REQUEST);

    // Registry stayed at seq=1 — the second announce never reached the
    // accept path.
    let reg = h.registry.read().await;
    let pin = reg.get(&LogicalId::new("bodylog-7f3a").unwrap()).unwrap();
    assert_eq!(pin.last_seen_seq, 1);
    drop(reg);

    h.cancel.cancel();
    let _ = h.task.await;
}

#[tokio::test]
async fn announce_replay_seq_returns_400() {
    let h = start_endpoint().await;
    let origin = seed_pin(&h, "bodylog-7f3a").await;

    let payload = signed_announce(&origin, "bodylog-7f3a", 1, [0xab; 32], None);
    let sealed = seal_for(&payload, h.resolver.pubkey());

    let client = https_client();
    let url = format!("https://{}/announce", h.local_addr);
    let resp = client
        .post(&url)
        .body(sealed.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // Replay the same sealed bytes — must be rejected.
    let resp = client.post(&url).body(sealed).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    h.cancel.cancel();
    let _ = h.task.await;
}
