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
