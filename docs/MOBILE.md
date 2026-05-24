# Mobile SDK — `@mcp-bridge/mobile`

Status: exploratory, current revision 2026-05-23. Companion to [`ARCHITECTURE.md`](ARCHITECTURE.md) (cross-cutting design), [`DAEMON.md`](DAEMON.md) (native daemon), and [`UI.md`](UI.md) (Bridge Console). This document covers the **Origin-side** — everything that runs on the phone.

The Resolver from [`ARCHITECTURE.md`](ARCHITECTURE.md) §3 is one half of the system. The other half is the mobile MCP server inside a host app, paired with the **Bridge Peer SDK** for the pairing and announce machinery. The Bridge Peer is what this document specifies.

---

## 1. Position and scope

`@mcp-bridge/mobile` provides the phone-side runtime that host apps (BodyLog, future apps) integrate to expose their on-device MCP server to a paired Resolver. The SDK owns:

- QR scanning and parsing of Resolver invites.
- Cryptographic operations: Origin keypair generation, payload sealing, signing.
- Pairing protocol implementation (`mcp-pair/v0.1`).
- Announce lifecycle (`mcp-announce/v0.1`) — Bonjour + HTTP POST, including the seq counter and freshness window.
- Per-Resolver pin storage (typically multiple — phone can be paired with one or more laptops).
- Status events to the host app.

Out of scope for this document:

- The MCP server itself — host apps bring their own MCP implementation. The SDK doesn't host an MCP server.
- The Resolver's internals → [`DAEMON.md`](DAEMON.md).
- The wire protocols themselves → [`ARCHITECTURE.md`](ARCHITECTURE.md) §4.

---

## 2. Architecture: one Kotlin Multiplatform core, multiple packagings

Protocol logic, cryptography, payload sealing and signing, state machines, and per-Resolver pin storage live in a single **Kotlin Multiplatform** common module. Platform-specific bits (Bonjour / NSD subscription, Keychain / Keystore access, Camera capture, screenshot protection) live in `expect` / `actual` declarations for the iOS and Android targets.

This single core compiles to:

- An **iOS framework** via Kotlin/Native, packaged as an `xcframework` and consumable from Swift via Objective-C interop.
- An **Android AAR** via the JVM target.
- A **JS bundle** via Kotlin/JS — used by the Capacitor and pure-JS packagings as a thin transport layer; the host platform's native paths remain canonical for any non-web stack.

Six packagings wrap this core for different host-app stacks:

| Packaging | Wraps | Registry | Host apps that need it |
|---|---|---|---|
| **Native Kotlin** | KMP Android AAR | Maven Central — `dev.mcpbridge:mobile-android` | Android Kotlin / Compose |
| **Native Swift** | KMP iOS xcframework | CocoaPods + SPM — `MCPBridgeMobile` | iOS Swift / SwiftUI |
| **Kotlin Multiplatform** | KMP common module directly | Maven Central — `dev.mcpbridge:mobile-kmp` | KMP host apps that share Android + iOS code |
| **React Native** | TurboModule wrapping the KMP iOS framework and Android AAR | npm — `@mcp-bridge/react-native` | React Native apps (0.75+, New Architecture) |
| **Flutter** | Federated plugin wrapping the KMP iOS framework and Android AAR | pub.dev — `mcp_bridge_mobile` | Flutter apps (3.16+) |
| **Capacitor / web JS** | TypeScript API over Capacitor bridge → KMP iOS / Android targets | npm — `@mcp-bridge/mobile` | Ionic, Capacitor, web-hybrid apps (BodyLog) |

Cross-implementation conformance is enforced by JSON test vectors in `test-vectors/` ([`ARCHITECTURE.md`](ARCHITECTURE.md) §11, [`CONTRIBUTING.md`](CONTRIBUTING.md) wire-protocol section). Any packaging must pass the same fixtures or it is not conformant — but with one KMP core driving all of them, conformance drift between packagings is structurally hard to introduce.

### 2.1 Why KMP for the core

Three separate native cores (Swift, Kotlin/Android, JS) would mean three reimplementations of the same wire protocol, three places where the SAS derivation or signature canonicalization could subtly diverge, and three security-review burdens. KMP collapses this to one implementation while still emitting platform-native binaries.

Honest trade-offs:

- **iOS distribution**: the framework is built by Kotlin/Native, not Swift. Distributing the xcframework via CocoaPods and SPM is well-supported, but Swift-side debugging of the SDK's *internals* is rougher than pure-Swift would be. The SDK's *public API* presents as idiomatic Swift; only stepping into the protocol guts surfaces the Kotlin/Native origin.
- **Interop constraints**: KMP-iOS has rules — no nested generics across the boundary; `suspend` functions surface as Swift `async`; `Flow<T>` surfaces as a custom hot-stream type. The SDK's public API is designed within these constraints (no exotic shapes that hurt at the Swift boundary).
- **Build complexity**: KMP requires a Gradle build to produce the iOS framework. Swift consumers don't see this — they get a prebuilt xcframework via CocoaPods / SPM. The build pipeline that *produces* the xcframework has more moving parts than a pure-Swift one.

The benefit is one well-tested protocol core, one set of unit tests for the protocol logic, and one place to fix a wire-protocol bug across all platforms. The cost is build-pipeline complexity we own, not host-app complexity.

---

## 3. Bridge Peer SDK — surface

The TypeScript-flavored surface (Capacitor / JS). Native surfaces mirror it with platform-idiomatic naming.

```ts
import { BridgePeer, type ResolverInvite, type OriginConfig } from "@mcp-bridge/mobile";

// One-time SDK setup, called early in the host app's bootstrap
await BridgePeer.init({
  origin: {
    name: "BodyLog",                                  // shown to the user during pair
    logicalId: "bodylog-7f3a-...",                    // stable across IP / port changes
    scope: ["tools", "resources"],
    server: {
      url: "https://127.0.0.1:54321/",                // host app's MCP server endpoint
      certFingerprint: "sha256:...",                  // pinned for cert rotation
      caPem: "-----BEGIN CERTIFICATE-----\n...",      // self-signed CA delivered to Resolver
    },
    authProvider: async () => ({                      // host app provides current bearer
      type: "bearer",
      value: await keychainGet("mcp-bearer"),
    }),
  },
});

// Pairing — host app's "Connect to computer" button calls this
const invite: ResolverInvite = await BridgePeer.scanResolverInvite();
// User sees the SAS in their Bridge Console window;
// the host app shows the same SAS for cross-screen verification
const confirmed = await hostAppShowSasConfirmation({
  resolverDisplayName: invite.displayName,
  sas: invite.sas,
});

if (confirmed) {
  await BridgePeer.pair(invite);
}

// Announce lifecycle — typically called from the app's foreground transitions
BridgePeer.startAutomaticAnnounce();   // begins 30s heartbeat + change-detection
BridgePeer.announceNow();              // force an announce, e.g. after a Wi-Fi change
BridgePeer.stopAutomaticAnnounce();    // when entering background, to save battery

// Multi-Resolver management
const pairings = await BridgePeer.listPairings();
// → [{ resolverPubkey, resolverDisplayName, pairedAt, lastSeenAt }, ...]
await BridgePeer.unpair({ resolverPubkey });

// Status stream
BridgePeer.onStatus((event) => {
  switch (event.type) {
    case "scanning":              /* camera open */ break;
    case "invite_received":       /* show SAS confirmation */ break;
    case "paired":                /* update host app UI */ break;
    case "announce_sent":         /* heartbeat */ break;
    case "announce_failed":       /* offline, will retry */ break;
    case "auth_rotation_requested": /* daemon asked for new bearer */ break;
    case "error":                 /* { code, message, recoverable } */ break;
  }
});

// Cleanup on app shutdown
await BridgePeer.dispose();
```

