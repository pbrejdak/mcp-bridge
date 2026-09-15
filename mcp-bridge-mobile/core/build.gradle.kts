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
        // BouncyCastle on every JVM-flavored target (per ADR 0006).
        // iOS goes through libsodium / CryptoKit in a later commit.
        jvmMain.dependencies {
            implementation(libs.bouncycastle)
        }
        androidMain.dependencies {
            implementation(libs.bouncycastle)
        }
    }
}

// Generate `SasWordlist.kt` from the canonical text fixture so the SDK
// ships with the same 2048 words the daemon and the SPEC mandate.
// Touching the .txt is a wire-protocol change (gated by CI's SHA-256
// check in `.github/workflows/ci.yml`); regeneration is implicit.
abstract class GenerateSasWordlistTask : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val wordlist: RegularFileProperty

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val words = wordlist.get().asFile.readLines().filter { it.isNotEmpty() }
        check(words.size == 2048) {
            "expected exactly 2048 SAS words, got ${words.size}"
        }
        val pkgDir = outputDir.get().asFile.resolve("dev/mcpbridge/mobile/sas")
        pkgDir.mkdirs()
        pkgDir.resolve("SasWordlist.kt").writer().use { w ->
            w.appendLine("// Generated from test-vectors/sas-wordlist-v1.txt. Do not edit.")
            w.appendLine("// See docs/SPEC.md §4.2 and test-vectors/README.md.")
            w.appendLine()
            w.appendLine("package dev.mcpbridge.mobile.sas")
            w.appendLine()
            w.appendLine("internal val SAS_WORDLIST_V1: Array<String> = arrayOf(")
            for (word in words) {
                w.append("    \"").append(word).appendLine("\",")
            }
            w.appendLine(")")
        }
    }
}

val generateSasWordlist = tasks.register<GenerateSasWordlistTask>("generateSasWordlist") {
    wordlist.set(layout.projectDirectory.file("../../test-vectors/sas-wordlist-v1.txt"))
    outputDir.set(layout.buildDirectory.dir("generated/source/sasWordlist/kotlin"))
}

kotlin.sourceSets.commonMain.configure {
    kotlin.srcDir(generateSasWordlist.map { it.outputDir })
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
