//! Server Registry — the on-disk store of paired Origins.
//!
//! Per [`docs/DAEMON.md`](../../../../docs/DAEMON.md) §7.1: atomic JSON
//! read/write at `<data_dir>/registry.json`. File mode 0600 on Unix so
//! only the owning user can read.
//!
//! Atomic write algorithm:
//!   1. Write to `<path>.tmp`.
//!   2. fsync the temp file.
//!   3. Rename to `<path>` (POSIX-atomic).
//!
//! Concurrency: the registry is owned by a single `Arc<RwLock<Registry>>`
//! handed to the daemon's tasks. Reads are frequent (proxy hot path);
//! writes are rare (pair accept, revoke). If contention becomes
//! measurable, swap for an actor pattern like `InviteRegister`.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;
use tracing::{debug, info};

use crate::pair::logical_id::LogicalId;

use super::pin::ServerPin;

/// On-disk wire format. Top-level wrapper so we can version the file
/// without touching the per-pin shape.
#[derive(Debug, Default, Serialize, Deserialize)]
struct OnDisk {
    /// Schema version. Bump when the on-disk shape changes incompatibly.
    version: u32,
    pins: Vec<ServerPin>,
}

const ON_DISK_VERSION: u32 = 1;

/// The Server Registry — paired Origins keyed by [`LogicalId`].
#[derive(Debug, Default)]
pub struct Registry {
    pins: HashMap<LogicalId, ServerPin>,
}

impl Registry {
    /// New empty registry. Tests use this; production goes through
    /// [`Registry::load_or_empty`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from disk. If the file does not exist, returns an empty
    /// registry (the "first launch" case). Any I/O or parse error other
    /// than `NotFound` is propagated.
    pub async fn load_or_empty(path: &Path) -> Result<Self, RegistryError> {
        match fs::read(path).await {
            Ok(bytes) => {
                let parsed: OnDisk = serde_json::from_slice(&bytes)?;
                if parsed.version != ON_DISK_VERSION {
                    return Err(RegistryError::UnsupportedVersion {
                        got: parsed.version,
                        expected: ON_DISK_VERSION,
                    });
                }
                let mut pins = HashMap::with_capacity(parsed.pins.len());
                for pin in parsed.pins {
                    pins.insert(pin.logical_id.clone(), pin);
                }
                info!(path = ?path, count = pins.len(), "registry loaded");
                Ok(Self { pins })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                debug!(path = ?path, "no registry file yet — starting empty");
                Ok(Self::new())
            }
            Err(e) => Err(RegistryError::Io(e)),
        }
    }

    /// Write the registry to disk atomically. Creates the parent
    /// directory if needed.
    pub async fn save(&self, path: &Path) -> Result<(), RegistryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(RegistryError::Io)?;
        }

        let snapshot = OnDisk {
            version: ON_DISK_VERSION,
            pins: self.pins.values().cloned().collect(),
        };
        let bytes = serde_json::to_vec_pretty(&snapshot)?;

        let tmp_path = tmp_path_for(path);
        fs::write(&tmp_path, &bytes)
            .await
            .map_err(RegistryError::Io)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            fs::set_permissions(&tmp_path, perms)
                .await
                .map_err(RegistryError::Io)?;
        }

        // fsync the tmp file to push bytes to disk before the rename.
        let file = fs::OpenOptions::new()
            .write(true)
            .open(&tmp_path)
            .await
            .map_err(RegistryError::Io)?;
        file.sync_all().await.map_err(RegistryError::Io)?;
        drop(file);

        // Atomic on POSIX; near-atomic on Windows (rename replaces).
        fs::rename(&tmp_path, path)
            .await
            .map_err(RegistryError::Io)?;

        debug!(path = ?path, count = self.pins.len(), "registry saved");
        Ok(())
    }

    /// Insert or replace a pin. Returns the previous value if any.
    pub fn insert(&mut self, pin: ServerPin) -> Option<ServerPin> {
        self.pins.insert(pin.logical_id.clone(), pin)
    }

    /// Borrow a pin by logical ID.
    #[must_use]
    pub fn get(&self, lid: &LogicalId) -> Option<&ServerPin> {
        self.pins.get(lid)
    }

    /// Iterate all pins in arbitrary order.
    pub fn iter(&self) -> impl Iterator<Item = &ServerPin> {
        self.pins.values()
    }

    /// Number of pins.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pins.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
    }

    /// Remove a pin entirely. Returns the removed value if any.
    pub fn remove(&mut self, lid: &LogicalId) -> Option<ServerPin> {
        self.pins.remove(lid)
    }
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