### 3.1 Method semantics

| Method | Semantics |
|---|---|
| `init(config)` | Idempotent. Loads or generates the Origin keypair in secure storage. Loads existing Resolver pins. **If the keypair is freshly generated but the host app's `lastKnownInstall` cookie indicates a prior install, emits `reset_after_uninstall` so the host can show "your pairings were lost on reinstall — re-pair to reconnect"** (audit M-C3). Returns when SDK is ready. |
| `scanResolverInvite(opts?)` | Opens the camera, scans until QR detected or timeout. **Default `timeoutMs: 60000`, configurable** (audit M-H2). Throws on cancel / timeout. |
| `pair(invite, opts?)` | Builds the `mcp-pair/v0.1` payload (sealed + signed), POSTs to `invite.lanAddr`, awaits response. **Default `timeoutMs: 15000`** (audit M-H2). **Requires `opts.userConfirmedSas: true`** — the SDK refuses to send the payload unless the host app explicitly attests that the user confirmed the SAS phrase against Bridge Console (audit M-M2 — SAS confirmation is API contract, not just design guidance). |
| `getPendingPair()` | **Returns the in-progress invite (if any) plus its remaining lifetime, or `null`** (audit M-H1). Lets the host resume a pair flow after a crash or backgrounding. |
| `cancelPendingPair()` | Drops any pending invite. Idempotent. |
| `startAutomaticAnnounce()` | Starts a foreground announce loop. **30s cadence + immediate on network change via `NWPathMonitor` (iOS) / `ConnectivityManager.NetworkCallback` (Android), surfaced through KMP `expect`/`actual`** (audit M-H4). Auto-stops when the app moves to background (audit M-H3); resumes on foreground. Idempotent. |
| `stopAutomaticAnnounce()` | Stops the loop. Pins stay; daemon marks them Unreachable until announce resumes. |
| `announceNow(opts?)` | Fires an announce immediately. Increments `seq`. **Default per-attempt `timeoutMs: 10000`** (audit M-H2). |
| `listPairings()` | Returns the current Resolver pins. |
| `unpair({ resolverPubkey, notifyResolver?: boolean = true })` | Removes the pin locally. **If `notifyResolver` is true (default) and the Resolver is currently reachable, sends a best-effort `mcp-announce` record with a `revoked: true` flag** so the daemon can clean up immediately instead of waiting for the user to notice an Unreachable pin (audit M-M4). |
| `onStatus(callback)` | Subscribes to lifecycle events. Returns an unsubscribe function. |
| `dispose()` | Stops announce loop, closes camera, releases native handles. The Origin keypair stays in secure storage. |

### 3.2 Status event taxonomy

```ts
type StatusEvent =
  | { type: "idle" }
  | { type: "scanning" }
  | { type: "invite_received"; resolverPubkey: string; displayName: string; sas: string }
  | { type: "awaiting_user_confirmation"; sas: string }
  | { type: "pairing" }
  | { type: "paired"; resolverPubkey: string }
  | { type: "announce_started" }
  | { type: "announce_sent"; resolverPubkey: string; seq: number }
  | { type: "announce_failed"; resolverPubkey: string; reason: "no_network" | "no_resolver" | "sig_error" | "timeout" }
  | { type: "auth_rotation_requested"; resolverPubkey: string }
  | { type: "unpaired"; resolverPubkey: string }
  | { type: "reset_after_uninstall"; priorPinCount: number }   // emitted by init() when keychain is empty
                                                                 // but the host app's persisted state says
                                                                 // there were prior pins
  | { type: "error"; code: string; message: string; recoverable: boolean };
```

Host apps subscribe selectively. The SDK does not render UI itself — events drive the host app's own UI.

### 3.3 Native Swift surface

The Swift API mirrors the TypeScript shape, with Swift-idiomatic naming and types:

```swift
import MCPBridgeMobile

try await BridgePeer.shared.initialize(
    OriginConfig(
        name: "BodyLog",
        logicalId: "bodylog-7f3a-...",
        scope: [.tools, .resources],
        server: ServerConfig(/* ... */),
        authProvider: { try await Keychain.get("mcp-bearer") }
    )
)

let invite = try await BridgePeer.shared.scanResolverInvite()
// Show SAS confirmation in the host's SwiftUI / UIKit code, then:
try await BridgePeer.shared.pair(invite)

let stream = BridgePeer.shared.statusStream  // AsyncSequence<StatusEvent>
for await event in stream {
    switch event {
    case .paired(let pubkey): /* ... */
    case .announceSent(let pubkey, let seq): /* ... */
    case .error(let code, let message, _): /* ... */
    default: break
    }
}
```

`StatusEvent` is a Swift enum with associated values, generated from the Kotlin sealed-class equivalent by Kotlin/Native's Objective-C interop.

### 3.4 Native Kotlin surface

```kotlin
import dev.mcpbridge.mobile.BridgePeer
import dev.mcpbridge.mobile.OriginConfig

BridgePeer.init(OriginConfig(
    name = "BodyLog",
    logicalId = "bodylog-7f3a-...",
    scope = listOf("tools", "resources"),
    server = ServerConfig(/* ... */),
    authProvider = { keychainGet("mcp-bearer") },
))

val invite = BridgePeer.scanResolverInvite()
// Show SAS confirmation in the host's Compose / View code, then:
BridgePeer.pair(invite)

BridgePeer.statusFlow().collect { event ->
    when (event) {
        is StatusEvent.Paired -> /* ... */
        is StatusEvent.AnnounceSent -> /* ... */
        is StatusEvent.Error -> /* ... */
        else -> {}
    }
}
```

`statusFlow()` returns a `Flow<StatusEvent>` — the canonical Kotlin reactive shape. On Android-only consumers this is `kotlinx.coroutines.flow.Flow`; in KMP common code the same `Flow` is available across targets.

### 3.5 React Native bindings

The React Native packaging wraps the same KMP-built native binaries via a **TurboModule** (the New Architecture native-module spec, stable since RN 0.75). It is not the pure-JS path — it is a proper native module with codegen-driven type-safe bridging.

**Imperative API** mirrors the Capacitor surface:

```ts
import { BridgePeer } from "@mcp-bridge/react-native";

await BridgePeer.init({ origin: { /* ... */ } });
const invite = await BridgePeer.scanResolverInvite();
await BridgePeer.pair(invite);
```

**React-idiomatic hooks** for the common patterns:

