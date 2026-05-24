# `mcp-bridged` — Claude working agreement

This file captures the conventions Claude must follow when writing or modifying code in this crate. It complements (not duplicates) [`docs/CONTRIBUTING.md`](../docs/CONTRIBUTING.md) and the spec/architecture documents.

**Read first when touching this crate:**
- The spec section your subsystem implements — [`docs/SPEC.md`](../docs/SPEC.md) for wire protocols, [`docs/DAEMON.md`](../docs/DAEMON.md) for daemon internals.
- The threat model row your change interacts with — [`docs/THREAT-MODEL.md`](../docs/THREAT-MODEL.md).

## 1. Philosophy

- **Make invalid states unrepresentable.** Newtype-wrap every ID (`LogicalId`, `PinId`), every encoded form (`Ed25519Pubkey`, `Base64Url`), every secret (`BearerToken`, `LoopbackKey`). The compiler then enforces correctness; runtime asserts shouldn't be necessary.
- **Trust internal callers; validate at trust boundaries.** Re-validate only when bytes enter from network, filesystem, IPC, or user input. Don't re-check what crossed a typed API.
- **No half-implementations.** A function either fully implements its contract or returns a typed error. `todo!()` is for the scaffold only; remove it as you wire each subsystem.
- **Pre-1.0 breaking changes are allowed but flagged.** Per [`docs/CONTRIBUTING.md`](../docs/CONTRIBUTING.md) §3.4. Don't add backwards-compat shims that have no purpose yet.

## 2. Module organization

- **One concept per file.** `Sas`, `Invite`, `Payload` each get their own file under `pair/`. `mod.rs` re-exports.
- **Mirror [`docs/DAEMON.md`](../docs/DAEMON.md) §3.** If you'd add a new module, check the layout doc first.
- **Visibility defaults to `pub(crate)` or private.** Mark `pub` only when something outside the crate consumes it. The `unreachable_pub` lint catches over-exposure.
- **Unit tests live in `#[cfg(test)] mod tests` at the bottom of each source file.** Cross-module integration tests live in `mcp-bridged/tests/`.
- **No deep nesting.** >3 module levels means reshape.

## 3. Error handling

- **`thiserror` per subsystem.** Each module exports its own `Error` enum (`pair::Error`, `announce::Error`, `proxy::Error`). Variants carry structured fields, not `String` payloads.
- **`anyhow::Result` only at the binary boundary** (`main.rs`, the CLI surface). Library code returns concrete errors.
- **Use `?` with `.context("…")`** at each level the error crosses a subsystem boundary so the chain is readable.
- **No `unwrap` / `expect` in production paths.** Acceptable in `#[cfg(test)]` and at startup in `main.rs` where panic is the right behaviour.
- **`Result<T, E>` for fallibility; `Option<T>` only for genuinely-absent values.** Don't conflate the two.
- **No `Box<dyn Error>` in our APIs.** Concrete enum or `anyhow::Error` with context.

## 4. Async & tokio

- **One tokio multi-thread runtime per process.** Created in `main.rs`.
- **Every long-lived task takes a `tokio_util::sync::CancellationToken` clone** and observes it via `select!`. Graceful shutdown signals the root token.
- **Tasks are owned by a supervisor** ([`docs/DAEMON.md`](../docs/DAEMON.md) §4). No bare `tokio::spawn` outside the supervisor.
- **Bounded channels for backpressure.** `mpsc::channel(N)`, never `unbounded_channel()`. Activity events use `broadcast` (slow consumers drop, not block).
- **Never block in async.** Filesystem reads, JSON parsing of large blobs, cryptographic signing — wrap with `tokio::task::spawn_blocking`.
- **Don't hold a sync mutex across `.await`.** Clippy enforces this; if you bypass it, justify in a `// SAFETY: …` style note.
- **Avoid `Arc<Mutex<T>>` as a default sharing primitive.** Prefer message-passing. When shared state is genuinely needed, use `parking_lot::Mutex` for sync or `tokio::sync::RwLock` if held across `.await`.
- **`select!` with a cancellation arm is the idiomatic shape.** No polling loops.

## 5. Memory hygiene & secrets

