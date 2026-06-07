// KMP build root for the Bridge Peer SDK protocol core.
//
// See docs/MOBILE.md §2 and docs/decisions/0003-kmp-for-mobile-core.md
// for the architectural decision driving this layout. The core compiles
// to an iOS xcframework, an Android AAR, and a JS bundle from a single
// Kotlin source tree.

@file:Suppress("UnstableApiUsage")

pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
        google()
    }
}

dependencyResolutionManagement {
    repositoriesMode = RepositoriesMode.FAIL_ON_PROJECT_REPOS
    repositories {
        mavenCentral()
        google()
    }
}

rootProject.name = "mcp-bridge-mobile-core"