```tsx
import {
  useBridgePeer,
  useResolverPairings,
  useBridgeStatus,
} from "@mcp-bridge/react-native";

function ConnectScreen() {
  const { scan, pair, unpair } = useBridgePeer();
  const pairings = useResolverPairings();
  const status = useBridgeStatus();

  const onConnect = async () => {
    const invite = await scan();
    // SDK suspends here until the host shows SAS confirmation and the user taps Confirm
    await pair(invite);
  };

  return (
    <View>
      {pairings.map((p) => (
        <PairingRow
          key={p.resolverPubkey}
          name={p.resolverDisplayName}
          lastSeen={p.lastSeenAt}
          onRemove={() => unpair(p.resolverPubkey)}
        />
      ))}
      <Button title="Connect to computer" onPress={onConnect} />
      {status.type === "announce_failed" && (
        <Text style={{ color: "amber" }}>Reconnecting…</Text>
      )}
    </View>
  );
}
```

The hooks subscribe to the SDK's native event emitter under the hood and rerender on transitions. The exported `BridgePeer` namespace shape is identical to the Capacitor SDK so the conceptual model (and the documentation) is shared.

**Minimum versions and configuration**:

- React Native 0.75 — TurboModule stable.
- New Architecture enabled — `newArchEnabled=true` in `android/gradle.properties`, `RCT_NEW_ARCH_ENABLED=1` in iOS `Podfile.properties.json`. Old-architecture support is not in v1.
- iOS deployment target ≥ 14.0 (for KMP/Native xcframework compatibility).
- Android `minSdkVersion` ≥ 26 (Android 8.0).

**Codegen**: TurboModule type definitions live in `src/specs/MCPBridgeMobile.ts` and are codegen'd via React Native's pipeline at host-app build time. Host apps do not write codegen config — the npm package handles it.

**Permissions**: declared in the host app's `Info.plist` and `AndroidManifest.xml` per §6.1 and §7.1. The npm package provides example snippets in its README; it cannot inject manifest entries from a library.

### 3.6 Kotlin Multiplatform host apps

Host apps that are themselves built with KMP get the cleanest possible integration: they depend on the SDK's `commonMain` module directly and access the Bridge Peer from shared code.

```kotlin
// commonMain/src/.../ConnectViewModel.kt
import dev.mcpbridge.mobile.BridgePeer
import dev.mcpbridge.mobile.OriginConfig
import kotlinx.coroutines.flow.collect

class ConnectViewModel {
    suspend fun startPairing() {
        BridgePeer.init(OriginConfig(
            name = "BodyLog",
            logicalId = "bodylog-7f3a-...",
            scope = listOf("tools", "resources"),
            server = ServerConfig(/* ... */),
            authProvider = { /* ... */ },
        ))

        val invite = BridgePeer.scanResolverInvite()
        // Show SAS confirmation in platform-specific UI, await user, then:
        BridgePeer.pair(invite)
    }

    val pairings = BridgePeer.pairingsFlow()  // Flow<List<ResolverPin>>
}
```

The same view-model code runs on Android (JVM target) and iOS (Kotlin/Native target), pulling in the SDK's `expect` / `actual` implementations transparently.

KMP host apps still write platform-specific UI (Compose for Android, SwiftUI for iOS), but the pairing flow, status subscription, and lifecycle management are shared.

**Dependency declaration** in `build.gradle.kts`:

```kotlin
kotlin {
    sourceSets {
        commonMain.dependencies {
            implementation("dev.mcpbridge:mobile-kmp:0.1.0")
        }
    }
}
```

Both Android and iOS targets in the host app's build will resolve the corresponding KMP artefact transitively.

### 3.7 Flutter bindings

The Flutter packaging is a **federated plugin** on pub.dev that bridges to the same KMP-built native binaries via Flutter platform channels. Federated structure lets the iOS and Android implementations evolve independently while keeping a single Dart surface for host apps.

```dart
import 'package:mcp_bridge_mobile/mcp_bridge_mobile.dart';

await BridgePeer.init(OriginConfig(
  name: 'BodyLog',
  logicalId: 'bodylog-7f3a-...',
  scope: [Scope.tools, Scope.resources],
  server: ServerConfig(/* ... */),
  authProvider: () async => await keychainGet('mcp-bearer'),
));

final invite = await BridgePeer.scanResolverInvite();
// Show SAS confirmation in Flutter widgets, await user, then:
await BridgePeer.pair(invite);

// Status stream — Dart Stream of sealed StatusEvent
BridgePeer.statusStream.listen((event) {
  switch (event) {
    case StatusPaired(:final resolverPubkey): /* ... */
    case StatusAnnounceSent(:final seq):       /* ... */
    case StatusAnnounceFailed(:final reason):  /* ... */
    case StatusError(:final code, :final message): /* ... */
    default: break;
  }
});
```

A Flutter widget for the typical paired-list pattern:

```dart
class ConnectedComputersList extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    return StreamBuilder<List<ResolverPin>>(
      stream: BridgePeer.pairingsStream,
      builder: (ctx, snap) {
        final pairings = snap.data ?? const [];
        return ListView(
          children: [
            ...pairings.map((p) => ListTile(
                  title: Text(p.resolverDisplayName),
                  subtitle: Text('Last active ${formatTimeAgo(p.lastSeenAt)}'),
                  trailing: IconButton(
                    icon: const Icon(Icons.link_off),
                    onPressed: () => BridgePeer.unpair(p.resolverPubkey),
                  ),
                )),
            ListTile(
              title: const Text('Connect to computer'),
              leading: const Icon(Icons.add_link),
              onTap: () async {
                final invite = await BridgePeer.scanResolverInvite();
                final confirmed = await showSasConfirmation(context, invite);
                if (confirmed) await BridgePeer.pair(invite);
              },
            ),
          ],
        );
      },
    );
  }
}
```

The SDK exposes plain Dart `Stream`s and `Future`s — no opinions on state management. Riverpod, Provider, Bloc, and GetX all work without adaptation; host apps pick their own.

**Federated plugin structure** (pub.dev convention for serious cross-platform plugins):

- `mcp_bridge_mobile` — front-end package host apps depend on. Defines the public Dart API and re-exports types.
- `mcp_bridge_mobile_platform_interface` — abstract platform-interface contract. Pins the API shape across iOS and Android implementations.
- `mcp_bridge_mobile_ios` — iOS implementation. Swift code that wraps the KMP iOS xcframework (same xcframework consumed by the Native Swift packaging — `MCPBridgeMobile.podspec` reused as a transitive Pod dependency).
- `mcp_bridge_mobile_android` — Android implementation. Kotlin code that uses the KMP Android AAR (same AAR consumed by the Native Kotlin packaging).

Host apps depend on `mcp_bridge_mobile`; the federated implementations are pulled in transitively per platform.

**Minimum versions**:

- Flutter 3.16+ (Dart 3.2+; the public API uses sealed classes and switch-pattern matching).
- iOS deployment target ≥ 14.0 (KMP xcframework requirement).
- Android `minSdkVersion` ≥ 26.

**Permissions**: declared in the host app's `Info.plist` and `AndroidManifest.xml` per §6.1 and §7.1, identical to the React Native and native packagings. The pub.dev README has copy-pasteable snippets. The SDK does not pull in the `permission_handler` package — it exposes a `requestCameraPermission()` method on `BridgePeer` so host apps can integrate with their existing permission flow (or wire it up to `permission_handler` themselves).

