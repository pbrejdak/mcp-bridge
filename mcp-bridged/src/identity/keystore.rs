//! OS keychain-backed persistence for the Resolver's secret material.
//!
//! Per [`docs/DAEMON.md`](../../../../docs/DAEMON.md) §7.2 and
//! [`mcp-bridged/CLAUDE.md`](../../../CLAUDE.md) §6: secrets live in
//! the OS keychain via the [`keyring`] crate (macOS Security framework,
//! Windows credential vault, Linux Secret Service).
//!
//! Today the keystore manages three kinds of entries:
//!
//! - **Resolver keypair** — one entry, account `resolver-keypair-v1`,
//!   value = base64url(32-byte Ed25519 seed).
//! - **Per-Pin bearer token** — one entry per LID, account
//!   `bearer/<lid>`, value = raw bearer-token string.
//! - **Per-Pin loopback key** — one entry per LID, account
//!   `loopback/<lid>`, value = base64url(32-byte random key).
//!
//! `Entry` instances are cached on first access by account name so that
//! the in-memory `keyring::mock` backend used in tests round-trips
//! correctly (it stores secrets per-`Entry`, not per-(service, account)).
//! Production backends ignore the cache distinction.

use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use keyring::Entry;
use thiserror::Error;

use crate::adapters::Sentinel;
use crate::adapters::sentinel::ParseError as SentinelParseError;
use crate::pair::bearer_token::{BearerToken, BearerTokenError};
use crate::pair::logical_id::LogicalId;
use crate::pair::loopback_key::LoopbackKey;

use super::keypair::Keypair;

/// Keychain service identifier — matches the `directories` qualifier
/// used in [`crate::config`].
pub const KEYCHAIN_SERVICE: &str = "dev.mcpbridge.mcp-bridged";

/// Account name for the Resolver keypair. Bumping the `v1` suffix lets
/// future encoding migrations land beside the old entry.
pub const RESOLVER_KEYPAIR_ACCOUNT: &str = "resolver-keypair-v1";

/// Account name for the per-install adapter sentinel UUID.
pub const SENTINEL_ACCOUNT: &str = "sentinel-v1";

fn bearer_account_for(lid: &LogicalId) -> String {
    format!("bearer/{}", lid.as_str())
}

fn loopback_account_for(lid: &LogicalId) -> String {
    format!("loopback/{}", lid.as_str())
}

/// Handle to the OS keychain, scoped to one logical service.
///
/// Holds an internal cache of `keyring::Entry` instances by account
/// name. Entries are created lazily on first use and kept for the
/// lifetime of the keystore — required because the
/// [`keyring::mock`] backend stores secrets per-`Entry`, not per
/// (service, account).
#[allow(missing_debug_implementations)] // keyring::Entry is not Debug.
pub struct Keystore {
    service: String,
    entries: Mutex<HashMap<String, Entry>>,
}

impl Keystore {
    /// Default keystore using the canonical service name.
    pub fn new() -> Result<Self, KeystoreError> {
        Self::for_service(KEYCHAIN_SERVICE)
    }

    /// Construct a keystore scoped to a custom service name. Used by
    /// tests so parallel runs don't collide.
    pub fn for_service(service: &str) -> Result<Self, KeystoreError> {
        Ok(Self {
            service: service.to_owned(),
            entries: Mutex::new(HashMap::new()),
        })
    }

    /// Obtain (or lazily create + cache) the `keyring::Entry` for one
    /// account name.
    fn with_entry<R, F>(&self, account: &str, f: F) -> Result<R, KeystoreError>
    where
        F: FnOnce(&Entry) -> Result<R, KeystoreError>,
    {
        let mut map = self
            .entries
            .lock()
            .expect("keystore entries mutex is uncontended and never poisoned");
        if !map.contains_key(account) {
            let entry = Entry::new(&self.service, account).map_err(KeystoreError::Backend)?;
            map.insert(account.to_owned(), entry);
        }
        let entry = map.get(account).expect("just inserted above");
        f(entry)
    }

    // ------------------------------------------------------------------
    // Resolver keypair
    // ------------------------------------------------------------------

    /// Load the Resolver keypair if one is stored, `Ok(None)` otherwise.
    pub fn load_resolver_keypair(&self) -> Result<Option<Keypair>, KeystoreError> {
        self.with_entry(RESOLVER_KEYPAIR_ACCOUNT, |entry| {
            match entry.get_password() {
                Ok(encoded) => Ok(Some(Keypair::from_seed(decode_32_bytes(&encoded)?))),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(KeystoreError::Backend(e)),
            }
        })
    }

    /// Save the Resolver keypair, overwriting any existing entry.
    pub fn save_resolver_keypair(&self, kp: &Keypair) -> Result<(), KeystoreError> {
        let encoded = URL_SAFE_NO_PAD.encode(kp.to_seed_bytes().as_slice());
        self.with_entry(RESOLVER_KEYPAIR_ACCOUNT, |entry| {
            entry.set_password(&encoded).map_err(KeystoreError::Backend)
        })
    }

