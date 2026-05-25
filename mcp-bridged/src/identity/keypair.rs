//! Ed25519 keypair wrapper.
//!
//! Holds an [`ed25519_dalek::SigningKey`] (with the `zeroize` feature) plus
//! its derived [`Ed25519Pubkey`]. The private bytes are zeroed on drop;
//! `Debug` redacts the private half so accidentally printing a `Keypair`
//! never leaks key material.

use std::fmt;

use ed25519_dalek::ed25519::signature::Signer;
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;

use super::pubkey::Ed25519Pubkey;
use super::signature::Signature;

/// An Ed25519 keypair. Private bytes are zeroized on drop by
/// `ed25519_dalek` when the `zeroize` feature is enabled.
pub struct Keypair {
    signing: SigningKey,
    public: Ed25519Pubkey,
}

impl Keypair {
    /// Generate a fresh keypair from the OS CSPRNG.
    #[must_use]
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        let public = Ed25519Pubkey::from_bytes(signing.verifying_key().to_bytes());
        Self { signing, public }
    }

    /// Reconstruct a keypair from a 32-byte seed. Useful for tests and for
    /// loading from the OS keychain.
    #[must_use]
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(&seed);
        let public = Ed25519Pubkey::from_bytes(signing.verifying_key().to_bytes());
        Self { signing, public }
    }

    /// Borrow the public half.
    #[must_use]
    pub fn pubkey(&self) -> &Ed25519Pubkey {
        &self.public
    }

    /// Sign `message` with the private half.
    pub fn sign(&self, message: &[u8]) -> Signature {
        let sig = self.signing.sign(message);
        Signature::from_bytes(sig.to_bytes())
    }
}

impl fmt::Debug for Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // finish_non_exhaustive emits `..` so clippy understands the elided
        // field (the private signing key) is intentional.
        f.debug_struct("Keypair")
            .field("pubkey", &self.public)
            .finish_non_exhaustive()
    }
}

/// Verify `signature` over `message` against this public key.
///
/// Uses `verify_strict`, which rejects non-canonical signatures
/// (malleability guard).
pub fn verify(
    pubkey: &Ed25519Pubkey,
    message: &[u8],
    signature: &Signature,
) -> Result<(), VerifyError> {
    let key = VerifyingKey::from_bytes(pubkey.as_bytes()).map_err(|_| VerifyError::InvalidKey)?;
    let sig = ed25519_dalek::Signature::from_bytes(signature.as_bytes());
    key.verify_strict(message, &sig)
        .map_err(|_| VerifyError::Mismatch)
}

/// Failure modes for [`verify`].
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("public key is not on the Ed25519 curve")]
    InvalidKey,
    #[error("signature does not match")]
    Mismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_distinct_keypairs() {
        let a = Keypair::generate();
        let b = Keypair::generate();
        assert_ne!(a.pubkey(), b.pubkey());
    }

    #[test]
    fn from_seed_is_deterministic() {
        let a = Keypair::from_seed([7; 32]);
        let b = Keypair::from_seed([7; 32]);
        assert_eq!(a.pubkey(), b.pubkey());
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let kp = Keypair::generate();
        let msg = b"the quick brown fox jumps over the lazy dog";
        let sig = kp.sign(msg);
        verify(kp.pubkey(), msg, &sig).expect("signature should verify");
    }

    #[test]
    fn verify_rejects_modified_message() {
        let kp = Keypair::generate();
        let msg = b"original message";
        let sig = kp.sign(msg);
        let tampered = b"modified message";
        assert!(matches!(
            verify(kp.pubkey(), tampered, &sig),
            Err(VerifyError::Mismatch)
        ));
    }

    #[test]
    fn verify_rejects_wrong_pubkey() {
        let signer = Keypair::generate();
        let other = Keypair::generate();
        let msg = b"hello";
        let sig = signer.sign(msg);
        assert!(matches!(
            verify(other.pubkey(), msg, &sig),
            Err(VerifyError::Mismatch)
        ));
    }

    #[test]
    fn verify_rejects_swapped_signature() {
        let kp = Keypair::generate();
        let a = b"message A";
        let b = b"message B";
        let sig_for_a = kp.sign(a);
        assert!(matches!(
            verify(kp.pubkey(), b, &sig_for_a),
            Err(VerifyError::Mismatch)
        ));
    }

    #[test]
    fn debug_redacts_private_bytes() {
        use std::fmt::Write as _;

        let kp = Keypair::generate();
        let dbg = format!("{kp:?}");
        assert!(
            dbg.contains(".."),
            "Debug should mark elided fields with `..`"
        );
        assert!(dbg.contains(&kp.pubkey().to_string()));
        // The signing key's raw bytes must not appear in Debug output.
        let mut private_hex = String::with_capacity(64);
        for b in kp.signing.to_bytes() {
            write!(private_hex, "{b:02x}").expect("write to String never fails");
        }
        assert!(
            !dbg.contains(&private_hex),
            "Debug output leaked private-key hex"
        );
    }
}