**Transport choice — platform channels, not FFI**: Flutter platform channels handle the SDK's surface comfortably — async method calls (pair, announce, scan) round-trip in single-digit milliseconds, and the status / pairings streams are far below the channel's throughput ceiling. Dart FFI would offer slightly lower latency but at the cost of building a C-symbol surface on top of Kotlin/Native's Obj-C interop layer, which is not worth the engineering. Platform channels are the right tool here.

---

## 4. Pairing flow on the phone side

The companion to [`ARCHITECTURE.md`](ARCHITECTURE.md) §5.1, from the phone's perspective:

```
Host app                Bridge Peer                Resolver
   │                         │                         │
   │ user taps               │                         │
   │ "Connect to computer"   │                         │
   ├───────────────────►│                         │
   │                         │ open camera             │
   │                         │ ...                     │
   │                         │ QR detected, parse      │
   │                         │ ✓ verify spec="mcp-pair/v0.1"
   │                         │ ✓ extract resolver:     │
   │                         │   {pubkey, lanAddr,     │
   │                         │    sas, displayName,    │
   │                         │    nonce}               │
   │ status: invite_received │                         │
   │◄─────────────────────────┤                         │
   │                         │                         │
   │ host shows:             │                         │
   │ "Pair BodyLog with      │                         │
   │  Patryk's MacBook Pro?  │                         │
   │  Verification:          │                         │
   │  tiger-river-marble-clay│                         │
   │  ◄ glance at Console"   │                         │
   │                         │                         │
   │ user confirms           │                         │
   ├───────────────────►│                         │
   │                         │                         │
   │                         │ load Origin keypair     │
   │                         │ build pair payload incl │
   │                         │   target_resolver_pubkey│
   │                         │ sign(origin.privkey,    │
   │                         │      canonical JSON)    │
   │                         │ seal(crypto_box,        │
   │                         │      resolver.pubkey)   │
   │                         │                         │
   │                         │ POST sealed body to     │
   │                         │ invite.lanAddr          │
   │                         ├──────────────────────►│
   │                         │                         │
   │                         │                         │ unseal,
   │                         │                         │ verify sig + nonce,
   │                         │                         │ pin origin.pubkey,
   │                         │                         │ confirm
   │                         │                         │
   │                         │◄──────────────────────┤
   │                         │ persist Resolver pin    │
   │                         │ (in iOS Keychain /      │
   │                         │  Android Keystore)      │
   │                         │                         │
   │ status: paired          │                         │
   │◄─────────────────────────┤                         │
   │                         │                         │
   │ host updates UI:        │                         │
   │ "Connected to           │                         │
   │  Patryk's MacBook Pro"  │                         │
```

User-visible actions on the phone: tap Connect, aim camera at QR, glance at SAS, tap Confirm. Four taps and one glance — matching the desktop side's count from [`ARCHITECTURE.md`](ARCHITECTURE.md) §5.1.

---

## 5. Announce lifecycle

Announces keep the Resolver pointed at the current Origin identity as it drifts (new Wi-Fi → new IP, token rotation, cert renewal). The daemon-side rules are in [`ARCHITECTURE.md`](ARCHITECTURE.md) §4.2. The phone-side responsibilities:

### 5.1 What triggers an announce

| Trigger | Action |
|---|---|
| `startAutomaticAnnounce()` called | Fire immediately, then schedule 30s heartbeat |
| Network reachability changed (Wi-Fi switch, cellular ↔ Wi-Fi) | Fire immediately |
| App moves to foreground | Fire immediately (covers "phone was asleep") |
| `auth_rotated_at` changes (host app reports new bearer) | Fire immediately with new `auth_rotated_at` |
| `cert_rotated_at` changes (host app reports new TLS cert) | Fire immediately with new `cert_rotated_at` |
| 30s timer elapsed | Fire heartbeat |
| `announceNow()` called explicitly | Fire immediately |

### 5.2 Per-Resolver state the SDK maintains

For each paired Resolver:

```ts
type ResolverPin = {
  resolverPubkey: string;
  resolverDisplayName: string;
  lastAnnounceSeq: number;        // strictly increasing per pin
  lastSuccessfulAnnounce: Date;
  lastKnownLanAddr?: string;      // cached from invite, refreshed if Resolver moves
  pairedAt: Date;
};
```

`lastAnnounceSeq` is the source of truth for `seq` — incremented before each announce, persisted across app restarts so the counter never regresses (daemon would reject a regression as replay).

### 5.3 Carrier selection

For each announce, the SDK tries in order:

1. **Bonjour TXT** — sealed body, broadcast on the per-Resolver service type (`_mcp-bridge-<hmac>._tcp.local`). Fastest, lowest battery cost.
2. **HTTP POST** to `lastKnownLanAddr` — fallback when Bonjour multicast is suppressed (corporate Wi-Fi, hotel guest network).

If both fail for a paired Resolver three times in a row, the SDK emits `announce_failed` with reason `no_resolver` and falls back to a 5-minute retry interval until the next network event.

---

## 6. iOS implementation

The iOS code paths described here are implemented in Kotlin/Native (§2) and surfaced to Swift via Objective-C interop. From the host app's point of view the behavior is identical to a pure-Swift SDK — only the SDK's *internals* live in Kotlin. Storage attributes, entitlement names, and permission strings below are exactly what the host app declares in its own `Info.plist` and entitlements.

### 6.1 Permissions and entitlements

| Item | Value | Triggered when |
|---|---|---|
| `NSLocalNetworkUsageDescription` (Info.plist) | "Discover paired computers on your Wi-Fi to send tool calls" | first Bonjour use |
| `NSBonjourServices` (Info.plist) | `_mcp-bridge-*._tcp` | enumerated by the SDK |
| `com.apple.developer.networking.multicast` (entitlement) | required on iOS 17+ for mDNS | implicit |
| `NSCameraUsageDescription` (Info.plist) | "Scan the verification code shown by MCP Bridge on your computer" | first `scanResolverInvite()` |

The host app must include these in its own `Info.plist` and entitlements. The SDK documents the strings to use but cannot inject entitlements from a library.

**Pre-flight UX**: the host app should display its own "we are about to ask for Wi-Fi permission" sheet before the OS prompt fires. The OS prompt copy is fixed; the explanation must happen in advance. Mirrors the macOS pre-flight pattern in [`UX.md`](UX.md) §13.2.

### 6.2 Background hosting reality

iOS does not let an app run a long-lived MCP server in the background. The truthful design:

- **Foreground**: the host app's MCP server runs and the Bridge Peer announces normally.
- **Backgrounded with `audio` / `location` entitlement**: technically possible but inappropriate for most apps and rejected by App Review without a real justification.
- **Suspended (the common case)**: the MCP server is unreachable. The daemon-side Loopback Listener returns 503 with `X-MCP-Bridge-Reason: origin-unreachable` and the Bridge Console shows the server as Offline ([`UX.md`](UX.md) §12).
- **Wake-up via push**: if the host app receives a silent push (`content-available: 1`), it briefly enters background and can serve a single MCP request before suspension. This requires the AI app (Claude Desktop, Cursor) to wake the daemon, which wakes the phone — an architecture the v1 spec does not implement.

