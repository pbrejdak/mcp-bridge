package dev.mcpbridge.mobile.model

import kotlinx.serialization.Serializable

/**
 * A scope the Origin's MCP server advertises to the paired Resolver.
 *
 * The names here are wire-level — they appear in `mcp-pair/v0.1`
 * payloads. New scopes are an additive protocol change.
 */
@Serializable
public enum class Scope {
    @kotlinx.serialization.SerialName("tools")
    TOOLS,

    @kotlinx.serialization.SerialName("resources")
    RESOURCES,

    @kotlinx.serialization.SerialName("prompts")
    PROMPTS,
}

/**
 * TLS configuration for the host app's loopback MCP server. The
 * Resolver pins `certFingerprint` and trusts only the embedded
 * `caPem` — see docs/SPEC.md §4 (mcp-pair/v0.1 schema).
 */
@Serializable
public data class ServerConfig(
    /** Must be `https://127.0.0.1:<port>/`. The SDK validates this on init. */
    val url: String,
    /** SHA-256 of the leaf certificate, hex-lowercase. */
    val certFingerprint: String,
    /** PEM-encoded self-signed CA. */
    val caPem: String,
)

/**
 * Origin-side configuration passed to `BridgePeer.init`. Carries the
 * host-app's identity, the loopback MCP server's TLS details, and the
 * scopes advertised in the pair payload.
 *
 * The bearer-token retrieval callback (`authProvider` in the
 * TypeScript surface) is held in the language-specific wrapper layer
 * — it never crosses into this shared model because it is not
 * serializable.
 */
@Serializable
public data class OriginConfig(
    val name: String,
    val logicalId: String,
    val scope: List<Scope>,
    val server: ServerConfig,
)
