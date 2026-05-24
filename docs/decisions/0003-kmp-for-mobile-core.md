# 0003 — Kotlin Multiplatform for the mobile SDK core

- **Status**: Accepted
- **Date**: 2026-05-23
- **Deciders**: Project founders
- **Supersedes**: —
- **Related**: [`MOBILE.md`](../MOBILE.md), [`SPEC.md`](../SPEC.md)

## Context

The [Bridge Peer](../GLOSSARY.md#bridge-peer) SDK runs inside host apps on the user's phone. It must:

- Speak the [`mcp-pair/v0.1`](../SPEC.md#4-mcp-pairv01--pairing-protocol) and [`mcp-announce/v0.1`](../SPEC.md#5-mcp-announcev01--identity-refresh-protocol) wire protocols byte-identically across every packaging.
- Implement signature canonicalization (RFC 8785), Ed25519 signing, libsodium `crypto_box` sealing, and SAS derivation — all of which have to agree to the bit with the Resolver.
- Ship to host apps written in **Swift / SwiftUI**, **Kotlin / Compose**, **Kotlin Multiplatform**, **React Native**, **Flutter**, and **Capacitor / web** (the matrix in [`MOBILE.md`](../MOBILE.md) §2).

The naive approach is three separate native cores: Swift on iOS, Kotlin on Android, TypeScript for the web/JS surface. That guarantees three reimplementations of the same wire protocol, three places where SAS derivation or signature canonicalization can subtly diverge, and three independent security-review burdens.

## Decision

Implement the protocol core **once**, in **Kotlin Multiplatform** (KMP), with `expect` / `actual` declarations for the platform-specific surfaces (camera capture, Keychain / Keystore access, Bonjour / NSD subscription, screenshot protection).

The KMP core compiles to:

- An **iOS framework** via Kotlin/Native, packaged as an `xcframework` and consumable from Swift via Objective-C interop.
- An **Android AAR** via the JVM target.
- A **JS bundle** via Kotlin/JS — used by the Capacitor and pure-JS packagings as the protocol layer; native paths remain canonical for any non-web stack.

Every packaging in [`MOBILE.md`](../MOBILE.md) §2 wraps this single core.

## Alternatives considered

- **Three separate cores (Swift / Kotlin / TS)** — see Context. Rejected on the grounds that wire-protocol drift between implementations is a security risk, not just a maintenance one. A signature canonicalization bug in one implementation does not surface as "tests fail" — it surfaces as "this host app silently can't pair on Tuesday."
- **C / Rust shared core via FFI** — the obvious cross-platform choice for pure protocol code. Rejected because:
  - We need camera, Keychain/Keystore, NSD/NSNetService, and lifecycle hooks. None of those is pleasant from a C/Rust core; we'd still need substantial per-platform glue.
  - Rust → Swift bindings via UniFFI are workable but immature for `async` patterns; the host-app integration story would be rougher than KMP-iOS via Objective-C interop.
  - The team's working Rust competence is concentrated in the daemon, not the mobile SDK.
- **TypeScript-everywhere via React Native + bundled JS engine** — pushes one runtime to all platforms. Rejected because native Swift and native Kotlin host apps are first-class targets, and forcing those to embed a JS engine for the protocol core is the wrong default.

## Consequences

What this enables:

- **One implementation of the protocol** — one set of unit tests, one place to fix a wire-protocol bug, one place to apply [SPEC](../SPEC.md) updates. The [conformance test vectors](../GLOSSARY.md#conformance-test-vectors) prove the core agrees with itself across targets and with the Resolver.
- **Native-quality host integration** — Swift sees an Objective-C-flavored Swift API; Kotlin sees a Kotlin API; TS sees a TS API. None of these have to embed a foreign runtime.
- **Conformance drift between packagings is structurally hard to introduce** — every packaging compiles from the same source.

Costs we accept:

- **Build-pipeline complexity we own**: producing the iOS xcframework requires a Gradle build with the Kotlin/Native toolchain. Swift consumers don't see this — they get a prebuilt artifact via CocoaPods/SPM — but the pipeline that *produces* the artifact has more moving parts than a pure-Swift build.
- **iOS debugging is rougher inside the SDK internals**: stepping into protocol-core code crosses the Kotlin/Native boundary; tooling is improving but not as smooth as pure-Swift. The SDK's *public API* presents as idiomatic Swift, so this only affects SDK contributors, not host-app authors.
- **KMP-iOS interop constraints** that shape the public API: no nested generics across the boundary; `suspend` surfaces as Swift `async`; `Flow<T>` surfaces as a custom hot-stream type. The API is designed within these constraints.
- **One more toolchain in CI**: Kotlin, Gradle, Kotlin/Native, plus the JS/TS toolchain for the web packagings. Build time grows.

What would force a revisit:

- If Kotlin/Native's iOS interop story significantly regresses (or KMP itself becomes unmaintained).
- If a non-protocol responsibility migrates into the SDK in a way that makes the iOS-Kotlin/Native overhead disproportionate.