For v1, the truthful contract: **the phone must be foregrounded on the host app for the MCP server to be reachable**. The Console UX is honest about this: Offline → "Open BodyLog on your phone to reconnect."

The Bridge Peer SDK observes `UIApplication.willResignActive` and stops announcing; `UIApplication.didBecomeActive` triggers an immediate announce.

### 6.3 Storage

| Item | Location | Attributes |
|---|---|---|
| Origin private key | iOS Keychain | `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; not synced to iCloud Keychain |
| Origin public key | iOS Keychain | same |
| Pinned Resolver pubkeys + display names | iOS Keychain | same |
| `lastAnnounceSeq` per pin | App Group container or local-only `UserDefaults` | `NSFileProtectionCompleteUntilFirstUserAuthentication` |
| Last known LAN address per Resolver | local-only `UserDefaults` | **excluded from iCloud Backup via `NSURLIsExcludedFromBackupKey` on the containing plist; while individual LAN IPs are low-sensitivity, they can identify the user's home/work network and should not migrate via backup** (audit M-H5) |

`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` is the right access class: data is decryptable after first unlock (so announce can run in foreground without user interaction) but does not migrate to a new device via iCloud Backup. This is intentional — Resolver pins should not survive a phone migration; the user re-pairs on the new device.

### 6.4 Camera and QR scanning

Implemented via `AVFoundation`:

- `AVCaptureSession` with a `AVCaptureMetadataOutput` filtered for `.qr`.
- Camera permission requested via `AVCaptureDevice.requestAccess(for: .video)`.
- Auto-focus + auto-exposure for low-light reliability.
- Returns the QR's payload as raw bytes; the SDK parses as JSON and validates against the `mcp-pair/v0.1` invite schema.
- Timeout default 60s; cancellable via host-app gesture.

### 6.5 Universal Links and AirDrop

The SDK does not register Universal Links itself; the host app does. For receiving a `mcp-pair://` deeplink (e.g., from the smart landing page on the user's Mac), the host app registers the scheme and forwards the URL to `BridgePeer.handleDeepLink(url)`.

For AirDrop of the install URL **to** the Mac, no SDK involvement: the host app uses `UIActivityViewController` with the URL produced by the SDK's invite generator (for the inverse Direction A flow), which is a fallback path.

### 6.6 Backgrounding behavior — screenshots

When the host app moves to the background, iOS captures a thumbnail for the app switcher. If a pairing-flow screen with the SAS is visible at the moment, the SAS gets snapshotted to disk in the app's container.

The SDK provides `BridgePeer.uiSensitivityHint` to flag UI states; host apps using it should overlay a blur or solid color when `applicationWillResignActive` fires during sensitive screens. Concretely: the SDK emits `awaiting_user_confirmation` and the host app's responsibility is to set the privacy overlay.

---

## 7. Android implementation

The Android code paths described here are the JVM target of the KMP core (§2). For Native Kotlin and KMP host-app consumers this is the SDK directly; for React Native and Capacitor consumers the same JVM target is wrapped by their respective bridges. Permissions, manifest entries, and storage decisions below are exactly what the host app declares.

### 7.1 Permissions

| Permission | Manifest | Runtime prompt |
|---|---|---|
| `android.permission.INTERNET` | `<uses-permission>` | no |
| `android.permission.ACCESS_NETWORK_STATE` | `<uses-permission>` | no |
| `android.permission.CAMERA` | `<uses-permission>` | yes, at scan time |
| `android.permission.CHANGE_WIFI_MULTICAST_STATE` | `<uses-permission>` | no |
| `android.permission.FOREGROUND_SERVICE` (API 28+) | `<uses-permission>` | no |
| `android.permission.FOREGROUND_SERVICE_DATA_SYNC` (API 34+) | `<uses-permission>` | no |
| `android.permission.POST_NOTIFICATIONS` (API 33+) | `<uses-permission>` | yes, at first FG service start |

The Bridge Peer SDK is a library; the host app is responsible for declaring permissions in its manifest. The SDK provides a Kotlin helper to request runtime permissions in the right order.

### 7.2 Background hosting

Android is more permissive than iOS but the user-visible cost is higher:

- **Foreground service** with a persistent notification is the supported way to keep the MCP server alive in the background. API 34+ requires the `FOREGROUND_SERVICE_DATA_SYNC` type (or `connectedDevice` once Android exposes it; currently `dataSync` is the closest match).
- **Without foreground service**: the host app is subject to Doze and App Standby; the MCP server is reachable only when the host app is in foreground or briefly woken (e.g., via FCM high-priority push).
- **Manufacturer quirks**: Samsung, Xiaomi, Oppo, OnePlus, Huawei all override Android's background policies. Apps frequently need to instruct users to disable battery optimization for the host app. [dontkillmyapp.com](https://dontkillmyapp.com) is the unofficial reference.

The SDK exposes `BridgePeer.startForegroundService(notificationConfig)` and `stopForegroundService()` so the host app controls the policy. The persistent notification is good UX, not bad UX — it makes the "your phone is hosting an MCP server right now" transparent to the user.

### 7.3 Storage

| Item | Location | Attributes |
|---|---|---|
| Origin private key | Android Keystore | `KeyProperties.PURPOSE_SIGN`; `setUserAuthenticationRequired(false)` so announce works without per-operation biometric prompt; hardware-backed where available |
| Origin public key | Android Keystore | same |
| Pinned Resolver pubkeys + display names | `EncryptedSharedPreferences` | AES256_GCM |
| `lastAnnounceSeq` per pin | `EncryptedSharedPreferences` | same |
| Last known LAN address | `SharedPreferences` with `android:allowBackup="false"` and `BackupAgent`-level exclusion | **excluded from Google Backup; LAN IPs can identify the user's home/work network and should not migrate via backup** (audit M-H5) |

`EncryptedSharedPreferences` uses a master key stored in the Android Keystore, providing the same effective protection as iOS Keychain for the per-pin state.

### 7.4 NSD (Bonjour equivalent)

Android's `NsdManager` exposes Bonjour-style discovery. Quality varies by manufacturer; the SDK uses a more robust path on devices where `NsdManager` has known issues:

- Default: `NsdManager` for service discovery, with timeouts ~3x iOS values to accommodate slower delivery.
- Fallback: direct multicast DNS via `jmDNS` (a Java mDNS library) for devices where `NsdManager` is broken.
- Last fallback: HTTP POST to `lastKnownLanAddr`.

### 7.5 Camera and QR

`CameraX` is the modern API. The SDK uses `MlKit`'s barcode scanner for QR decoding — fast, on-device, no network.

Camera permission requested via `ActivityCompat.requestPermissions` from the host activity; the SDK provides a `requestCameraPermission()` helper that handles the result callback.

### 7.6 App Links

The host app declares `android:autoVerify="true"` for the `mcp-pair://` and `https://mcpbridge.me/p/...` patterns in its manifest. The SDK provides `handleAppLink(intent)` for the host's deep-link receiver.

`assetlinks.json` published at `https://<host-app-domain>/.well-known/assetlinks.json` is the host app's responsibility (it's per host-app domain, not per Bridge).

### 7.7 Screenshot prevention

