# Bridge Console — UX Design

Status: exploratory, current revision 2026-05-23. Companion to [`UI.md`](UI.md) (technical UI stack), [`DAEMON.md`](DAEMON.md), and [`ARCHITECTURE.md`](ARCHITECTURE.md). This document covers what the user *sees and does* — vocabulary, flows, screens, copy, empty states, error recovery.

`UI.md` defines the technical structure (Tauri, Svelte, windows, plugins). This document defines the design that sits inside those windows — the interface the user actually touches.

---

## 1. Design principles

Five rules that pre-decide most design questions for this product.

1. **The architecture's vocabulary is internal.** "Origin / Resolver / Consumer / Pin" never appear in user-facing copy. They are documentation terms, not UI terms.
2. **Trust through visibility.** This is a privacy / security tool. The user should always be able to see: which servers are connected, who can call them, what calls happened. The product is transparent or it is nothing.
3. **One glance, not one read.** The tray icon and tray popover have to be parseable at-a-glance. The full Console window is where details live.
4. **Reversible by default.** Every destructive action (revoke, rotate, uninstall) is explicit, named, and undoable for at least 10 s.
5. **Quiet by default.** The product does not interrupt unless the user's stuff is broken or the user needs to make a security decision. No daily nags, no badges for benign events, no notifications for tool calls.

---

## 2. Vocabulary translation

Architecture term (internal) → UI copy (what the user sees):

| Internal | UI copy |
|---|---|
| Origin | the server's actual name ("BodyLog"). Never "origin" |
| Resolver | "MCP Bridge" (chrome only) — or just "Bridge" in body copy; usually unnamed |
| Consumer | "AI app" — or the specific name ("Claude Desktop", "Cursor") |
| Server Pin | "connection" or "server" |
| logical_id | never shown |
| pubkey / fingerprint | never shown except in Settings → Identity (advanced) |
| `mcp-pair`, `mcp-announce` | never shown |
| Reachable | "Connected" — green dot |
| Unreachable | "Offline" — amber dot, with "Will reconnect automatically" helper |
| Revoked | not shown in main list (lives in a "Removed" archive in Settings) |
| Loopback key | never shown |
| SAS | "Verification phrase" |
| Pair invite | "Setup code" |
| `target_resolver_pubkey` and friends | never shown |

Test for any string before it ships: would a non-technical user know what this means? If no, rewrite.

---

## 3. User flows (top-level)

The five flows that need to be smooth. Anything else is secondary.

### 3.1 First-time setup (the make-or-break flow)

```
Phone: tap "Connect to computer" in BodyLog
   │
   ▼
Phone: shows verification code page (URL + QR + "Send to Mac" button)
   │
   ▼  user AirDrops URL to Mac (or scans QR with laptop webcam,
   │  or types URL)
   ▼
Mac browser: opens mcpbridge.me/p/<token>
   │
   ▼
   ┌─ Bridge installed? ──── yes ──► Bridge tray icon appears active;
   │                                  Pair window opens with QR + SAS
   ▼ no
Mac browser: smart landing page, "Download Bridge for macOS"
   │
   ▼  user runs installer, grants Local Network permission
   ▼
Bridge first-launch: skips Welcome (token present); goes straight to
                     Pair window with QR + SAS
   │
   ▼
Phone: scan QR with BodyLog
   │
   ▼
Phone: "Pair BodyLog with Patryk's MacBook Pro? Verification: tiger-river-marble-clay"
       User glances at Bridge Pair window — same phrase — taps Confirm
   │
   ▼
Bridge Pair window: "BodyLog wants to be available in:
                     ☑ Claude Desktop  ☑ Cursor"
                    [Install]
   │
   ▼
Bridge Pair window: ✓ "BodyLog is ready. Restart Claude to use it."
                    [Restart Claude] [Close]
```

Action count for the user: AirDrop URL (1 tap), scan QR (1 motion), tap Confirm (1 tap), tap Install (1 click), tap Restart Claude (1 click). **Five clicks, one glance at the verification phrase.**

### 3.2 Daily check-in

User opens tray → glances at server list → sees "3 connected, all healthy" → closes. Three seconds.

### 3.3 "It stopped working" recovery

User opens tray → sees a server marked Offline with helper text "Reconnecting…" → either it reconnects within seconds (no action) or "Server can't be reached. [Show details] [Re-add]" appears.

