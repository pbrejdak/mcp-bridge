//! `mcp-pair/v0.1` implementation: invite QR construction, sealed payload
//! acceptance, SAS derivation. See [`docs/SPEC.md`] §4 for the normative
//! grammar and [`docs/DAEMON.md`] §3 for the module layout.
//!
//! Submodules to land: `invite`, `payload`.
//!
//! [`docs/SPEC.md`]: ../../../../docs/SPEC.md
//! [`docs/DAEMON.md`]: ../../../../docs/DAEMON.md

pub mod accept;
pub mod auth;
pub mod backend_url;
pub mod backend_verifier;
pub mod bearer_token;
pub mod cert_fingerprint;
pub mod endpoint;
pub mod invite;
pub mod invite_register;
pub mod lan_addr;
pub mod local_dest;
pub mod logical_id;
pub mod loopback_key;
pub mod nonce;
pub mod payload;
pub mod rustls_backend_verifier;
pub mod sas;
pub mod seal;
pub mod token_refresh;

pub use accept::{AcceptError, accept_direction_b};
pub use auth::{Auth, AuthType};
pub use backend_url::BackendUrl;
pub use bearer_token::BearerToken;
pub use cert_fingerprint::CertFingerprint;
pub use invite::{Direction, Invite, ResolverInfo, SpecVersion};
pub use invite_register::InviteRegister;
pub use lan_addr::LanAddr;
pub use logical_id::LogicalId;
pub use nonce::Nonce;
pub use payload::{BackendInfo, OriginInfo, PairPayload, Scope};
pub use sas::Sas;
pub use token_refresh::{
    BearerTokenRefresher, RefreshError, WellKnownPathRefresher, WELL_KNOWN_REFRESH_PATH,
};
