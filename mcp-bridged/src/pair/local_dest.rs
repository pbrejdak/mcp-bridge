//! Rule 10 — verify that a pair POST arrived on the IP/port the issuing
//! invite advertised.
//!
//! Per [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.5 (Direction-B, rule 10):
//! "The POST was received on the IP/port advertised in `resolver.lan_addr`."
//!
//! Defends against a Resolver that runs multiple pair endpoints (e.g.
//! one per network interface) issuing an invite for endpoint A that gets
//! POSTed to endpoint B. With a single-endpoint Resolver the check is
//! tautological; the SPEC requires it anyway.
//!
//! `.local` hostnames are accepted unchecked at this layer for v0.1 —
//! resolving them via mDNS to compare against `local.ip()` is out of
//! scope until the discovery agent lands. The invariant we still hold:
//! the request came in over our own loopback or LAN bind, so it cannot
//! originate from outside the host.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;

use thiserror::Error;

use super::lan_addr::LanAddr;

/// Verify that `local` matches the host/port advertised in `lan_addr`.
pub fn verify_request_destination(
    local: SocketAddr,
    lan_addr: &LanAddr,
) -> Result<(), DestinationError> {
    let expected_port = lan_addr.port_or_default();
    if local.port() != expected_port {
        return Err(DestinationError::PortMismatch {
            expected: expected_port,
            got: local.port(),
        });
    }

    let host_str = lan_addr.host_str();
    // url::Host returns IPv6 with brackets in host_str(); strip them so
    // Ipv6Addr::from_str accepts the literal.
    let host_for_ip = host_str.trim_start_matches('[').trim_end_matches(']');

    if let Ok(v4) = Ipv4Addr::from_str(host_for_ip) {
        if local.ip() != IpAddr::V4(v4) {
            return Err(DestinationError::IpMismatch {
                expected: IpAddr::V4(v4),
                got: local.ip(),
            });
        }
        return Ok(());
    }
    if let Ok(v6) = Ipv6Addr::from_str(host_for_ip) {
        if local.ip() != IpAddr::V6(v6) {
            return Err(DestinationError::IpMismatch {
                expected: IpAddr::V6(v6),
                got: local.ip(),
            });
        }
        return Ok(());
    }

    // .local hostname — Phase 1 accepts without mDNS resolution.
    // LanAddr::new already constrained the domain shape to `*.local`,
    // so we know this is not a public DNS name.
    Ok(())
}

/// Failure modes for [`verify_request_destination`].
#[derive(Debug, Error)]
pub enum DestinationError {
    #[error("request arrived on port {got}; invite advertised {expected}")]
    PortMismatch { expected: u16, got: u16 },
    #[error("request arrived on {got}; invite advertised {expected}")]
    IpMismatch { expected: IpAddr, got: IpAddr },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_match_accepts() {
        let lan = LanAddr::new("https://10.0.0.5:8765/pair").unwrap();
        let local: SocketAddr = "10.0.0.5:8765".parse().unwrap();
        verify_request_destination(local, &lan).unwrap();
    }

    #[test]
    fn ipv4_wrong_port_rejects() {
        let lan = LanAddr::new("https://10.0.0.5:8765/pair").unwrap();
        let local: SocketAddr = "10.0.0.5:9999".parse().unwrap();
        let err = verify_request_destination(local, &lan).unwrap_err();
        assert!(matches!(
            err,
            DestinationError::PortMismatch {
                expected: 8765,
                got: 9999
            }
        ));
    }

    #[test]
    fn ipv4_wrong_ip_rejects() {
        let lan = LanAddr::new("https://10.0.0.5:8765/pair").unwrap();
        let local: SocketAddr = "10.0.0.6:8765".parse().unwrap();
        let err = verify_request_destination(local, &lan).unwrap_err();
        assert!(matches!(err, DestinationError::IpMismatch { .. }));
    }

    #[test]
    fn ipv6_match_accepts() {
        let lan = LanAddr::new("https://[fe80::1]:8765/pair").unwrap();
        let local: SocketAddr = "[fe80::1]:8765".parse().unwrap();
        verify_request_destination(local, &lan).unwrap();
    }

    #[test]
    fn ipv6_wrong_ip_rejects() {
        let lan = LanAddr::new("https://[fe80::1]:8765/pair").unwrap();
        let local: SocketAddr = "[fe80::2]:8765".parse().unwrap();
        let err = verify_request_destination(local, &lan).unwrap_err();
        assert!(matches!(err, DestinationError::IpMismatch { .. }));
    }

    #[test]
    fn local_hostname_accepts_any_local_ip() {
        // Phase 1: .local hostnames pass without mDNS resolution.
        let lan = LanAddr::new("https://patryk.local:8765/pair").unwrap();
        let local: SocketAddr = "10.0.0.5:8765".parse().unwrap();
        verify_request_destination(local, &lan).unwrap();
    }

    #[test]
    fn local_hostname_still_checks_port() {
        let lan = LanAddr::new("https://patryk.local:8765/pair").unwrap();
        let local: SocketAddr = "10.0.0.5:9999".parse().unwrap();
        let err = verify_request_destination(local, &lan).unwrap_err();
        assert!(matches!(err, DestinationError::PortMismatch { .. }));
    }

    #[test]
    fn default_port_443_when_omitted() {
        let lan = LanAddr::new("https://10.0.0.5/pair").unwrap();
        let on_443: SocketAddr = "10.0.0.5:443".parse().unwrap();
        verify_request_destination(on_443, &lan).unwrap();

        let on_8765: SocketAddr = "10.0.0.5:8765".parse().unwrap();
        assert!(matches!(
            verify_request_destination(on_8765, &lan).unwrap_err(),
            DestinationError::PortMismatch {
                expected: 443,
                got: 8765
            }
        ));
    }
}
