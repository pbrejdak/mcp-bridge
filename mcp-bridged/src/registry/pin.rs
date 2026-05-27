//! [`ServerPin`] — the Resolver-side record of one paired Origin.
//!
//! Shape derived from [`docs/ARCHITECTURE.md`](../../../../docs/ARCHITECTURE.md)
//! §3.1. Phase 1 stores the protocol-visible fields plus the
//! per-pin runtime state. Secret material (bearer token, per-Consumer
//! loopback keys) lands in the keychain in a follow-up commit; it is
//! deliberately absent here so the on-disk registry file never holds
//! those bytes.

use serde::{Deserialize, Serialize};

use crate::identity::Ed25519Pubkey;
use crate::identity::display_name::DisplayName;
use crate::pair::backend_url::BackendUrl;
use crate::pair::cert_fingerprint::CertFingerprint;
use crate::pair::logical_id::LogicalId;
use crate::pair::payload::{PairPayload, Scope};

/// Per-pin connectivity state. Cycles between Reachable and Unreachable
/// as backend health flaps; flips to Revoked on user revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinState {
    Reachable,
    Unreachable,
    Revoked,
}

/// A paired Origin, as stored in the Resolver's registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerPin {
    pub logical_id: LogicalId,
    pub origin_pubkey: Ed25519Pubkey,
    pub display_name: DisplayName,
    pub backend_url: BackendUrl,
    pub backend_fp: CertFingerprint,
    pub scope: Vec<Scope>,
    pub state: PinState,
    /// Strictly-increasing per-LID sequence from the most recent accepted
    /// announce; defends against replay (SPEC.md §5.5 rule 4).
    pub last_seen_seq: u64,
    /// Last observed `auth_rotated_at` from an accepted announce — when
    /// the Origin rotates its bearer token, the Resolver re-fetches.
    pub auth_rotated_at: Option<u64>,
    /// Last observed `cert_rotated_at`; the SPEC §5.8 ratchet that lets
    /// the Origin renew its TLS leaf without re-pairing.
    pub cert_rotated_at: Option<u64>,
    /// Unix timestamp when this pin was first accepted.
    pub created_at: u64,
}

impl ServerPin {
    /// Build a `ServerPin` from a freshly-validated `PairPayload`.
    /// Caller supplies the wall-clock timestamp so tests can pin it.
    #[must_use]
    pub fn from_accepted_payload(payload: &PairPayload, created_at: u64) -> Self {
        Self {
            logical_id: payload.origin.logical_id.clone(),
            origin_pubkey: payload.origin.pubkey,
            display_name: payload.origin.name.clone(),
            backend_url: payload.backend.url.clone(),
            backend_fp: payload.backend.fp,
            scope: payload.scope.clone(),
            state: PinState::Reachable,
            last_seen_seq: 0,
            auth_rotated_at: None,
            cert_rotated_at: None,
            created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{DisplayName, Keypair, Signature};
    use crate::pair::auth::{Auth, AuthType};
    use crate::pair::bearer_token::BearerToken;
    use crate::pair::invite::{Direction, SpecVersion};
    use crate::pair::nonce::Nonce;
    use crate::pair::payload::{BackendInfo, OriginInfo};

    fn sample_payload() -> PairPayload {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
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
            nonce: Nonce::from_bytes([0u8; 16]),
            target_resolver_pubkey: Some(*resolver.pubkey()),
            sig: Signature::from_bytes([0u8; 64]),
        };
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = origin.sign(&canonical);
        payload
    }

    #[test]
    fn from_accepted_payload_extracts_protocol_fields() {
        let payload = sample_payload();
        let pin = ServerPin::from_accepted_payload(&payload, 1_700_000_000);
        assert_eq!(pin.logical_id.as_str(), "bodylog-7f3a");
        assert_eq!(pin.origin_pubkey, payload.origin.pubkey);
        assert_eq!(pin.display_name.as_str(), "BodyLog");
        assert_eq!(pin.backend_url.as_str(), "https://10.0.0.42:54321/");
        assert_eq!(pin.scope, vec![Scope::Tools, Scope::Resources]);
        assert_eq!(pin.state, PinState::Reachable);
        assert_eq!(pin.last_seen_seq, 0);
        assert_eq!(pin.created_at, 1_700_000_000);
    }

    #[test]
    fn pin_serializes_to_expected_shape() {
        let payload = sample_payload();
        let pin = ServerPin::from_accepted_payload(&payload, 1_700_000_000);
        let value: serde_json::Value = serde_json::to_value(&pin).unwrap();
        assert_eq!(value["logical_id"], "bodylog-7f3a");
        assert_eq!(value["state"], "Reachable");
        assert_eq!(value["scope"], serde_json::json!(["tools", "resources"]));
        assert_eq!(value["last_seen_seq"], 0);
        assert!(
            value.get("bearer_token").is_none(),
            "registry must not serialize bearer-token bytes"
        );
    }

    #[test]
    fn pin_round_trips_through_json() {
        let payload = sample_payload();
        let original = ServerPin::from_accepted_payload(&payload, 42);
        let json = serde_json::to_string(&original).unwrap();
        let back: ServerPin = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }
}
