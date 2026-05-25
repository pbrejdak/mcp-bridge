//! `mcp-pair/v0.1` signed pair payload (the inner, signed body sent by
//! the Origin to the Resolver).
//!
//! See [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.4 for the JSON shape
//! and §4.5 for the acceptance rules.
//!
//! This module covers the rules a typed deserializer can enforce plus the
//! cryptographic signature check (rule 4) and the target-Resolver binding
//! (rule 8). Rule 9 (nonce against the active invite register), rule 6
//! (TLS fingerprint match — runs at connection time), and rule 10 (HTTP
//! context) belong to layers outside this file.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::identity::{self, Ed25519Pubkey, Signature, VerifyError};

use super::auth::{Auth, AuthError};
use super::backend_url::BackendUrl;
use super::cert_fingerprint::CertFingerprint;
use super::invite::{Direction, SpecVersion};
use super::logical_id::LogicalId;
use super::nonce::Nonce;
use crate::identity::DisplayName;

/// MCP capabilities a Pin exposes. Reserved future values rejected by serde.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Tools,
    Resources,
    Prompts,
}

/// The Origin-side identity carried in the pair payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginInfo {
    pub name: DisplayName,
    pub pubkey: Ed25519Pubkey,
    pub logical_id: LogicalId,
}

/// The Origin backend's network identity carried in the pair payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendInfo {
    pub url: BackendUrl,
    pub fp: CertFingerprint,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ca: Option<String>,
}

/// A signed `mcp-pair/v0.1` payload — the body of the sealed
/// Direction-B envelope (after unsealing) or the QR contents in
/// Direction A.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairPayload {
    pub spec: SpecVersion,
    pub direction: Direction,
    pub origin: OriginInfo,
    pub backend: BackendInfo,
    pub auth: Auth,
    pub scope: Vec<Scope>,
    pub nonce: Nonce,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target_resolver_pubkey: Option<Ed25519Pubkey>,
    pub sig: Signature,
}

/// Same field set as [`PairPayload`] without `sig`. Serialized form is
/// what the signature is computed over (RFC 8785 / JCS canonicalization).
#[derive(Debug, Serialize)]
struct PairPayloadUnsigned<'a> {
    spec: &'a SpecVersion,
    direction: &'a Direction,
    origin: &'a OriginInfo,
    backend: &'a BackendInfo,
    auth: &'a Auth,
    scope: &'a [Scope],
    nonce: &'a Nonce,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_resolver_pubkey: &'a Option<Ed25519Pubkey>,
}

impl PairPayload {
    /// Build the field set that the signature covers.
    fn unsigned(&self) -> PairPayloadUnsigned<'_> {
        PairPayloadUnsigned {
            spec: &self.spec,
            direction: &self.direction,
            origin: &self.origin,
            backend: &self.backend,
            auth: &self.auth,
            scope: &self.scope,
            nonce: &self.nonce,
            target_resolver_pubkey: &self.target_resolver_pubkey,
        }
    }

    /// Produce the RFC 8785 canonical-JSON bytes of the field set covered
    /// by the signature. Used both by signers (to sign) and verifiers (to
    /// re-derive the message and check).
    pub fn canonical_signing_bytes(&self) -> Result<Vec<u8>, ValidationError> {
        serde_jcs::to_vec(&self.unsigned()).map_err(ValidationError::Canonicalize)
    }

    /// Validate the cryptographic and shape rules from SPEC.md §4.5 that
    /// this layer owns:
    ///
    /// - rule 1 — `spec` value (enforced by [`SpecVersion`] deserialization).
    /// - rule 2 — direction matches Direction-B.
    /// - rule 4 — `sig` validates against `origin.pubkey`.
    /// - rule 8 — `target_resolver_pubkey` equals the Resolver's pubkey.
    /// - shape — `auth` block is internally consistent.
    ///
    /// Callers MUST additionally enforce:
    /// - rule 9 — nonce belongs to an unexpired unconsumed invite (see
    ///   [`super::InviteRegister`]).
    /// - rule 6 — TLS fingerprint match at the time of backend connect.
    /// - rule 10 — POST received on the advertised IP/port.
    pub fn validate_direction_b(
        &self,
        resolver_pubkey: &Ed25519Pubkey,
    ) -> Result<(), ValidationError> {
        if self.direction != Direction::ResolverOffered {
            return Err(ValidationError::WrongDirection {
                got: self.direction,
            });
        }
        self.auth.validate()?;
        let target = self
            .target_resolver_pubkey
            .as_ref()
            .ok_or(ValidationError::MissingTargetResolverPubkey)?;
        if target != resolver_pubkey {
            return Err(ValidationError::TargetResolverMismatch);
        }
        let canonical = self.canonical_signing_bytes()?;
        identity::verify(&self.origin.pubkey, &canonical, &self.sig)?;
        Ok(())
    }
}

