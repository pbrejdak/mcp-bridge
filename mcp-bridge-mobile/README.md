# `mcp-bridge-mobile/` — Bridge Peer SDK

Phone-side runtime for MCP Bridge. Specified in [`docs/MOBILE.md`](../docs/MOBILE.md);
build phase lives in [`docs/ARCHITECTURE.md`](../docs/ARCHITECTURE.md) §11
(phase 4).

The Bridge Peer is what runs inside the host app on the user's phone.
It pairs with the desktop Resolver (the [`mcp-bridged`](../mcp-bridged)
daemon), seals/signs pair and announce payloads, and surfaces a
`BridgePeer` API to the host app.

## Layout

| Directory | Purpose | Status |
|---|---|---|
| [`core/`](core/) | KMP protocol core — `commonMain`/`androidMain`/`iosMain`/`jvmMain`. Wire protocol, crypto, state machines, pin storage. The single source of truth every packaging wraps. | **Scaffolded** — see [`core/README.md`](core/README.md) |
| _(coming)_ `capacitor/` | npm `@mcp-bridge/mobile` — TypeScript surface over Capacitor bridge → KMP iOS / Android binaries. | Not started |
| _(coming)_ `react-native/` | npm `@mcp-bridge/react-native` — TurboModule + hooks. | Not started |
| _(coming)_ `flutter/` | pub.dev `mcp_bridge_mobile` federated plugin. | Not started |
| _(coming)_ `swift-wrapper/` | CocoaPods / SPM `MCPBridgeMobile` Swift surface over the xcframework. | Not started |

The architectural decision behind picking KMP as the protocol-core
language is [`docs/decisions/0003-kmp-for-mobile-core.md`](../docs/decisions/0003-kmp-for-mobile-core.md):
one protocol implementation, multiple packagings, no per-language
crypto drift.

## Order of work

1. **`core/` first.** Every packaging wraps it. Implementing crypto +
   wire protocols here gates everything else.
2. **One packaging at a time** — likely `capacitor/` first since
   BodyLog (the canonical Origin host app) is Capacitor-based.
3. Other packagings come as host apps adopt the SDK.

## Toolchain

- **JDK 17+** — `gradle.properties` pins `jvmToolchain(17)`.
- **Gradle wrapper** — checked in at `core/gradlew`; bootstraps the
  pinned distribution on first run.
- **Android SDK** — required for the Android target. Each developer
  writes their local SDK path in `core/local.properties` (gitignored).
  Min API 26, compile API 34. See [`docs/MOBILE.md`](../docs/MOBILE.md) §7.1.
- **Xcode 14+** — required for the iOS targets (Kotlin/Native uses the
  installed clang for final linking). The first iOS build downloads
  ~700 MB of Kotlin/Native dependencies into `~/.konan/`.
