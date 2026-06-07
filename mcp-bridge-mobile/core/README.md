# `mcp-bridge-mobile/core/` — KMP protocol core

The Bridge Peer SDK's wire-protocol implementation, written once in
Kotlin Multiplatform and compiled to:

- An **iOS framework** via Kotlin/Native (consumed from Swift via
  Obj-C interop; built as an xcframework for distribution).
- An **Android AAR** via the JVM target.
- A **plain JVM jar** for direct JVM consumers and for unit-testing
  the protocol logic outside the Android emulator.
- _(planned)_ A **JS bundle** via Kotlin/JS for the Capacitor and
  pure-web packagings.

Spec: [`docs/SPEC.md`](../../docs/SPEC.md). Implementation guide:
[`docs/MOBILE.md`](../../docs/MOBILE.md). ADR for the KMP choice:
[`docs/decisions/0003-kmp-for-mobile-core.md`](../../docs/decisions/0003-kmp-for-mobile-core.md).

## Status

**Scaffold.** What's in place:

- Gradle 8.14.3 + Kotlin 2.1.21 + Android Gradle Plugin 8.7.3
  configured.
- `commonMain` source set with the typed protocol models — `OriginConfig`,
  `ServerConfig`, `Scope`, `ResolverInvite`, `ResolverPin`.
- `commonTest` with kotlin.test + 2 sanity checks.
- Targets enabled: `jvm()`, `androidTarget()`, `iosArm64()`,
  `iosSimulatorArm64()`, `iosX64()`.
- Verified green: `./gradlew assemble`, `./gradlew :allTests`,
  `./gradlew linkDebugFrameworkIosSimulatorArm64`.

What's **not** in place yet:

- No cryptography. Ed25519, libsodium-style sealed box, BLAKE2b, HKDF,
  RFC 8785 canonical JSON — all TBD.
- No actual `BridgePeer` API surface. The TypeScript shape in
  [`docs/MOBILE.md`](../../docs/MOBILE.md) §3 is the target.
- No `expect` / `actual` declarations yet for Keychain/Keystore,
  camera, mDNS — those land alongside their first consumers.
- No conformance test runner for [`test-vectors/`](../../test-vectors).
- No JS target.

## Build

First-time setup needs `local.properties` pointing at the Android SDK
(it is gitignored):

```bash
echo "sdk.dir=$ANDROID_SDK_ROOT" > local.properties
# or, on macOS with Android Studio installed:
echo "sdk.dir=$HOME/Library/Android/sdk" > local.properties
```

Then:

```bash
./gradlew assemble           # all targets except iOS framework links
./gradlew :allTests          # unit tests on JVM + both Android variants
./gradlew linkDebugFrameworkIosSimulatorArm64
                             # iOS sim arm64 framework (slow first time
                             # — downloads ~700 MB into ~/.konan/)
```

## Toolchain expectations

| Tool | Version | Why |
|---|---|---|
| JDK | 17+ | `kotlin { jvmToolchain(17) }`; Android `JavaVersion.VERSION_17` |
| Gradle | 8.14.3 (via wrapper) | Pinned in `gradle/wrapper/gradle-wrapper.properties` |
| Kotlin | 2.1.21 | Pinned in `gradle/libs.versions.toml` |
| Android SDK | compileSdk 34, minSdk 26 | Per [`docs/MOBILE.md`](../../docs/MOBILE.md) §7.1 |
| Xcode | 14+ | For iOS Kotlin/Native final-link step |

Kotlin/Native downloads its own LLVM and sysroot into `~/.konan/` on
first build (~700 MB). If a download is interrupted, delete the
half-extracted dependency directory and rerun — KGP fails with a
`FileAlreadyExistsException` when retrying over a half-extracted dir.

## Next milestones

1. **RFC 8785 canonical JSON encoder** in `commonMain`. Wire signatures
   depend on byte-identical canonicalization with the Rust daemon.
2. **Ed25519** sign/verify, ideally per-target actuals using each
   platform's hardware-backed crypto (Apple `CryptoKit`,
   Android `KeyPairGenerator` with `AndroidKeyStore`,
   JVM `java.security.Signature` for tests). Library choice is an
   open ADR (see [`docs/MOBILE.md`](../../docs/MOBILE.md) §14).
3. **`crypto_box`** (libsodium-flavoured sealed box) for the pair
   payload — likely a Kotlin port driven by the test vectors, since
   libsodium's KMP story is rougher.
4. **Test-vector runner in `commonTest`** so every target verifies the
   same fixtures the Rust daemon already passes.
5. **`expect`/`actual` for storage** — wire the Resolver pin store to
   iOS Keychain and Android EncryptedSharedPreferences.
6. **Public `BridgePeer` API** matching [`docs/MOBILE.md`](../../docs/MOBILE.md) §3.
