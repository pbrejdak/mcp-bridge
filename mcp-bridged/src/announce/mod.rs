//! `mcp-announce/v0.1` implementation: mDNS / Bonjour subscriber, unicast
//! HTTP POST endpoint, signature + seq + freshness + rate-limit checks. See
//! [`docs/SPEC.md`] §5 for the normative grammar and [`docs/DAEMON.md`] §3
//! for the module layout.
//!
//! Submodules:
//! - [`payload`] — signed announce body + canonical-JSON / sig verify.
//! - [`accept`] — sealed bytes → registry mutation (SPEC §5.5 rules).
//!
//! Submodules to land: `unicast`, `bonjour`.
//!
//! [`docs/SPEC.md`]: ../../../../docs/SPEC.md
//! [`docs/DAEMON.md`]: ../../../../docs/DAEMON.md

pub mod accept;
pub mod payload;

pub use accept::{AcceptError, AcceptedAnnounce, accept_announce};
pub use payload::{AnnounceBackend, AnnouncePayload, SpecVersion, ValidationError};
