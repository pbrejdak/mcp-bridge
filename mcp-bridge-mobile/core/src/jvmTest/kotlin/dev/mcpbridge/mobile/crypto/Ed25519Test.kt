package dev.mcpbridge.mobile.crypto

import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Ed25519 conformance against the RFC 8032 §7.1 Known Answer Tests.
 *
 * Three test vectors picked from the RFC cover (a) empty-message
 * signing, (b) a one-byte message, and (c) a two-byte message. These
 * are the canonical fixtures every Ed25519 implementation in the
 * world is expected to pass; they're more authoritative than a
 * fixture we'd write ourselves.
 *
 * In `jvmTest` (not commonTest) because the iOS actual is currently a
 * `NotImplementedError` stub. Once the iOS libsodium/CryptoKit
 * wiring lands, the body here moves to commonTest and runs against
 * every target.
 */
class Ed25519Test {
    @Test
    fun rfc8032Test1_emptyMessage() {
        val seed = hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
        val expectedPubkey =
            hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        val expectedSig = hex(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555f" +
                "b8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        )

        val keyPair = Ed25519.fromSeed(seed)
        assertContentEquals(expectedPubkey, keyPair.publicKey)
        assertContentEquals(seed, keyPair.privateKey)

        val sig = Ed25519.sign(seed, ByteArray(0))
        assertContentEquals(expectedSig, sig)

        assertTrue(Ed25519.verify(keyPair.publicKey, ByteArray(0), sig))
    }

    @Test
    fun rfc8032Test2_oneByteMessage() {
        val seed = hex("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb")
        val expectedPubkey =
            hex("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c")
        val expectedSig = hex(
            "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da08" +
                "5ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
        )

        val keyPair = Ed25519.fromSeed(seed)
        assertContentEquals(expectedPubkey, keyPair.publicKey)

        val message = hex("72")
        val sig = Ed25519.sign(seed, message)
        assertContentEquals(expectedSig, sig)

        assertTrue(Ed25519.verify(keyPair.publicKey, message, sig))
    }

    @Test
    fun rfc8032Test3_twoByteMessage() {
        val seed = hex("c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7")
        val expectedPubkey =
            hex("fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025")
        val expectedSig = hex(
            "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18" +
                "ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
        )

        val keyPair = Ed25519.fromSeed(seed)
        assertContentEquals(expectedPubkey, keyPair.publicKey)

        val message = hex("af82")
        val sig = Ed25519.sign(seed, message)
        assertContentEquals(expectedSig, sig)

        assertTrue(Ed25519.verify(keyPair.publicKey, message, sig))
    }

    @Test
    fun verifyRejectsCorruptedSignature() {
        val seed = hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
        val keyPair = Ed25519.fromSeed(seed)
        val sig = Ed25519.sign(seed, ByteArray(0))

        // Flip a bit. Any single-bit perturbation must cause verify to
        // fail — Ed25519's signature surface has no malleability margin.
        sig[0] = (sig[0].toInt() xor 1).toByte()
        assertFalse(Ed25519.verify(keyPair.publicKey, ByteArray(0), sig))
    }

    @Test
    fun verifyRejectsTamperedMessage() {
        val seed = hex("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb")
        val keyPair = Ed25519.fromSeed(seed)
        val originalMessage = hex("72")
        val sig = Ed25519.sign(seed, originalMessage)

        // Verifying against a different message must reject — even if
        // the signature was valid for the original.
        assertFalse(Ed25519.verify(keyPair.publicKey, hex("73"), sig))
    }

    @Test
    fun verifyRejectsWrongLengthSignature() {
        val seed = hex("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb")
        val keyPair = Ed25519.fromSeed(seed)
        // Returns false (not throws) per the contract — invalid bytes
        // are an unverifiable signature, not a programmer error.
        assertFalse(Ed25519.verify(keyPair.publicKey, ByteArray(0), ByteArray(63)))
        assertFalse(Ed25519.verify(keyPair.publicKey, ByteArray(0), ByteArray(65)))
        assertFalse(Ed25519.verify(keyPair.publicKey, ByteArray(0), ByteArray(0)))
    }

    @Test
    fun rejectsWrongSeedLength() {
        assertFailsWith<IllegalArgumentException> { Ed25519.fromSeed(ByteArray(31)) }
        assertFailsWith<IllegalArgumentException> { Ed25519.fromSeed(ByteArray(33)) }
    }

    @Test
    fun rejectsWrongPrivateKeyLengthForSign() {
        assertFailsWith<IllegalArgumentException> { Ed25519.sign(ByteArray(31), ByteArray(0)) }
        assertFailsWith<IllegalArgumentException> { Ed25519.sign(ByteArray(33), ByteArray(0)) }
    }

    @Test
    fun rejectsWrongPublicKeyLengthForVerify() {
        assertFailsWith<IllegalArgumentException> {
            Ed25519.verify(ByteArray(31), ByteArray(0), ByteArray(64))
        }
        assertFailsWith<IllegalArgumentException> {
            Ed25519.verify(ByteArray(33), ByteArray(0), ByteArray(64))
        }
    }

    @Test
    fun signaturesAreDeterministic() {
        // RFC 8032 specifies deterministic Ed25519 — signing the same
        // message with the same key must produce byte-identical output.
        val seed = hex("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb")
        val message = hex("72")
        val sig1 = Ed25519.sign(seed, message)
        val sig2 = Ed25519.sign(seed, message)
        assertContentEquals(sig1, sig2)
        assertEquals(64, sig1.size)
    }
}

private fun hex(s: String): ByteArray {
    require(s.length % 2 == 0) { "hex string must have even length" }
    val out = ByteArray(s.length / 2)
    var i = 0
    while (i < s.length) {
        out[i / 2] = (
            (Character.digit(s[i], 16) shl 4) + Character.digit(s[i + 1], 16)
            ).toByte()
        i += 2
    }
    return out
}
