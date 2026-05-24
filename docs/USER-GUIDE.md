# MCP Bridge — User Guide

This guide is for people *using* MCP Bridge — not the developers who build it.

If you want to know how Bridge works inside, see [`ARCHITECTURE.md`](ARCHITECTURE.md) and [`SPEC.md`](SPEC.md). If you are reporting a security issue, see [`SECURITY.md`](SECURITY.md).

> **Status**: this guide covers Bridge v0.x while the project is in pre-1.0. Screens and copy match the design in [`UX.md`](UX.md); a few small details may differ from the shipped build until v1.0.

---

## 1. What Bridge does, in one paragraph

You install **MCP Bridge** once on your computer. After that, any app on your phone that hosts an AI-compatible server — a fitness tracker, a smart-home hub, a journal — can talk to AI apps on your computer (Claude Desktop, Cursor, Continue, others) by scanning a QR code. Bridge keeps the connection alive as your phone changes Wi-Fi networks, restarts, or rotates its security keys. Your data stays on your devices.

---

## 2. Connect your first server

The whole flow takes about a minute. You need:

- The app on your phone that has the server you want to use (for example, BodyLog).
- Either AirDrop, an iCloud-shared clipboard, the same Wi-Fi network, or a webcam on your computer — any one of these is enough to move a short link from phone to computer.

### Step 1 — On your phone

Open the app and tap **Connect to computer** (the exact label varies by app).

The app will show:

- A short setup link like `mcpbridge.me/p/abc123`.
- A QR code containing the same link.
- A **Send to Mac / Send to PC** button.

### Step 2 — Move the link to your computer

Pick whichever method is easiest for you:

| Method | What to do |
|---|---|
| **AirDrop (iPhone to Mac)** | Tap **Send to Mac**, pick your Mac. Safari opens the link. |
| **Universal Clipboard** | Tap **Copy** on the phone, then ⌘V into a browser address bar on your Mac. |
| **QR code with computer webcam** | Use your laptop's built-in QR scanner or any QR app on your computer to open the link. |
| **Type the link** | About six characters — `mcpbridge.me/p/` + the short token. |
| **Email / SMS to yourself** | Tap **Send via…**, send to yourself, open on your computer. |

The link on your computer opens to one of two pages.

**If Bridge is already installed**: Bridge's pair window opens automatically. Skip to Step 4.

**If Bridge is not installed yet**: the page offers a download for your operating system. Continue with Step 3.

### Step 3 — Install Bridge (first server only)

1. Click the download button for your OS.
2. Open the installer when it finishes downloading.
3. Approve the OS's permission prompts:
   - **macOS**: Gatekeeper ("downloaded from the internet"); Local Network access.
   - **Windows**: SmartScreen; the Windows Firewall prompt for the local network.
   - **Linux**: distro-specific (varies; the installer guides you).
4. Bridge launches automatically and opens its pair window on the same setup link from Step 1. You do not need to re-scan or re-send anything.

You install Bridge once. From this point on, adding more servers (from the same app or a new one) skips Step 3.

### Step 4 — Confirm and install into your AI apps

The pair window on your computer shows a QR code and a four-word **verification phrase** (e.g. `tiger-river-marble-clay`).

On your phone, the app you started in Step 1 is now showing the same verification phrase together with the name of your computer ("Pair BodyLog with Patryk's MacBook Pro? Verification: `tiger-river-marble-clay`").

**Look at both screens. The phrases must match exactly.**