/// Failure modes for [`PairPayload::validate_direction_b`].
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("direction must be `resolver_offered`, got `{got:?}`")]
    WrongDirection { got: Direction },
    #[error("missing `target_resolver_pubkey` for Direction-B payload")]
    MissingTargetResolverPubkey,
    #[error("target_resolver_pubkey does not match the Resolver's pubkey")]
    TargetResolverMismatch,
    #[error("auth block is inconsistent: {0}")]
    Auth(#[from] AuthError),
    #[error("signature verification failed: {0}")]
    Signature(#[from] VerifyError),
    #[error("could not canonicalize payload: {0}")]
    Canonicalize(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::identity::{DisplayName, Keypair};
    use crate::pair::bearer_token::BearerToken;

    /// Construct a fully-signed payload bound to `resolver_pubkey` and
    /// signed by `origin_kp`. The signature covers the canonical-JSON
    /// rendering of every field except `sig`.
    fn build_signed_payload(origin_kp: &Keypair, resolver_pubkey: &Ed25519Pubkey) -> PairPayload {
        // Build an unsigned skeleton with a placeholder signature, then
        // overwrite the sig with the real one computed over the canonical
        // bytes of the rest.
        let mut payload = PairPayload {
            spec: SpecVersion::McpPairV0_1,
            direction: Direction::ResolverOffered,
            origin: OriginInfo {
                name: DisplayName::new("BodyLog").unwrap(),
                pubkey: *origin_kp.pubkey(),
                logical_id: LogicalId::new("bodylog-7f3a").unwrap(),
            },
            backend: BackendInfo {
                url: BackendUrl::new("https://10.0.0.42:54321/").unwrap(),
                fp: CertFingerprint::from_bytes([0xab; 32]),
                ca: None,
            },
            auth: Auth {
                ty: crate::pair::auth::AuthType::Bearer,
                value: Some(BearerToken::new("token-abc").unwrap()),
            },
            scope: vec![Scope::Tools, Scope::Resources],
            nonce: Nonce::from_bytes([0u8; 16]),
            target_resolver_pubkey: Some(*resolver_pubkey),
            sig: Signature::from_bytes([0u8; 64]),
        };
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = origin_kp.sign(&canonical);
        payload
    }

    #[test]
    fn validates_correctly_signed_payload() {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
        let payload = build_signed_payload(&origin, resolver.pubkey());

        payload.validate_direction_b(resolver.pubkey()).unwrap();
    }

    #[test]
    fn rejects_wrong_direction() {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
        let mut payload = build_signed_payload(&origin, resolver.pubkey());
        payload.direction = Direction::OriginOffered;

        let err = payload.validate_direction_b(resolver.pubkey()).unwrap_err();
        assert!(matches!(err, ValidationError::WrongDirection { .. }));
    }

    #[test]
    fn rejects_wrong_target_resolver_pubkey() {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
        let other_resolver = Keypair::generate();
        let payload = build_signed_payload(&origin, resolver.pubkey());

        let err = payload
            .validate_direction_b(other_resolver.pubkey())
            .unwrap_err();
        assert!(matches!(err, ValidationError::TargetResolverMismatch));
    }

    #[test]
    fn rejects_missing_target_resolver_pubkey() {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
        let mut payload = build_signed_payload(&origin, resolver.pubkey());
        payload.target_resolver_pubkey = None;

        let err = payload.validate_direction_b(resolver.pubkey()).unwrap_err();
        assert!(matches!(err, ValidationError::MissingTargetResolverPubkey));
    }

    #[test]
    fn rejects_tampered_payload() {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
        let mut payload = build_signed_payload(&origin, resolver.pubkey());

        // Tamper with a non-signature field after signing.
        payload.scope = vec![Scope::Prompts];

        let err = payload.validate_direction_b(resolver.pubkey()).unwrap_err();
        assert!(matches!(err, ValidationError::Signature(_)));
    }

    #[test]
    fn rejects_signature_from_wrong_origin() {
        let origin = Keypair::generate();
        let imposter = Keypair::generate();
        let resolver = Keypair::generate();

        // Build a payload as if `origin` were the signer, but actually
        // sign with `imposter`'s key. The origin.pubkey field still claims
        // it was signed by `origin`, so verification must reject.
        let mut payload = build_signed_payload(&origin, resolver.pubkey());
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = imposter.sign(&canonical);

        let err = payload.validate_direction_b(resolver.pubkey()).unwrap_err();
        assert!(matches!(err, ValidationError::Signature(_)));
    }

    #[test]
    fn rejects_inconsistent_auth_block() {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
        let mut payload = build_signed_payload(&origin, resolver.pubkey());
        // bearer type but no value
        payload.auth = Auth {
            ty: crate::pair::auth::AuthType::Bearer,
            value: None,
        };
        // re-sign so the signature check itself doesn't fire first
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = origin.sign(&canonical);

        let err = payload.validate_direction_b(resolver.pubkey()).unwrap_err();
        assert!(matches!(err, ValidationError::Auth(_)));
    }

    #[test]
    fn canonical_bytes_omit_sig_field() {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
        let payload = build_signed_payload(&origin, resolver.pubkey());

        let bytes = payload.canonical_signing_bytes().unwrap();
        let json = std::str::from_utf8(&bytes).unwrap();
        assert!(
            !json.contains("\"sig\""),
            "canonical bytes contain sig: {json}"
        );
    }

    #[test]
    fn round_trips_through_json() {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
        let payload = build_signed_payload(&origin, resolver.pubkey());

        let json = serde_json::to_string(&payload).unwrap();
        let back: PairPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back, payload);
        back.validate_direction_b(resolver.pubkey()).unwrap();
    }
}
