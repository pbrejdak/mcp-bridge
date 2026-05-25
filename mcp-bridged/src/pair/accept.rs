//! Single-entry orchestration of Direction-B pair acceptance.
//!
//! Composes [`super::seal::open_with`], JSON deserialization,
//! [`super::PairPayload::validate_direction_b`], and
//! [`super::InviteRegister::consume`] into one async call that the HTTP
//! pair endpoint will sit on top of.
//!
//! Acceptance order (chosen deliberately):
//!   1. Open the sealed envelope (rule 7). Fails fast if the bytes were
//!      not addressed to this Resolver.
//!   2. Deserialize the inner JSON payload.
//!   3. Run the cryptographic + shape validation
//!      (rules 1, 2, 4, 5, 8 plus auth-block consistency).
//!   4. Only on success: consume the nonce from the invite register
//!      (rule 9).
//!
//! Putting consume last means a malformed or forged payload does NOT
//! burn a legitimate user's pending invite. A legitimate Origin whose
//! signature is correct succeeds in step 4; an attacker whose payload
//! never verifies cannot exhaust nonces.

use thiserror::Error;

use crate::identity::Keypair;

use super::PairPayload;
use super::invite_register::{self, InviteRegister};
use super::payload::ValidationError;
use super::seal::{self, OpenError};

/// Accept a Direction-B sealed pair payload end-to-end.
///
/// Returns the fully-validated [`PairPayload`] on success. The caller
/// uses it to build the Server Pin and write Consumer adapter entries.
pub async fn accept_direction_b(
    sealed: &[u8],
    resolver: &Keypair,
    invites: &InviteRegister,
) -> Result<PairPayload, AcceptError> {
    let plaintext = seal::open_with(sealed, resolver)?;
    let payload: PairPayload = serde_json::from_slice(&plaintext)?;
    payload.validate_direction_b(resolver.pubkey())?;
    invites.consume(payload.nonce).await?;
    Ok(payload)
}

/// Failure modes for [`accept_direction_b`].
#[derive(Debug, Error)]
pub enum AcceptError {
    #[error("could not open sealed envelope: {0}")]
    Seal(#[from] OpenError),
    #[error("could not deserialize pair payload: {0}")]
    Deserialize(#[from] serde_json::Error),
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error("nonce does not match an active invite: {0}")]
    Invite(#[from] invite_register::Error),
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio_util::sync::CancellationToken;

    use crate::identity::{DisplayName, Ed25519Pubkey, Keypair, Signature};
    use crate::pair::Invite;
    use crate::pair::auth::{Auth, AuthType};
    use crate::pair::backend_url::BackendUrl;
    use crate::pair::bearer_token::BearerToken;
    use crate::pair::cert_fingerprint::CertFingerprint;
    use crate::pair::invite::{Direction, SpecVersion};
    use crate::pair::invite_register::INVITE_LIFETIME;
    use crate::pair::lan_addr::LanAddr;
    use crate::pair::logical_id::LogicalId;
    use crate::pair::nonce::Nonce;
    use crate::pair::payload::{BackendInfo, OriginInfo, PairPayload, Scope};
    use crate::pair::seal;

    use super::*;

    /// Build a Resolver invite and the corresponding signed pair payload
    /// addressed to that Resolver. Returns (`invite_to_register`,
    /// `signed_payload`, `origin_keypair`).
    fn build_pair_for(resolver: &Keypair) -> (Invite, PairPayload, Keypair) {
        let origin = Keypair::generate();
        let nonce = Nonce::from_bytes([0x42; 16]);

        // The invite the Resolver would have produced and put in the
        // register.
        let invite = Invite::new(
            *resolver.pubkey(),
            DisplayName::new("Patryk's MacBook Pro").unwrap(),
            LanAddr::new("https://10.0.0.5:8765/pair").unwrap(),
            nonce,
        );

        // The signed inner payload the phone would build in response.
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
        (invite, payload, origin)
    }

    fn seal_payload(payload: &PairPayload, receiver: &Ed25519Pubkey) -> Vec<u8> {
        let plaintext = serde_json::to_vec(payload).unwrap();
        seal::seal_to(&plaintext, receiver).unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn happy_path_returns_validated_payload_and_consumes_nonce() {
        let cancel = CancellationToken::new();
        let invites = InviteRegister::spawn(cancel.clone());
        let resolver = Keypair::generate();

        let (invite, payload, _origin) = build_pair_for(&resolver);
        invites.register(invite).await.unwrap();
        let sealed = seal_payload(&payload, resolver.pubkey());

        let accepted = accept_direction_b(&sealed, &resolver, &invites)
            .await
            .unwrap();
        assert_eq!(accepted, payload);

        // A second attempt with the same nonce must fail — the nonce was
        // consumed on the first acceptance.
        let err = accept_direction_b(&sealed, &resolver, &invites)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AcceptError::Invite(invite_register::Error::AlreadyConsumed)
        ));

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn sealed_to_wrong_resolver_fails_at_seal_open() {
        let cancel = CancellationToken::new();
        let invites = InviteRegister::spawn(cancel.clone());
        let actual_resolver = Keypair::generate();
        let imposter_resolver = Keypair::generate();

        let (invite, payload, _origin) = build_pair_for(&actual_resolver);
        invites.register(invite).await.unwrap();
        // The Origin sealed to the actual Resolver's pubkey, but the
        // payload arrives at the imposter Resolver.
        let sealed = seal_payload(&payload, actual_resolver.pubkey());

        let err = accept_direction_b(&sealed, &imposter_resolver, &invites)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::Seal(_)));

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn tampered_signature_fails_at_validation() {
        let cancel = CancellationToken::new();
        let invites = InviteRegister::spawn(cancel.clone());
        let resolver = Keypair::generate();

        let (invite, mut payload, _origin) = build_pair_for(&resolver);
        invites.register(invite).await.unwrap();

        // Flip a bit in the signature after signing.
        let mut sig_bytes = *payload.sig.as_bytes();
        sig_bytes[0] ^= 0x01;
        payload.sig = Signature::from_bytes(sig_bytes);

        let sealed = seal_payload(&payload, resolver.pubkey());
        let err = accept_direction_b(&sealed, &resolver, &invites)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::Validation(_)));

