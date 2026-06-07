package dev.mcpbridge.mobile.crypto

import kotlin.test.Test
import kotlin.test.assertEquals

/**
 * Conformance tests against the FIPS 180-2 sample digests for SHA-256.
 * The "" / "abc" / 56-byte cases exercise the empty-input edge case,
 * the single-block case, and the two-block case (the 56-byte input
 * needs a continuation block because length encoding occupies the
 * last 8 bytes of a block).
 */
class Sha256Test {
    @Test
    fun emptyInput() {
        assertEquals(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            Sha256.digest(ByteArray(0)).toHex(),
        )
    }

    @Test
    fun fipsAbc() {
        assertEquals(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            Sha256.digest("abc".encodeToByteArray()).toHex(),
        )
    }

    @Test
    fun fipsTwoBlockSample() {
        val msg = "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        assertEquals(
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            Sha256.digest(msg.encodeToByteArray()).toHex(),
        )
    }

    @Test
    fun millionAs() {
        // FIPS-style: SHA-256("a" * 1_000_000). Exercises the streaming
        // update path across many blocks.
        val state = Sha256.State()
        val chunk = ByteArray(1000) { 'a'.code.toByte() }
        repeat(1000) { state.update(chunk) }
        assertEquals(
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
            state.finish().toHex(),
        )
    }

    @Test
    fun zerosForSasInput() {
        // The 48-byte all-zero input (32-byte pubkey ‖ 16-byte nonce)
        // from the accept-canonical invite fixture. Source of truth for
        // the SAS derivation test downstream.
        assertEquals(
            "17b0761f87b081d5cf10757ccc89f12be355c70e2e29df288b65b30710dcbcd1",
            Sha256.digest(ByteArray(48)).toHex(),
        )
    }
}

private fun ByteArray.toHex(): String {
    val sb = StringBuilder(size * 2)
    for (b in this) {
        val v = b.toInt() and 0xFF
        sb.append(HEX[v ushr 4])
        sb.append(HEX[v and 0x0F])
    }
    return sb.toString()
}

private val HEX = "0123456789abcdef".toCharArray()
