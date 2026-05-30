//! `mcp-announce/v0.1` signed announce payload — the inner body sealed
//! to the Resolver's pubkey over the mDNS TXT or HTTP POST carrier.
//!
//! See [`docs/SPEC.md`](../../../../docs/SPEC.md) §5.4 for the JSON shape
//! and §5.5 for the acceptance rules.
//!
//! This module owns the shape, canonical-JSON signing bytes, and the
//! cryptographic signature check. Replay defence (seq), freshness (exp),
//! and ratchet rules (auth/cert rotated_at) belong to the accept layer
//! in [`super::accept`] which combines this module with registry state.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::identity::{self, Ed25519Pubkey, Signature, VerifyError};
use crate::pair::backend_url::BackendUrl;
use crate::pair::cert_fingerprint::CertFingerprint;
use crate::pair::logical_id::LogicalId;

/// Protocol version identifier carried in the `spec` field.
///
/// Single-variant enum (today) so that future versions add variants
/// rather than mutating a magic string, and so that an unknown `spec`
/// fails to deserialize per [`docs/SPEC.md`](../../../../docs/SPEC.md) §3.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecVersion {
    #[serde(rename = "mcp-announce/v0.1")]
    McpAnnounceV0_1,
}

/// Origin backend identity carried in an announce. Same fields as the
/// pair payload's [`BackendInfo`](crate::pair::payload::BackendInfo)
/// minus the `ca` extension — the announce ratchet only touches the
/// pinned URL and fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnounceBackend {
    pub url: BackendUrl,
    pub fp: CertFingerprint,
}

/// A signed `mcp-announce/v0.1` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncePayload {
    pub spec: SpecVersion,
    pub lid: LogicalId,
    pub backend: AnnounceBackend,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub auth_rotated_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cert_rotated_at: Option<u64>,
    pub seq: u64,
    pub exp: u64,
    pub sig: Signature,
}

/// Same field set as [`AnnouncePayload`] without `sig`. Serialized form
/// is what the signature is computed over (RFC 8785 canonicalization).
#[derive(Debug, Serialize)]
struct AnnouncePayloadUnsigned<'a> {
    spec: &'a SpecVersion,
    lid: &'a LogicalId,
    backend: &'a AnnounceBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_rotated_at: &'a Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cert_rotated_at: &'a Option<u64>,
    seq: u64,
    exp: u64,
}

impl AnnouncePayload {
    fn unsigned(&self) -> AnnouncePayloadUnsigned<'_> {
        AnnouncePayloadUnsigned {
            spec: &self.spec,
            lid: &self.lid,
            backend: &self.backend,
            auth_rotated_at: &self.auth_rotated_at,
            cert_rotated_at: &self.cert_rotated_at,
            seq: self.seq,
            exp: self.exp,
        }
    }

    /// Produce the RFC 8785 canonical-JSON bytes the signature covers.
    /// Used both by signers and verifiers.
    pub fn canonical_signing_bytes(&self) -> Result<Vec<u8>, ValidationError> {
        serde_jcs::to_vec(&self.unsigned()).map_err(ValidationError::Canonicalize)
    }

    /// Verify the cryptographic signature against `origin_pubkey`.
    /// Caller MUST additionally enforce SPEC §5.5 rules 2, 4, 5, 6, 7
    /// (pin lookup, seq, exp, URL shape, fp/ratchet).
    pub fn verify_sig(&self, origin_pubkey: &Ed25519Pubkey) -> Result<(), ValidationError> {
        let canonical = self.canonical_signing_bytes()?;
        identity::verify(origin_pubkey, &canonical, &self.sig)?;
        Ok(())
    }
}

