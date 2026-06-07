package dev.mcpbridge.mobile.crypto

/**
 * Pure-Kotlin SHA-256 implementation (FIPS 180-4 §6.2).
 *
 * ## Why not platform-native
 *
 * [ADR 0006](../../../../../../../../../../docs/decisions/0006-kmp-crypto-stack.md)
 * picks platform-native APIs for **keyed** crypto — Ed25519, X25519,
 * the libsodium `crypto_box` envelope — where side-channel resistance
 * and hardware-bound keys matter. SHA-256 is an unkeyed hash; constant
 * time isn't a property we need to defend, and shipping the same
 * Kotlin code on every target gives us the cleanest cross-platform
 * conformance story for the SAS derivation
 * ([`docs/SPEC.md`](../../../../../../../../../../docs/SPEC.md) §4.2)
 * and the daily mDNS service-type derivation ([SPEC §5.2](../../../../../../../../../../docs/SPEC.md)).
 *
 * ## Performance
 *
 * The compute budget is dominated by Ed25519, not SHA-256. The hashes
 * we compute are short (≤ 48 bytes for SAS, ≤ a few KB for canonical
 * payloads) and this implementation is comfortably above microbench
 * limits for those sizes on every target we ship to.
 *
 * ## Test surface
 *
 * Covered by [`Sha256Test`] against the FIPS 180-2 sample vectors
 * (empty string, "abc", and the 56-byte multi-block test), and
 * end-to-end by [`SasDerivationTest`] via the `test-vectors/invite/`
 * fixture.
 */
public object Sha256 {
    /** Compute the 32-byte SHA-256 digest of `input`. */
    public fun digest(input: ByteArray): ByteArray {
        val state = State()
        state.update(input)
        return state.finish()
    }

    /** Streaming-friendly state for callers that need to feed multiple buffers. */
    public class State {
        // Initial hash values H_0..H_7 from FIPS 180-4 §5.3.3.
        private var h0 = 0x6a09e667
        private var h1 = -0x4498517b   // 0xbb67ae85
        private var h2 = 0x3c6ef372
        private var h3 = -0x5ab00ac6   // 0xa54ff53a
        private var h4 = 0x510e527f
        private var h5 = -0x64fa9774   // 0x9b05688c
        private var h6 = 0x1f83d9ab
        private var h7 = 0x5be0cd19

        private val block = ByteArray(64)
        private var blockLen = 0
        private var totalBitsLow = 0L

        public fun update(buf: ByteArray, off: Int = 0, len: Int = buf.size - off): State {
            require(off >= 0 && len >= 0 && off + len <= buf.size) {
                "out of range: off=$off len=$len size=${buf.size}"
            }
            totalBitsLow += len.toLong() * 8
            var i = off
            val end = off + len
            // Top up the partial block.
            if (blockLen > 0) {
                val take = minOf(64 - blockLen, end - i)
                buf.copyInto(block, blockLen, i, i + take)
                blockLen += take
                i += take
                if (blockLen == 64) {
                    compress(block, 0)
                    blockLen = 0
                }
            }
            // Process full blocks directly from buf.
            while (end - i >= 64) {
                compress(buf, i)
                i += 64
            }
            if (i < end) {
                buf.copyInto(block, 0, i, end)
                blockLen = end - i
            }
            return this
        }

        public fun finish(): ByteArray {
            val totalBits = totalBitsLow
            block[blockLen++] = 0x80.toByte()
            if (blockLen > 56) {
                while (blockLen < 64) block[blockLen++] = 0
                compress(block, 0)
                blockLen = 0
            }
            while (blockLen < 56) block[blockLen++] = 0
            // Length encoded as a big-endian 64-bit word.
            for (i in 0..7) {
                block[56 + i] = (totalBits ushr (56 - i * 8)).toByte()
            }
            compress(block, 0)

            val out = ByteArray(32)
            writeBeInt(h0, out, 0)
            writeBeInt(h1, out, 4)
            writeBeInt(h2, out, 8)
            writeBeInt(h3, out, 12)
            writeBeInt(h4, out, 16)
            writeBeInt(h5, out, 20)
            writeBeInt(h6, out, 24)
            writeBeInt(h7, out, 28)
            return out
        }

