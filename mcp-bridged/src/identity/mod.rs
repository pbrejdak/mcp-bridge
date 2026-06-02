//! Resolver Ed25519 keypair lifecycle: first-launch generation, rotation,
//! display-name management. See [`docs/DAEMON.md`] §3 and §5.1
//! (`identity.rotate`, `identity.display_name`).
//!
//! [`docs/DAEMON.md`]: ../../../../docs/DAEMON.md

pub mod display_name;
pub mod keypair;
pub mod keystore;
pub mod pubkey;
pub mod signature;
pub mod tls_cert;

pub use display_name::DisplayName;
pub use keypair::{Keypair, VerifyError, verify};
pub use keystore::{Keystore, KeystoreError};
pub use pubkey::Ed25519Pubkey;
pub use signature::Signature;
pub use tls_cert::{SelfSignedCert, generate_for as generate_self_signed_cert};

use std::sync::{Arc, RwLock};

/// Atomically swappable handle to the Resolver keypair.
///
/// Used by long-lived subsystems (pair endpoint, mDNS subscriber, IPC
/// dispatcher) that need to keep working when `identity.rotate`
/// replaces the keypair underneath them. Readers do
/// `handle.read().unwrap().clone()` to snapshot the current
/// `Arc<Keypair>` (cheap — just an Arc clone), then drop the lock
/// before doing any `.await`. Writers atomically swap with
/// `*handle.write().unwrap() = Arc::new(new_kp)`.
///
/// `std::sync::RwLock` is intentional: the lock is held for nanoseconds
/// (a single pointer load or store) and never across an `.await`, so
/// the async tokio RwLock would be pure overhead. Per CLAUDE.md §4.
pub type SharedKeypair = Arc<RwLock<Arc<Keypair>>>;

/// Helper to construct a [`SharedKeypair`] from a freshly-loaded
/// keypair.
#[must_use]
pub fn shared_keypair(kp: Keypair) -> SharedKeypair {
    Arc::new(RwLock::new(Arc::new(kp)))
}

/// Helper to construct a [`SharedKeypair`] from an existing
/// `Arc<Keypair>`. Used by tests that need to share the keypair
/// between a [`SharedKeypair`]-holding subsystem and a separate
/// payload-signing path.
#[must_use]
pub fn shared_keypair_from_arc(arc: Arc<Keypair>) -> SharedKeypair {
    Arc::new(RwLock::new(arc))
}

/// Snapshot the current keypair behind a [`SharedKeypair`]. Holds the
/// read lock just long enough to clone the inner Arc; the returned
/// `Arc<Keypair>` keeps the keypair alive across `.await` points.
#[must_use]
pub fn current_keypair(handle: &SharedKeypair) -> Arc<Keypair> {
    handle.read().expect("resolver keypair RwLock poisoned").clone()
}

/// Atomically replace the keypair behind a [`SharedKeypair`].
pub fn swap_keypair(handle: &SharedKeypair, new_kp: Keypair) {
    *handle
        .write()
        .expect("resolver keypair RwLock poisoned") = Arc::new(new_kp);
}