### 3.4 Remove a server

Tray → click server → "Remove" — confirmation toast appears with Undo for 10 s.

### 3.5 Bridge has been compromised / rotate identity

Settings → Identity → "Rotate identity" with strong warning copy. After rotation, all paired phones need to re-pair. Surfaced in tray as a banner: "Identity rotated — re-pair your phones."

---

## 4. Tray icon — visual states

Single monochrome template image (`NSStatusItem` template, Win monochrome icon, Linux symbolic icon). Five visual states layered on it:

| State | Treatment |
|---|---|
| Idle, all healthy | Solid icon |
| Idle, some offline | Solid icon + small amber dot overlay (bottom-right) |
| Activity (mid tool call) | Brief 200 ms opacity pulse — optional, off by default to avoid distraction |
| Pairing in progress | Slow pulse animation (0.5 Hz) — visible but unobtrusive |
| Needs attention (cert changed, daemon crashed, update ready) | Solid icon + small red dot |
| Daemon stopped | Hollow / outlined version of the icon |

Icon concept (to brief whoever draws it): two small filled dots connected by an arc. Reads as "connection" without resorting to a literal bridge silhouette. Works at 16×16, 22×22, 32×32. Must look correct in macOS dark menu bar (template-image inversion) and Windows light/dark taskbar.

---

## 5. Tray menu (right-click)

Native OS menu via `tauri-plugin-tray`. Order matters — most-used first, destructive last.

```
Open Bridge Console            ⌘⇧B
─────────────────────────
Add server…                    ⌘N
─────────────────────────
BodyLog                  ● connected   ▸
HomeHub                 ● connected   ▸
Journal                 ○ offline     ▸
─────────────────────────
Pause all servers
Settings…                      ⌘,
─────────────────────────
About MCP Bridge
Quit                           ⌘Q
```

Each server submenu (`▸`):

```
Open BodyLog details
─────────────────────
Available in:
  ✓ Claude Desktop
  ✓ Cursor
─────────────────────
Remove BodyLog…
```

"Pause all servers" returns 503 from the Loopback Listener for a configurable duration (5 m / 30 m / until I resume) — useful when the user is on a public network and wants to be cautious without revoking.

---

## 6. Tray popover (left-click)

`NSPopover`-style sheet anchored to the tray icon. 320 × 400. Compact, scannable.

```
┌──────────────────────────────────────┐
│ MCP Bridge                       ⚙  │
│ 3 servers • All healthy              │
├──────────────────────────────────────┤
│                                      │
│  ● BodyLog                            │
│    Active 3 min ago                  │
│    Claude Desktop, Cursor       ⋯   │
│                                      │
│  ● HomeHub                           │
│    Active 1 h ago                    │
│    Claude Desktop               ⋯   │
│                                      │
│  ○ Journal                           │
│    Offline — reconnecting            │
│    Cursor                       ⋯   │
│                                      │
├──────────────────────────────────────┤
│  ⊕ Add server                        │
│  ↗ Open Bridge Console               │
└──────────────────────────────────────┘
```

Header status line is the at-a-glance summary:

| Situation | Header reads |
|---|---|
| All paired servers connected | `N servers • All healthy` |
| Some offline | `N servers • 1 offline` |
| Setup pending | `N servers • 1 pairing` |
| Daemon down | `Bridge service stopped — Restart` (header is the recovery affordance) |
| No servers yet | `No servers yet • Add one to start` |

Click a server row → expands inline to show recent activity + Remove. Or click the `⋯` → menu mirroring the tray menu's server submenu.

---

## 7. Pair flow — the four screens

The pair flow is the highest-stakes UX in the product. Get this right and most other UX problems become small.

### 7.1 Step 1 — Invite

Window: 480 × 640, undecorated chrome (modal feel), single column.

```
┌────────────────────────────────────────────┐
│                                            │
│              Add a server                  │
│                                            │
│   On your phone, open the app you want to  │
│   connect (BodyLog, HomeHub, etc.) and tap  │
│   its "Connect to computer" option.        │
│                                            │
│   ┌──────────────────────────────────┐    │
│   │                                  │    │
│   │                                  │    │
│   │           [ QR CODE ]            │    │
│   │                                  │    │
│   │                                  │    │
│   └──────────────────────────────────┘    │
│                                            │
│     Verification phrase                    │
│   ┌──────────────────────────────────┐    │
│   │   tiger · river · marble · clay  │    │
│   └──────────────────────────────────┘    │
│                                            │
│   Your phone will show this same phrase    │
│   before connecting. If it doesn't match,  │
│   tap Cancel on your phone.                │
│                                            │
│   ⏱  Expires in 4:48                       │
│                                            │
│                              [   Cancel   ]│
└────────────────────────────────────────────┘
```