`FLAG_SECURE` set on activities showing the SAS prevents the screenshot in the app switcher and prevents screen recording during the sensitive moment. The SDK exposes a helper to apply/unapply `FLAG_SECURE` keyed to its `awaiting_user_confirmation` event.

---

## 8. Mobile-side privacy and security

This section is the phone-side counterpart to [`PRIVACY.md`](PRIVACY.md). The threat model differs from desktop because:

- We have far less memory control (no `Zeroize` equivalent; JVM and Swift ARC manage memory).
- We rely heavily on OS-provided secret storage (Keychain, Keystore) — which is best-in-class on mobile.
- The OS mediates app sandboxing more aggressively than desktop — more trust we can lean on, less per-app discipline required.

### 8.1 Secrets and key storage

| Secret | Storage | Lifetime |
|---|---|---|
| Origin private key (Ed25519) | iOS Keychain / Android Keystore, hardware-backed where available | until user uninstalls host app, or `BridgePeer.rotateOriginIdentity()` called |
| Origin bearer token (per-Resolver session) | host app's responsibility — typically Keychain / EncryptedSharedPreferences | host-app defined; rotated per session ideally |
| Pinned Resolver pubkeys | iOS Keychain / Android Keystore | until `unpair()` or uninstall |
| TLS cert + private key for the host's MCP server | host app's responsibility — typically Keychain / Keystore | host-app defined |

The SDK never holds the Origin private key in process memory longer than the duration of a sign operation. Native sign operations on iOS Keychain (`SecKeyCreateSignature`) and Android Keystore (`Signature.sign`) keep the key inside the secure enclave / hardware-backed keystore; the key never leaves the secure boundary.

### 8.2 Memory hygiene

Mobile platforms do not expose anything like Rust's `zeroize`. Mitigations:

- **iOS**: use `Data` types and rely on ARC. For especially sensitive byte buffers (the SAS-confirmation phrase, the bearer token in transit), use `UnsafeMutablePointer` patterns with `memset_s` to zero on dispose. The SDK does this for the bearer token and unwrapped pair payloads.
- **Android**: use `CharArray` over `String` for sensitive data where possible (Strings are interned and survive longer); explicitly `Arrays.fill(buf, 0.toChar())` on dispose. The SDK applies this to bearer tokens and unwrapped pair payloads.

We do not promise zero residue — JVMs and ARC make that impossible without OS support. We promise minimum dwell time in memory and use the OS's secure-storage facilities for everything that has a longer-than-instant lifetime.

### 8.3 Backgrounding and screenshots

- iOS: SDK fires `awaiting_user_confirmation` → host applies blur on `applicationWillResignActive`.
- Android: SDK fires `awaiting_user_confirmation` → host sets `FLAG_SECURE`.

The host app is responsible for actually applying these — the SDK can't reach the host's view hierarchy from a library.

### 8.4 App uninstall residue

- **iOS 11+**: app uninstall deletes the app's keychain entries (different from earlier iOS where keychain entries survived). Origin keys, pinned Resolver pubkeys, and per-pin state all go.
- **Android**: app uninstall deletes the app's data including `EncryptedSharedPreferences`. Keystore entries scoped to the app's UID are also removed. Origin keys go.

Net effect: uninstalling the host app cleanly removes all SDK state. No further user action required.

### 8.5 Backups

- **iOS**: `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` attribute means Origin keys are not backed up to iCloud and do not migrate to a new device. The user re-pairs on a new phone.
- **Android**: `android:allowBackup` is set to `false` for the SDK's data (the host app should match in its manifest if it cares). Origin keys in Keystore are not exportable regardless of backup settings.

### 8.6 Origin identity rotation

`BridgePeer.rotateOriginIdentity()` exists for the case where the user thinks their phone has been compromised. It:

