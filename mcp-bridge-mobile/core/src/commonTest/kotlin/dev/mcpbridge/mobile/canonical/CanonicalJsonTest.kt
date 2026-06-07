package dev.mcpbridge.mobile.canonical

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

/**
 * Tests for the RFC 8785 (JCS) canonical-JSON encoder.
 *
 * Daemon parity is the binding contract — the Rust side uses
 * `serde_jcs::to_vec`. Each expected output below is also what
 * serde_jcs produces for the same input, hand-verified.
 */
class CanonicalJsonTest {
    private val json = Json

    @Test
    fun emptyObjectAndArray() {
        assertEquals("{}", CanonicalJson.encodeToString(buildJsonObject {}))
        assertEquals("[]", CanonicalJson.encodeToString(buildJsonArray {}))
    }

    @Test
    fun nullAndBooleans() {
        assertEquals(
            """{"a":null,"b":true,"c":false}""",
            CanonicalJson.encodeToString(
                json.parseToJsonElement("""{"a":null,"b":true,"c":false}"""),
            ),
        )
    }

    @Test
    fun integerNumbers() {
        assertEquals(
            """{"a":0,"b":1,"c":-1,"d":9007199254740992}""",
            CanonicalJson.encodeToString(buildJsonObject {
                put("a", 0)
                put("b", 1)
                put("c", -1)
                put("d", 9_007_199_254_740_992L)
            }),
        )
    }

    @Test
    fun objectKeysSortByUtf16CodeUnit() {
        val input = buildJsonObject {
            put("b", 2)
            put("a", 1)
            put("c", 3)
        }
        assertEquals("""{"a":1,"b":2,"c":3}""", CanonicalJson.encodeToString(input))
    }

    @Test
    fun arrayOrderIsPreserved() {
        val input = json.parseToJsonElement("""[3,1,2]""")
        assertEquals("""[3,1,2]""", CanonicalJson.encodeToString(input))
    }

    @Test
    fun stringEscapesUseOnlyTheSevenShortcutsPlusU00XX() {
        // U+0001: no shortcut, must emit "".
        // U+0008, U+0009, U+000A, U+000C, U+000D: shortcuts (\b \t \n \f \r).
        // 0x22 (") and 0x5C (\): shortcuts.
        // 0x2F (/): NOT escaped per RFC 8785 §3.2.2.2.
        // U+007F (DEL): NOT in the JCS escape set; verbatim.
        val rawString = "\u0001\u0008\u0009\u000A\u000C\u000D\"\\/\u007F"
        val input = buildJsonObject { put("s", rawString) }
        val canonical = CanonicalJson.encodeToString(input)
        val expectedEscaped = "\\u0001\\b\\t\\n\\f\\r\\\"\\\\/"
        assertEquals("""{"s":"$expectedEscaped"}""", canonical)
    }

    @Test
    fun nonAsciiCharactersAreEmittedVerbatim() {
        // U+20AC (€), U+1F980 (🦀, surrogate pair), U+00F1 (ñ) all pass through.
        val input = buildJsonObject {
            put("currency", "€")
            put("crab", "🦀")
            put("greeting", "hola, señor")
        }
        val canonical = CanonicalJson.encodeToString(input)
        assertEquals(
            "{\"crab\":\"🦀\"," +
                "\"currency\":\"€\"," +
                "\"greeting\":\"hola, señor\"}",
            canonical,
        )
    }

    @Test
    fun unicodeKeysSortByUtf16CodeUnitNotByLocale() {
        // 'Z' is U+005A (90); 'a' is U+0061 (97). ASCII order: Z < a < b.
        val input = buildJsonObject {
            put("a", 1)
            put("Z", 2)
            put("b", 3)
        }
        assertEquals("""{"Z":2,"a":1,"b":3}""", CanonicalJson.encodeToString(input))
    }

    @Test
    fun nestedStructuresCanonicalize() {
        val input = json.parseToJsonElement(
            """{"outer":{"z":1,"a":[3,2,1],"m":{"y":true,"x":null}}}""",
        )
        assertEquals(
            """{"outer":{"a":[3,2,1],"m":{"x":null,"y":true},"z":1}}""",
            CanonicalJson.encodeToString(input),
        )
    }

    @Test
    fun nonIntegerNumberIsRejected() {
        val input = json.parseToJsonElement("""{"weight":1.5}""")
        val ex = assertFailsWith<CanonicalJsonException> {
            CanonicalJson.encodeToString(input)
        }
        assertTrue(
            ex.message!!.contains("1.5"),
            "exception should mention the offending value, got: ${ex.message}",
        )
    }

    @Test
    fun bytesAreUtf8() {
        val input = buildJsonObject { put("e", "€") }
        val bytes = CanonicalJson.encodeToBytes(input)
        // {"e":"€"} → 0x7B 0x22 0x65 0x22 0x3A 0x22 0xE2 0x82 0xAC 0x22 0x7D
        val expected = byteArrayOf(
            0x7B, 0x22, 0x65, 0x22, 0x3A, 0x22,
            0xE2.toByte(), 0x82.toByte(), 0xAC.toByte(),
            0x22, 0x7D,
        )
        assertEquals(expected.toList(), bytes.toList())
    }

    /**
     * End-to-end shape parity with what `serde_jcs::to_vec` produces
     * for a typical announce-payload-shaped object. The exact bytes
     * are what the daemon signs over; any drift here breaks Ed25519
     * verification.
     */
    @Test
    fun announceShapedPayloadMatchesDaemonOutput() {
        val payload = buildJsonObject {
            put("spec", "mcp-announce/v0.1")
            put("origin", "ed25519:abc")
            put("lid", "bodylog-7f3a")
            put("seq", 42)
            put("exp", 1_700_000_060L)
            put("loop_key", "base64url-stuff")
            put("auth_rotated_at", 1_700_000_000L)
        }
        val expected =
            """{"auth_rotated_at":1700000000,"exp":1700000060,"lid":"bodylog-7f3a","loop_key":"base64url-stuff","origin":"ed25519:abc","seq":42,"spec":"mcp-announce/v0.1"}"""
        assertEquals(expected, CanonicalJson.encodeToString(payload))
    }
}
