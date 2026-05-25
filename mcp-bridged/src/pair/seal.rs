//! `crypto_box`-style sealed-envelope open/seal for Direction-B pair
//! payloads.
//!
//! Implements [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.4 Direction-B
//! sealing — libsodium's `crypto_box_seal` algorithm built on top of
//! [`crypto_box::SalsaBox`]:
//!
//! 1. Sender generates an ephemeral X25519 keypair.
//! 2. Nonce = `blake2b(eph_pubkey || receiver_pubkey, 24 bytes)`.
//! 3. Ciphertext = `SalsaBox::new(receiver_pk, eph_sk).encrypt(nonce, plaintext)`.
//! 4. Sealed envelope = `eph_pubkey || ciphertext`.
//!
//! Opening reverses the process using the receiver's secret key. The
//! ephemeral pubkey gives the receiver everything it needs to reconstruct
//! the same nonce and run `SalsaBox::decrypt`.

use blake2::Blake2bVar;
use blake2::digest::{Update, VariableOutput};
use crypto_box::Nonce;
use crypto_box::aead::Aead;
use crypto_box::{PublicKey, SalsaBox, SecretKey};
use rand::rngs::OsRng;
use thiserror::Error;

use crate::identity::{Ed25519Pubkey, Keypair};

/// Length of the ephemeral X25519 pubkey prefix.
const EPHEMERAL_PUB_LEN: usize = 32;

/// Length of the XSalsa20 nonce SalsaBox uses.
const NONCE_LEN: usize = 24;

/// Length of the SalsaBox MAC tag.
const TAG_LEN: usize = 16;

