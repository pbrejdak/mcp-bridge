//! Integration test for the HTTPS pair endpoint.
//!
//! Spins up a real `PairEndpoint` on `127.0.0.1:0`, builds a signed
//! Direction-B pair payload, seals it to the Resolver's pubkey, POSTs
//! it via `reqwest` over HTTPS (with the self-signed cert accepted),
//! and asserts the response code.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use mcp_bridged::identity::{
    DisplayName, Ed25519Pubkey, Keypair, Signature, generate_self_signed_cert,
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

/// Start the endpoint on `127.0.0.1:0`, return its bound `SocketAddr`,
/// the shared resolver/invites handles, and a cancel handle.
async fn start_endpoint(
    backend_verifier: Arc<dyn mcp_bridged::pair::backend_verifier::BackendVerifier>,
) -> (
    SocketAddr,
    Arc<Keypair>,
    InviteRegister,
    CancellationToken,
    tokio::task::JoinHandle<()>,
) {
    let resolver = Arc::new(Keypair::generate());
    let cert = generate_self_signed_cert(IpAddr::V4(Ipv4Addr::LOCALHOST)).expect("cert generation");
    let cancel = CancellationToken::new();
    let invites = InviteRegister::spawn(cancel.clone());

    let endpoint = PairEndpoint {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        cert,
        resolver: resolver.clone(),
        invites: invites.clone(),
        backend_verifier,
    };
    let bound = endpoint.bind().expect("bind to 127.0.0.1:0");
    let local_addr = bound.local_addr;

    let cancel_for_task = cancel.clone();
    let task = tokio::spawn(async move {
        bound.serve(cancel_for_task).await.expect("server");
    });

    // Give the listener a tick to start accepting before the client connects.
    tokio::time::sleep(Duration::from_millis(50)).await;

    (local_addr, resolver, invites, cancel, task)
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
async fn happy_path_returns_204() {
    let (local_addr, resolver, invites, cancel, task) =
        start_endpoint(Arc::new(AlwaysAccept)).await;

    let lan_addr = lan_addr_for(local_addr);
    let (invite, payload) = build_signed_pair(&resolver, 1, &lan_addr);
    invites.register(invite).await.unwrap();
    let sealed = seal_payload(&payload, resolver.pubkey());

    let client = https_client();
    let url = format!("https://{local_addr}/pair");
    let resp = client.post(&url).body(sealed).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    cancel.cancel();
    let _ = task.await;
}

#[tokio::test]
async fn unknown_nonce_returns_400() {
    let (local_addr, resolver, _invites, cancel, task) =
        start_endpoint(Arc::new(AlwaysAccept)).await;

    let lan_addr = lan_addr_for(local_addr);
    let (_invite_not_registered, payload) = build_signed_pair(&resolver, 2, &lan_addr);
    let sealed = seal_payload(&payload, resolver.pubkey());

    let client = https_client();
    let url = format!("https://{local_addr}/pair");
    let resp = client.post(&url).body(sealed).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    // SPEC §5.3: no body leaked.
    assert!(resp.content_length() == Some(0) || resp.bytes().await.unwrap().is_empty());

    cancel.cancel();
    let _ = task.await;
}

#[tokio::test]
async fn backend_fingerprint_mismatch_returns_400() {
    let (local_addr, resolver, invites, cancel, task) =
        start_endpoint(Arc::new(AlwaysFingerprintMismatch)).await;

    let lan_addr = lan_addr_for(local_addr);
    let (invite, payload) = build_signed_pair(&resolver, 3, &lan_addr);
    invites.register(invite).await.unwrap();
    let sealed = seal_payload(&payload, resolver.pubkey());

    let client = https_client();
    let url = format!("https://{local_addr}/pair");
    let resp = client.post(&url).body(sealed).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    cancel.cancel();
    let _ = task.await;
}

#[tokio::test]
async fn garbage_body_returns_400() {
    let (local_addr, _resolver, _invites, cancel, task) =
        start_endpoint(Arc::new(AlwaysAccept)).await;

    let client = https_client();
    let url = format!("https://{local_addr}/pair");
    let resp = client
        .post(&url)
        .body(b"random non-sealed bytes that aren't even the right length".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    cancel.cancel();
    let _ = task.await;
}
