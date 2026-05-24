# MCP Bridge

A privacy-first universal pairing tool that connects mobile-hosted MCP servers to desktop AI clients without per-client configuration friction.

## Status

**Exploratory.** This repository currently contains design documentation only — no code has been written yet. The documentation set is comprehensive and treats the architecture as committed for planning purposes, but every claim is aspirational until the implementation lands.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the cross-cutting design.

## What it does

You install **MCP Bridge** once on your computer. From then on, any app on your phone that hosts a Model Context Protocol server — a fitness tracker, a home-automation hub, an on-device journal — can connect to your desktop AI clients (Claude Desktop, Cursor, Continue, others) by scanning a QR code. The bridge keeps everything working as your phone changes networks, rotates auth tokens, or renews its certificate.

Your data stays on your devices. The bridge does not send anything about your tool calls or activity to the internet.

## Why

Every MCP client today implements pairing differently — different config file paths, different schemas, different reload semantics. Every mobile MCP server author re-solves the same setup ceremony, badly. Worse, a phone-resident server's identity drifts constantly (new Wi-Fi, app restart, token rotation), so even a working configuration breaks between sessions.

MCP Bridge pushes all of that complexity into one component the user installs once. Mobile MCP servers integrate one SDK; desktop AI clients are configured exactly once against a stable localhost URL. Drift becomes invisible.

## Documentation

| Doc | What it covers |
|---|---|
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Cross-cutting design — Stable-Loopback Bridge pattern, trust model, sequence diagrams |
| [SPEC.md](docs/SPEC.md) | Normative wire protocol specification (`mcp-pair/v0.1`, `mcp-announce/v0.1`) |
| [DAEMON.md](docs/DAEMON.md) | Rust daemon internals — process model, IPC surface, request flow, observability |
| [UI.md](docs/UI.md) | Tauri + Svelte 5 stack — window architecture, IPC consumer, native-feel styling |
| [UX.md](docs/UX.md) | Interface design — vocabulary, flows, screen mockups, copy, empty/error states |
| [MOBILE.md](docs/MOBILE.md) | Origin-side mobile SDK — KMP core, six packagings (Native Kotlin/Swift, KMP, React Native, Flutter, Capacitor) |
| [PRIVACY.md](docs/PRIVACY.md) | Privacy charter — threat model, data lifetimes, egress allowlist, verification paths |
| [LEGAL.md](docs/LEGAL.md) | Legal source-of-truth — licensing, ToS, trademark, export classification, GDPR / CCPA |
| [SECURITY.md](docs/SECURITY.md) | Responsible disclosure policy |
| [CONTRIBUTING.md](docs/CONTRIBUTING.md) | How to contribute — DCO sign-off, privacy-first review checklist |
| [USER-GUIDE.md](docs/USER-GUIDE.md) | End-user documentation |
| [ROADMAP.md](docs/ROADMAP.md) | Phased plan and deferred work |
| [THREAT-MODEL.md](docs/THREAT-MODEL.md) | Consolidated threat model across daemon, mobile SDK, and UI |
| [GLOSSARY.md](docs/GLOSSARY.md) | Terminology reference |
| [decisions/](docs/decisions/) | Architecture Decision Records |
| [NOTICE](docs/NOTICE) | Apache 2.0 attribution |
| [CHANGELOG.md](CHANGELOG.md) | Version history |

## Sponsorship

MCP Bridge is independently maintained. Sponsorship funds code-signing certificates, Apple notarization, and ongoing maintenance — without it, signed installers aren't sustainable.

- **[GitHub Sponsors](https://github.com/sponsors/pbrejdak)** — for individual contributors; one-click from the repo.
- **[Open Collective](https://opencollective.com/mcp-bridge)** — for organizations that need invoices, VAT handling, or a transparent expense ledger.

Commercial support and SDK integration assistance are available — contact p.brejdak@gmail.com.

## License

Licensed under the **Apache License 2.0** — see [LICENSE](LICENSE) and [NOTICE](docs/NOTICE).

## Security

For security or privacy disclosures, see [SECURITY.md](docs/SECURITY.md). **Do not open public issues for vulnerabilities.**

## Contributing

See [CONTRIBUTING.md](docs/CONTRIBUTING.md). Developer Certificate of Origin sign-off is required on every commit.

## Code of conduct

This project follows the Contributor Covenant — see [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
