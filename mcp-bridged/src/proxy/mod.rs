//! Loopback Listener and Origin Connector — the request hot path. See
//! [`docs/SPEC.md`] §6 (loopback face) and [`docs/DAEMON.md`] §6 (proxy
//! request flow).
//!
//! Submodules:
//! - [`pinning_verifier`] — rustls verifier that pins the leaf cert
//!   to a known SHA-256.
//! - [`connector`] — outbound HTTPS client per Server Pin.
//! - `listener` — the inbound `127.0.0.1` HTTP listener (forthcoming).
//!
//! [`docs/SPEC.md`]: ../../../../docs/SPEC.md
//! [`docs/DAEMON.md`]: ../../../../docs/DAEMON.md

pub mod connector;
pub mod pinning_verifier;

pub use connector::{ConnectorError, ForwardRequest, ForwardResponse, OriginConnector};
pub use pinning_verifier::PinningVerifier;
