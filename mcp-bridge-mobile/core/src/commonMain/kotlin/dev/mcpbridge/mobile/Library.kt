// Bridge Peer SDK — protocol core entry point.
//
// This file is the compile-time anchor for the KMP module. The
// substantive protocol implementations live in sibling packages:
//   - model      — typed wire data classes (mcp-pair/v0.1, mcp-announce/v0.1).
//   - canonical  — RFC 8785 canonical JSON encoder.
//   - crypto     — Ed25519, crypto_box, BLAKE2b, HKDF (expect/actual).
//   - pair       — pair flow state machine and payload assembly.
//   - announce   — announce lifecycle, seq tracking, carrier selection.
//   - storage    — per-Resolver pin storage (expect/actual to Keychain / Keystore).
//
// See docs/MOBILE.md §2 for the architecture and
// docs/decisions/0003-kmp-for-mobile-core.md for the KMP rationale.
//
// The conformance posture: every protocol primitive defined here must
// pass the JSON fixtures in test-vectors/. The daemon-side Rust
// implementation passes the same fixtures — that's how cross-language
// drift is prevented.

package dev.mcpbridge.mobile

/** Module version. Tracks the wire protocol version it implements. */
public object BridgePeerCore {
    public const val VERSION: String = "0.1.0-SNAPSHOT"
}
