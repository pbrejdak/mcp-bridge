package dev.mcpbridge.mobile.sas

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class SasDerivationTest {
    /**
     * SPEC §4.2 KAT: the all-zeros pubkey + all-zeros nonce derive
     * the SAS phrase `voyage-sentence-voyage-deny`. This is the same
     * value the daemon's `accept-canonical.json` invite fixture
     * carries, so passing this proves Kotlin and Rust produce the
     * same SAS for the same inputs.
     */
    @Test
    fun allZerosMatchesSpecKat() {
        val pubkey = ByteArray(32)
        val nonce = ByteArray(16)
        assertEquals("voyage-sentence-voyage-deny", SasDerivation.derive(pubkey, nonce))
    }

    @Test
    fun produces4WordsJoinedByHyphen() {
        val pubkey = ByteArray(32) { (it).toByte() }
        val nonce = ByteArray(16) { (255 - it).toByte() }
        val sas = SasDerivation.derive(pubkey, nonce)
        val words = sas.split("-")
        assertEquals(SasDerivation.WORD_COUNT, words.size)
        for (word in words) {
            assertEquals(word, word.lowercase(), "word must be lowercase: $word")
            assertEquals(true, word.all { it in 'a'..'z' }, "word must be ASCII-only: $word")
        }
    }

    @Test
    fun rejectsWrongPubkeyLength() {
        val nonce = ByteArray(16)
        assertFailsWith<IllegalArgumentException> {
            SasDerivation.derive(ByteArray(31), nonce)
        }
        assertFailsWith<IllegalArgumentException> {
            SasDerivation.derive(ByteArray(33), nonce)
        }
    }

    @Test
    fun rejectsWrongNonceLength() {
        val pubkey = ByteArray(32)
        assertFailsWith<IllegalArgumentException> {
            SasDerivation.derive(pubkey, ByteArray(15))
        }
        assertFailsWith<IllegalArgumentException> {
            SasDerivation.derive(pubkey, ByteArray(17))
        }
    }
}