1. Generates a new Origin keypair.
2. Invalidates all existing Resolver pins (the new pubkey won't match any existing pin).
3. Notifies the host app via a `rotated` status event.

The user must re-pair every Resolver after rotation. This is intentional friction — rotation is a serious action.

### 8.7 Mobile-side egress allowlist

Parallel to [`ARCHITECTURE.md`](ARCHITECTURE.md) §6.1 and [`PRIVACY.md`](PRIVACY.md) §4 (audit M-C1). The SDK's complete outbound network behaviour:

**Listens on (host-app process)**:
- Whatever port the host's MCP server is bound to — typically `https://127.0.0.1:<port>/` on the loopback interface, not on the LAN interface.

**Connects outbound to**:
- Paired Resolver LAN addresses — `invite.lanAddr` for the pair POST; cached `lastKnownLanAddr` for HTTP-fallback announces.

**Multicast**:
- `224.0.0.251:5353` — Bonjour TXT broadcast for the sealed-body announce records.

That is the complete list. **The SDK never connects to the internet.** No analytics endpoints, no crash reporting, no update channel of its own. Updates ship with host-app releases through the host's normal store update mechanism (audit M-M7).

### 8.8 Host MCP server isolation from sibling apps

The host app's MCP server binds to `https://127.0.0.1:<port>/`. The OS routes loopback only within the same process boundary in most cases, but defense-in-depth matters (audit M-C2):

- **iOS**: loopback connections from other apps in the same App Group are technically possible if both apps share the keychain access group. **Do not place the MCP server's TLS key inside an App Group keychain.** Use the default app keychain (no group) so the key is unreachable to sibling apps.
- **Android**: loopback connections from other apps are not possible unless those apps run as the same UID (sharedUserId — deprecated and rare). Standard isolation is sufficient.
- **Cert pinning + bearer token** provide defense-in-depth on both platforms: even if a sibling app reached the loopback port, it would not have the bearer token (held in the host app's keychain entry, which is per-app) and would fail TLS verification against the daemon's pinned cert.

The SDK's pair-flow surface includes a `serverIsolation` config option that, when set to `strict` (default), refuses to start if it detects an App Group keychain attribute on the TLS key.

### 8.9 Threat model deltas vs desktop

Adversaries we resist on mobile that are not present on desktop:

| Adversary | Mitigation |
|---|---|
| Host-app process memory dumped via debugger / iOS sysdiagnose | Native secure storage (Keychain / Keystore); sign operations never expose private keys |
| Screenshot in app switcher leaking SAS | Blur / `FLAG_SECURE` on confirmation screens |
| App switcher snapshot uploaded to iCloud Backup | Backup attributes that exclude sensitive items |
| Same-device sibling apps (sandboxed but on same device) | OS sandbox isolation; we don't expose IPC outside the host app's process |
| `adb` debugging on Android (developer mode left on) | Hardware-backed Keystore prevents key extraction; `EncryptedSharedPreferences` mitigates non-Keystore state |

Adversaries we resist on desktop that the mobile side does **not** face directly:

- DNS rebinding from browser tabs — there is no Loopback Listener on the phone exposed to other local processes.
- Other local processes reading the daemon's loopback — same reason.
- Webview telemetry — no webview involved in the SDK.

---

## 9. Host-app integration contract

What the host app must provide:

| Item | Type | Notes |
|---|---|---|
| Display name | `string` | shown to the user during pair (e.g., "BodyLog") |
| Logical ID | `string` | stable across IP/port/cert changes; UUID recommended; never re-generate |
| MCP server endpoint | `https://` URL | typically `https://127.0.0.1:<port>/` or LAN address |
| TLS cert + private key | DER / PEM | self-signed is fine; cert pinned by the Resolver via `fp` |
| Bearer auth provider | callable | called by SDK on each announce / rotation event |
| Scope declaration | `("tools" \| "resources" \| "prompts")[]` | tells the daemon what to advertise to Consumers |
| Optional: tool list | `string[]` | for `allowed_tools` ACL on the Resolver side |

What the host app must do at integration:

- Declare the iOS / Android permissions and entitlements (§6.1, §7.1) in its manifest. The SDK provides the strings; the host app declares them.
- Provide its own "Connect to computer" button UI; the SDK does not render UI.
- Implement the SAS confirmation screen (we recommend a layout — see §11) and call `pair(invite)` only after user confirmation.
- Manage the foreground-service (Android) or foreground-only (iOS) hosting lifecycle.
- Apply screenshot protection during sensitive screens (§8.3).
- Subscribe to `onStatus` events and update its UI accordingly.

What the host app **must not** do:

- Skip the SAS confirmation. The SAS is the single OOB ceremony that defeats QR-substitution attacks ([`ARCHITECTURE.md`](ARCHITECTURE.md) C-3). Auto-confirming defeats the entire trust model. **The SDK now enforces this at the API level — `pair(invite, { userConfirmedSas: true })` is required and the SDK refuses to send otherwise** (audit M-M2).
- Store the Origin private key anywhere outside the SDK-managed Keychain / Keystore.
- Reuse one Origin keypair across multiple users on the same host app — if the host has user accounts, generate one Origin keypair per user account.
- Expose the host's MCP server outside the loopback / LAN to the internet without explicit user consent.
- Place the host's MCP-server TLS key in an iOS App Group keychain (audit M-C2). Sibling apps in the same group would gain access.
- Configure a crash reporter (Crashlytics, Sentry, Bugsnag, Firebase) to capture full process memory without scrubbing. **Sensitive data the SDK touches** — unwrapped pair payloads, bearer tokens, Origin private-key handles — **can be in memory at crash time and would leak via the crash report**. Host apps should either disable memory snapshots for the SDK's threads or apply a beforeSend hook that strips fields matching `mcp_bridge` / `bridge_peer` / `origin_keypair` / `bearer` / `resolver_pubkey` (audit M-H8).

### 9.1 Multi-Origin per host app

For v1, **one Origin per host-app process** (audit M-H7). Host apps that need to expose multiple distinct MCP servers should:

- Run a single Origin with multiple `scope` entries and one logical MCP server that internally federates.
- Or call `BridgePeer.init()` with a different `logicalId` per Origin context (e.g., per user account in a multi-account app), one at a time — switching Origins requires `BridgePeer.dispose()` + re-init.

Multi-Origin in a single process (concurrent BridgePeer instances) is **not in v1**. The pair flow's camera state, foreground announce loop, and status stream all assume one Origin at a time.

---

## 10. Distribution and versioning

### 10.1 Packages

| Channel | Package | Format | Wraps |
|---|---|---|---|
| Maven Central | `dev.mcpbridge:mobile-android` | AAR | KMP Android target — direct consumption from Native Kotlin host apps |
| Maven Central | `dev.mcpbridge:mobile-kmp` | KMP multiplatform artefact | KMP common — for KMP host apps that share Android + iOS code |
| CocoaPods | `MCPBridgeMobile` | xcframework | KMP iOS target — direct consumption from Native Swift host apps |
| Swift Package Manager | `MCPBridgeMobile` | xcframework | same as CocoaPods |
| npm | `@mcp-bridge/react-native` | TurboModule + TS hooks | KMP iOS framework + Android AAR via React Native New Architecture |
| pub.dev | `mcp_bridge_mobile` (+ federated implementations) | Flutter plugin | KMP iOS xcframework + Android AAR via Flutter platform channels |
| npm | `@mcp-bridge/mobile` | Capacitor plugin + TS | KMP iOS framework + Android AAR via Capacitor bridge |
| Gradle Plugin Portal | none in v1 | — | — |

All artefacts are produced from a single Gradle build of the KMP source tree. Release tagging produces every package in one CI workflow run with matching SemVer.

### 10.2 Versioning

SDK SemVer tracks the **wire protocol** version it speaks:

- `0.1.x` series — implements `mcp-pair/v0.1` and `mcp-announce/v0.1`.
- `0.2.0` — implements `mcp-pair/v0.2` (if and when). Breaking on the wire side.
- Minor version bumps within `0.1.x` for non-protocol changes (bug fixes, new SDK methods, performance).

When the wire protocol changes incompatibly, the daemon-side and SDK must bump together. The conformance test vectors in `test-vectors/` are versioned with the protocol.

### 10.3 Compatibility matrix

Published per release in the SDK's README and `mcpbridge.me/mobile`:

```
SDK 0.1.x  ↔  mcp-bridged 0.1.x
SDK 0.2.x  ↔  mcp-bridged 0.2.x (forthcoming)
```

Pre-1.0 we will not maintain cross-version compatibility between SDK and daemon. Post-1.0 we will.

---

## 11. UI recommendations for host apps

The SDK ships without UI; host apps render their own. To keep the user experience consistent across host apps, we provide design guidance (not a component library). Three screens the host app must build:

### 11.1 The "Connect" button

A button or menu item labeled in the user's mental model — "Connect to computer", "Use BodyLog with AI apps", "Pair with computer." Avoid "Pair Origin with Resolver"; that's jargon.

### 11.2 The SAS confirmation screen

The most important UI surface. Wireframe:

```
┌──────────────────────────────────────────────┐
│                                              │
│  Connect to Patryk's MacBook Pro?            │
│                                              │
│  Make sure this verification phrase matches  │
│  what's shown on your computer:              │
│                                              │
│   ┌────────────────────────────────────┐   │
│   │  tiger · river · marble · clay     │   │
│   └────────────────────────────────────┘   │
│                                              │
│  If the phrases don't match, tap Cancel.     │
│                                              │
│            [  Cancel  ]   [  Connect  ]      │
│                                              │
└──────────────────────────────────────────────┘
```

This screen must:

- Show the SAS prominently — the entire screen's reason to exist.
- Show the Resolver's display name above the SAS so the user knows what they're connecting to.
- Use the host app's own typography and color, but keep the SAS in a clearly visible monospace block.
- Apply screenshot protection (§8.3) — the host app must not show this screen to the iOS app-switcher snapshot.
- Cancel must be at least as prominent as Connect — destructive caution is the right default for unfamiliar code.

### 11.3 The paired-Resolvers list

The host app's settings should list paired Resolvers so the user can audit and revoke from the phone side:

```
Connected computers

  Patryk's MacBook Pro          Last active 3 min ago
  Patryk's iMac                 Last active 2 h ago

  [ Add another computer ]
```

Each row tappable, revealing details and a "Disconnect" affordance that calls `unpair()`.

---

## 12. Testing strategy

Three layers, mirroring the daemon side ([`DAEMON.md`](DAEMON.md) §10):

1. **Unit tests** — per platform, run on CI matrix.
2. **Integration tests** — XCTest on iOS, JUnit/AndroidX Test on Android — exercise the SDK against a mock Resolver.
3. **Conformance tests** — the same JSON test vectors in `test-vectors/` that the daemon side uses. Every SDK release must pass.

End-to-end pairing tests are deferred to v0.3 — they need real devices on a real network, which is awkward for CI. We rely on the conformance tests + manual QA during release for v1.

---

## 13. What is not in v1

- **iOS push wake-up** for background MCP serving — requires the AI client to wake the daemon to wake the phone. Architecturally interesting; not v1.
- **.NET MAUI packaging** — planned for v0.2. NuGet wrapper over the KMP iOS xcframework + Android AAR via .NET for iOS / .NET for Android binding libraries. Largest deferred-platform audience.
- **Tauri Mobile packaging** — planned for v0.2 or v0.3. Rust crate over UniFFI bindings to the KMP-built native binaries; natural alignment with the Rust daemon.
- **NativeScript packaging** — on-demand; ship when a real user asks. Architecture is straightforward (npm package as NativeScript plugin wrapping the same native binaries as Capacitor and React Native).
- **Unity packaging** — interesting for AR / spatial / sensor-heavy non-game apps; defer to v0.3+ unless specific demand.
- **watchOS / Wear OS** — wearables as Origins is intriguing for health/fitness data but the SAS confirmation UX and constrained Bonjour stack make this awkward. Defer; revisit when desktop side has matured.
- **visionOS** — should fall out of the iOS xcframework automatically when KMP's `iosArm64` target adds a `visionOS` slice; no separate packaging needed. Confirm during the v0.2 build pipeline work.
- **Xamarin Classic** — deprecated; not supported. Migrate to MAUI.
- **Embedded (ESP32, Arduino, RTOS)** — different design space; out of scope for this SDK. Would require a stripped-trust-model "bridge-microcontroller" SDK as a separate project.
- **WatchOS / WearOS hosts** — phones only.
- **Cross-device origin migration** — the user re-pairs on a new phone; we don't help them migrate.
- **Multi-tenant host apps** — one Origin identity per host app. Host apps with multiple user accounts need to manage that themselves by calling `init()` with a different `logicalId` per user.
- **Origin discovery of new Resolvers** — phone does not actively look for new Resolvers to pair with. Pairing is always user-initiated via QR scan.
- **Mesh announce** — phone announces to each paired Resolver independently. We do not relay announces between Resolvers.
- **SDK-rendered UI** — the SDK provides logic; host apps provide UI. We may ship a Capacitor UI plugin later if there is demand.

---

## 14. Open mobile-side decisions

1. **Bonjour vs jmDNS fallback on Android.** `NsdManager` quality varies; do we ship jmDNS as a transitive dependency or skip Android Bonjour entirely and rely on HTTP POST?
2. **iOS multicast entitlement at scale.** Apple requires justification for the `com.apple.developer.networking.multicast` entitlement. Host apps will each need to request it; we should publish the canonical justification text.
3. **Camera permission ergonomics.** The first time `scanResolverInvite()` is called, the camera permission prompt fires. We could offer a "pre-warm" method so host apps trigger the prompt earlier in their UX. Probably yes; needs a name.
4. **`auth_rotation_requested` flow.** Currently the SDK emits the event; the host app fetches a new bearer and the SDK announces the rotation. Alternative: SDK calls `authProvider` directly. The current shape gives the host more visibility; not sure it matters.
5. **Capacitor plugin packaging.** One package for both iOS and Android, or split into platform packages? Industry trend is unified; we should follow unless there's a reason not to.
6. **Origin keypair per host-app user vs per host-app install.** Currently per install. If the host app has user accounts, the user-account boundary should probably gate the keypair too. Needs documentation guidance for host-app authors.
7. **React Native old-architecture support.** TurboModule-only means RN < 0.75 hosts can't use the SDK. We could ship a parallel old-NativeModule binding, but the engineering cost is real and the trend is decisively toward New Architecture. Recommend: no v1 support; revisit if a meaningful number of users are stuck on old RN.
8. **KMP iOS distribution: xcframework vs static framework vs both.** xcframework is the modern format, supports SPM and CocoaPods, and handles simulator + device + Mac Catalyst slices cleanly. Static framework is smaller but loses simulator support. Recommend: xcframework only.
9. **KMP common module Coroutines binary compatibility.** `kotlinx.coroutines` is in active development and KMP host apps may pin a different version than the SDK. Recommend: declare a wide `api` dependency range, test against latest + minimum-supported, document the matrix.
10. **Cross-target test matrix.** A bug in Kotlin/Native's signature canonicalization that doesn't reproduce on JVM is the nightmare scenario. Conformance tests must run on every target (`iosArm64`, `iosSimulatorArm64`, `iosX64`, `androidJvm`, `jsNode`) — CI workflow needs to fan out per target.
11. **Flutter federated split — 4 packages or monolithic plugin?** Federated is the modern Flutter recommendation and lets the iOS / Android implementations move independently (e.g., bump the iOS xcframework without re-publishing the Dart surface). Cost is publishing four pub.dev packages per release. Recommend: federated; the discipline pays off and the publishing pipeline is automatable.
12. **Flutter `permission_handler` integration.** Flutter ecosystem leans on the `permission_handler` package for runtime permissions, but pulling it in as a transitive dependency forces a version on every consumer. Recommend: do not depend on `permission_handler`. Expose `BridgePeer.requestCameraPermission()` and let host apps decide whether to call it directly or route through their existing permission stack.
13. **Flutter desktop targets (macOS, Windows, Linux).** Flutter supports desktop, but the SDK's role on desktop is unusual — the bridge is a separate process; a Flutter desktop app would be a Consumer, not an Origin. Recommend: no Flutter desktop targets in v1. Revisit only if a clear use case emerges.

---

## 15. Status

Not committed. Companion to [`ARCHITECTURE.md`](ARCHITECTURE.md) §13. Build phase plan lives in [`ARCHITECTURE.md`](ARCHITECTURE.md) §11; mobile SDK work is phase 4.

When the SDK is built, this document should link to the actual implementations:

- `mcp-bridge-mobile/core/` — KMP Gradle project (`commonMain`, `iosMain`, `androidMain`, `jsMain`) — the protocol core and platform-specific implementations.
- `mcp-bridge-mobile/swift-wrapper/` — Swift Package + CocoaPods spec wrapping the KMP-built xcframework, surfacing the public Swift API.
- `mcp-bridge-mobile/react-native/` — TurboModule + TypeScript hooks API; codegen specs.
- `mcp-bridge-mobile/flutter/` — Federated Flutter plugin (`mcp_bridge_mobile`, `mcp_bridge_mobile_platform_interface`, `mcp_bridge_mobile_ios`, `mcp_bridge_mobile_android`).
- `mcp-bridge-mobile/capacitor/` — Capacitor plugin TypeScript API over the platform bridges.
- `mcp-bridge-mobile/web/` — Pure-JS Kotlin/JS distribution for non-platform use cases (testing, simulators).

Each section above should be linked to the file or class that enforces the claim, the same way [`PRIVACY.md`](PRIVACY.md) expects to be linked once implementation lands.
