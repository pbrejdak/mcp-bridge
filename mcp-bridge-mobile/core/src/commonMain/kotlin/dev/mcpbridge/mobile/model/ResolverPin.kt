package dev.mcpbridge.mobile.model

import kotlinx.datetime.Instant
import kotlinx.serialization.Serializable

/**
 * A paired Resolver as stored in the SDK's per-Resolver pin store.
 *
 * Persisted in platform-secure storage (iOS Keychain /
 * Android Keystore-backed EncryptedSharedPreferences). See
 * docs/MOBILE.md §6.3 / §7.3.
 *
 * `lastAnnounceSeq` is the source of truth for the announce counter
 * and must never regress across app restarts — the daemon rejects a
 * regression as replay.
 */
@Serializable
public data class ResolverPin(
    /** Base64url-encoded Ed25519 public key. */
    val resolverPubkey: String,
    val resolverDisplayName: String,
    val pairedAt: Instant,
    val lastSuccessfulAnnounce: Instant?,
    /** Strictly increasing per pin; incremented before each announce. */
    val lastAnnounceSeq: Long,
    /** Last LAN URL the announce HTTP fallback should target. */
    val lastKnownLanAddr: String?,
)
