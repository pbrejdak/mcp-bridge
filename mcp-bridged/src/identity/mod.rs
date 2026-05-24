//! Resolver Ed25519 keypair lifecycle: first-launch generation, rotation,
//! display-name management. See [`docs/DAEMON.md`] §3 and §5.1
//! (`identity.rotate`, `identity.display_name`).
//!
//! [`docs/DAEMON.md`]: ../../../../docs/DAEMON.md

pub mod display_name;
pub mod pubkey;

pub use display_name::DisplayName;
pub use pubkey::Ed25519Pubkey;
