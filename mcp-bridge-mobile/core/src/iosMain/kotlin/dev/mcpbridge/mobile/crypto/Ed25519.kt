package dev.mcpbridge.mobile.crypto

/**
 * iOS Ed25519 placeholder. The real implementation will go through
 * either Apple `CryptoKit` (`Curve25519.Signing`) for short-lived
 * keys + the libsodium framework for `crypto_box`-side construction,
 * or libsodium for both — that wiring (CocoaPods/SPM + Kotlin/Native
 * cinterop) lands in a follow-up commit.
 *
 * No iOS test exercises this stub today. The expect/actual scaffold
 * is here so the SDK compiles for iosArm64 / iosSimulatorArm64 /
 * iosX64 alongside the working JVM and Android implementations.
 */
internal actual object Ed25519 {
    private const val NOT_IMPLEMENTED_MESSAGE: String =
        "iOS Ed25519 actual is not implemented yet — see ADR 0006."

    actual fun fromSeed(seed: ByteArray): Ed25519KeyPair {
        throw NotImplementedError(NOT_IMPLEMENTED_MESSAGE)
    }

    actual fun sign(privateKey: ByteArray, message: ByteArray): ByteArray {
        throw NotImplementedError(NOT_IMPLEMENTED_MESSAGE)
    }

    actual fun verify(
        publicKey: ByteArray,
        message: ByteArray,
        signature: ByteArray,
    ): Boolean {
        throw NotImplementedError(NOT_IMPLEMENTED_MESSAGE)
    }
}
