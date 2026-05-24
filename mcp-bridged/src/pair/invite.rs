//! `mcp-pair/v0.1` Resolver invite (Direction B).
//!
//! See [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.2 for the JSON shape
//! and §4.3 for the 60-second lifetime contract (enforced at the
//! invite-register layer, not here).

use serde::{Deserialize, Serialize};

use crate::identity::{DisplayName, Ed25519Pubkey};

use super::lan_addr::LanAddr;
use super::nonce::Nonce;
use super::sas::{self, Sas};

/// Protocol version identifier carried in the `spec` field.
///
/// A single-variant enum (today) so that future versions add variants
/// rather than mutating a magic string, and so that an unknown `spec`
/// fails to deserialize per [`docs/SPEC.md`](../../../../docs/SPEC.md) §3.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecVersion {
    #[serde(rename = "mcp-pair/v0.1")]
    McpPairV0_1,
}

/// Pairing direction per [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Resolver shows QR; phone scans (default flow).
    ResolverOffered,
    /// Origin shows QR; Resolver scans (webcam fallback).
    OriginOffered,
}

/// The Resolver-side half of an invite — pubkey, name, derived SAS, LAN
/// callback address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolverInfo {
    pub pubkey: Ed25519Pubkey,
    pub display_name: DisplayName,
    pub sas: Sas,
    pub lan_addr: LanAddr,
}

/// A Direction-B pairing invite that the Resolver displays as a QR.
///
/// JSON shape matches [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.2
/// byte-for-byte under the default `serde_json` field ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub spec: SpecVersion,
    pub direction: Direction,
    pub resolver: ResolverInfo,
    pub nonce: Nonce,
    /// Optional `mcp-pair://` universal-link form of this invite.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub uri: Option<String>,
}

impl Invite {
    /// Construct a fresh Direction-B invite, deriving the SAS from
    /// `(pubkey, nonce)` per [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.2.
    #[must_use]
    pub fn new(
        pubkey: Ed25519Pubkey,
        display_name: DisplayName,
        lan_addr: LanAddr,
        nonce: Nonce,
    ) -> Self {
        let sas = sas::derive(pubkey.as_bytes(), nonce.as_bytes());
        Self {
            spec: SpecVersion::McpPairV0_1,
            direction: Direction::ResolverOffered,
            resolver: ResolverInfo {
                pubkey,
                display_name,
                sas,
                lan_addr,
            },
            nonce,
            uri: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_invite() -> Invite {
        Invite::new(
            Ed25519Pubkey::from_bytes([0u8; 32]),
            DisplayName::new("Patryk's MacBook Pro").unwrap(),
            LanAddr::new("https://10.0.0.5:8765/pair").unwrap(),
            Nonce::from_bytes([0u8; 16]),
        )
    }

    #[test]
    fn new_derives_sas_from_pubkey_and_nonce() {
        let inv = sample_invite();
        // Same (pubkey, nonce) as sas::tests::kat_all_zeros.
        assert_eq!(inv.resolver.sas.as_str(), "voyage-sentence-voyage-deny");
    }

    #[test]
    fn new_defaults_to_direction_b_and_v01_spec() {
        let inv = sample_invite();
        assert_eq!(inv.direction, Direction::ResolverOffered);
        assert_eq!(inv.spec, SpecVersion::McpPairV0_1);
        assert!(inv.uri.is_none());
    }

    #[test]
    fn json_shape_matches_spec_4_2() {
        let inv = sample_invite();
        let value: serde_json::Value = serde_json::to_value(&inv).unwrap();

        assert_eq!(value["spec"], "mcp-pair/v0.1");
        assert_eq!(value["direction"], "resolver_offered");
        assert_eq!(
            value["resolver"]["pubkey"],
            "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        );
        assert_eq!(value["resolver"]["display_name"], "Patryk's MacBook Pro");
        assert_eq!(value["resolver"]["sas"], "voyage-sentence-voyage-deny");
        assert_eq!(value["resolver"]["lan_addr"], "https://10.0.0.5:8765/pair");
        assert_eq!(value["nonce"], "AAAAAAAAAAAAAAAAAAAAAA");
        assert!(
            value.get("uri").is_none(),
            "uri should be omitted when None"
        );
    }

    #[test]
    fn round_trip_through_json() {
        let inv = sample_invite();
        let json = serde_json::to_string(&inv).unwrap();
        let back: Invite = serde_json::from_str(&json).unwrap();
        assert_eq!(back, inv);
    }

    #[test]
    fn round_trip_preserves_uri_when_present() {
        let mut inv = sample_invite();
        inv.uri = Some("mcp-pair://abc123".to_owned());
        let json = serde_json::to_string(&inv).unwrap();
        assert!(json.contains("\"uri\":\"mcp-pair://abc123\""));
        let back: Invite = serde_json::from_str(&json).unwrap();
        assert_eq!(back.uri.as_deref(), Some("mcp-pair://abc123"));
    }

    #[test]
    fn unknown_spec_is_rejected() {
        let bad = r#"{
            "spec": "mcp-pair/v9.99",
            "direction": "resolver_offered",
            "resolver": {
                "pubkey": "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "display_name": "x",
                "sas": "voyage-sentence-voyage-deny",
                "lan_addr": "https://10.0.0.5:8765/pair"
            },
            "nonce": "AAAAAAAAAAAAAAAAAAAAAA"
        }"#;
        let r: Result<Invite, _> = serde_json::from_str(bad);
        assert!(r.is_err());
    }

    #[test]
    fn unknown_direction_is_rejected() {
        let bad = r#"{
            "spec": "mcp-pair/v0.1",
            "direction": "sideways",
            "resolver": {
                "pubkey": "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "display_name": "x",
                "sas": "voyage-sentence-voyage-deny",
                "lan_addr": "https://10.0.0.5:8765/pair"
            },
            "nonce": "AAAAAAAAAAAAAAAAAAAAAA"
        }"#;
        let r: Result<Invite, _> = serde_json::from_str(bad);
        assert!(r.is_err());
    }
}
