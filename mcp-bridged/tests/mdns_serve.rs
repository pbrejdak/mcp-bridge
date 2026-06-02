//! Integration test for [`announce::mdns::serve_mdns`].
//!
//! Wires a [`FakeMdnsSubscriber`] to the bridge task, pushes a signed
//! announce wrapped in the SPEC §5.2 TXT-record envelope, and asserts
//! both the registry mutation and the on-disk save.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mcp_bridged::announce::mdns::{self, FakeMdnsSubscriber, MdnsAnnouncement};
use mcp_bridged::announce::payload::{
    AnnounceBackend, AnnouncePayload, SpecVersion as AnnounceSpec,
};
use mcp_bridged::announce::{AnnounceRateLimiter, MdnsSubscriber};
use mcp_bridged::identity::{DisplayName, Ed25519Pubkey, Keypair, Keystore, Signature};
use mcp_bridged::identity::keystore::install_mock_backend;
use mcp_bridged::pair::token_refresh::{BearerTokenRefresher, WellKnownPathRefresher};
use mcp_bridged::proxy::ConnectorCache;
use mcp_bridged::pair::auth::{Auth, AuthType};
use mcp_bridged::pair::backend_url::BackendUrl;
use mcp_bridged::pair::bearer_token::BearerToken;
use mcp_bridged::pair::cert_fingerprint::CertFingerprint;
use mcp_bridged::pair::invite::{Direction, SpecVersion};
use mcp_bridged::pair::logical_id::LogicalId;
use mcp_bridged::pair::nonce::Nonce;
use mcp_bridged::pair::payload::{BackendInfo, OriginInfo, PairPayload, Scope};
use mcp_bridged::pair::seal;
use mcp_bridged::registry::{Registry, ServerPin};
use tempfile::TempDir;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Build a registry with one pin and return (registry, the origin
/// keypair that signed it, registry_path, tempdir guard).
fn seed_registry(
    resolver_pubkey: Ed25519Pubkey,
    lid: &str,
) -> (Arc<RwLock<Registry>>, Keypair, PathBuf, TempDir) {
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
        target_resolver_pubkey: Some(resolver_pubkey),
        sig: Signature::from_bytes([0u8; 64]),
    };
    let canonical = payload.canonical_signing_bytes().unwrap();
    payload.sig = origin.sign(&canonical);

    let mut reg = Registry::new();
    reg.insert(ServerPin::from_accepted_payload(&payload, now_unix() - 100));
    let registry = Arc::new(RwLock::new(reg));

    let tmp = tempfile::tempdir().unwrap();
    let registry_path = tmp.path().join("registry.json");
    (registry, origin, registry_path, tmp)
}

fn signed_announce(origin: &Keypair, lid: &str, seq: u64) -> AnnouncePayload {
    let mut payload = AnnouncePayload {
        spec: AnnounceSpec::McpAnnounceV0_1,
        lid: LogicalId::new(lid).unwrap(),
        backend: AnnounceBackend {
            url: BackendUrl::new("https://10.0.0.42:54321/").unwrap(),
            fp: CertFingerprint::from_bytes([0xab; 32]),
        },
        auth_rotated_at: None,
        cert_rotated_at: None,
        seq,
        exp: now_unix(),
        sig: Signature::from_bytes([0u8; 64]),
    };
    let canonical = payload.canonical_signing_bytes().unwrap();
    payload.sig = origin.sign(&canonical);
    payload
}

fn build_txt(sealed: &[u8]) -> HashMap<String, String> {
    let mut txt = HashMap::new();
    txt.insert(
        mdns::BODY_KEY.to_owned(),
        URL_SAFE_NO_PAD.encode(sealed),
    );
    txt
}