This is the highest-stakes section. Get every rule right.

- **`secrecy::SecretBox<T>` wraps any in-flight secret.** Bearer tokens, loopback keys, unwrapped pair-payload bytes. Forces `.expose_secret()` at the use site — every exposure is searchable in `git grep`.
- **`#[derive(zeroize::Zeroize, zeroize::ZeroizeOnDrop)]` on every type that stores sensitive bytes at rest.** Don't rely on Rust's drop semantics for secret cleanup.
- **`Box<[u8; N]>` for fixed-size keys.** Stack arrays can be moved or copied by the optimizer, defeating `Zeroize`. Heap-allocated arrays cannot.
- **No `#[derive(Debug)]` on secret types.** Implement `Debug` and `Display` manually to print `Secret(redacted)`. The auto-derive is a leak waiting to happen.
- **Constant-time compare every secret.** `constant_time_eq` for byte slices, `subtle::ConstantTimeEq` for fancier comparisons. Never `==` for tokens, MACs, signatures, loopback keys.
- **Verify signatures *before* parsing inner payloads.** A malformed signed payload must fail at signature check, never at downstream JSON parsing — that ordering is part of the security argument.
- **No `unsafe`** (denied at the workspace level). Where genuinely needed, `#[allow(unsafe_code)]` per item with a `// SAFETY:` paragraph stating the invariant in plain English.
- **Core dumps disabled** for the daemon at startup (RLIMIT_CORE on Unix, equivalent on Windows). Per [`docs/DAEMON.md`](../docs/DAEMON.md) §7.3.
- **Never log `Authorization` headers, loopback `?key=`, bearer tokens, or pair-payload bodies** at any verbosity. The redaction layer in `observability/` is the enforcement point — don't add ad-hoc redaction at call sites.

## 6. Cross-platform & native targets

The daemon ships on macOS, Windows, and Linux. Code is correct only when it works on all three.

- **`directories::ProjectDirs` for every filesystem path.** Never hardcode `~/Library/Application Support/…` or `%APPDATA%\…`. Use `config_dir()`, `data_dir()`, `cache_dir()`, `runtime_dir()`.
- **Per-OS dependencies go in `[target.'cfg(...)'.dependencies]`.** The frozen-for-v1 dep list in [`docs/DAEMON.md`](../docs/DAEMON.md) §12 shows the pattern. Don't add a Unix-only crate to `[dependencies]`.
- **OS keychain via the `keyring` crate.** Wraps macOS Security Framework, Windows DPAPI/Credential Vault, Linux Secret Service. Failure modes differ across platforms — wrap with our own `KeystoreError`.
- **mDNS / Bonjour lives behind a platform-trait abstraction.** macOS: `astro-dnssd` (Apple-native). Linux + Windows: `zeroconf`. Open architectural decision per [`docs/DAEMON.md`](../docs/DAEMON.md) §14 #1.
- **Signals**: `tokio::signal::ctrl_c()` portable; `tokio::signal::unix::signal()` for SIGTERM/SIGHUP; `tokio::signal::windows` for Windows-specific.
- **File permissions**: `std::os::unix::fs::PermissionsExt` for mode `0600` on Unix; ACL APIs on Windows (`windows::Win32::Security`). Files we write must be user-only.
- **Paths are `Path` / `PathBuf`**, never `String`. UTF-8 is not guaranteed on either platform.
- **TLS via `rustls` only.** No `openssl`, no `native-tls`. Avoids per-OS crypto-library quirks and the OpenSSL version dance.
- **Loopback binds `127.0.0.1`** (and optionally `::1`). Never `[::]` or `0.0.0.0`.
- **`.gitattributes` locks `test-vectors/*` to LF line endings.** The pinned wordlist SHA-256 assumes LF. Don't add a Windows-line-ending fixture.
- **User-facing strings**: "Open Bridge Console", not "open the menu bar icon" — the menu bar doesn't exist on Windows.
- **`cfg_if::cfg_if!` for multi-line per-OS branches**; plain `#[cfg(...)]` for single statements.
- **Cross-test platform-specific code with conditional integration tests.** `#[cfg(target_os = "macos")] mod macos_tests;` etc. CI matrix should run them; today CI is Linux-only and that's a known gap (see ROADMAP).