Notes:
- QR is the primary visual; verification phrase is secondary but prominent.
- Helper copy below the phrase explains *why* it exists — most users won't read it, but the explanation is there for the ones who do, and it builds trust for the ones who later wonder.
- Countdown timer to reinforce that the code is short-lived.
- No "what is this" intro modal — the action is the explanation.

### 7.2 Step 2 — Receiving

Once the phone scans and posts:

```
┌────────────────────────────────────────────┐
│                                            │
│         Receiving from BodyLog…             │
│                                            │
│   ┌──────────────────────────────────┐    │
│   │                                  │    │
│   │       (animated chevron)         │    │
│   │                                  │    │
│   └──────────────────────────────────┘    │
│                                            │
│   Waiting for confirmation on your phone.  │
│                                            │
│   Make sure the verification phrase on     │
│   your phone matches:                      │
│                                            │
│     tiger · river · marble · clay          │
│                                            │
│                              [   Cancel   ]│
└────────────────────────────────────────────┘
```

Two seconds at most before this transitions — but during those seconds, the verification phrase stays visible so the user can compare without thrashing.

### 7.3 Step 3 — Confirm install

Once the daemon validates the payload:

```
┌────────────────────────────────────────────┐
│                                            │
│              Connect BodyLog                │
│                                            │
│   BodyLog is ready to be available in your  │
│   AI apps. Choose which apps can use it:   │
│                                            │
│   ┌──────────────────────────────────┐    │
│   │ ☑  Claude Desktop                │    │
│   │    Detected                      │    │
│   │    Permission: All tools  ⌄      │    │
│   ├──────────────────────────────────┤    │
│   │ ☑  Cursor                        │    │
│   │    Detected                      │    │
│   │    Permission: All tools  ⌄      │    │
│   ├──────────────────────────────────┤    │
│   │ ☐  Continue                      │    │
│   │    Not detected                  │    │
│   └──────────────────────────────────┘    │
│                                            │
│   You can change these later in Bridge.    │
│                                            │
│              [  Cancel  ]   [  Connect  ]  │
└────────────────────────────────────────────┘
```

Notes:
- "All tools" is the default permission — progressive disclosure for users who want to restrict. Tapping `⌄` reveals a per-tool checklist.
- Apps not detected are listed but unchecked, with helper text — clarifies *why* the box is empty rather than implicitly hiding the option.
- The primary action is "Connect," not "Install." The word "install" suggests something invasive; "connect" matches the user's mental model.

### 7.4 Step 4 — Success

```
┌────────────────────────────────────────────┐
│                                            │
│                  ✓                         │
│                                            │
│         BodyLog is now connected            │
│                                            │
│   Available in Claude Desktop, Cursor.     │
│                                            │
│   You may need to restart these apps to    │
│   start using BodyLog.                      │
│                                            │
│   ┌──────────────────────────────────┐    │
│   │  ↻  Restart Claude Desktop       │    │
│   │  ↻  Restart Cursor               │    │
│   └──────────────────────────────────┘    │
│                                            │
│                              [   Done    ] │
└────────────────────────────────────────────┘
```

Restart buttons invoke the OS to relaunch each AI app (via Tauri shell + OS-specific relaunch). If restart can't be automated for a given client, the row reads "Quit and reopen Cursor manually" — honest about the limit.

### 7.5 Pair error states

One screen template, five concrete copies — one per failure cause:

```
┌────────────────────────────────────────────┐
│                                            │
│            Couldn't pair BodyLog            │
│                                            │
│   {ERROR-SPECIFIC SENTENCE}                │
│                                            │
│   {WHAT-TO-DO SENTENCE}                    │
│                                            │
│                                            │
│   [ Show technical details ⌄ ]             │
│                                            │
│              [  Cancel  ]   [  Try again  ]│
└────────────────────────────────────────────┘
```

