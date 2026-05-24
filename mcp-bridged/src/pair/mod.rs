//! `mcp-pair/v0.1` implementation: invite QR construction, sealed payload
//! acceptance, SAS derivation. See [`docs/SPEC.md`] §4 for the normative
//! grammar and [`docs/DAEMON.md`] §3 for the module layout.
//!
//! Submodules to land: `invite`, `payload`.
//!
//! [`docs/SPEC.md`]: ../../../../docs/SPEC.md
//! [`docs/DAEMON.md`]: ../../../../docs/DAEMON.md

pub mod sas;