    /// Delete the Resolver keypair. No-op if absent.
    pub fn delete_resolver_keypair(&self) -> Result<(), KeystoreError> {
        self.with_entry(RESOLVER_KEYPAIR_ACCOUNT, delete_if_present)
    }

    /// Load-or-generate idiom for daemon first launch.
    pub fn load_or_generate_resolver_keypair(&self) -> Result<Keypair, KeystoreError> {
        if let Some(kp) = self.load_resolver_keypair()? {
            return Ok(kp);
        }
        let kp = Keypair::generate();
        self.save_resolver_keypair(&kp)?;
        Ok(kp)
    }

    // ------------------------------------------------------------------
    // Per-Pin bearer token
    // ------------------------------------------------------------------

    /// Save (or overwrite) the bearer token for `lid`.
    pub fn save_bearer_token(
        &self,
        lid: &LogicalId,
        token: &BearerToken,
    ) -> Result<(), KeystoreError> {
        let account = bearer_account_for(lid);
        self.with_entry(&account, |entry| {
            entry
                .set_password(token.expose())
                .map_err(KeystoreError::Backend)
        })
    }

    /// Load the bearer token for `lid`, or `Ok(None)` if absent.
    pub fn load_bearer_token(&self, lid: &LogicalId) -> Result<Option<BearerToken>, KeystoreError> {
        let account = bearer_account_for(lid);
        self.with_entry(&account, |entry| match entry.get_password() {
            Ok(value) => Ok(Some(BearerToken::new(value)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeystoreError::Backend(e)),
        })
    }

    /// Delete the bearer token for `lid`. No-op if absent.
    pub fn delete_bearer_token(&self, lid: &LogicalId) -> Result<(), KeystoreError> {
        let account = bearer_account_for(lid);
        self.with_entry(&account, delete_if_present)
    }

    // ------------------------------------------------------------------
    // Per-Pin loopback key
    // ------------------------------------------------------------------

    /// Save (or overwrite) the loopback key for `lid`.
    pub fn save_loopback_key(
        &self,
        lid: &LogicalId,
        key: &LoopbackKey,
    ) -> Result<(), KeystoreError> {
        let account = loopback_account_for(lid);
        let encoded = URL_SAFE_NO_PAD.encode(key.as_bytes());
        self.with_entry(&account, |entry| {
            entry.set_password(&encoded).map_err(KeystoreError::Backend)
        })
    }

    /// Load the loopback key for `lid`, or `Ok(None)` if absent.
    pub fn load_loopback_key(&self, lid: &LogicalId) -> Result<Option<LoopbackKey>, KeystoreError> {
        let account = loopback_account_for(lid);
        self.with_entry(&account, |entry| match entry.get_password() {
            Ok(encoded) => Ok(Some(LoopbackKey::from_bytes(decode_32_bytes(&encoded)?))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeystoreError::Backend(e)),
        })
    }

    /// Delete the loopback key for `lid`. No-op if absent.
    pub fn delete_loopback_key(&self, lid: &LogicalId) -> Result<(), KeystoreError> {
        let account = loopback_account_for(lid);
        self.with_entry(&account, delete_if_present)
    }

    // ------------------------------------------------------------------
    // Adapter sentinel UUID
    // ------------------------------------------------------------------

    /// Load the adapter sentinel if one is stored, `Ok(None)` otherwise.
    pub fn load_sentinel(&self) -> Result<Option<Sentinel>, KeystoreError> {
        self.with_entry(SENTINEL_ACCOUNT, |entry| match entry.get_password() {
            Ok(s) => Ok(Some(s.parse::<Sentinel>()?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeystoreError::Backend(e)),
        })
    }

    /// Save the adapter sentinel, overwriting any existing entry.
    pub fn save_sentinel(&self, sentinel: Sentinel) -> Result<(), KeystoreError> {
        self.with_entry(SENTINEL_ACCOUNT, |entry| {
            entry
                .set_password(&sentinel.to_string())
                .map_err(KeystoreError::Backend)
        })
    }

    /// Load-or-generate idiom for daemon first launch.
    pub fn load_or_generate_sentinel(&self) -> Result<Sentinel, KeystoreError> {
        if let Some(s) = self.load_sentinel()? {
            return Ok(s);
        }
        let s = Sentinel::random();
        self.save_sentinel(s)?;
        Ok(s)
    }
}

fn delete_if_present(entry: &Entry) -> Result<(), KeystoreError> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(KeystoreError::Backend(e)),
    }
}

