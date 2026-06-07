package dev.mcpbridge.mobile.canonical

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.longOrNull

/**
 * RFC 8785 (JSON Canonicalization Scheme, JCS) encoder.
 *
 * Used to produce the byte sequence a signature covers. The
 * canonical form must agree with `serde_jcs` on the daemon side
 * (see `mcp-bridged/src/announce/payload.rs::canonical_signing_bytes`)
 * to the bit — `Ed25519_Verify` is otherwise meaningless.
 *
 * ## Rules implemented (per RFC 8785 §3)
 *
 * - **Object keys**: sorted ascending by UTF-16 code unit. Duplicate
 *   keys are not allowed in source JSON and are not produced here.
 * - **Strings**: shortest valid escape — only the seven required
 *   shorthand escapes (`\"`, `\\`, `\b`, `\t`, `\n`, `\f`, `\r`) plus
 *   `\u00xx` for the remaining ASCII controls; everything else is
 *   emitted verbatim, including non-ASCII.
 * - **Arrays**: element order preserved.
 * - **No insignificant whitespace** between tokens.
 * - **Numbers**: integer-only support in v0.1. The two MCP Bridge
 *   wire protocols (`mcp-pair/v0.1`, `mcp-announce/v0.1`) carry only
 *   integer-valued numerics — timestamps in Unix seconds, the
 *   announce `seq` counter. Floating-point JSON numbers are rejected
 *   with [CanonicalJsonException]; supporting ECMA-262 7.1.12.1
 *   Number-to-String is a non-trivial KMP exercise that we defer
 *   until a protocol field needs it.
 *
 * ## Why not use kotlinx-serialization's default encoder
 *
 * `Json { prettyPrint = false }.encodeToString(element)` does not sort
 * object keys (it preserves input order), does not normalize numeric
 * formats, and does not constrain string escapes to the JCS subset.
 * The output coincides with canonical form for some inputs and not
 * for others — silent divergence is exactly the failure mode JCS is
 * designed to prevent.
 */
public object CanonicalJson {
    /** Encode the given element to RFC 8785 canonical JSON bytes (UTF-8). */
    public fun encodeToBytes(element: JsonElement): ByteArray =
        encodeToString(element).encodeToByteArray()

    /** Encode the given element to an RFC 8785 canonical JSON string. */
    public fun encodeToString(element: JsonElement): String {
        val sb = StringBuilder()
        write(element, sb)
        return sb.toString()
    }

    private fun write(element: JsonElement, out: StringBuilder) {
        when (element) {
            is JsonNull -> out.append("null")
            is JsonObject -> writeObject(element, out)
            is JsonArray -> writeArray(element, out)
            is JsonPrimitive -> writePrimitive(element, out)
        }
    }

    private fun writeObject(obj: JsonObject, out: StringBuilder) {
        out.append('{')
        val sortedKeys = obj.keys.sortedWith(Utf16Order)
        for ((i, key) in sortedKeys.withIndex()) {
            if (i > 0) out.append(',')
            writeString(key, out)
            out.append(':')
            write(obj.getValue(key), out)
        }
        out.append('}')
    }

    private fun writeArray(arr: JsonArray, out: StringBuilder) {
        out.append('[')
        for ((i, item) in arr.withIndex()) {
            if (i > 0) out.append(',')
            write(item, out)
        }
        out.append(']')
    }

    private fun writePrimitive(p: JsonPrimitive, out: StringBuilder) {
        if (p.isString) {
            writeString(p.content, out)
            return
        }
        p.booleanOrNull?.let {
            out.append(if (it) "true" else "false")
            return
        }
        p.longOrNull?.let {
            out.append(it.toString())
            return
        }
        // Non-integer numeric — we'd need ECMA-262 7.1.12.1
        // Number-to-String. See the doc note on the object.
        throw CanonicalJsonException(
            "non-integer numeric primitive ${p.content} cannot be canonicalized in v0.1",
        )
    }

    private fun writeString(s: String, out: StringBuilder) {
        out.append('"')
        var i = 0
        while (i < s.length) {
            val ch = s[i]
            when (ch.code) {
                0x22 -> out.append("\\\"")        // "
                0x5C -> out.append("\\\\")        // \
                0x08 -> out.append("\\b")
                0x09 -> out.append("\\t")
                0x0A -> out.append("\\n")
                0x0C -> out.append("\\f")
                0x0D -> out.append("\\r")
                else -> {
                    if (ch.code < 0x20) {
                        out.append("\\u")
                        out.append(ch.code.toString(16).padStart(4, '0'))
                    } else {
                        out.append(ch)
                    }
                }
            }
            i++
        }
        out.append('"')
    }

    /**
     * UTF-16 code-unit ordering for object keys. Both kotlinx.String
     * comparison and `String.compareTo` are UTF-16-code-unit-ordered
     * on every KMP target today, but we pin the comparator explicitly
     * so the ordering cannot drift if the stdlib semantics change.
     */
    private object Utf16Order : Comparator<String> {
        override fun compare(a: String, b: String): Int {
            val limit = minOf(a.length, b.length)
            var i = 0
            while (i < limit) {
                val ca = a[i].code
                val cb = b[i].code
                if (ca != cb) return ca - cb
                i++
            }
            return a.length - b.length
        }
    }
}

public class CanonicalJsonException(message: String) : RuntimeException(message)
