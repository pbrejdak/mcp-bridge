//! OS keychain-backed persistence for the Resolver's Ed25519 keypair.
//!
//! Per [`docs/DAEMON.md`](../../../../docs/DAEMON.md) §7.2 and
//! [`mcp-bridged/CLAUDE.md`](../../../CLAUDE.md) §6: keypair lives in
//! the OS keychain via the [`keyring`] crate (macOS Security framework,
//! Windows credential vault, Linux Secret Service).
//!
//! Storage shape: a single keychain entry under
//! `(service="dev.mcpbridge.mcp-bridged", account="resolver-keypair-v1")`
//! whose value is the base64url-no-pad encoding of the 32-byte Ed25519
//! seed. Versioning the account name lets us migrate without touching
//! the previous entry if a future release changes the encoding.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use keyring::Entry;
use thiserror::Error;

use super::keypair::Keypair;

/// Keychain service identifier — matches the `directories` qualifier
/// used in [`crate::config`].
pub const KEYCHAIN_SERVICE: &str = "dev.mcpbridge.mcp-bridged";

/// Keychain account name for the Resolver keypair. Bumping the `v1`
/// suffix lets us migrate to a new encoding without colliding with the
/// old entry.
pub const RESOLVER_KEYPAIR_ACCOUNT: &str = "resolver-keypair-v1";

/// Handle to the OS keychain, scoped to one logical service.
///
/// Holds the underlying `keyring::Entry` directly so that save / load /
/// delete operations all reference the same backend object — this is
/// also necessary for the `keyring::mock` backend used in tests, which
/// stores secrets per-`Entry` rather than per-(service, account).
#[derive(Debug)]
pub struct Keystore {
    resolver_keypair_entry: Entry,
}

impl Keystore {
    /// Default keystore using the canonical service name.
    pub fn new() -> Result<Self, KeystoreError> {
        Self::for_service(KEYCHAIN_SERVICE)
    }

    /// Construct a keystore scoped to a custom service name. Used by
    /// tests so parallel runs don't collide, and by future multi-tenant
    /// daemon flavours.
    pub fn for_service(service: &str) -> Result<Self, KeystoreError> {
        let entry =
            Entry::new(service, RESOLVER_KEYPAIR_ACCOUNT).map_err(KeystoreError::Backend)?;
        Ok(Self {
            resolver_keypair_entry: entry,
        })
    }

    /// Load the Resolver keypair if one is stored, `Ok(None)` otherwise.
    pub fn load_resolver_keypair(&self) -> Result<Option<Keypair>, KeystoreError> {
        match self.resolver_keypair_entry.get_password() {
            Ok(encoded) => Ok(Some(Keypair::from_seed(decode_seed(&encoded)?))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeystoreError::Backend(e)),
        }
    }

    /// Save the Resolver keypair, overwriting any existing entry.
    pub fn save_resolver_keypair(&self, kp: &Keypair) -> Result<(), KeystoreError> {
        let seed = kp.to_seed_bytes();
        let encoded = encode_seed(&seed);
        self.resolver_keypair_entry
            .set_password(&encoded)
            .map_err(KeystoreError::Backend)?;
        Ok(())
    }

    /// Delete the Resolver keypair from the keychain. No-op if absent.
    pub fn delete_resolver_keypair(&self) -> Result<(), KeystoreError> {
        match self.resolver_keypair_entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeystoreError::Backend(e)),
        }
    }

    /// Load the Resolver keypair, generating + saving a fresh one if no
    /// entry exists. The "first launch" idiom for the daemon.
    pub fn load_or_generate_resolver_keypair(&self) -> Result<Keypair, KeystoreError> {
        if let Some(kp) = self.load_resolver_keypair()? {
            return Ok(kp);
        }
        let kp = Keypair::generate();
        self.save_resolver_keypair(&kp)?;
        Ok(kp)
    }
}

/// Base64url-encode a 32-byte Ed25519 seed for keychain storage.
fn encode_seed(seed: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(seed)
}

/// Decode a base64url-encoded seed back into the 32-byte form.
fn decode_seed(encoded: &str) -> Result<[u8; 32], KeystoreError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(KeystoreError::Base64)?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| KeystoreError::WrongLength { got: bytes.len() })
}

/// Failure modes for [`Keystore`] operations.
#[derive(Debug, Error)]
pub enum KeystoreError {
    #[error("OS keychain error: {0}")]
    Backend(#[from] keyring::Error),
    #[error("stored value is not valid base64url: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("stored seed is {got} bytes; expected 32")]
    WrongLength { got: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Switch the global keyring backend to the in-memory mock. Each
    /// test calls this once before doing keystore work. Mock storage is
    /// process-global so tests must use distinct service names to avoid
    /// cross-test interference.
    fn install_mock_backend() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        });
    }

    #[test]
    fn encode_decode_round_trips() {
        let seed = [0xABu8; 32];
        let encoded = encode_seed(&seed);
        let back = decode_seed(&encoded).unwrap();
        assert_eq!(back, seed);
    }

    #[test]
    fn decode_rejects_wrong_length() {
        let encoded = URL_SAFE_NO_PAD.encode([0u8; 31]);
        assert!(matches!(
            decode_seed(&encoded),
            Err(KeystoreError::WrongLength { got: 31 })
        ));
    }

    #[test]
    fn decode_rejects_invalid_base64() {
        assert!(matches!(
            decode_seed("!!!not-base64!!!"),
            Err(KeystoreError::Base64(_))
        ));
    }

    #[test]
    fn load_on_empty_returns_none() {
        install_mock_backend();
        let ks = Keystore::for_service("test-load-on-empty").unwrap();
        ks.delete_resolver_keypair().unwrap();
        assert!(ks.load_resolver_keypair().unwrap().is_none());
    }

    #[test]
    fn save_then_load_round_trips() {
        install_mock_backend();
        let ks = Keystore::for_service("test-save-load").unwrap();
        ks.delete_resolver_keypair().unwrap();
        let kp = Keypair::generate();
        let expected_pub = *kp.pubkey();
        ks.save_resolver_keypair(&kp).unwrap();
        let loaded = ks.load_resolver_keypair().unwrap().expect("entry present");
        assert_eq!(loaded.pubkey(), &expected_pub);
    }

    #[test]
    fn load_or_generate_creates_on_first_call() {
        install_mock_backend();
        let ks = Keystore::for_service("test-load-or-generate-first").unwrap();
        ks.delete_resolver_keypair().unwrap();
        let a = ks.load_or_generate_resolver_keypair().unwrap();
        let b = ks.load_or_generate_resolver_keypair().unwrap();
        assert_eq!(
            a.pubkey(),
            b.pubkey(),
            "second call must return the same key"
        );
    }

    #[test]
    fn delete_is_idempotent() {
        install_mock_backend();
        let ks = Keystore::for_service("test-delete-idempotent").unwrap();
        ks.delete_resolver_keypair().unwrap();
        ks.delete_resolver_keypair().unwrap(); // second time must also Ok
    }
}
