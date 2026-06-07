# 0006 — KMP crypto stack for the Bridge Peer SDK

- **Status**: Accepted
- **Date**: 2026-06-07
- **Deciders**: Project founders
- **Supersedes**: —
- **Related**: [`0003-kmp-for-mobile-core.md`](0003-kmp-for-mobile-core.md), [`SPEC.md`](../SPEC.md), [`MOBILE.md`](../MOBILE.md), [`THREAT-MODEL.md`](../THREAT-MODEL.md)

## Context

[ADR 0003](0003-kmp-for-mobile-core.md) commits the [Bridge Peer](../GLOSSARY.md#bridge-peer) protocol core to Kotlin Multiplatform. That ADR picked the *language* and *packaging*; it deliberately deferred the *crypto stack*. Now we have to pick one, because the next milestone after canonical JSON is Ed25519 signing — which needs a key, which needs a backend.

The wire protocols ([`SPEC.md`](../SPEC.md) §4, §5) require:

- **Ed25519 sign/verify** for the pair-payload and announce-payload signatures.
- **X25519 ECDH + libsodium-compatible `crypto_box`** for sealing the pair-payload Direction-B and the mDNS-carried announce body. Concretely: XSalsa20-Poly1305 under a key derived from an ephemeral X25519 sender keypair and the recipient's X25519 public key (the recipient's pubkey itself is derived from the Resolver's Ed25519 identity via the Edwards-to-Montgomery map per [RFC 7748](https://www.rfc-editor.org/info/rfc7748)).
- **SHA-256** for the [SAS](../GLOSSARY.md#short-authentication-string) derivation ([`SPEC.md`](../SPEC.md) §4.2).
- **HMAC-SHA256** for the daily-rotated mDNS service type ([`SPEC.md`](../SPEC.md) §5.2).

Plus a non-crypto-primitive but security-critical requirement:

- **Hardware-backed key storage** for the Origin's long-lived Ed25519 identity key. iOS Keychain (with Secure Enclave on devices that have one), Android Keystore (with StrongBox where available). The Origin private key **MUST NOT** leave the secure boundary; signing happens inside the enclave.

The Rust daemon side already exists. It uses the RustCrypto family — [`ed25519-dalek`](https://crates.io/crates/ed25519-dalek), [`crypto_box`](https://crates.io/crates/crypto_box) (which transitively brings [`curve25519-dalek`](https://crates.io/crates/curve25519-dalek) and [`crypto_secretbox`](https://crates.io/crates/crypto_secretbox)), and [`sha2`](https://crates.io/crates/sha2). The wire format is libsodium-compatible NaCl, even though the implementation is a pure-Rust port.

Constraints from KMP itself ([ADR 0003](0003-kmp-for-mobile-core.md), [`MOBILE.md`](../MOBILE.md) §2.1):

- **No nested generics across the Kotlin/Native iOS boundary** — affects how we surface key types to Swift consumers.
- **Native debugging crosses the Kotlin/Native frontier** — bugs in pure-Kotlin crypto are painful to step into from Xcode. Errors in audited platform-native APIs are pinned at the boundary.
- **One Gradle build owns the iOS xcframework** — every native dep we add (libsodium framework, BouncyCastle jar) becomes part of every downstream packaging's binary footprint.

The constraint that drives this ADR more than any other is XSalsa20-Poly1305. It's the libsodium cipher inside `crypto_box`, and Apple `CryptoKit` does not expose it; standard JCA does not include it; most KMP-friendly crypto libraries (e.g. [`cryptography-kotlin`](https://github.com/whyoleg/cryptography-kotlin)) target ChaCha20-Poly1305 and AES-GCM instead. Anywhere we don't pull in a libsodium-aware dependency, we'd have to port XSalsa20-Poly1305 ourselves — a few hundred lines of constant-time arithmetic where a subtle bug is a security incident.

## Decision

Adopt a hybrid stack: **platform-native APIs for what they cover cleanly, [BouncyCastle](https://www.bouncycastle.org) for the JVM-side libsodium primitives, and the upstream [libsodium](https://libsodium.gitbook.io) framework on Apple platforms** — all behind a small `expect` / `actual` surface in `commonMain`.

Concretely:

| Concern | iOS (Kotlin/Native) | Android / JVM | Why |
|---|---|---|---|
| Ed25519 sign/verify | Apple `CryptoKit` (`Curve25519.Signing`) | Android Keystore with `Signature.getInstance("Ed25519")` (API 31+) falling back to BouncyCastle on API 26–30 | Secure Enclave on iOS where available; StrongBox-backed Keystore on Android where available |
| X25519 ECDH | Apple `CryptoKit` (`Curve25519.KeyAgreement`) | BouncyCastle | Both implementations are audited; ECDH happens with ephemeral keys so no enclave story is lost |
| Ed25519 → X25519 PK conversion | Custom Kotlin (≈30 lines, mechanical Edwards-to-Montgomery from [RFC 7748](https://www.rfc-editor.org/info/rfc7748)) | Same | Trivial map; identical bytes on every target by definition |
| `crypto_box` / XSalsa20-Poly1305 | `libsodium` framework embedded via SPM | BouncyCastle (`XSalsa20Poly1305` and ChaCha20-Poly1305 family) | iOS lacks the primitive in stdlib; BC has it on JVM and works on Android |
| SHA-256, HMAC-SHA256 | Apple `CryptoKit` | Standard JCA (`MessageDigest`, `Mac`) | Universally available, no rationale needed |
| Long-lived Origin key storage | Apple Keychain via Kotlin/Native — `SecKeyCreateRandomKey` with `kSecAttrTokenIDSecureEnclave` where supported, plain `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` otherwise | Android Keystore via `KeyPairGenerator` with `KeyGenParameterSpec` requesting `setIsStrongBoxBacked(true)` where supported | Mandated by [`MOBILE.md`](../MOBILE.md) §6.3 / §7.3 and the [`THREAT-MODEL.md`](../THREAT-MODEL.md) row for the Origin private key |
| Ephemeral X25519 sender key (for `crypto_box`) | In-process; zeroed after use | Same | Ephemeral by definition, never persisted |

The `commonMain` surface is small — about eight `expect` declarations:

```kotlin
internal expect object PlatformCrypto {
    fun sha256(input: ByteArray): ByteArray
    fun hmacSha256(key: ByteArray, input: ByteArray): ByteArray
    fun generateX25519Keypair(): X25519Keypair
    fun x25519Ecdh(privateKey: X25519Private, peerPublicKey: X25519Public): ByteArray
    fun cryptoBoxSeal(plaintext: ByteArray, recipientX25519PublicKey: ByteArray): ByteArray
    fun cryptoBoxOpen(sealed: ByteArray, x25519: X25519Keypair): ByteArray
    fun ed25519Sign(privateKey: Ed25519PrivateRef, message: ByteArray): ByteArray
    fun ed25519Verify(publicKey: ByteArray, message: ByteArray, signature: ByteArray): Boolean
}
```

`Ed25519PrivateRef` is opaque and platform-specific (a `SecKey` reference on iOS, a `PrivateKey` handle on Android/JVM) — the bytes never escape the secure boundary. The `KeyStore` interface that wraps platform-bound origin keys is a separate `expect`/`actual` per [`MOBILE.md`](../MOBILE.md) §6.3 / §7.3.

## Alternatives considered

- **Pure-Kotlin / port-everything in `commonMain`.** A single implementation in Kotlin/Common, compiled to every target, no `expect`/`actual`. Tempting because it makes the canonical-JSON style cross-target conformance trivial (the same bytes go through the same code on every platform). Rejected because porting Ed25519, X25519 ECDH, the Ed25519→X25519 Edwards-to-Montgomery map, and XSalsa20-Poly1305 in constant time is a security-grade undertaking — the wire-conformance fixtures would catch *output* drift but not side-channel leaks, and the audit story for a fresh Kotlin port of these primitives is not where we want it to be in v0.1.

- **libsodium everywhere via [lazysodium](https://github.com/terl/lazysodium-java).** `lazysodium-android` + `lazysodium-java` on the JVM side; `libsodium.framework` on iOS. Uniform implementation, byte-identical with the Rust daemon by construction (both wrap libsodium-format primitives). Rejected as the *default* because it forces every downstream packaging (Capacitor, React Native, Flutter, Swift, KMP-direct) to ship the libsodium `.so` / `.dylib`. That's a real distribution cost and adds an "OK, where does libsodium come from for this CI step?" footnote to every release pipeline. We keep it on the table for iOS specifically because there is no alternative there.

- **[cryptography-kotlin](https://github.com/whyoleg/cryptography-kotlin) as a unifying KMP layer.** Good library, modern API, growing ecosystem. It backs onto CryptoKit on Apple targets, JCA+BC on JVM, WebCrypto in browser. Rejected as the *primary* because XSalsa20-Poly1305 is not in its supported algorithm set as of the cutoff, and `crypto_box` specifically isn't a primitive there. We'd end up with cryptography-kotlin + libsodium-on-the-side; the simplicity argument disappears. It remains a possible refactor target once it covers the NaCl construction.

- **Defer crypto, ship signing as a stub.** Keep the Bridge Peer SDK at the canonical-JSON layer and let host apps integrate against the typed surface without any signing capability. Rejected because the SDK then can't pair, can't announce, and there is nothing to integrate against — the type surface alone is not load-bearing.

## Consequences

What this enables:

- **Hardware-backed Origin keys on both platforms.** The Ed25519 private key never appears in process memory in the success path. This is the strongest property in our threat model for the Origin side and we get it for free with native APIs.
- **Audited symmetric and ECDH paths.** CryptoKit + BouncyCastle + upstream libsodium are all well-reviewed. Drift between them is bounded by [`test-vectors/`](../../test-vectors/) conformance — once we have signed-payload and sealed-payload fixtures, every conforming implementation produces the same bytes or fails at fixture time.
- **Smaller blast radius for distribution.** The JVM side picks up BC as a single jar; the iOS side bundles libsodium via SPM/CocoaPods; no per-consumer "you must also install libsodium" footnote for Android.
- **Clear path for downstream packagings.** Capacitor, React Native, and Flutter packagings each wrap the same KMP binaries. Adding libsodium on iOS once at the KMP layer means the wrappers don't each need their own libsodium story.

Costs we accept:

- **BouncyCastle's reputation tax.** BC ships a lot of surface area. We need to pin to a recent version, depend on the JDK provider entries we actually use, and keep an eye on advisories. The alternative (porting XSalsa20-Poly1305) is worse.
- **Custom Edwards-to-Montgomery map in Kotlin.** Trivial in code (~30 lines, mechanical RFC 7748 §5), but it's still code we own and must test against fixtures. The cross-language fixtures cover this exactly because both sides convert the Resolver's Ed25519 pubkey to X25519 form before the seal.
- **libsodium framework embedded in iOS.** Adds a few hundred KB to the Bridge Peer xcframework. Documented in the SDK's distribution notes.
- **Surface drift between Apple CryptoKit and BC.** Two implementations of Ed25519 sign/verify, two implementations of X25519 ECDH. Conformance against shared fixtures is mandatory; the [`test-vectors/`](../../test-vectors/) directory is now load-bearing for the mobile side too, not just the daemon.

What would force a revisit:

- **CryptoKit gains XSalsa20-Poly1305 (or `crypto_box` directly).** Drop the libsodium framework dependency.
- **cryptography-kotlin gains `crypto_box`.** Reconsider as the unifying layer.
- **An audit finds a divergence between BC and a reference libsodium impl on the JVM side.** Switch the JVM side to lazysodium.
- **Apple deprecates or restricts the keychain APIs we use** for hardware-backed Ed25519 storage. Pivot to whatever replaces them.

## Notes

The `commonMain/crypto/` package will contain the `expect` declarations and the (small) shared Kotlin code: the Edwards-to-Montgomery map, the NaCl `crypto_box` envelope assembly, and the SAS derivation (which only needs SHA-256). Per-target actuals live in `iosMain/`, `androidMain/`, `jvmMain/`.

The audit log entry for the BouncyCastle dependency follows the [`CONTRIBUTING.md`](../CONTRIBUTING.md) §6.1 checklist (network behaviour: none; license: MIT; alternative: pure-Kotlin port, rejected above; activity: actively maintained). The libsodium framework follows the same intake against the iOS distribution.

The Rust daemon's existing canonical-JSON and (forthcoming) sealed-payload conformance fixtures double as the KMP side's conformance fixtures. There is no separate "Kotlin test suite" for the crypto path — the cross-language fixtures *are* the test suite.