        @Suppress("NestedBlockDepth", "LocalVariableName")
        private fun compress(buf: ByteArray, offset: Int) {
            val w = IntArray(64)
            for (i in 0..15) {
                val b = offset + i * 4
                w[i] = ((buf[b].toInt() and 0xFF) shl 24) or
                    ((buf[b + 1].toInt() and 0xFF) shl 16) or
                    ((buf[b + 2].toInt() and 0xFF) shl 8) or
                    (buf[b + 3].toInt() and 0xFF)
            }
            for (i in 16..63) {
                val s0 = (w[i - 15].rotateRight(7)) xor (w[i - 15].rotateRight(18)) xor (w[i - 15] ushr 3)
                val s1 = (w[i - 2].rotateRight(17)) xor (w[i - 2].rotateRight(19)) xor (w[i - 2] ushr 10)
                w[i] = w[i - 16] + s0 + w[i - 7] + s1
            }

            var a = h0; var b = h1; var c = h2; var d = h3
            var e = h4; var f = h5; var g = h6; var hh = h7

            for (i in 0..63) {
                val s1 = e.rotateRight(6) xor e.rotateRight(11) xor e.rotateRight(25)
                val ch = (e and f) xor (e.inv() and g)
                val temp1 = hh + s1 + ch + K[i] + w[i]
                val s0 = a.rotateRight(2) xor a.rotateRight(13) xor a.rotateRight(22)
                val maj = (a and b) xor (a and c) xor (b and c)
                val temp2 = s0 + maj
                hh = g; g = f; f = e
                e = d + temp1
                d = c; c = b; b = a
                a = temp1 + temp2
            }

            h0 += a; h1 += b; h2 += c; h3 += d
            h4 += e; h5 += f; h6 += g; h7 += hh
        }

        private companion object {
            // First 32 bits of the fractional parts of the cube roots
            // of the first 64 primes (FIPS 180-4 §4.2.2). Hex literals
            // wrap via Int.toInt() because Kotlin Ints are signed.
            private val K = intArrayOf(
                0x428a2f98.toInt(), 0x71374491.toInt(), -0x4a3f0431, -0x164a245b,
                0x3956c25b.toInt(), 0x59f111f1.toInt(), -0x6dc07d5c, -0x54e3a12b,
                -0x27f85568, 0x12835b01.toInt(), 0x243185be.toInt(), 0x550c7dc3.toInt(),
                0x72be5d74.toInt(), -0x7f214e02, -0x6423f959, -0x3e640e8c,
                -0x1b64963f, -0x1041b87a, 0x0fc19dc6.toInt(), 0x240ca1cc.toInt(),
                0x2de92c6f.toInt(), 0x4a7484aa.toInt(), 0x5cb0a9dc.toInt(), 0x76f988da.toInt(),
                -0x67c1aeae, -0x57ce3993, -0x4ffcd838, -0x40a68039,
                -0x391ff40d, -0x2a586eb9, 0x06ca6351.toInt(), 0x14292967.toInt(),
                0x27b70a85.toInt(), 0x2e1b2138.toInt(), 0x4d2c6dfc.toInt(), 0x53380d13.toInt(),
                0x650a7354.toInt(), 0x766a0abb.toInt(), -0x7e3d36d2, -0x6d8dd37b,
                -0x5d40175f, -0x57e599b5, -0x3db47490, -0x3893ae5d,
                -0x2e6d17e7, -0x2966f9dc, -0xbf1ca7b, 0x106aa070.toInt(),
                0x19a4c116.toInt(), 0x1e376c08.toInt(), 0x2748774c.toInt(), 0x34b0bcb5.toInt(),
                0x391c0cb3.toInt(), 0x4ed8aa4a.toInt(), 0x5b9cca4f.toInt(), 0x682e6ff3.toInt(),
                0x748f82ee.toInt(), 0x78a5636f.toInt(), -0x7b3787ec, -0x7338fdf8,
                -0x6f410006, -0x5baf9315, -0x41065c09, -0x398e870e,
            )

            private fun writeBeInt(v: Int, out: ByteArray, offset: Int) {
                out[offset] = (v ushr 24).toByte()
                out[offset + 1] = (v ushr 16).toByte()
                out[offset + 2] = (v ushr 8).toByte()
                out[offset + 3] = v.toByte()
            }
        }
    }
}
