package dev.mcpbridge.mobile.canonical

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.test.fail

/**
 * Drives every JSON fixture under `test-vectors/canonical/` through
 * [CanonicalJson.encodeToBytes] and asserts the result matches each
 * fixture's `canonical` field byte-for-byte.
 *
 * The Rust daemon's `tests/canonical_conformance.rs` runs the same
 * fixture set through `serde_jcs::to_vec`. Whatever each implementation
 * does, both must agree on the exact byte sequence — the SPEC §3.3
 * canonicalization rule is what Ed25519 signatures cover.
 *
 * Filesystem-based: this test lives in the JVM-only `jvmTest` source
 * set because reading a sibling repository directory at test time is
 * easy on JVM and awkward on Kotlin/Native. The commonTest unit suite
 * in `CanonicalJsonTest` covers the cross-target Kotlin parity story
 * (JVM + Android + iOS sim all produce identical bytes for the same
 * inputs); this test layers on the cross-language parity claim
 * against the Rust daemon.
 */
class CanonicalConformanceTest {
    private val json = Json

    @Serializable
    private data class Fixture(
        val name: String,
        val description: String = "",
        val input: JsonElement,
        val canonical: String,
    )

    @Test
    fun everyFixtureCanonicalizesToItsExpectedBytes() {
        val dir = fixtureDir()
        assertTrue(dir.isDirectory, "fixture dir missing: ${dir.absolutePath}")

        val fixtureFiles = dir.listFiles { f -> f.extension == "json" }
            ?.sortedBy { it.name }
            ?: error("listFiles returned null for ${dir.absolutePath}")
        assertTrue(fixtureFiles.isNotEmpty(), "no fixtures in ${dir.absolutePath}")

        val failures = mutableListOf<String>()

        for (file in fixtureFiles) {
            val fixture = json.decodeFromString(Fixture.serializer(), file.readText())
            val actual = CanonicalJson.encodeToBytes(fixture.input)
            val expected = fixture.canonical.encodeToByteArray()
            if (!actual.contentEquals(expected)) {
                failures += buildString {
                    append(fixture.name).append(" (").append(file.name).append("):\n")
                    append("  expect: ").append(bytesRepr(expected)).append('\n')
                    append("  got:    ").append(bytesRepr(actual))
                }
            }
        }

        if (failures.isNotEmpty()) {
            fail(
                "${failures.size} of ${fixtureFiles.size} canonical fixtures diverged:\n\n" +
                    failures.joinToString("\n\n"),
            )
        }
        // The Rust runner asserts on the same fixture count, so this
        // also enforces "neither side silently skipped any file".
        assertEquals(fixtureFiles.size, 6, "expected 6 fixtures; bump this if you add one")
    }

    /**
     * `test-vectors/canonical/` relative to the repository root. Gradle
     * runs tests with the working directory set to the module's project
     * dir (`mcp-bridge-mobile/core/`), so up-two-then-down lands at the
     * fixture directory.
     */
    private fun fixtureDir(): File {
        // First try cwd-relative (covers `./gradlew :jvmTest` from the
        // module dir). Fall back to a search up the tree for the case
        // where someone runs the test from an IDE that picks a
        // different working dir.
        val cwdRelative = File("../../test-vectors/canonical")
        if (cwdRelative.isDirectory) return cwdRelative.canonicalFile

        var probe: File? = File(".").canonicalFile
        while (probe != null) {
            val candidate = File(probe, "test-vectors/canonical")
            if (candidate.isDirectory) return candidate
            probe = probe.parentFile
        }
        error(
            "could not locate test-vectors/canonical from " +
                File(".").canonicalPath,
        )
    }

    private fun bytesRepr(b: ByteArray): String {
        val sb = StringBuilder("\"")
        for (byte in b) {
            val c = byte.toInt() and 0xFF
            when (c) {
                0x22 -> sb.append("\\\"")
                0x5C -> sb.append("\\\\")
                0x08 -> sb.append("\\b")
                0x09 -> sb.append("\\t")
                0x0A -> sb.append("\\n")
                0x0C -> sb.append("\\f")
                0x0D -> sb.append("\\r")
                in 0x20..0x7E -> sb.append(c.toChar())
                else -> sb.append("\\x").append(c.toString(16).padStart(2, '0'))
            }
        }
        sb.append('"')
        return sb.toString()
    }
}