| Cause | Headline copy | Action |
|---|---|---|
| Token expired | "The setup code expired. They're only valid for 5 minutes." | Try again → new code |
| Network unreachable | "Bridge couldn't reach BodyLog. Make sure your phone and computer are on the same Wi-Fi." | Try again |
| Verification mismatched | "The verification phrase didn't match. Someone may be trying to intercept this connection. Try again on a trusted network." | Try again on a different network |
| Cancelled on phone | "You cancelled on your phone." | Try again |
| AI app config locked | "Bridge can't update Claude Desktop's settings — the app may be running. Quit Claude Desktop and try again." | Try again |

The "Show technical details" disclosure exposes the daemon's error code, signature failure reason, IP info — for the user who *does* want to know.

---

## 8. Console window — Servers tab

The Console is where users go for non-glance interaction. 900 × 600, standard window chrome.

```
┌──────────────────────────────────────────────────────────────────────┐
│ Bridge Console                                                  – □ x│
├──────────────────────────────────────────────────────────────────────┤
│  Servers     Activity     Settings                       [+ Add]    │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ● BodyLog                                              Used 3 min ago│
│    Available in Claude Desktop · Cursor                            ⋯ │
│                                                                      │
│  ● HomeHub                                              Used 1 h ago │
│    Available in Claude Desktop                                     ⋯ │
│                                                                      │
│  ○ Journal                                              Offline      │
│    Available in Cursor — reconnecting                              ⋯ │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

Click a server row → server detail panel slides in from the right (sheet) without leaving the page.

### 8.1 Server detail panel

```
┌──────────────────────────────────────────────────────────────────────┐
│  ← Servers                                          BodyLog           │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ● Connected · Active 3 min ago                                      │
│                                                                      │
│  Name                                                                │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ BodyLog                                                     │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  Available in                                                        │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ ✓  Claude Desktop          All tools ⌄        [ Remove ]   │    │
│  │ ✓  Cursor                  Read-only ⌄        [ Remove ]   │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  Tools exposed by this server                                        │
│  • read_body_log                                                     │
│  • start_session                                                     │
│  • stop_session                                                      │
│  • get_session_summary                                               │
│                                                                      │
│  Recent activity                                                     │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ 3 min ago · Claude Desktop · get_session_summary  · 47 ms │    │
│  │ 12 min ago · Cursor · read_body_log              · 88 ms  │    │
│  │ 2 h ago · Claude Desktop · start_session         · 120 ms │    │
│  │                                          [ See all → ]    │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  Advanced  ⌄                                                         │
│                                                                      │
│  ─────────────────────────────────────────────                       │
│  [ Remove BodyLog completely ]                                        │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

"Advanced" disclosure reveals: server's public-key fingerprint, current backend URL (after announcing), pinned cert fingerprint, last announce time, sequence counter — the technical details that satisfy security-conscious users without cluttering the default view.

The destructive action is alone at the bottom, with full confirmation:

```
┌──────────────────────────────────────────────────────────────┐
│  Remove BodyLog?                                              │
│                                                              │
│  This will remove BodyLog from Claude Desktop and Cursor.     │
│  Your phone won't need to be re-paired to add it back.       │
│                                                              │
│              [  Cancel  ]   [  Remove BodyLog  ]              │
└──────────────────────────────────────────────────────────────┘
```

After confirmation: toast at top with **Undo** for 10 s. Tag-based removal in the AI app configs (`ARCHITECTURE.md §5.4`) means undo is genuinely cheap.

---

## 9. Console window — Activity tab

Live, scrolling list. Newest first.

```
┌──────────────────────────────────────────────────────────────────────┐
│  Servers     Activity     Settings                                   │
├──────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┬─────────────┬───────────┐                          │
│  │ All servers │ All apps    │ All       │  ◀ filters                │
│  └─────────────┴─────────────┴───────────┘                          │
│                                                                      │
│  just now    BodyLog     · Claude Desktop · get_session_summary · ✓ │
│  3 min ago   BodyLog     · Cursor         · read_body_log       · ✓ │
│  3 min ago   BodyLog     · Cursor         · read_body_log       · ✓ │
│  12 min ago  HomeHub    · Claude Desktop · list_devices        · ✓ │
│  18 min ago  HomeHub    · Claude Desktop · get_temperature     · ✓ │
│  45 min ago  Journal    · Cursor         · search_entries      · ✗ │  ← red for fail
│  …                                                                  │
└──────────────────────────────────────────────────────────────────────┘
```

