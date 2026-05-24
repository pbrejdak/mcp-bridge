# 0002 — Tauri over Electron for Bridge Console

- **Status**: Accepted
- **Date**: 2026-05-23
- **Deciders**: Project founders
- **Supersedes**: —
- **Related**: [`UI.md`](../UI.md), [`PRIVACY.md`](../PRIVACY.md), [0001](0001-stable-loopback-over-config-rewriter.md)

## Context

[Bridge Console](../GLOSSARY.md#bridge-console) is the user-facing UI for MCP Bridge — a menu-bar / system-tray app with a pair sheet, a Servers list, an activity feed, and a Settings surface. It is a small, mostly-static UI that talks to the Rust daemon over a local JSON-RPC socket.

The candidate frameworks for desktop apps with web-tech UIs are well-known:

- **Electron** — Chromium + Node, the incumbent. Mature ecosystem, predictable cross-platform behavior, huge community.
- **Tauri** — Rust core + the OS-native webview (WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux).
- **Native UI toolkits per platform** (AppKit / SwiftUI on macOS, WinUI on Windows, GTK on Linux) — three codebases.
- **Flutter Desktop** — single codebase, Skia-rendered UI everywhere.

Our constraints:

- **Privacy-first posture** ([`PRIVACY.md`](../PRIVACY.md)). The UI must not phone home and must not embed third-party SDKs by default.
- **Small footprint** ([0001](0001-stable-loopback-over-config-rewriter.md) commits us to "one always-on daemon"; the UI launches on demand). A multi-hundred-megabyte UI sitting next to a small daemon would undermine the privacy/footprint narrative.
- **OS-native look and feel** is a goal, not just a nice-to-have. Bridge sits in the menu bar / tray; it should feel like part of the OS, not a web app trapped in a chrome.
- **A Rust ecosystem already in play** — the daemon is Rust; the same codebase already has tooling for builds, signing, and provenance.

## Decision

Use **Tauri** for [Bridge Console](../GLOSSARY.md#bridge-console), with **Svelte 5** as the UI framework inside the webview.

## Alternatives considered

- **Electron** — would have been the safest, least-controversial choice. Ruled out because:
  - **Bundle size**: Electron ships its own Chromium (~120-150 MB per app). Tauri ships a small Rust binary (~3-5 MB) and uses the OS webview.
  - **Process memory**: each Electron app duplicates a browser. A small UI process is preferable when most of the work is in the daemon.
  - **Third-party-by-default**: Chromium ships features that phone home unless explicitly disabled. WKWebView / WebView2 / WebKitGTK have their own telemetry concerns ([`UI.md`](../UI.md) §5.4 hardens them explicitly), but the surface is smaller and more auditable.
  - **OS-native chrome**: the OS-native webviews integrate more naturally with platform vibrancy, traffic lights, Mica backdrop, etc.
- **Native UI per platform (AppKit, WinUI, GTK)** — would deliver the best feel on each platform but triples implementation effort. Pre-1.0 we cannot afford three UIs. We get a meaningful fraction of the native feel via OS-native webviews and per-platform CSS (see [0005](0005-path-b-per-platform-icons.md)).
- **Flutter Desktop** — single codebase, but the UI is rendered by Skia rather than by the OS. Result: a coherent app that looks the same everywhere, but distinctly *non*-native everywhere. Wrong tradeoff for a menu-bar utility.

## Consequences

What this enables:

- **Single-digit-MB app bundles** that match the privacy/footprint narrative of [0001](0001-stable-loopback-over-config-rewriter.md).
- **One Rust toolchain** across daemon and UI shell — same CI build matrix, same signing pipeline.
- **Webview hardening surface is finite and documented** ([`UI.md`](../UI.md) §5.4) — feature-policy headers, telemetry disablement, persistent-cache opt-out.
- **Platform integration** via Tauri plugins (`tray`, `positioner`, `single-instance`, `deep-link`, `window-effects`) without re-implementing per platform.

Costs we accept:

- **Three webviews to test against** instead of one. WKWebView ≠ WebView2 ≠ WebKitGTK; we maintain a per-platform compatibility matrix and accept that some CSS or JS features need fallbacks.
- **Smaller ecosystem than Electron** for niche desktop integrations. Some Electron-first libraries (e.g., specific native menu helpers) we re-implement or do without.
- **Tauri plugin maturity varies** — a few we depend on are 1.x-recent. We budget time to track upstream fixes.
- **WebView2 install / version matters on Windows** — the runtime is preinstalled on Windows 11 but historically not on Windows 10. The installer ensures it.

What would force a revisit:

- If the OS-webview compatibility matrix becomes painful enough that the engineering tax exceeds Electron's footprint tax.
- If a future UI demands rendering features the OS webviews don't support and Chromium does (uncommon for a settings UI).
