//! `mcp-announce/v0.1` implementation: mDNS / Bonjour subscriber, unicast
//! HTTP POST endpoint, signature + seq + freshness + rate-limit checks. See
//! [`docs/SPEC.md`] §5 for the normative grammar and [`docs/DAEMON.md`] §3
//! for the module layout.
//!
//! Submodules:
//! - [`payload`] — signed announce body + canonical-JSON / sig verify.
//! - [`accept`] — sealed bytes → registry mutation (SPEC §5.5 rules).
//! - [`rate_limit`] — per-IP + per-LID token buckets (SPEC §5.6).
//! - [`mdns`] — SPEC §5.2 mDNS carrier protocol pieces + subscriber
//!   trait. Per-OS backends live in a follow-up commit.
//!
//! Submodules to land: `unicast` (HTTP serve task), platform mDNS
//! backends.
//!
//! [`docs/SPEC.md`]: ../../../../docs/SPEC.md
//! [`docs/DAEMON.md`]: ../../../../docs/DAEMON.md

pub mod accept;
pub mod mdns;
pub mod mdns_backend;
pub mod payload;
pub mod rate_limit;

pub use accept::{
    AcceptError, AcceptedAnnounce, accept_announce, apply_announce, parse_sealed_announce,
};
pub use mdns::{FakeMdnsSubscriber, MdnsAnnouncement, MdnsSubscriber};
pub use mdns_backend::MdnsSdSubscriber;
pub use payload::{AnnounceBackend, AnnouncePayload, SpecVersion, ValidationError};
pub use rate_limit::AnnounceRateLimiter;