Click any row → row expands to show duration, arguments preview (if verbose logging on; otherwise hidden), response status. Argument preview shows the redaction explicitly: `args: <hidden — turn on Verbose logging to see>` so the user knows the data exists but isn't logged.

Filters are pills, multi-select.

If verbose logging is on, a persistent banner sits above the list:

```
┌──────────────────────────────────────────────────────────────────────┐
│  ⚠  Verbose logging is on — tool arguments and responses are being  │
│     recorded to your computer. [ Turn off ]                         │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 10. Console window — Settings tab

Sub-tabs along the left edge.

```
┌──────────────────────────────────────────────────────────────────────┐
│  General                                                             │
│  ─────────                                                           │
│  Privacy & logging      ← left rail                                  │
│  Identity                                                            │
│  Updates                                                             │
│  About                                                               │
└──────────────────────────────────────────────────────────────────────┘
```

**General**:
- "Open Bridge automatically when I log in" — toggle (daemon already does; this is whether the Console window opens too)
- "Show recent activity in tray popover" — toggle
- "Notify me when a server reconnects after being offline" — toggle, default off

**Privacy & logging**:
- "Verbose logging" — toggle with explainer: "Records tool arguments and responses to your computer. Use this when diagnosing a problem. Auto-turns off after 1 hour." (Default duration; selectable 15 min / 1 h / 4 h.)
- "Pause discovery" — toggle. Stops Bonjour announce subscription so Bridge does not broadcast or listen for new server announces on the LAN. Existing connections keep working. Use on hostile networks (hotels, conferences). Helper: "Stops Bridge from announcing or discovering servers on this network. Already-connected servers keep working."
- "Outbound connections" — opens the auditor view (last 24 h of every outbound connection the daemon made: destination, purpose, byte counts, count). The user-facing "trust through visibility" surface.
- "Copy diagnostics" — button. Produces the redacted bundle from `DAEMON.md §8.2`. Shows a preview of what's being included so the user can review before pasting anywhere.
- "Open logs folder" — link
- "Update channel" lives under Updates; "Check manually only" toggle disables the daily update check.

**Identity**:
- Display name (the name the user's phone sees when pairing): editable text field
- Rotate identity — danger button with strong copy: "This will disconnect all your paired phones. You'll need to re-pair each one. Use this if you think someone else has gained access to this computer."

**Updates**:
- Current version
- Last checked, "Check now" button
- Update channel (Stable / Beta) — radio
- Auto-apply updates — toggle

**About**:
- Version, build hash (small)
- Source link, license
- "Credits" reveal — Bits UI, Tauri, etc.

---

## 11. Empty states

Each empty state is an opportunity to *teach*. Avoid "Nothing here" sterility.

### 11.1 No servers paired yet (first launch, no deeplink)

```
┌──────────────────────────────────────────────────────────────────────┐
│                                                                      │
│                          [ illustration ]                            │
│                                                                      │
│                       No servers connected yet                       │
│                                                                      │
│        Bridge connects servers on your phone to AI apps on this      │
│        computer. To start, open the app on your phone that has a     │
│        server (like BodyLog) and tap "Connect to computer."           │
│                                                                      │
│                          [ + Add server ]                            │
│                                                                      │
│                       What does Bridge do? →                         │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

### 11.2 No activity yet

```
            No activity yet

  As your AI apps use your servers, calls
  will appear here in real time.
```

### 11.3 Daemon unreachable

A banner across the top of every window, not an empty state per se:

```
┌──────────────────────────────────────────────────────────────────────┐
│  ⚠  Bridge service stopped — your AI apps can't reach your servers  │
│     right now.                              [ Restart Bridge ]      │
└──────────────────────────────────────────────────────────────────────┘
```

Background: the daemon auto-restarts via launchd / systemd / Scheduled Task. The "Restart Bridge" button forces an immediate restart attempt. If the daemon is genuinely broken, the button reveals a "Show error details" disclosure with the supervisor's last error.

---

## 12. Error states and copy reference

Errors that the user might see, with the exact recommended copy.