/// Failure modes for [`Registry`] disk operations.
#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("registry I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("registry JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("registry file is schema version {got}; this build supports {expected}")]
    UnsupportedVersion { got: u32, expected: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::identity::{DisplayName, Keypair, Signature};
    use crate::pair::auth::{Auth, AuthType};
    use crate::pair::backend_url::BackendUrl;
    use crate::pair::bearer_token::BearerToken;
    use crate::pair::cert_fingerprint::CertFingerprint;
    use crate::pair::invite::{Direction, SpecVersion};
    use crate::pair::nonce::Nonce;
    use crate::pair::payload::{BackendInfo, OriginInfo, PairPayload, Scope};

    fn sample_pin(lid: &str, nonce_byte: u8) -> ServerPin {
        let origin = Keypair::generate();
        let resolver = Keypair::generate();
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
            nonce: Nonce::from_bytes([nonce_byte; 16]),
            target_resolver_pubkey: Some(*resolver.pubkey()),
            sig: Signature::from_bytes([0u8; 64]),
        };
        let canonical = payload.canonical_signing_bytes().unwrap();
        payload.sig = origin.sign(&canonical);
        ServerPin::from_accepted_payload(&payload, 1_700_000_000)
    }

    #[tokio::test]
    async fn load_or_empty_on_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let r = Registry::load_or_empty(&path).await.unwrap();
        assert!(r.is_empty());
    }

    #[tokio::test]
    async fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");

        let mut a = Registry::new();
        a.insert(sample_pin("alpha", 1));
        a.insert(sample_pin("beta", 2));
        a.save(&path).await.unwrap();

        let b = Registry::load_or_empty(&path).await.unwrap();
        assert_eq!(b.len(), 2);
        let alpha_lid = LogicalId::new("alpha").unwrap();
        let beta_lid = LogicalId::new("beta").unwrap();
        assert_eq!(b.get(&alpha_lid).unwrap().display_name.as_str(), "BodyLog");
        assert!(b.get(&beta_lid).is_some());
    }

    #[tokio::test]
    async fn insert_returns_previous() {
        let mut r = Registry::new();
        let first = sample_pin("alpha", 1);
        let lid = first.logical_id.clone();
        assert!(r.insert(first).is_none());
        let second = sample_pin("alpha", 2);
        let prev = r.insert(second).expect("previous returned");
        assert_eq!(prev.logical_id, lid);
    }

    #[tokio::test]
    async fn remove_takes_pin_out() {
        let mut r = Registry::new();
        r.insert(sample_pin("alpha", 1));
        let lid = LogicalId::new("alpha").unwrap();
        assert!(r.remove(&lid).is_some());
        assert!(r.get(&lid).is_none());
        assert!(r.remove(&lid).is_none());
    }

    #[tokio::test]
    async fn save_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b/c/registry.json");
        let r = Registry::new();
        r.save(&nested).await.unwrap();
        assert!(nested.exists());
    }

    #[tokio::test]
    async fn save_writes_atomically_via_tmp_then_rename() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        let mut r = Registry::new();
        r.insert(sample_pin("alpha", 1));
        r.save(&path).await.unwrap();
        // After save, only the final file exists; no leftover .tmp.
        let tmp = tmp_path_for(&path);
        assert!(path.exists());
        assert!(!tmp.exists(), "leftover {tmp:?} after save");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn save_sets_mode_0600_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        let r = Registry::new();
        r.save(&path).await.unwrap();
        let perms = std::fs::metadata(&path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[tokio::test]
    async fn unsupported_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        std::fs::write(&path, br#"{"version": 999, "pins": []}"#).unwrap();
        let r = Registry::load_or_empty(&path).await;
        assert!(matches!(
            r,
            Err(RegistryError::UnsupportedVersion {
                got: 999,
                expected: 1
            })
        ));
    }
}