/// Failure modes for [`AnnouncePayload::verify_sig`].
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("signature verification failed: {0}")]
    Signature(#[from] VerifyError),
    #[error("could not canonicalize payload: {0}")]
    Canonicalize(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::identity::Keypair;

    fn build_signed_announce(origin_kp: &Keypair, seq: u64, exp: u64) -> AnnouncePayload {
        let mut payload = AnnouncePayload {
            spec: SpecVersion::McpAnnounceV0_1,
            lid: LogicalId::new("bodylog-7f3a").unwrap(),
            backend: AnnounceBackend {
                url: BackendUrl::new("https://10.0.0.42:54321/").unwrap(),
                fp: CertFingerprint::from_bytes([0xab; 32]),
            },
            auth_rotated_at: None,
            cert_rotated_at: None,
            seq,
            exp,
            sig: Signature::from_bytes([0u8; 64]),
        };
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = origin_kp.sign(&canonical);
        payload
    }

    #[test]
    fn verify_sig_accepts_a_correctly_signed_payload() {
        let origin = Keypair::generate();
        let payload = build_signed_announce(&origin, 1, 1_700_000_000);
        assert!(payload.verify_sig(origin.pubkey()).is_ok());
    }

    #[test]
    fn verify_sig_rejects_wrong_signer() {
        let origin = Keypair::generate();
        let other = Keypair::generate();
        let payload = build_signed_announce(&origin, 1, 1_700_000_000);
        assert!(matches!(
            payload.verify_sig(other.pubkey()),
            Err(ValidationError::Signature(_))
        ));
    }

    #[test]
    fn verify_sig_rejects_tampered_seq() {
        let origin = Keypair::generate();
        let mut payload = build_signed_announce(&origin, 1, 1_700_000_000);
        // Bump seq after the signature is computed.
        payload.seq = 99;
        assert!(matches!(
            payload.verify_sig(origin.pubkey()),
            Err(ValidationError::Signature(_))
        ));
    }

    #[test]
    fn verify_sig_rejects_tampered_backend_fp() {
        let origin = Keypair::generate();
        let mut payload = build_signed_announce(&origin, 1, 1_700_000_000);
        payload.backend.fp = CertFingerprint::from_bytes([0xee; 32]);
        assert!(matches!(
            payload.verify_sig(origin.pubkey()),
            Err(ValidationError::Signature(_))
        ));
    }

    #[test]
    fn json_round_trip_preserves_payload() {
        let origin = Keypair::generate();
        let payload = build_signed_announce(&origin, 7, 1_700_000_500);
        let json = serde_json::to_string(&payload).unwrap();
        let back: AnnouncePayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back, payload);
        assert!(back.verify_sig(origin.pubkey()).is_ok());
    }

    #[test]
    fn json_shape_matches_spec_5_4() {
        let origin = Keypair::generate();
        let mut payload = build_signed_announce(&origin, 42, 1_700_000_000);
        payload.auth_rotated_at = Some(1_699_999_000);
        payload.cert_rotated_at = Some(1_699_999_500);
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = origin.sign(&canonical);

        let value: serde_json::Value = serde_json::to_value(&payload).unwrap();
        assert_eq!(value["spec"], "mcp-announce/v0.1");
        assert_eq!(value["lid"], "bodylog-7f3a");
        assert_eq!(value["backend"]["url"], "https://10.0.0.42:54321/");
        assert_eq!(value["seq"], 42);
        assert_eq!(value["exp"], 1_700_000_000);
        assert_eq!(value["auth_rotated_at"], 1_699_999_000);
        assert_eq!(value["cert_rotated_at"], 1_699_999_500);
        // sig is the only field NOT covered by canonical bytes — it
        // must round-trip but the value is opaque to readers.
        assert!(value.get("sig").is_some());
    }

    #[test]
    fn optional_rotation_fields_are_omitted_when_none() {
        let origin = Keypair::generate();
        let payload = build_signed_announce(&origin, 1, 1_700_000_000);
        let value: serde_json::Value = serde_json::to_value(&payload).unwrap();
        assert!(value.get("auth_rotated_at").is_none());
        assert!(value.get("cert_rotated_at").is_none());
    }

    #[test]
    fn unknown_spec_is_rejected() {
        let bad = r#"{
            "spec": "mcp-announce/v9.99",
            "lid": "bodylog-7f3a",
            "backend": {
                "url": "https://10.0.0.42:54321/",
                "fp": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            },
            "seq": 1,
            "exp": 1700000000,
            "sig": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        }"#;
        let r: Result<AnnouncePayload, _> = serde_json::from_str(bad);
        assert!(r.is_err());
    }

    #[test]
    fn canonical_bytes_omit_sig_field() {
        let origin = Keypair::generate();
        let payload = build_signed_announce(&origin, 1, 1_700_000_000);
        let canonical = payload.canonical_signing_bytes().unwrap();
        let text = std::str::from_utf8(&canonical).unwrap();
        assert!(!text.contains("\"sig\""));
        // spot-check JCS key ordering: keys must be lexicographic.
        let lid_pos = text.find("\"lid\"").unwrap();
        let seq_pos = text.find("\"seq\"").unwrap();
        assert!(lid_pos < seq_pos, "lid must precede seq in canonical JSON");
    }
}
