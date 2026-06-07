// Bridge Peer SDK — KMP protocol core.
//
// Implements docs/SPEC.md (mcp-pair/v0.1, mcp-announce/v0.1) and is
// consumed by every mobile-side packaging in docs/MOBILE.md §2.
// Targets:
//   - jvm()        — direct JVM consumers and as the build the
//                    androidTarget shares its `expect`/`actual`s with.
//   - androidTarget() — produces the AAR consumed by Native Kotlin,
//                       React Native, Flutter, Capacitor.
//   - iosArm64() / iosSimulatorArm64() / iosX64()
//                  — produce the xcframework consumed by Native Swift,
//                    React Native, Flutter, Capacitor on iOS.
//   - js()         — produces the JS bundle (deferred — wired in when
//                    the Capacitor packaging needs it).
//
// iOS targets are commented out in this initial scaffold because they
// require the Kotlin/Native toolchain to be downloaded on first build
// (~hundreds of MB) and we want the first `./gradlew assemble` to be
// quick. Uncomment when wiring iOS in.

@file:Suppress("UnstableApiUsage")

plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.android.library)
}

group = "dev.mcpbridge"
version = "0.1.0-SNAPSHOT"

kotlin {
    jvmToolchain(libs.versions.jvm.target.get().toInt())

    androidTarget {
        publishLibraryVariants("release")
    }

    jvm()

    listOf(iosArm64(), iosSimulatorArm64(), iosX64()).forEach {
        it.binaries.framework {
            baseName = "McpBridgeMobileCore"
            isStatic = true
        }
    }

    sourceSets {
        commonMain.dependencies {
            implementation(libs.kotlinx.serialization.json)
            implementation(libs.kotlinx.coroutines.core)
            implementation(libs.kotlinx.datetime)
        }
        commonTest.dependencies {
            implementation(libs.kotlin.test)
        }
    }
}

android {
    namespace = "dev.mcpbridge.mobile.core"
    compileSdk = libs.versions.android.compile.sdk.get().toInt()
    defaultConfig {
        minSdk = libs.versions.android.min.sdk.get().toInt()
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