fn decode_32_bytes(encoded: &str) -> Result<[u8; 32], KeystoreError> {
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
    #[error("stored value is {got} bytes; expected 32")]
    WrongLength { got: usize },
    #[error("stored bearer token failed validation: {0}")]
    BearerToken(#[from] BearerTokenError),
    #[error("stored sentinel is not a valid UUID: {0}")]
    Sentinel(#[from] SentinelParseError),
}

/// Switch the global keyring backend to the in-memory mock. Idempotent.
///
/// **Test-only.** Calling this in production code permanently disables
/// access to the OS keychain for the rest of the process. The function
/// is `pub` rather than `pub(crate)` because the integration tests
/// under `mcp-bridged/tests/` need to call it before constructing a
/// `Keystore`; CI lints check that no non-test target invokes it.
pub fn install_mock_backend() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lid(s: &str) -> LogicalId {
        LogicalId::new(s).unwrap()
    }

    // -- Resolver keypair -------------------------------------------

    #[test]
    fn resolver_load_on_empty_returns_none() {
        install_mock_backend();
        let ks = Keystore::for_service("test-resolver-empty").unwrap();
        ks.delete_resolver_keypair().unwrap();
        assert!(ks.load_resolver_keypair().unwrap().is_none());
    }

    #[test]
    fn resolver_save_then_load_round_trips() {
        install_mock_backend();
        let ks = Keystore::for_service("test-resolver-save-load").unwrap();
        ks.delete_resolver_keypair().unwrap();
        let kp = Keypair::generate();
        let pubkey = *kp.pubkey();
        ks.save_resolver_keypair(&kp).unwrap();
        let loaded = ks.load_resolver_keypair().unwrap().unwrap();
        assert_eq!(loaded.pubkey(), &pubkey);
    }

    #[test]
    fn resolver_load_or_generate_is_stable() {
        install_mock_backend();
        let ks = Keystore::for_service("test-resolver-stable").unwrap();
        ks.delete_resolver_keypair().unwrap();
        let a = ks.load_or_generate_resolver_keypair().unwrap();
        let b = ks.load_or_generate_resolver_keypair().unwrap();
        assert_eq!(a.pubkey(), b.pubkey());
    }

    // -- Bearer tokens ----------------------------------------------

    #[test]
    fn bearer_save_then_load_round_trips() {
        install_mock_backend();
        let ks = Keystore::for_service("test-bearer-save-load").unwrap();
        let lid = sample_lid("bodylog-7f3a");
        let token = BearerToken::new("abc123").unwrap();
        ks.save_bearer_token(&lid, &token).unwrap();
        let loaded = ks.load_bearer_token(&lid).unwrap().unwrap();
        assert_eq!(loaded.expose(), "abc123");
    }

    #[test]
    fn bearer_load_on_unknown_lid_returns_none() {
        install_mock_backend();
        let ks = Keystore::for_service("test-bearer-unknown").unwrap();
        let lid = sample_lid("not-there");
        assert!(ks.load_bearer_token(&lid).unwrap().is_none());
    }

    #[test]
    fn bearer_delete_is_idempotent() {
        install_mock_backend();
        let ks = Keystore::for_service("test-bearer-delete-idempotent").unwrap();
        let lid = sample_lid("foo");
        ks.delete_bearer_token(&lid).unwrap();
        ks.delete_bearer_token(&lid).unwrap();
    }

    // -- Loopback keys ----------------------------------------------

    #[test]
    fn loopback_save_then_load_round_trips() {
        install_mock_backend();
        let ks = Keystore::for_service("test-loopback-save-load").unwrap();
        let lid = sample_lid("alpha");
        let key = LoopbackKey::from_bytes([0xAB; 32]);
        ks.save_loopback_key(&lid, &key).unwrap();
        let loaded = ks.load_loopback_key(&lid).unwrap().unwrap();
        assert_eq!(loaded, key);
    }

    #[test]
    fn loopback_load_on_unknown_lid_returns_none() {
        install_mock_backend();
        let ks = Keystore::for_service("test-loopback-unknown").unwrap();
        let lid = sample_lid("unknown-lid");
        assert!(ks.load_loopback_key(&lid).unwrap().is_none());
    }

    // -- Multiple entries co-exist ----------------------------------

    #[test]
    fn resolver_bearer_and_loopback_coexist() {
        install_mock_backend();
        let ks = Keystore::for_service("test-coexist").unwrap();
        let lid = sample_lid("bodylog");

        let kp = Keypair::generate();
        let pubkey = *kp.pubkey();
        let bearer = BearerToken::new("token-xyz").unwrap();
        let lbk = LoopbackKey::from_bytes([0xCD; 32]);

        ks.save_resolver_keypair(&kp).unwrap();
        ks.save_bearer_token(&lid, &bearer).unwrap();
        ks.save_loopback_key(&lid, &lbk).unwrap();

        assert_eq!(
            ks.load_resolver_keypair().unwrap().unwrap().pubkey(),
            &pubkey
        );
        assert_eq!(
            ks.load_bearer_token(&lid).unwrap().unwrap().expose(),
            "token-xyz"
        );
        assert_eq!(ks.load_loopback_key(&lid).unwrap().unwrap(), lbk);
    }
}