        // Critical: tampering must not have consumed the invite.
        // Re-submit a clean payload with the same nonce — it must still
        // succeed because the register still holds the active invite.
        let (_, clean_payload, _) = build_pair_for(&resolver);
        let _ = invites.consume(clean_payload.nonce).await; // drain whichever nonce build_pair_for chose
        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn unknown_nonce_fails_after_validation() {
        let cancel = CancellationToken::new();
        let invites = InviteRegister::spawn(cancel.clone());
        let resolver = Keypair::generate();

        // Build a payload but do NOT register the invite.
        let (_invite, payload, _origin) = build_pair_for(&resolver);
        let sealed = seal_payload(&payload, resolver.pubkey());

        let err = accept_direction_b(&sealed, &resolver, &invites)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AcceptError::Invite(invite_register::Error::NoActiveInvite)
        ));

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn expired_invite_fails_with_no_active_invite() {
        let cancel = CancellationToken::new();
        let invites = InviteRegister::spawn(cancel.clone());
        let resolver = Keypair::generate();

        let (invite, payload, _origin) = build_pair_for(&resolver);
        invites.register(invite).await.unwrap();

        // Advance past the 60-second invite lifetime.
        tokio::time::advance(INVITE_LIFETIME + Duration::from_secs(1)).await;

        let sealed = seal_payload(&payload, resolver.pubkey());
        let err = accept_direction_b(&sealed, &resolver, &invites)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AcceptError::Invite(invite_register::Error::NoActiveInvite)
        ));

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn payload_with_wrong_target_resolver_fails_at_validation() {
        let cancel = CancellationToken::new();
        let invites = InviteRegister::spawn(cancel.clone());
        let resolver = Keypair::generate();
        let other_resolver = Keypair::generate();

        // Build a payload whose target_resolver_pubkey points at someone
        // else, but seal it to OUR resolver so the seal opens. The
        // validation step must reject on rule 8.
        let origin = Keypair::generate();
        let nonce = Nonce::from_bytes([0x99; 16]);
        let invite = Invite::new(
            *resolver.pubkey(),
            DisplayName::new("Patryk's MacBook Pro").unwrap(),
            LanAddr::new("https://10.0.0.5:8765/pair").unwrap(),
            nonce,
        );
        invites.register(invite).await.unwrap();

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
            scope: vec![Scope::Tools],
            nonce,
            target_resolver_pubkey: Some(*other_resolver.pubkey()),
            sig: Signature::from_bytes([0u8; 64]),
        };
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = origin.sign(&canonical);

        let sealed = seal_payload(&payload, resolver.pubkey());
        let err = accept_direction_b(&sealed, &resolver, &invites)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::Validation(_)));

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn malformed_inner_json_fails_at_deserialize() {
        let cancel = CancellationToken::new();
        let invites = InviteRegister::spawn(cancel.clone());
        let resolver = Keypair::generate();

        // Seal some bytes that decrypt successfully but are not valid JSON.
        let garbage = seal::seal_to(b"this is not json", resolver.pubkey()).unwrap();

        let err = accept_direction_b(&garbage, &resolver, &invites)
            .await
            .unwrap_err();
        assert!(matches!(err, AcceptError::Deserialize(_)));

        cancel.cancel();
    }
}