#[tokio::test]
async fn mdns_serve_accepts_a_signed_announce_and_persists() {
    let resolver = Arc::new(Keypair::generate());
    let (registry, origin, registry_path, _tmp) =
        seed_registry(*resolver.pubkey(), "bodylog-7f3a");

    let subscriber = Arc::new(FakeMdnsSubscriber::new());
    let rate_limiter = Arc::new(AnnounceRateLimiter::mdns_default());
    let cancel = CancellationToken::new();

    let subscriber_for_task: Arc<dyn MdnsSubscriber> = subscriber.clone();
    let resolver_for_task = resolver.clone();
    let registry_for_task = registry.clone();
    let path_for_task = registry_path.clone();
    let limiter_for_task = rate_limiter.clone();
    let cancel_for_task = cancel.clone();
    install_mock_backend();
    let keystore = Arc::new(Keystore::for_service("mdns-test-1").expect("keystore"));
    let cache = Arc::new(ConnectorCache::new());
    let refresher: Arc<dyn BearerTokenRefresher> = Arc::new(WellKnownPathRefresher);
    let task = tokio::spawn(async move {
        mdns::serve_mdns(
            subscriber_for_task,
            resolver_for_task,
            registry_for_task,
            path_for_task,
            limiter_for_task,
            keystore,
            cache,
            refresher,
            cancel_for_task,
        )
        .await
    });

    // Let the subscriber's `subscribe` call complete before pushing.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let payload = signed_announce(&origin, "bodylog-7f3a", 7);
    let plaintext = serde_json::to_vec(&payload).unwrap();
    let sealed = seal::seal_to(&plaintext, resolver.pubkey()).unwrap();
    subscriber
        .push(MdnsAnnouncement {
            instance_name: "phone-1".to_owned(),
            txt: build_txt(&sealed),
            source_addr: "10.0.0.42".parse().unwrap(),
        })
        .await;

    // Give the bridge task a moment to consume + apply + persist.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "registry did not advance within 2s",
        );
        {
            let reg = registry.read().await;
            if let Some(pin) = reg.get(&LogicalId::new("bodylog-7f3a").unwrap()) {
                if pin.last_seen_seq == 7 {
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Disk reflects the accept.
    assert!(registry_path.exists());
    let on_disk = Registry::load_or_empty(&registry_path).await.unwrap();
    let pin = on_disk
        .get(&LogicalId::new("bodylog-7f3a").unwrap())
        .unwrap();
    assert_eq!(pin.last_seen_seq, 7);

    cancel.cancel();
    let _ = task.await;
}

#[tokio::test(start_paused = true)]
async fn mdns_serve_resubscribes_after_utc_midnight() {
    // Drive the outer loop through a synthetic day-rollover. paused
    // time advances cheaply; the bridge should drop its current
    // subscription and re-subscribe via the trait once we cross the
    // 24h boundary.
    let resolver = Arc::new(Keypair::generate());
    let (registry, _origin, registry_path, _tmp) =
        seed_registry(*resolver.pubkey(), "bodylog-7f3a");

    let subscriber = Arc::new(FakeMdnsSubscriber::new());
    let rate_limiter = Arc::new(AnnounceRateLimiter::mdns_default());
    let cancel = CancellationToken::new();

    let subscriber_for_task: Arc<dyn MdnsSubscriber> = subscriber.clone();
    install_mock_backend();
    let keystore = Arc::new(Keystore::for_service("mdns-test-2").expect("keystore"));
    let cache = Arc::new(ConnectorCache::new());
    let refresher: Arc<dyn BearerTokenRefresher> = Arc::new(WellKnownPathRefresher);
    let task = tokio::spawn({
        let resolver = resolver.clone();
        let registry = registry.clone();
        let registry_path = registry_path.clone();
        let cancel = cancel.clone();
        async move {
            mdns::serve_mdns(
                subscriber_for_task,
                resolver,
                registry,
                registry_path,
                rate_limiter,
                keystore,
                cache,
                refresher,
                cancel,
            )
            .await
        }
    });

    // Let the first subscribe happen.
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(
        subscriber.subscribe_count(),
        1,
        "must subscribe once on startup",
    );

    // Cross any possible UTC midnight (real-time `now` is somewhere
    // inside today, so 86_405s is always strictly past the next
    // midnight, regardless of when the test happens to run).
    tokio::time::advance(Duration::from_secs(86_410)).await;
    tokio::time::sleep(Duration::from_millis(10)).await;

    assert_eq!(
        subscriber.subscribe_count(),
        2,
        "must re-subscribe after the UTC midnight rollover",
    );

    cancel.cancel();
    let _ = task.await;
}

#[tokio::test]
async fn mdns_serve_drops_malformed_txt_without_touching_registry() {
    let resolver = Arc::new(Keypair::generate());
    let (registry, _origin, registry_path, _tmp) =
        seed_registry(*resolver.pubkey(), "bodylog-7f3a");

    let subscriber = Arc::new(FakeMdnsSubscriber::new());
    let rate_limiter = Arc::new(AnnounceRateLimiter::mdns_default());
    let cancel = CancellationToken::new();

    let subscriber_for_task: Arc<dyn MdnsSubscriber> = subscriber.clone();
    let resolver_for_task = resolver.clone();
    let registry_for_task = registry.clone();
    let path_for_task = registry_path.clone();
    let limiter_for_task = rate_limiter.clone();
    let cancel_for_task = cancel.clone();
    install_mock_backend();
    let keystore = Arc::new(Keystore::for_service("mdns-test-3").expect("keystore"));
    let cache = Arc::new(ConnectorCache::new());
    let refresher: Arc<dyn BearerTokenRefresher> = Arc::new(WellKnownPathRefresher);
    let task = tokio::spawn(async move {
        mdns::serve_mdns(
            subscriber_for_task,
            resolver_for_task,
            registry_for_task,
            path_for_task,
            limiter_for_task,
            keystore,
            cache,
            refresher,
            cancel_for_task,
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // No body= key.
    let mut empty_txt = HashMap::new();
    empty_txt.insert("noise".to_owned(), "ignored".to_owned());
    subscriber
        .push(MdnsAnnouncement {
            instance_name: "garbage".to_owned(),
            txt: empty_txt,
            source_addr: "10.0.0.99".parse().unwrap(),
        })
        .await;

    // Settle.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let reg = registry.read().await;
    let pin = reg.get(&LogicalId::new("bodylog-7f3a").unwrap()).unwrap();
    assert_eq!(
        pin.last_seen_seq, 0,
        "registry must be untouched by malformed TXT",
    );
    drop(reg);

    cancel.cancel();
    let _ = task.await;
}
