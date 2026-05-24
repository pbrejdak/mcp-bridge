# 0005 — Path B: per-platform native icon sets

- **Status**: Accepted
- **Date**: 2026-05-23
- **Deciders**: Project founders
- **Supersedes**: —
- **Related**: [`UI.md`](../UI.md) §3, §8, [0002](0002-tauri-over-electron.md)

## Context

[Bridge Console](../GLOSSARY.md#bridge-console) is a menu-bar / tray application whose entire reason to exist is to feel like part of the host operating system. Icons appear in dense, OS-adjacent contexts: tray status, table rows, toolbar buttons, inline within copy. A user's eye reads these icons against the OS's own iconography — Finder, System Settings, Control Panel, GNOME Files — not against a generic web-app vocabulary.

Three approaches are mainstream:

- **Path A — single cross-platform icon set** (e.g., Lucide, Phosphor, Heroicons everywhere). One source of truth; consistent look; trivially licensed; immediate.
- **Path B — per-platform native icon sets**: SF Symbols on macOS, Fluent on Windows, an idiomatic GTK set on Linux. Multiple sources; matches OS conventions per platform; more work.
- **Path C — bespoke icon set drawn in-house** for Bridge specifically. Maximum brand control; minimum platform integration; significantly more design work.

The constraint set:

- The app is small. We touch dozens of icons, not hundreds.
- We have one UI codebase across three platforms ([0002](0002-tauri-over-electron.md)), but per-platform CSS branches and a small `platform.os` capability already exist.
- We want the tray icon for macOS to look like every other tray icon on macOS, the toolbar icons in the Console window to feel like System Settings, and on Windows we want Fluent-flavored chevrons and Mica-friendly strokes — not a generic web look glued to a native chrome.
- Licensing: SF Symbols requires the macOS / iOS SDK and has Apple's well-known usage restrictions; @fluentui/svg-icons is MIT; @lucide/svelte is ISC.

## Decision

Adopt **Path B**: ship per-platform icon sets behind a single `<Icon name="…" />` Svelte component that picks the right source at runtime based on `platform.os`.

| Platform | Source | License |
|---|---|---|
| macOS | SF Symbols via a Tauri command (NSImage → base64 PNG) | Apple SDK terms |
| Windows | `@fluentui/svg-icons` | MIT |
| Linux | `@lucide/svelte` (idiomatic, ISC-licensed fallback) | ISC |

A single `IconName` type union enumerates every name the app uses; each platform supplies its own mapping. CI catches a platform that is missing a mapping for a referenced name.

## Alternatives considered

- **Path A — single cross-platform set everywhere** — the safe, fast choice. Rejected because Bridge Console is supposed to feel native, not "a web app pretending to be native." A consistent Lucide-everywhere look would be more coherent within the app than per-platform sets, but distinctly *less* coherent with the OS the user is actually using. Wrong tradeoff for a menu-bar utility.
- **Path C — bespoke icon set** — disproportionate design investment for a UI surface that pre-1.0 is small enough to be drawn by a single designer in a weekend. Maximum control but the wrong direction: we want to disappear into the OS, not assert brand.
- **SF Symbols everywhere via re-licensed PNG dumps** — Apple's usage terms preclude this; not a viable path.

## Consequences

What this enables:

- **Each platform's tray icon and toolbar imagery looks at home.** A macOS user sees SF Symbols rendered through the same `NSImage` pipeline the OS uses; a Windows user sees Fluent geometry next to their Mica backdrop.
- **CI catches missing per-platform mappings** before they ship.
- **Lower attentional cost for the user** — the icons read as "this thing speaks my OS," which is the only thing Bridge Console is trying to say at a glance.

Costs we accept:

- **Three sources to track.** Each icon name has to exist in three vocabularies; not every concept maps 1:1. When a needed icon is missing from one set we approximate with the closest available glyph.
- **Build-time complexity on macOS.** SF Symbols rendering requires a Tauri command on the Rust side. We pay the complexity once, in `Icon.svelte` and one Tauri command.
- **Per-platform visual review.** A PR that adds an icon must visually pass on all three platforms; design QA is heavier than Path A would impose.
- **Licensing care for any future Apple-platform redistribution** — SF Symbols cannot be redistributed outside Apple-platform contexts. The Tauri command on macOS renders them through Apple's APIs, which is compliant with current SF Symbols terms.

What would force a revisit:

- A future port to a platform without an obvious native icon set we can adopt cleanly. We would extend Path B with a fourth mapping rather than abandoning it.
- A significant change in SF Symbols licensing.
