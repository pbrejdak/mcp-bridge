package dev.mcpbridge.mobile.model

import kotlinx.serialization.Serializable

/**
 * Decoded `mcp-pair/v0.1` invite produced by the Resolver and surfaced
 * to the phone via QR scan. The SDK reconstructs this from the
 * scanned bytes after validating spec/version/expiry.
 *
 * Field semantics are normative — see docs/SPEC.md §4.
 */
@Serializable
public data class ResolverInvite(
    /** Base64url-encoded Ed25519 public key of the Resolver. */
    val resolverPubkey: String,
    /** User-facing Resolver name (e.g. "Patryk's MacBook Pro"). */
    val displayName: String,
    /** SAS phrase the user verifies cross-screen with Bridge Console. */
    val sas: String,
    /** LAN address (`https://host:port/pair`) the pair POST targets. */
    val lanAddr: String,
    /** Single-use nonce the Resolver inserts into the invite. */
    val nonce: String,
    /** Unix-seconds expiry — defaults to 60s after creation. */
    val expiresAt: Long,
)
