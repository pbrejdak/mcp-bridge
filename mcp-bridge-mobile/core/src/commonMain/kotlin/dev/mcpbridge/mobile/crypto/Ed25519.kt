package dev.mcpbridge.mobile.crypto

/**
 * Ed25519 sign/verify primitives, per [RFC 8032](https://www.rfc-editor.org/rfc/rfc8032).
 *
 * ## Where the implementation lives
 *
 * [ADR 0006](../../../../../../../../../../docs/decisions/0006-kmp-crypto-stack.md)
 * picks platform-native backends so that long-lived Origin private
 * keys can be hardware-bound (iOS Keychain / Android Keystore /
 * StrongBox where available). This `expect` surface deliberately
 * stays byte-array-flavoured because it is what the conformance
 * fixtures need; the production [`KeyStore`](../keystore/KeyStore.kt)
 * wrapper (forthcoming) layers opaque-handle, never-touches-process-
 * memory semantics on top.
 *
 * Current actuals:
 * - **JVM**: [BouncyCastle](https://www.bouncycastle.org)'s
 *   `Ed25519PrivateKeyParameters` / `Ed25519Signer` — chosen over the
 *   JDK's SunEC Ed25519 because BC is also available on Android API
 *   26+ and using the same backend on both removes one cross-platform
 *   conformance variable.
 * - **Android**: same as JVM — BouncyCastle. Hardware-backed Keystore
 *   wraps this in a future commit; today's path is in-process bytes
 *   for testing parity with the daemon.
 * - **iOS**: stub that throws `NotImplementedError`. Real impl arrives
 *   with the libsodium framework wiring (CocoaPods/SPM + cinterop).
 *   The expect/actual hierarchy keeps the SDK compiling for iOS in
 *   the meantime; nothing in the iOS test path exercises this yet.
 */
internal expect object Ed25519 {
    /**
     * Derive a deterministic Ed25519 keypair from a 32-byte seed
     * (RFC 8032 §5.1.5). The seed *is* the private key in the RFC's
     * terminology — every other "private" representation derives from
     * it.
     *
     * @throws IllegalArgumentException if `seed.size != 32`.
     */
    fun fromSeed(seed: ByteArray): Ed25519KeyPair

    /**
     * Produce a 64-byte detached Ed25519 signature over `message` with
     * `privateKey` (the 32-byte seed).
     *
     * @throws IllegalArgumentException if `privateKey.size != 32`.
     */
    fun sign(privateKey: ByteArray, message: ByteArray): ByteArray

    /**
     * Verify a 64-byte detached Ed25519 signature. Returns `true` iff
     * the signature is valid for `message` under `publicKey`. Returns
     * `false` (rather than throwing) for any malformed inputs that
     * could only have produced an invalid signature — see RFC 8032
     * §5.1.7 on the verifier contract.
     *
     * @throws IllegalArgumentException if `publicKey.size != 32`.
     */
    fun verify(publicKey: ByteArray, message: ByteArray, signature: ByteArray): Boolean
}

/**
 * An Ed25519 keypair as raw bytes. `privateKey` is the 32-byte seed
 * (RFC 8032 secret-key form), `publicKey` is the 32-byte compressed
 * encoding of the curve point.
 *
 * Intentionally NOT a `data class` — `equals` on `ByteArray` is
 * reference equality, and exposing that as a value semantic invites
 * subtle bugs. Construct via [Ed25519.fromSeed].
 */
internal class Ed25519KeyPair(
    val privateKey: ByteArray,
    val publicKey: ByteArray,
)
