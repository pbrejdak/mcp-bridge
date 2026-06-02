# Bridge Console

Desktop GUI for the MCP Bridge daemon. Tauri 2 shell, Svelte 5 + TypeScript
renderer. Talks to the running [`mcp-bridged`](../mcp-bridged/) daemon over
the same UDS / Windows named-pipe the CLI uses.

**Status**: MVP. One Console window today, showing daemon status + paired
servers. Tray icon, pair-window (QR + SAS), activity feed, and settings
land in follow-up sessions — see [`docs/UI.md`](../docs/UI.md) for the
full design.

## Prerequisites

- Rust 1.85+ (the workspace toolchain).
- Node.js 22+ and npm 10+.
- A running `mcp-bridged` daemon (see the root [README](../README.md)).

On the first dev run, `tauri dev` may install platform OS dependencies
(WebKit on Linux). On macOS / Windows the bundled webview is used.

## Develop

```sh
cd bridge-console
npm install
npm run tauri dev
```

This starts Vite on `localhost:1420`, builds the Tauri shell, and opens
the Console window. Edits to `src/` hot-reload; edits to `src-tauri/src/`
trigger a Tauri rebuild.

The Console queries the daemon via `daemon_call`, a Tauri command that
wraps [`mcp_bridged::ipc::call_local`]. If the daemon isn't running you
get a "Daemon not reachable" banner and a Refresh button — start the
daemon with `mcp-bridge daemon` (or `mcp-bridge daemon --install`) and
click Refresh.

### macOS Tahoe (26.x) — launchd-installed daemon hangs

On macOS 26 (Tahoe) with an unsigned / ad-hoc-signed daemon binary,
the launchd-spawned daemon hangs indefinitely in dyld's
code-signature validation — the process is alive but never finishes
boot, never writes logs, never binds the IPC socket. The same binary
runs cleanly in 2 seconds when invoked directly from a shell.

Until proper Developer ID code signing + notarization land (tracked
in [`docs/ROADMAP.md`](../docs/ROADMAP.md) §6, blocked on the
$99/yr Apple Developer account), run the daemon in the foreground:

```sh
# Build the daemon once.
cargo build --release --bin mcp-bridge

# In one terminal, run it in the foreground.
./target/release/mcp-bridge daemon

# In another, develop the Console against the running daemon.
cd bridge-console && npm run tauri dev
```

`mcp-bridge daemon --uninstall` followed by no `--install` removes
any stale launchd plist. The Console talks to the foreground daemon
over the same IPC socket either way.

## Build

```sh
npm run tauri build
```

Produces a native bundle under `src-tauri/target/release/bundle/`.
Bundle format depends on the host OS (`.app` + `.dmg` on macOS,
`.msi` + `.exe` on Windows, `.AppImage` + `.deb` on Linux).

## Code layout

```
bridge-console/
├── src-tauri/           — Tauri 2 Rust shell
│   ├── Cargo.toml
│   ├── tauri.conf.json  — window + bundle config
│   ├── build.rs
│   └── src/main.rs      — Tauri entry + daemon_call command
│
├── src/                 — Svelte 5 frontend
│   ├── main.ts          — mount App
│   ├── App.svelte       — current Console view
│   └── lib/ipc.ts       — typed daemon_call wrapper + method names
│
├── index.html
├── package.json
├── vite.config.ts
├── tsconfig.json
└── svelte.config.js
```

## What's missing vs. UI.md design

The full design under [`docs/UI.md`](../docs/UI.md) calls for three
windows (tray, pair, console), shadcn-svelte component library, platform-
specific icon sets (SF Symbols / Fluent / Lucide), Mica/vibrancy effects,
deep-link handling, and more. This MVP ships the Console window only —
each follow-up commit will fill in one piece.