- ✅ Same four words, same order → tap **Confirm** on the phone.
- ❌ Different words, or the phone shows a verification phrase you cannot find on your computer → **tap Cancel on the phone.** Something is wrong — see [§7 If the verification phrase doesn't match](#7-if-the-verification-phrase-doesnt-match) below.

After you confirm, the pair window on your computer asks which AI apps should be able to use this server:

```
BodyLog wants to be available in:
  ☑ Claude Desktop
  ☑ Cursor
  ☐ Continue
[Install]
```

Untick anything you don't want, then click **Install**. Bridge writes the connection into each AI app's settings.

### Step 5 — Restart the AI app

Most AI apps re-read their settings on the next launch. The pair window will show:

> ✓ BodyLog is ready. Restart Claude to use it.
> [ Restart Claude ] [ Close ]

Click **Restart Claude** (or quit and reopen the app manually). The next time you talk to Claude, BodyLog's tools appear in the picker.

That's the whole setup. You don't have to do it again for this server — even if your phone changes Wi-Fi, restarts, or rotates its tokens.

---

## 3. Daily use

After setup, Bridge is silent. You generally don't need to look at it.

### Where Bridge lives on your screen

- **macOS**: a small icon in the menu bar (top-right of your screen).
- **Windows**: a small icon in the system tray (bottom-right of your screen, possibly in the overflow popup).
- **Linux**: a tray icon if your desktop environment supports `AppIndicator` / `StatusNotifierItem`.

The icon has a few states:

| What you see | What it means |
|---|---|
| Solid icon | Everything is connected and healthy |
| Solid icon + small amber dot | At least one server is currently offline (your phone may be asleep) |
| Solid icon + small red dot | Something needs your attention (e.g., a server's security identity changed) |
| Hollow / outlined icon | Bridge service has stopped |

### Quick status — click the tray icon

A small popover opens with a list of your servers and a colored dot next to each:

- 🟢 **Connected** — ready to use.
- 🟡 **Offline** — Bridge can't reach the server right now. This is usually your phone being asleep or off Wi-Fi. Bridge will reconnect automatically when it can.
- ⚪️ **Pairing** — currently being set up.

You can click any server to see details, or click **Open Console** to see the full window with the activity feed and settings.

---

## 4. The Console window

Click **Open Console** from the tray popover, or use **Bridge → Open Console** in your OS menu bar.

The Console has three tabs.

### Servers

The list of servers you have paired with this computer. Each row shows the server's name, status, when it was last seen, and which AI apps are using it.

Click a server to see its detail panel:

- **Status** — Connected / Offline, with last-seen time.
- **Used by** — the AI apps that have this server configured.
- **Tools** — what this server makes available (read-only).
- **Identity** — a short fingerprint, in case you need to verify it later.
- **Remove** — see [§5](#5-remove-a-server).

### Activity

A real-time feed of tool calls your AI apps make to your servers. Each row shows the timestamp, the AI app that made the call, the server, and the tool name. Tool arguments and results are **not** shown unless you have enabled Verbose mode in Settings (see [§8](#8-verbose-mode-troubleshooting-only)).

The activity feed is **in memory only** by default — closing Bridge clears it. You can opt in to a 7-day persistent feed in Settings if you want a history.

### Settings

Where you change Bridge's behavior. The notable sections:

- **General** — startup behavior, theme.
- **AI apps** — which AI apps Bridge knows how to write to. You can re-attach an AI app here if its config file moved.
- **Privacy** — the activity-feed retention, Verbose-mode toggle, "Pause discovery" switch, update-channel preferences, "Copy diagnostics" button.
- **Identity** — Bridge's own cryptographic identity. Includes a "Rotate identity" button — see [§9](#9-bridge-has-been-compromised--rotate-identity).
- **About** — version, links to source, license, security policy.

---

## 5. Remove a server

In the tray popover or the Servers tab, click the server, then click **Remove**.

You'll see a confirmation toast at the bottom of the window:

> Removed BodyLog. [ Undo ]

You have **10 seconds** to undo. After that, Bridge:

- Removes the server's entry from every AI app's settings.
- Deletes the keys and tokens stored for that server.
- Stops listening for that server on your network.

The next time you restart the affected AI apps, the server is gone from their picker.

If you re-add the same server later, it will be a fresh pairing — Bridge does not keep the old keys around.

### A few subtleties

- Removing the server here does *not* uninstall the app on your phone or change anything on the phone side. It only removes the connection between that server and this computer.
- If the same server is paired with another of your computers, that pairing is independent. Each computer has its own copy. Remove on one computer does not affect any other.

---

## 6. Why is my server showing Offline?

Most of the time, "Offline" means one of these:

- **Your phone is asleep** or in low-power mode.
- **Your phone is off Wi-Fi** and the server isn't reachable on cellular.
- **You're on a guest / public Wi-Fi** that blocks device-to-device traffic.
- **The phone app's server isn't running** (background task killed, app force-quit).

Bridge keeps trying to reach the server in the background. As soon as it succeeds, the status flips back to Connected — you don't need to do anything.

### If a server stays Offline longer than expected

1. **Wake your phone** and bring it to the foreground in the app that hosts the server. Many phone apps suspend their server when backgrounded for a while.
2. **Check Wi-Fi.** Both your phone and your computer need to be on the same Wi-Fi network (or both need to be on a network where multicast / Bonjour is allowed).
3. **Check the app's own status screen.** Most apps have a "Bridge connected" indicator inside their own settings.
4. **Open the Bridge tray popover and click the server.** If the detail panel shows a specific error (e.g., "Bridge can't reach 192.168.x.x"), follow the message.

### Recovery options in the popover

When a server is Offline, the row reveals two actions:

- **Show details** — opens the detail panel with the last error and the troubleshooting hints above.
- **Re-add** — if Bridge thinks the server is gone for good, this restarts the pairing flow from scratch. Use this as a last resort; you'll lose any per-AI-app customization for that server.

---

## 7. If the verification phrase doesn't match

The four-word verification phrase exists for exactly one reason: to make sure the QR code you scanned came from *your* computer, not someone else's.

**The phrase on your phone and the phrase on your computer must be identical, word for word, in the same order.**

If they don't match, **cancel the pairing on your phone immediately**. Do not tap Confirm. Then:

1. **Close the pair window on your computer.**
2. **Start over from your phone** — open the app and tap **Connect to computer** again to generate a fresh setup code.
3. **Make sure you're using your own computer** — if someone else on your network is also running Bridge, you may have scanned their QR by accident. Look at the computer name shown on the phone next to the verification phrase; it should be your computer's name.

If the phrases still don't match on a fresh attempt, file a report through [`SECURITY.md`](SECURITY.md) — this should never happen in normal use, and we want to know about it.

---

## 8. Verbose mode (troubleshooting only)

By default, Bridge does **not** record the *contents* of your AI app's calls — only the timestamp, the server, and the tool name. Tool arguments and results are deliberately not logged.

If you are troubleshooting a tool that misbehaves, you can turn on **Verbose mode** in Settings → Privacy. Verbose mode:

- Records full tool arguments and results in the activity feed.
- Records full request/response bodies in the rolling log files.
- Shows a persistent banner across the top of every Bridge window: **Verbose mode is on**.
- Automatically turns itself off after the duration you choose (15 minutes, 1 hour, or 4 hours).

Bridge will never put you into Verbose mode without telling you. The banner is there so you cannot forget it's on.

**Authentication tokens and the per-AI-app access keys are *never* logged, in Verbose mode or out.**

---

## 9. Bridge has been compromised / rotate identity

If you have reason to believe Bridge's keys on this computer have been compromised — for example, the computer was unattended in a public place and you can't be sure no one had access — you can rotate Bridge's identity from scratch:

**Settings → Identity → Rotate identity**

The button is behind a strong-warning confirmation because it has a real cost:

- All your paired phones will show their servers as Offline until you re-pair them.
- You will need to walk through pairing again (the flow in [§2](#2-connect-your-first-server)) for each server.
- AI apps will continue to point at the same loopback URLs — no AI-app reconfiguration is needed.

After you rotate, Bridge's tray icon shows a persistent reminder until every previously paired phone has re-paired.

---

## 10. Move to a new computer

Bridge identities are intentionally tied to one computer. Pairings do not transfer — your phone has to pair with each computer separately.

To move to a new computer:

1. **On the new computer**: install Bridge (Step 3 above).
2. **For each server** you want available on the new computer: open the source app on your phone, tap **Connect to computer**, send the link to the new computer, and walk through pairing as in [§2](#2-connect-your-first-server).
3. **On the old computer** (when you no longer need it): remove each server (see [§5](#5-remove-a-server)) before retiring the machine. Or, if the computer is being wiped anyway, simply uninstall.

Both computers can be paired with the same server independently — Bridge does not require you to "transfer" anything. Each pairing is its own relationship.

---

## 11. Uninstall

The fully clean removal:

```
mcp-bridge uninstall --purge
```

`--purge` removes:

- The Bridge binary and its support files.
- The server registry (your pairings).
- All keys and tokens stored for those pairings.
- Bridge's entries in every AI app's settings (using the tag Bridge wrote alongside each entry, so other entries you have added by hand are not touched).
- Bridge's log files.
- Bridge's keychain entries.

If you uninstall through your OS's normal app-removal flow (drag to Trash on macOS, Settings → Apps on Windows, your package manager on Linux), the Bridge binary is removed but the data files may persist. The `--purge` form removes everything.

If you uninstall without purging and reinstall later, your previous pairings will still be there.

---

## 12. Privacy — what Bridge does and doesn't do

The short version:

- **Nothing about your tool calls or content leaves your computer.** Bridge does not have an "anonymous metrics" mode, an "analytics" mode, or a "crash reporting" mode. There is no telemetry.
- **Bridge does make one outside connection by default**: once a day, it asks `updates.mcpbridge.me` whether there is a Bridge update available. This request carries no identifier and no query parameters. You can turn it off in Settings → Privacy.
- **Bridge is open source.** You can verify every claim above by reading the source.

For the full statement and the threat model, see [`PRIVACY.md`](PRIVACY.md).

### Confirm what's going on right now

Open the Console → Settings → Privacy → **Outbound connections**. This lists every outside address Bridge has contacted in the current session, in real time. If anything unexpected appears there, [report it as a security issue](SECURITY.md).

### Copy a diagnostics bundle

If you need to share information with support or a developer, use **Settings → Privacy → Copy diagnostics**. This puts a redacted bundle on your clipboard containing:

- Bridge version, OS version.
- The current pair list (server names, last-seen times — not keys).
- The most recent log entries, with authentication tokens and access keys stripped out.

The redaction is best-effort, not perfect. **Look at the bundle before you paste it anywhere** — if there's anything in it you don't want to share, you don't have to share it.

---

## 13. Get help

- **Documentation**: this guide, plus the rest of [`docs/`](.).
- **Bug reports**: open a GitHub issue ([`CONTRIBUTING.md`](CONTRIBUTING.md) §2 has the template).
- **Security or privacy concerns**: do **not** open a public issue. Follow [`SECURITY.md`](SECURITY.md).
- **Asking a question**: open a GitHub Discussion (preferred — it is searchable) or check whether your question is already answered there.

---

## 14. Common questions

**Does Bridge work without internet?**
Yes, for everything except the once-a-day update check (which you can disable). Pairing, server discovery, and tool calls all run over your local network and loopback — no internet required.

**Can two computers share the same pairing with a phone?**
Each computer has its own pairing. The phone can be paired with as many computers as you want, but each pairing is set up independently. This is by design — see [`ARCHITECTURE.md`](ARCHITECTURE.md) §10.

**Does Bridge store my AI conversations?**
No. Bridge sees the tool *calls* your AI app makes to your servers — and even those are not recorded by default. The conversations themselves stay between you and your AI app; Bridge never sees them.

**Why does the AI app sometimes need to be restarted?**
The first time Bridge writes a server into an AI app's settings, most AI apps need to re-read the file. After that, you generally don't need to restart again — even when the server on your phone changes IP or rotates keys, the loopback URL stays the same.

**What if my phone is on cellular and my computer is at home?**
Bridge supports it via a fallback that doesn't depend on the local network (HTTP POST instead of Bonjour discovery). The app on your phone has to be configured to know your Bridge's address; not every app does this today. Check the source app's documentation.

**Is Bridge free?**
Yes. Bridge is open source under the Apache License 2.0 — see [`LICENSE`](../LICENSE).

---

If something in this guide is inaccurate, unclear, or out of date, please open an issue or a doc PR. The guide is meant to stay honest about what the shipped build actually does.
