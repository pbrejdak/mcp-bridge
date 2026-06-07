package dev.mcpbridge.mobile

import dev.mcpbridge.mobile.model.OriginConfig
import dev.mcpbridge.mobile.model.Scope
import dev.mcpbridge.mobile.model.ServerConfig
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class LibraryTest {
    @Test
    fun versionIsSet() {
        assertTrue(BridgePeerCore.VERSION.isNotEmpty())
    }

    @Test
    fun originConfigConstructs() {
        val cfg = OriginConfig(
            name = "BodyLog",
            logicalId = "bodylog-7f3a-...",
            scope = listOf(Scope.TOOLS, Scope.RESOURCES),
            server = ServerConfig(
                url = "https://127.0.0.1:54321/",
                certFingerprint = "sha256:abc...",
                caPem = "-----BEGIN CERTIFICATE-----\n...",
            ),
        )
        assertEquals("BodyLog", cfg.name)
        assertEquals(2, cfg.scope.size)
    }
}
