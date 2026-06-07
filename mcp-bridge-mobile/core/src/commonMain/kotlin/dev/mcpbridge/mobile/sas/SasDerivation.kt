package dev.mcpbridge.mobile.sas

import dev.mcpbridge.mobile.crypto.Sha256

/**
 * Short Authentication String (SAS) derivation per
 * [docs/SPEC.md](../../../../../../../../../../docs/SPEC.md) §4.2.
 *
 * The SAS is what the user reads cross-screen between the phone and
 * the Bridge Console to confirm a pair — four lowercase words joined
 * by `-`. The bytes the encoder sees are the Resolver's Ed25519
 * pubkey concatenated with the invite nonce; the SHA-256 of that
 * concatenation produces four 16-bit indices into the locked-at-v0.1
 * [BIP39 English wordlist](../../../../../../../../../../test-vectors/sas-wordlist-v1.txt).
 *
 * The conformance contract is byte-equality with the daemon (see
 * `mcp-bridged/src/pair/sas.rs` and the `test-vectors/invite/`
 * fixtures). Drift on either side breaks the user-facing comparison
 * silently, which is exactly the failure mode the SAS is supposed to
 * defend against.
 */
public object SasDerivation {
    /** Word count in the SAS phrase per [SPEC](../../../../../../../../../../docs/SPEC.md) §4.2. */
    public const val WORD_COUNT: Int = 4

    /** Expected byte length of the Resolver Ed25519 public key. */
    public const val PUBKEY_LEN: Int = 32

    /** Expected byte length of the invite nonce. */
    public const val NONCE_LEN: Int = 16

    /**
     * Compute the four-word SAS phrase for a given Resolver pubkey
     * and invite nonce.
     *
     * @throws IllegalArgumentException if either input is the wrong
     *   length (the daemon's typed surface enforces these
     *   invariants; the SDK does the same so a host-app bug is
     *   caught at the API boundary rather than producing a
     *   plausible-looking but incorrect SAS).
     */
    public fun derive(resolverPubkey: ByteArray, nonce: ByteArray): String {
        require(resolverPubkey.size == PUBKEY_LEN) {
            "resolverPubkey must be $PUBKEY_LEN bytes, got ${resolverPubkey.size}"
        }
        require(nonce.size == NONCE_LEN) {
            "nonce must be $NONCE_LEN bytes, got ${nonce.size}"
        }

        val state = Sha256.State()
        state.update(resolverPubkey)
        state.update(nonce)
        val h = state.finish()

        // First 8 bytes of H, parsed as 4 big-endian uint16s, each
        // taken mod 2048. The wordlist has exactly 2048 entries, so
        // the mod is the same as masking the low 11 bits — but the
        // SPEC writes it as a mod and we follow the spec literally.
        val words = ArrayList<String>(WORD_COUNT)
        for (i in 0 until WORD_COUNT) {
            val hi = h[i * 2].toInt() and 0xFF
            val lo = h[i * 2 + 1].toInt() and 0xFF
            val idx = ((hi shl 8) or lo) % SAS_WORDLIST_V1.size
            words.add(SAS_WORDLIST_V1[idx])
        }
        return words.joinToString("-")
    }
}