| Situation | Copy |
|---|---|
| Backend offline (phone asleep / Wi-Fi off) | "BodyLog is offline. It will reconnect automatically when your phone is reachable." |
| Cert mismatch | "BodyLog's certificate changed unexpectedly. This could mean the connection is being intercepted. [ Show details ] [ Re-pair BodyLog ]" |
| Token expired | "This setup code expired. They're only valid for 5 minutes. [ Get a new code ]" |
| Adapter config schema drift | "Bridge can't update Claude Desktop's settings — its file format changed. [ Re-attach Claude Desktop ]" |
| Loopback port collision | "Another app is using port 8765. Bridge switched to 8766 and updated your AI apps." (info toast, not blocking) |
| Update available | "Bridge 0.2.0 is ready to install. [ See what's new ] [ Install on next launch ]" (toast, dismissable) |
| Identity rotated | "Bridge identity rotated. Re-pair your phones to reconnect." (persistent banner until all phones re-pair) |
| Daemon won't start | "Bridge service won't start. [ Show error ] [ Reinstall Bridge ]" |

---

## 13. First-launch onboarding

Two paths.

### 13.1 Installed via deeplink (token present)

The installer carries the token (filename suffix or post-install URI handoff). Bridge launches directly into Pair Step 1 with the QR + verification phrase. A small toast top-right:

> Welcome to MCP Bridge. Finish setting up BodyLog to get started.

No multi-step welcome. The user is already mid-flow; respect that.

### 13.2 Installed standalone (no token)

A single welcome screen, not a wizard:

```
┌──────────────────────────────────────────────────────────────────────┐
│                                                                      │
│                          [ illustration ]                            │
│                                                                      │
│                       Welcome to MCP Bridge                          │
│                                                                      │
│       Bridge lets servers on your phone — like BodyLog — talk to      │
│       AI apps on your computer (Claude Desktop, Cursor, and more).   │
│                                                                      │
│       Pairing is one-time per server. Your data stays on your        │
│       devices.                                                       │
│                                                                      │
│                       [ Add your first server ]                      │
│                                                                      │
│                             Skip for now                             │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

"Add your first server" → Pair Step 1.
"Skip for now" → Console window with the empty state from §11.1.

Pre-flight for permissions: a single sheet *before* the OS Local Network prompt fires, explaining why:

```
┌──────────────────────────────────────────────────────────────────────┐
│                                                                      │
│   Allow Bridge to use your local network                             │
│                                                                      │
│   Your computer will ask for permission to communicate on your       │
│   local Wi-Fi. Bridge needs this to discover servers on your phone.  │
│                                                                      │
│   Your data, tool calls, and activity stay on your devices. The      │
│   only outside connection Bridge makes is a once-a-day check for     │
│   its own updates, which you can turn off in Settings.               │
│                                                                      │
│                              [   Continue   ]                        │
└──────────────────────────────────────────────────────────────────────┘
```

This pre-flight is the single most important UX detail of first-launch. The macOS Local Network permission prompt is *infamously* opaque; users routinely deny it then complain that the app doesn't work. The pre-flight tells the user what's about to happen, gives them confidence, and explains why "do not send to the internet" matters.

---

## 14. Notifications strategy

In-app vs OS notifications — different bars for different reasons.

**In-app toasts only**:
- Action confirmations: "BodyLog removed" + Undo
- Background pair completed: "BodyLog is now connected" (when the user wasn't in the Pair window)
- Update available: shown in tray popover header *and* in toast on next Console open

**OS notifications** (delivered through `tauri-plugin-notification`, banner-style):
- A server's cert changed unexpectedly (security event, deserves interrupt)
- Daemon won't start (the user's stuff is broken)
- Pair completed *if* the Pair window was closed before completion (rare but possible if user dismissed)

**No notifications for**: regular tool calls (would be a flood), normal reconnects, regular updates not flagged urgent.

Notifications all carry a "Don't show this again" affordance.

---

## 15. Microcopy reference

A short pass through tone and word choice.

| Surface | Don't | Do |
|---|---|---|
| Primary CTA | "Install" | "Connect" |
| Pair confirmation | "Authorize Origin pubkey" | "Allow BodyLog?" |
| Status: working | "Reachable" | "Connected" or "Active" |
| Status: not working | "Unreachable" | "Offline" |
| Destructive | "Revoke pin" | "Remove BodyLog" |
| Helper after destructive | "The pin will be deleted from the registry" | "You can add it back without re-pairing your phone" |
| Settings: identity | "Resolver public key" | "Bridge identity" |
| Settings: rotation | "Rotate Ed25519 keypair" | "Reset this computer's identity" |
| Cert change warning | "Backend TLS fingerprint mismatch detected" | "BodyLog's certificate changed unexpectedly. This could mean someone is intercepting." |
| Onboarding | "MCP servers expose tools and resources via JSON-RPC" | "Bridge lets servers on your phone talk to AI apps on your computer." |

Rule of thumb: read the string out loud. If it sounds like documentation, rewrite it. If it sounds like a sentence a person would say, ship it.

---

## 16. Accessibility

- **Keyboard navigation**: every interactive element reachable via Tab; visible focus rings; Escape always closes the current sheet/popover; arrow keys navigate lists; Enter activates.
- **Screen readers**: every icon-only button has an `aria-label`; status indicators carry text ("Connected", "Offline") in addition to color so the indicator isn't color-only; the tray popover announces server count when opened.
- **Reduced motion**: `prefers-reduced-motion` disables the tray icon pulse, the receiving-step chevron animation, the popover slide-in. Replaced with instant transitions.
- **High contrast**: `prefers-contrast: more` increases ring widths, makes status dots larger and bordered, removes vibrancy on macOS in favor of solid background.
- **Colorblind**: status indicators always pair color with shape — solid filled circle for connected, hollow circle for offline, triangle for needs-attention. The dot is redundant with the helper text.

---

## 17. Motion and animation

The full motion budget for v1:

| Element | Motion | Duration | Reduced-motion alternative |
|---|---|---|---|
| Tray popover open/close | Fade + slight slide-from-top | 180 ms | Instant fade |
| Tray icon pairing pulse | Opacity 1 → 0.6 → 1 | 1200 ms loop | Static (no pulse) |
| Tray icon activity pulse | Opacity 1 → 0.7 → 1 (one shot) | 200 ms | None |
| Pair window step transitions | Cross-fade | 220 ms | Instant |
| Server detail slide-in | Slide from right edge | 240 ms | Instant |
| Toast appear/disappear | Slide + fade | 200 ms | Fade only |
| List row expand (server detail / activity expand) | Height + fade | 200 ms | Instant |

Nothing animates longer than ~250 ms. Nothing autoplays beyond the pairing pulse. Nothing emphasises change for change's sake.

---

## 18. Open UX decisions

1. **Verification phrase: 4 words or 6 digits?** Words are easier to verify aloud and harder to mistype. Digits are shorter on screen. Recommend: 4 words, prove out with users; 6 digits is the v2 alternative if users find words awkward to scan against the phone.
2. **Tray vs Console as the primary touchpoint.** Tray is faster, Console is more powerful. Default: tray is primary, Console opens explicitly. Validate by usage telemetry post-launch — except there's no telemetry, so validate by user feedback.
3. **Should the tray popover show recent activity inline?** Adds glanceability vs. clutter. Recommend: off by default, toggle in Settings.
4. **First-launch welcome screen vs go-straight-to-pair.** Currently: welcome only when no deeplink. Possible alternative: always welcome, deeplink users see the welcome behind the Pair window so they can navigate back if curious. Recommend: keep the current split — respect the user's flow when they have one.
5. **How to surface "Bridge is paused" state in the tray icon.** The current design has no icon variant for "paused all servers." Probably needs a small bar/dot overlay similar to "needs attention" but distinct.
6. **Per-tool ACL visibility.** Settings panel exposes "Permission: All tools / Read-only / Custom." Custom opens a checklist. The question: do we ship Custom in v1 or hide it behind a power-user toggle? Recommend: hide behind a "Show advanced permissions" toggle in Settings; most users never go below "All tools / Read-only."
7. **Onboarding illustration vs no illustration.** A welcoming illustration in empty states warms the product up but costs design time and adds platform-feel friction (illustrations are noticeably brand-y on macOS where the platform aesthetic is restrained). Recommend: minimal line-art monochrome only, not full color.

---

## 19. Status

Not committed. This document is a UX design proposal that fleshes out the surfaces sketched at the technical level in [`UI.md`](UI.md). Build phase plan and effort estimates live in [`ARCHITECTURE.md §11`](ARCHITECTURE.md).