## 7. Testing

- **Async tests via `#[tokio::test]`**, `flavor = "multi_thread"` when real concurrency is exercised.
- **`tokio::time::pause()` for time-based tests.** Never `sleep` in tests; advance virtual time.
- **Spec conformance tests are JSON-driven.** Fixtures in [`test-vectors/`](../test-vectors/), deserialized by `serde_json`. One test function per fixture or per (fixture, expected-outcome) pair.
- **Determinism is mandatory.** Seed RNGs with a fixed value (`rand::rngs::StdRng::seed_from_u64(_)`). Flaky tests are real bugs.
- **No external network in tests.** Tests must run offline. Use `wiremock` for HTTP mocks; mock mDNS and keychain via the platform-traits.
- **`proptest` for parsers, canonicalizers, and any input-driven invariant.**
- **`insta` for snapshot testing** when stable serialized output matters (registry JSON shape, log redaction output).

## 8. Documentation

- **Default: no comments.** Per [`docs/CONTRIBUTING.md`](../docs/CONTRIBUTING.md) §9.
- **`//!` module-level docs link to the spec section the module implements** — e.g. "Implements [`docs/SPEC.md`](../docs/SPEC.md) §5.5."
- **No PR or issue references in code.** That metadata belongs in commit messages.

## 9. Logging & observability

- **`tracing` over `log`.** Structured fields, not formatted messages: `tracing::info!(pin_id = %pin.id, "loaded")`, not `info!("loaded {}", pin.id)`.
- **One span per logical operation**, entered via `.instrument(span)` for async paths.
- **Levels**: `error!` = user must see; `warn!` = recoverable but suspicious; `info!` = noteworthy state changes; `debug!` = developer; `trace!` = firehose.
- **Centralized redaction** ([`docs/DAEMON.md`](../docs/DAEMON.md) §8.2) — don't redact at call sites. Adding a new field that could contain user data means updating the redaction policy table in the same PR.
- **Verbose mode is opt-in, time-limited, and shows a persistent UI banner** while on. Don't add code paths that bypass these guarantees.

## 10. Cargo & dependencies

- **Workspace inheritance for shared fields** — `version.workspace = true`, etc. Don't restate.
- **MSRV pinned in [`rust-toolchain.toml`](../rust-toolchain.toml)** (currently 1.85.0). Don't use features newer than MSRV without bumping it (and updating [`docs/DAEMON.md`](../docs/DAEMON.md) §12).
- **Dependency intake follows the [`docs/CONTRIBUTING.md`](../docs/CONTRIBUTING.md) §6.1 checklist** — network behaviour, license, lighter alternative, activity, [`NOTICE`](../docs/NOTICE) entry. CI's `cargo deny` enforces license + advisory.

## 11. What we do not do

These would all silently land if not explicitly forbidden — list exists to make the answer "no" easy.

- **No telemetry, analytics, crash reporting, or "anonymous metrics."** Not now, not later. Hard rule from [`docs/PRIVACY.md`](../docs/PRIVACY.md).
- **No new outbound network connections** without an RFC + updated [`docs/ARCHITECTURE.md`](../docs/ARCHITECTURE.md) §6.1 + UI exposure.
- **No `unsafe`** (workspace lint denies).
- **No `unwrap` / `expect` in production paths.**
- **No `openssl`** — `rustls` only.
- **No `Box<dyn Error>` across our APIs.**
- **No global mutable state.** Pass dependencies; supervise tasks.
- **No `Arc<Mutex<T>>` as the default sharing primitive.** Reach for message-passing first.

## See also

- [`docs/SPEC.md`](../docs/SPEC.md) — normative wire-protocol grammar (the contract).
- [`docs/DAEMON.md`](../docs/DAEMON.md) — daemon design (the implementation guide).
- [`docs/THREAT-MODEL.md`](../docs/THREAT-MODEL.md) — adversaries each subsystem must resist.
- [`docs/CONTRIBUTING.md`](../docs/CONTRIBUTING.md) — review process, DCO, dependency intake.
- [`docs/decisions/`](../docs/decisions/) — load-bearing architectural decisions.