/// Seal `plaintext` to `receiver`'s public key. Returns
/// `eph_pubkey || salsa_box_ciphertext`.
///
/// The ephemeral X25519 keypair is generated fresh from [`OsRng`] for each
/// seal; the secret half is dropped before this function returns.
pub fn seal_to(plaintext: &[u8], receiver: &Ed25519Pubkey) -> Result<Vec<u8>, SealError> {
    let receiver_x_bytes = receiver
        .to_x25519_bytes()
        .map_err(|_| SealError::InvalidReceiverPubkey)?;
    let receiver_x = PublicKey::from(receiver_x_bytes);

    let eph_sk = SecretKey::generate(&mut OsRng);
    let eph_pk_bytes: [u8; 32] = *eph_sk.public_key().as_bytes();

    let nonce = derive_nonce(&eph_pk_bytes, &receiver_x_bytes);
    let salsa = SalsaBox::new(&receiver_x, &eph_sk);
    let ciphertext = salsa
        .encrypt(&nonce, plaintext)
        .map_err(|_| SealError::Encrypt)?;

    let mut out = Vec::with_capacity(EPHEMERAL_PUB_LEN + ciphertext.len());
    out.extend_from_slice(&eph_pk_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Open a sealed envelope addressed to `receiver`.
pub fn open_with(sealed: &[u8], receiver: &Keypair) -> Result<Vec<u8>, OpenError> {
    if sealed.len() < EPHEMERAL_PUB_LEN + TAG_LEN {
        return Err(OpenError::TooShort);
    }
    let (eph_pk_slice, ciphertext) = sealed.split_at(EPHEMERAL_PUB_LEN);
    let eph_pk_bytes: [u8; 32] = eph_pk_slice
        .try_into()
        .expect("split_at guarantees 32-byte prefix");
    let eph_pk = PublicKey::from(eph_pk_bytes);

    let receiver_x_secret = SecretKey::from(receiver.x25519_secret_bytes());
    let receiver_x_pubkey: PublicKey = receiver_x_secret.public_key();
    let nonce = derive_nonce(&eph_pk_bytes, receiver_x_pubkey.as_bytes());

    let salsa = SalsaBox::new(&eph_pk, &receiver_x_secret);
    salsa
        .decrypt(&nonce, ciphertext)
        .map_err(|_| OpenError::Decrypt)
}

/// Libsodium `crypto_box_seal` nonce derivation:
/// `blake2b(eph_pk || recv_pk, output = 24 bytes)`.
fn derive_nonce(eph_pk: &[u8; 32], recv_pk: &[u8; 32]) -> Nonce {
    let mut hasher = Blake2bVar::new(NONCE_LEN).expect("blake2b accepts a 24-byte output");
    hasher.update(eph_pk);
    hasher.update(recv_pk);
    let mut out = [0u8; NONCE_LEN];
    hasher
        .finalize_variable(&mut out)
        .expect("output buffer matches the configured size");
    Nonce::from(out)
}

/// Failure modes for [`seal_to`].
#[derive(Debug, Error)]
pub enum SealError {
    #[error("receiver pubkey is not a valid Ed25519 point")]
    InvalidReceiverPubkey,
    #[error("encryption failed (unexpected — SalsaBox does not fail on valid keys)")]
    Encrypt,
}

/// Failure modes for [`open_with`].
#[derive(Debug, Error)]
pub enum OpenError {
    #[error("sealed envelope is shorter than the {EPHEMERAL_PUB_LEN}+{TAG_LEN}-byte minimum")]
    TooShort,
    #[error("decryption failed: wrong receiver, tampered ciphertext, or unrelated bytes")]
    Decrypt,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_recovers_the_plaintext() {
        let receiver = Keypair::generate();
        let plaintext = b"the quick brown fox jumps over the lazy dog";

        let sealed = seal_to(plaintext, receiver.pubkey()).unwrap();
        let opened = open_with(&sealed, &receiver).unwrap();

        assert_eq!(opened, plaintext);
    }

    #[test]
    fn round_trip_handles_empty_plaintext() {
        let receiver = Keypair::generate();
        let sealed = seal_to(b"", receiver.pubkey()).unwrap();
        let opened = open_with(&sealed, &receiver).unwrap();
        assert!(opened.is_empty());
    }

    #[test]
    fn each_seal_uses_a_fresh_ephemeral_keypair() {
        let receiver = Keypair::generate();
        let plaintext = b"identical inputs";

        let a = seal_to(plaintext, receiver.pubkey()).unwrap();
        let b = seal_to(plaintext, receiver.pubkey()).unwrap();

        // First 32 bytes are the ephemeral pubkey — must differ between seals.
        assert_ne!(&a[..EPHEMERAL_PUB_LEN], &b[..EPHEMERAL_PUB_LEN]);
        // And so the whole sealed envelope must differ.
        assert_ne!(a, b);
    }

    #[test]
    fn open_with_wrong_receiver_fails() {
        let intended = Keypair::generate();
        let imposter = Keypair::generate();
        let sealed = seal_to(b"hello", intended.pubkey()).unwrap();

        let err = open_with(&sealed, &imposter).unwrap_err();
        assert!(matches!(err, OpenError::Decrypt));
    }

    #[test]
    fn open_rejects_tampered_ciphertext() {
        let receiver = Keypair::generate();
        let mut sealed = seal_to(b"hello", receiver.pubkey()).unwrap();
        // Flip a bit in the ciphertext body (past the ephemeral-pubkey prefix).
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;

        let err = open_with(&sealed, &receiver).unwrap_err();
        assert!(matches!(err, OpenError::Decrypt));
    }

    #[test]
    fn open_rejects_tampered_ephemeral_pubkey() {
        let receiver = Keypair::generate();
        let mut sealed = seal_to(b"hello", receiver.pubkey()).unwrap();
        // Flip a bit in the ephemeral-pubkey prefix → nonce mismatch on
        // open and the integrity check fails.
        sealed[0] ^= 0x01;

        let err = open_with(&sealed, &receiver).unwrap_err();
        assert!(matches!(err, OpenError::Decrypt));
    }

    #[test]
    fn open_rejects_too_short_input() {
        let receiver = Keypair::generate();
        let err = open_with(&[0u8; 16], &receiver).unwrap_err();
        assert!(matches!(err, OpenError::TooShort));
    }

    /// End-to-end: build a real PairPayload, serialize it, seal to the
    /// Resolver, open it, deserialize, and run the full
    /// `validate_direction_b` flow. This is the canonical Direction-B
    /// receive path minus the HTTP context and nonce-register lookup.
    #[test]
    fn end_to_end_with_real_pair_payload() {
        use crate::identity::DisplayName;
        use crate::identity::Signature;
        use crate::pair::auth::{Auth, AuthType};
        use crate::pair::backend_url::BackendUrl;
        use crate::pair::bearer_token::BearerToken;
        use crate::pair::cert_fingerprint::CertFingerprint;
        use crate::pair::invite::{Direction, SpecVersion};
        use crate::pair::logical_id::LogicalId;
        use crate::pair::nonce::Nonce;
        use crate::pair::payload::{BackendInfo, OriginInfo, PairPayload, Scope};

        let origin = Keypair::generate();
        let resolver = Keypair::generate();

        // Build the signed payload.
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

        // Origin side: serialize, seal to the Resolver's pubkey.
        let plaintext = serde_json::to_vec(&payload).unwrap();
        let sealed = seal_to(&plaintext, resolver.pubkey()).unwrap();

        // Resolver side: open, deserialize, validate.
        let opened = open_with(&sealed, &resolver).unwrap();
        let received: PairPayload = serde_json::from_slice(&opened).unwrap();
        assert_eq!(received, payload);
        received.validate_direction_b(resolver.pubkey()).unwrap();
    }
}
