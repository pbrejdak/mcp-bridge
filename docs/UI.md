# Bridge Console — UI Layer

Status: exploratory, current revision 2026-05-23. Companion to [`ARCHITECTURE.md`](ARCHITECTURE.md) (cross-cutting design) and [`DAEMON.md`](DAEMON.md) (native daemon internals). This document covers the user-facing UI process: tech stack, window architecture, IPC consumption, styling for OS-native feel.

The Resolver from [`ARCHITECTURE.md §3`](ARCHITECTURE.md) runs as two cooperating processes. Bridge Console is the on-demand UI half — it renders state and dispatches user actions over local IPC. The always-on, no-UI half is [`mcp-bridged`](DAEMON.md).

---

## 1. Position and scope

Bridge Console:

- Provides the tray icon and tray menu (the user's primary touchpoint).
- Opens windows on demand — Pair sheet, Console main window — and closes them freely without affecting the daemon or the request path.
- Communicates with `mcp-bridged` over a local socket (JSON-RPC 2.0).
- Owns no security-critical state. The daemon owns the keystore, the registry, the proxy. The UI is presentation + control plane only.

Out of scope for this document:

- The daemon internals → [`DAEMON.md`](DAEMON.md).
- The wire protocols (`mcp-pair`, `mcp-announce`) → [`ARCHITECTURE.md §4`](ARCHITECTURE.md).
- The Stable-Loopback Bridge pattern → [`ARCHITECTURE.md §2`](ARCHITECTURE.md).

---

## 2. Process model

- **Launched on demand** — from the tray icon (click), from the URI-scheme handler (`mcp-bridge://pair/<token>`), from the CLI (`mcp-bridge open`), or from the installer's first-launch.
- **Single instance** — second invocation focuses the existing instance (Tauri `tauri-plugin-single-instance`).
- **Independent of daemon liveness** — if `mcp-bridged` is unreachable, the Console reflects a "daemon unreachable" state and offers a Restart button rather than crashing.
- **Tray persists** — closing the main window keeps the tray icon alive; quitting from the tray menu actually exits.
- **No autostart on login** — the daemon autostarts; the Console only launches when the user wants it (or when a pair deeplink fires).

Lifecycle states:

```
Launching ─► Handshaking ─► Connected ─► Idle (tray only)
                  │              │ ▲
                  ▼              ▼ │ open/close window
              Reconnecting     WindowOpen
              (exp. backoff)
```

---

## 3. Stack overview

```
Bridge Console
├── Tauri 2.x                       — Rust shell, native window + tray + plugins
│
├── Frontend (renders in OS webview)
│   ├── Svelte 5 (runes)            — UI framework
│   ├── TypeScript (strict)         — types end-to-end
│   ├── Vite 6                      — bundler + dev server
│   │
│   ├── shadcn-svelte               — copy-paste components on top of Bits UI
│   ├── Bits UI                     — headless primitives (transitive)
│   │
│   ├── Tailwind v4                 — utilities + design tokens
│   ├── Platform-attribute CSS      — data-platform=macos|windows|linux
│   ├── System fonts only           — no fonts shipped with app
│   │
│   ├── Icons (Path B)
│   │   ├── macOS: SF Symbols via Tauri command (NSImage → base64 PNG)
│   │   ├── Windows: @fluentui/svg-icons (MIT)
│   │   └── Linux: @lucide/svelte (ISC)
│   │
│   ├── tinykeys                    — keyboard shortcuts with $mod abstraction
│   ├── svelte-sonner               — toasts
│   ├── mode-watcher                — dark mode follow
│   ├── clsx + tailwind-merge       — class composition
│   └── qrcode (vanilla)            — pair sheet QR rendering
│
└── Tauri plugins
    ├── tauri-plugin-tray           — native tray icon + menu
    ├── tauri-plugin-positioner     — NSPopover-style tray window on macOS
    ├── tauri-plugin-single-instance
    ├── tauri-plugin-deep-link      — mcp-bridge://pair/<token> handler
    ├── tauri-plugin-dialog         — native open/save dialogs
    ├── tauri-plugin-notification   — native OS notifications
    ├── tauri-plugin-context-menu   — native right-click menus inside windows
    ├── tauri-plugin-os             — platform detection (data-platform attribute)
    ├── tauri-plugin-clipboard      — diagnostics bundle copy
    ├── tauri-plugin-window-effects — macOS vibrancy, Windows Mica
    └── tauri-plugin-shell          — open external URLs (release notes etc.)
```

---

## 4. Window architecture

Three Tauri windows, all created on demand:

| Window | Size | Purpose | Created when |
|---|---|---|---|
| **Tray window** | 320 × 400, NSPopover-anchored on macOS, undecorated on Win/Linux | Quick actions, status badge, "Pair new server", "Open Console" | Tray icon click |
| **Pair window** | 480 × 640, modal-feel, undecorated chrome | QR + SAS + install checkboxes confirmation | Tray "Pair new server", URI-scheme deeplink, or daemon `pair.invite_displayed` event |
| **Console window** | 900 × 600, standard chrome, resizable, restores last size/position | Main UI — Servers list, Activity feed, Settings tabs | Tray "Open Console" or first launch |

```
              click tray icon
                    │
                    ▼
              Tray window
              (popover sheet)
                    │
        ┌───────────┼───────────┐
        ▼           ▼           ▼
   Pair window  Console     Quit
   (modal)     window
```

Each window has its own webview instance. The Console window's webview suspends when the window is closed; Tauri kills it after 30 s of inactivity to reclaim memory.

**macOS specifics**:
- Tray window: `NSPopover` style via `tauri-plugin-positioner` — rounded corners, arrow anchor to the menu-bar icon, dismiss-on-click-outside.
- Console window: `titleBarStyle: "overlay"` for the modern macOS look (content extends behind traffic lights).
- Window vibrancy: `setEffect("sidebar")` on the Console; pure background on the Pair sheet.

**Windows specifics**:
- Tray window: undecorated, custom rounded corners via CSS, drop-shadow via OS DWM.
- Console window: Mica backdrop on Windows 11 via `tauri-plugin-window-effects`.

**Linux specifics**:
- Tray window: undecorated, GTK CSD when available.
- Console window: standard GTK header bar.

---

## 5. Frontend stack details

### 5.1 Svelte 5

Runes-based reactivity throughout. No legacy `$:`, no stores-as-default. Use:

- `$state(value)` for reactive state.
- `$derived(expr)` for computed.
- `$effect(() => ...)` for side effects (Tauri event subscriptions, cleanup).
- `$props<T>()` for typed component props.

State that's shared across components lives in `src/lib/state/*.svelte.ts` modules using the same runes — no `writable` stores. Example pattern:

```ts
// src/lib/state/servers.svelte.ts
let _servers = $state<Server[]>([]);
export const servers = {
  get list() { return _servers; },
  set list(v) { _servers = v; },
};
```

Components consume `servers.list` reactively.

### 5.2 TypeScript

Strict mode on, all flags enabled:

```json
{
  "compilerOptions": {
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,
    "noImplicitOverride": true,
    "verbatimModuleSyntax": true
  }
}
```

IPC types generated from the Rust daemon's JSON-RPC schema via `ts-rs` (or `specta`) — single source of truth in the daemon, consumed as `.d.ts` in the UI. See §10.

### 5.3 Build

Vite 6 with `@sveltejs/vite-plugin-svelte`. Bundle target: ES2022 (system webviews all support it). No legacy fallbacks.

Per-window entry points so each window loads only what it needs:

```
src/
├── tray/main.ts        — tray window entry
├── pair/main.ts        — pair window entry
├── console/main.ts     — console window entry
└── shared/             — components and state used by ≥2 entries
```

`vite.config.ts` configures three rollup inputs. Result: tray window loads ~40 KB; pair window ~60 KB; console ~120 KB. Each window stays snappy because it doesn't pull in code for the others.

### 5.4 Webview privacy hardening

Each system webview ships its own telemetry / state-sharing channels. Tauri must explicitly disable them or the privacy-first claim does not survive scrutiny on Windows. See [`PRIVACY.md`](PRIVACY.md) §5 for the policy this implements.

**WebView2 (Windows)** — pass these flags via `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` when launching the Tauri shell:

```
--disable-features=msEdgeWebView2Telemetry,DomainReliability,OptimizationGuideModelDownloading
--disable-domain-reliability
--no-pings
--no-default-browser-check
```

Set via `tauri::Builder::default().setup()` so it applies to every window.

**WKWebView (macOS)** — configure each Tauri window with a non-persistent data store and a fresh process pool:

```rust
// src-tauri/src/webview.rs
unsafe {
    let config = WKWebViewConfiguration::new();
    config.setWebsiteDataStore(WKWebsiteDataStore::nonPersistentDataStore());
    config.setProcessPool(WKProcessPool::new());
}
```

Prevents cookies, IndexedDB, localStorage, and HSTS state from being shared with Safari or persisting across sessions.

**WebKitGTK (Linux)** — disable cache, offline-app cache, and crash reports via `WebKitWebContext` settings; clear the data store on quit.

**Tauri config** (cross-platform) in `tauri.conf.json`:

```jsonc
{
  "app": {
    "withGlobalTauri": false,
    "security": {
      "csp": "default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: tauri:",
      "freezePrototype": true,
      "dangerousDisableAssetCspModification": false
    }
  }
}
```

Tight CSP plus `withGlobalTauri: false` means injected JS cannot reach the IPC bridge or fetch external resources. No third-party fonts, no external images, no analytics scripts of any kind can load — even accidentally via copy-pasted snippets during development.

These settings are enforced from the Rust side at window creation; user-mutable webview prefs are not exposed.

---

## 6. Component layer

shadcn-svelte components installed into the repo via `npx shadcn-svelte@latest add <name>`. They land in `src/lib/components/ui/` and become *yours* — edit freely.

Initial component set:

| shadcn-svelte component | Used by |
|---|---|
| `dialog` | Pair sheet, confirm dialogs |
| `dropdown-menu` | Per-server "…" menu |
| `context-menu` | Server row right-click (wraps Tauri's native context menu) |
| `popover` | Server detail flyout, SAS tooltip on hover |
| `tabs` | Settings (General / Identity / Logging / Updates) |
| `switch` | Verbose logging toggle |
| `combobox` | Consumer picker in pair sheet |
| `tooltip` | Status badges, button hints |
| `command` | Cmd-K / Ctrl-K palette |
| `sonner` | Toasts |
| `separator`, `card`, `badge`, `button` | Layout primitives |

Custom feature components live in `src/lib/components/feature/`:

```
feature/
├── PairSheet.svelte         — QR + SAS + install checkboxes
├── ServerRow.svelte         — list item with state badge, last activity
├── ServerDetail.svelte      — Consumers, ACL, revoke per Consumer
├── ActivityFeed.svelte      — streaming tool-call list
├── ConsumerPicker.svelte    — which Consumers to install into
├── DaemonStatusBanner.svelte— surfaces "daemon unreachable" / "update available"
└── DiagnosticsExport.svelte — copy-bundle button
```

---

## 7. Styling and platform feel

Strategy from [`ARCHITECTURE.md UI plan`](ARCHITECTURE.md): **lean on native HTML controls + headless primitives + platform-attribute CSS** rather than imposing a design system.

### 7.1 System fonts

```css
:root {
  font-family:
    -apple-system, BlinkMacSystemFont,            /* macOS / iOS */
    "Segoe UI Variable", "Segoe UI",              /* Windows 11 / 10 */
    "Cantarell", "Ubuntu",                        /* GNOME / Ubuntu */
    system-ui, sans-serif;
}
```

No bundled fonts. The webview hands rendering to the OS text engine.

### 7.2 Platform-attribute CSS

`tauri-plugin-os` reports platform; the app sets `<html data-platform="macos|windows|linux">` at boot. Tailwind v4 configured with platform-conditional variants:

```css
@theme {
  --radius-control: 6px;
  --focus-ring: 0 0 0 3px rgb(0 122 255 / 0.5);
  --density: 1;
}

[data-platform="macos"] {
  --radius-control: 5px;
  --density: 1.05;
}

[data-platform="windows"] {
  --radius-control: 4px;
  --focus-ring: 0 0 0 2px #60cdff;
}

[data-platform="linux"] {
  --radius-control: 6px;
}
```

Tailwind classes consume the tokens: `rounded-[var(--radius-control)]`, `[--tw-ring-shadow:var(--focus-ring)]`.

### 7.3 Native HTML controls

`<button>`, `<input>`, `<select>`, `<input type="checkbox">`, `<input type="radio">` styled minimally — let the OS theme them. Custom widgets only where HTML has no equivalent (combobox, command palette, popover).

### 7.4 Window effects

| Platform | Effect | API |
|---|---|---|
| macOS | Sidebar vibrancy on Console main; transparent title bar | `tauri-plugin-window-effects: setEffect("sidebar")` |
| Windows 11 | Mica backdrop | `setEffect("mica")` |
| Windows 10 | Acrylic fallback | `setEffect("acrylic")` |
| Linux | Plain (no effect) | none |

### 7.5 Dark mode

`mode-watcher` reads `prefers-color-scheme`, propagates to `<html data-theme="light|dark">`. Tailwind v4 keys off `data-theme` rather than `.dark` class. The Console follows OS preference automatically.

---

## 8. Icons — Path B implementation

`<Icon name="..." size={16} />` Svelte component, three platform branches behind one interface:

```svelte
<!-- src/lib/components/Icon.svelte -->
<script lang="ts">
  import { platform } from "$lib/state/platform.svelte";
  import { iconMap, type IconName } from "$lib/icons/map";

  let { name, size = 16, class: cls = "" } = $props<{
    name: IconName;
    size?: number;
    class?: string;
  }>();
</script>

{#if platform.os === "macos"}
  <SfSymbol {name} {size} class={cls} />
{:else if platform.os === "windows"}
  <FluentIcon {name} {size} class={cls} />
{:else}
  <LucideIcon {name} {size} class={cls} />
{/if}
```

### 8.1 macOS — SF Symbols via Tauri command

SF Symbols are not accessible from a webview directly. A Tauri command on the Rust side renders the symbol via AppKit:

```rust
// src-tauri/src/icons.rs
#[tauri::command]
async fn render_sf_symbol(name: &str, size: u32, dark: bool) -> Result<String, String> {
    // Uses objc2 + AppKit to:
    // 1. NSImage(systemSymbolName: name)
    // 2. Apply size + color (foregroundColor based on dark mode)
    // 3. Render to NSBitmapImageRep
    // 4. PNG-encode
    // 5. base64 the bytes
    // Returns "data:image/png;base64,..."
}
```

Svelte component caches by `(name, size, dark)`:

```svelte
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  const cache = new Map<string, string>();

  let { name, size, class: cls } = $props();
  let dataUri = $state<string | null>(null);

  $effect(() => {
    const key = `${name}-${size}-${isDark}`;
    if (cache.has(key)) { dataUri = cache.get(key)!; return; }
    invoke<string>("render_sf_symbol", { name, size, dark: isDark }).then(uri => {
      cache.set(key, uri);
      dataUri = uri;
    });
  });
</script>

{#if dataUri}
  <img src={dataUri} width={size} height={size} alt="" class={cls} />
{/if}
```

First render of each icon costs one IPC roundtrip (~5 ms). Subsequent uses are synchronous from cache. App pre-warms common icons (settings, plus, x, check, chevron-right) on boot.

License: SF Symbols are licensed for use only in software running on Apple platforms. This branch executes only when `platform.os === "macos"`, which matches that constraint. Document this in the project README under "Licenses."

### 8.2 Windows — Fluent System Icons

`@fluentui/svg-icons` (MIT) provides SVG components. Tree-shaken — only icons referenced ship.

```svelte
<!-- src/lib/icons/FluentIcon.svelte -->
<script lang="ts">
  import { iconMap, type IconName } from "./map";
  let { name, size, class: cls } = $props<{ name: IconName; size: number; class?: string }>();
  const Component = iconMap.windows[name];
</script>

<Component {...{ width: size, height: size }} class={cls} />
```

### 8.3 Linux — Lucide

`@lucide/svelte` (ISC), same pattern as Windows. No native equivalent that's worth the integration cost in v1 (Adwaita icons are theme-dependent and complex to bridge).

### 8.4 The icon mapping table

```ts
// src/lib/icons/map.ts
export type IconName =
  | "settings" | "plus" | "x" | "check" | "chevron-right"
  | "shield" | "key" | "qr-code" | "link" | "refresh"
  | "trash" | "copy" | "external-link" | "more-horizontal"
  | "info" | "alert-triangle" | "alert-circle" | "check-circle"
  | "wifi-off" | "lock" | "user" | "search";

export const iconMap = {
  macos: {
    "settings": "gearshape",
    "plus": "plus",
    "x": "xmark",
    "check": "checkmark",
    "shield": "lock.shield",
    "qr-code": "qrcode",
    "link": "link",
    "refresh": "arrow.clockwise",
    "trash": "trash",
    "copy": "doc.on.doc",
    "external-link": "arrow.up.right.square",
    "more-horizontal": "ellipsis",
    "wifi-off": "wifi.slash",
    // …
  },
  windows: {
    "settings": () => import("@fluentui/svg-icons/icons/settings_20_regular.svg"),
    "plus": () => import("@fluentui/svg-icons/icons/add_20_regular.svg"),
    // …
  },
  linux: {
    "settings": "Settings",   // Lucide names
    "plus": "Plus",
    // …
  },
};
```

Add icons by appending to the union and mapping all three platforms — CI lint enforces every name maps in all three branches.

---

## 9. Keyboard shortcuts

### 9.1 In-window shortcuts — `tinykeys` with `$mod`

```ts
import { tinykeys } from "tinykeys";
import { onMount } from "svelte";

onMount(() => tinykeys(window, {
  "$mod+k":       () => commandPalette.open(),
  "$mod+,":       () => navigate("/settings"),
  "$mod+Shift+r": () => focusedServer && revoke(focusedServer),
  "Escape":       () => activeSheet?.close(),
  "/":            () => searchInput.focus(),
}));
```

`$mod` resolves to Cmd on macOS, Ctrl on Windows / Linux. Single source of shortcut strings; no per-platform branching in component code.

### 9.2 App-level shortcuts — Tauri menu plugin

For shortcuts that live in the OS-native menu bar (macOS) or window menu (Windows / Linux), use Tauri's menu API. It auto-renders `⌘,` / `Ctrl+,` correctly per platform:

```rust
// src-tauri/src/menu.rs
let menu = Menu::with_items(app, &[
    &Submenu::with_items(app, "Bridge", true, &[
        &PredefinedMenuItem::about(app, None, None)?,
        &PredefinedMenuItem::separator(app)?,
        &MenuItem::with_id(app, "prefs", "Preferences…", true, Some("CmdOrCtrl+,"))?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::quit(app, None)?,
    ])?,
    // …
])?;
```

The split: in-window things → `tinykeys`; app-level things → Tauri menu. Quit, Close Window, Preferences, About all go through Tauri menu.

---

## 10. IPC consumer side

### 10.1 Transport

Tauri's invoke + event APIs wrap the underlying socket. A thin `src/lib/ipc.ts` exposes typed wrappers:

```ts
// src/lib/ipc.ts
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type * as Daemon from "./generated/daemon-types";

export async function call<M extends keyof Daemon.Methods>(
  method: M,
  params: Daemon.Methods[M]["params"]
): Promise<Daemon.Methods[M]["result"]> {
  return invoke(`ipc.${method}`, { params });
}

export function on<E extends keyof Daemon.Events>(
  event: E,
  handler: (payload: Daemon.Events[E]) => void
): Promise<UnlistenFn> {
  return listen(`ipc.${event}`, (e) => handler(e.payload as Daemon.Events[E]));
}
```

The `Daemon.Methods` and `Daemon.Events` types are generated from the Rust daemon via `ts-rs`. Adding a new RPC method requires updating Rust; TypeScript types fall out automatically. See [`DAEMON.md §5`](DAEMON.md) for the full method/event list.

### 10.2 State stores

One module per resource, runes-based:

```ts
// src/lib/state/servers.svelte.ts
import { call, on } from "$lib/ipc";

let _list = $state<Server[]>([]);

export const servers = {
  get list() { return _list; },
  async refresh() { _list = await call("servers.list", {}); },
  async revoke(pin: string, consumer?: string) {
    await call("servers.revoke", { pin_id: pin, consumer });
    await this.refresh();
  },
};

// Wire up streaming updates
on("server.state_changed", ({ pin_id, state }) => {
  const i = _list.findIndex(s => s.pin_id === pin_id);
  if (i >= 0) _list[i] = { ..._list[i], state };
});
```

Components import the module and read `servers.list` reactively.

### 10.3 Reconnect logic

`src/lib/ipc.ts` exposes a `connection` rune that tracks `connected | reconnecting | error`. On the Tauri side, a background task pings `daemon.status` every 5 s; if the IPC roundtrip fails twice in a row, the connection state flips to `reconnecting` and the UI surfaces the `DaemonStatusBanner` with a "Restart daemon" button (which invokes `daemon.shutdown` then re-launches via the platform unit).

---

## 11. File structure

```
bridge-console/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs                — Tauri entry, command registration
│   │   ├── icons.rs               — render_sf_symbol command (macOS only)
│   │   ├── ipc_bridge.rs          — proxy invoke ↔ daemon JSON-RPC
│   │   ├── menu.rs                — Tauri menu definition
│   │   ├── tray.rs                — tray icon + menu
│   │   ├── deeplink.rs            — mcp-bridge://pair/<token> handler
│   │   └── windows.rs             — Tauri window definitions
│   ├── icons/                     — tray template images
│   ├── tauri.conf.json
│   └── Cargo.toml
│
├── src/
│   ├── tray/main.ts               — tray window entry
│   ├── pair/main.ts               — pair window entry
│   ├── console/main.ts            — console window entry
│   │
│   ├── lib/
│   │   ├── components/
│   │   │   ├── ui/                — shadcn-svelte (owned)
│   │   │   ├── feature/           — custom feature components
│   │   │   └── Icon.svelte        — platform-dispatching icon
│   │   ├── icons/
│   │   │   ├── map.ts             — name → platform-specific icon
│   │   │   ├── SfSymbol.svelte
│   │   │   ├── FluentIcon.svelte
│   │   │   └── LucideIcon.svelte
│   │   ├── state/
│   │   │   ├── servers.svelte.ts
│   │   │   ├── activity.svelte.ts
│   │   │   ├── settings.svelte.ts
│   │   │   ├── platform.svelte.ts
│   │   │   └── connection.svelte.ts
│   │   ├── ipc.ts                 — typed wrapper
│   │   ├── generated/
│   │   │   └── daemon-types.ts    — ts-rs output from Rust
│   │   ├── platform.ts            — OS detection helpers
│   │   ├── theme.ts               — mode-watcher integration
│   │   └── utils.ts               — cn(), clsx + tailwind-merge
│   │
│   ├── app.css                    — Tailwind + platform tokens
│   └── lib/components/ui/*.svelte
│
├── static/
│   └── (none for v1)
│
├── package.json
├── tsconfig.json
├── vite.config.ts
├── svelte.config.js
├── tailwind.config.ts
└── components.json                — shadcn-svelte registry config
```

---

## 12. npm dependencies (frozen for v1)

```jsonc
{
  "dependencies": {
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-clipboard-manager": "^2",
    "@tauri-apps/plugin-deep-link": "^2",
    "@tauri-apps/plugin-dialog": "^2",
    "@tauri-apps/plugin-notification": "^2",
    "@tauri-apps/plugin-os": "^2",
    "@tauri-apps/plugin-positioner": "^2",
    "@tauri-apps/plugin-shell": "^2",
    "@tauri-apps/plugin-single-instance": "^2",
    "@tauri-apps/plugin-window-effects": "^2",
    "tauri-plugin-context-menu": "^0.8",

    "svelte": "^5",
    "bits-ui": "^1",
    "mode-watcher": "^0.5",
    "svelte-sonner": "^0.3",
    "tinykeys": "^3",
    "qrcode": "^1.5",

    "clsx": "^2",
    "tailwind-merge": "^2",
    "tailwind-variants": "^0.3",

    "@fluentui/svg-icons": "^1",
    "@lucide/svelte": "^0.460"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2",
    "@sveltejs/vite-plugin-svelte": "^5",
    "typescript": "^5.6",
    "vite": "^6",
    "tailwindcss": "^4",
    "@tailwindcss/vite": "^4",
    "vitest": "^2",
    "@vitest/ui": "^2",
    "svelte-check": "^4",

    "shadcn-svelte": "^1"
  }
}
```

Tauri Rust side (`src-tauri/Cargo.toml`):

```toml
[dependencies]
tauri = { version = "2", features = ["devtools"] }
tauri-plugin-tray = "2"
tauri-plugin-positioner = "2"
tauri-plugin-single-instance = "2"
tauri-plugin-deep-link = "2"
tauri-plugin-dialog = "2"
tauri-plugin-notification = "2"
tauri-plugin-context-menu = "0.8"
tauri-plugin-os = "2"
tauri-plugin-clipboard-manager = "2"
tauri-plugin-window-effects = "2"
tauri-plugin-shell = "2"

serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
ts-rs = "10"                       # generate TS types from Rust

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.5"
objc2-app-kit = "0.2"
objc2-foundation = "0.2"
```

---

## 13. Build, signing, distribution

- **Bundle config** — `tauri.conf.json` per-platform: macOS DMG + PKG, Windows MSI + NSIS, Linux AppImage + deb + rpm.
- **Code signing**:
  - macOS — Apple Developer ID + notarization. Build in GitHub Actions matrix.
  - Windows — Authenticode cert. SmartScreen reputation builds over time.
  - Linux — distro packaging signatures via the relevant key.
- **SLSA provenance** — published alongside each release ([`ARCHITECTURE.md §11`](ARCHITECTURE.md)).
- **Updater** — the Console does not auto-update itself directly; it surfaces the daemon's `update.available` event and the user clicks "Update," which triggers the daemon's `update.apply()` (see [`DAEMON.md §9`](DAEMON.md)). The daemon's restart cascades to the Console on next launch.

---

## 14. Testing strategy

- **Unit tests** — Vitest for state modules, IPC wrapper logic, icon mapping. Run on every commit.
- **Component tests** — `@testing-library/svelte` for feature components. PairSheet, ServerRow, ActivityFeed have explicit tests.
- **Integration tests** — Tauri's `WebDriver` (via `tauri-driver`) for end-to-end flows: open Console, see BodyLog, revoke, see toast. Run on PR merge to main.
- **Visual regression** — deferred to v0.3; not worth the maintenance now.

---

## 15. What's not in v1

- Per-locale i18n. English only at launch; structure permits adding later via `svelte-i18n`.
- Customizable themes. System dark mode follow only.
- Embedded help / docs. Link to website.
- Custom title-bar drag region on Linux beyond GTK defaults.
- Activity feed search / filters. Linear scroll only.
- Per-Consumer custom allow-lists in the UI. Backend supports it ([`DAEMON.md §5`](DAEMON.md) `servers.update_acl`); UI exposes "Allow all" / "Read-only" presets only, advanced custom lists deferred to v0.3.
- Touch / iPad optimisations. Desktop only.
- Drag-and-drop reordering of servers.

---

## 16. Open UI-side decisions

1. **Persisted UI prefs — Tauri plugin-store vs daemon-side?** Things like "last selected tab" or "Console window position" could live in `tauri-plugin-store` (LevelDB-style local file) or in the daemon's `settings`. Daemon-side keeps everything in one place but adds an IPC roundtrip on every prefs read. Likely: daemon-side for prefs that affect daemon behaviour (`verbose_logging`, `update_channel`), `tauri-plugin-store` for purely-UI prefs (window position, last tab).
2. **shadcn-svelte upgrade strategy.** Components copied into the repo don't auto-update. Either pin all components at install date and update manually with breaking-change review, or write a CI job that runs `shadcn-svelte diff` weekly and opens PRs. Probably: manual quarterly review.
3. **Webview developer tools in production builds.** Helpful for diagnosing field issues; risky as an attack surface if always-on. Likely: opt-in via a hidden `--dev-tools` flag or a 5-click "easter egg" in About.
4. **macOS Continuity Camera as an alternative QR delivery path.** If the user's iPhone is signed into the same iCloud, the Mac can use it as a webcam — opens an interesting path for Direction-A fallback ([`ARCHITECTURE.md §5.5`](ARCHITECTURE.md)) without requiring a built-in Mac webcam. Investigate.

---

## 17. Status

Not committed. Companion to [`ARCHITECTURE.md §13`](ARCHITECTURE.md). Build phase plan and effort estimates live in [`ARCHITECTURE.md §11`](ARCHITECTURE.md).
