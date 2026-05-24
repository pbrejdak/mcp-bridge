# MCP Bridge — Wire Protocol Specification

Version: **0.1 (Draft)**
Status: **Pre-1.0** — breaking changes permitted, must be flagged per [`CONTRIBUTING.md`](CONTRIBUTING.md) §3.4.
Last revised: 2026-05-24.

This document defines the normative wire protocols used by **MCP Bridge** to pair an Origin (a mobile MCP server) with a Resolver (the `mcp-bridged` daemon) and to keep that pairing alive across identity drift. It is the authoritative reference for third-party SDK implementers and for alternative Resolver implementations.

Architectural context, sequence diagrams, and design rationale live in [`ARCHITECTURE.md`](ARCHITECTURE.md). This file restricts itself to byte-level grammar, validation rules, and conformance requirements.

---

## 1. Conformance language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be interpreted as described in [BCP 14](https://www.rfc-editor.org/info/bcp14) ([RFC 2119](https://www.rfc-editor.org/info/rfc2119), [RFC 8174](https://www.rfc-editor.org/info/rfc8174)) when, and only when, they appear in all capitals, as shown here.

An implementation that satisfies every **MUST** and **MUST NOT** clause in this document for a given role (Origin, Resolver, Consumer) is a **conforming implementation** of that role at this version.

---

## 2. Terminology

| Term | Definition |
|---|---|
| **Origin** | An MCP server hosted on a user's mobile device, or any MCP server using the Bridge Peer SDK to pair with a Resolver. |
| **Resolver** | A long-running process on the user's computer (`mcp-bridged`) that maintains a stable localhost URL per Origin. |
| **Consumer** | A standard MCP client (Claude Desktop, Cursor, Continue, …) configured against the Resolver's loopback URL. |
| **Server Pin** | A Resolver-side record binding an Origin's long-lived public key to a backend URL, certificate fingerprint, bearer token, and per-Consumer access keys. |
| **Pair payload** | The `mcp-pair/v0.1` message that establishes a Server Pin. |
| **Invite** | The `mcp-pair/v0.1` message a Resolver displays as a QR (Direction B only) to solicit a pair payload. |
| **Announce** | An `mcp-announce/v0.1` message broadcast or POSTed by an Origin to refresh backend identity on a Pin. |
| **SAS** | Short Authentication String — a four-word phrase derived from a public key plus nonce, used for out-of-band confirmation. |
| **LID** | Logical ID — an Origin-chosen identifier that remains stable across IP, port, token, and certificate rotation. |
| **Canonical JSON** | The deterministic JSON encoding defined by [RFC 8785](https://www.rfc-editor.org/info/rfc8785) (JCS). |

A fuller terminology reference is in [`GLOSSARY.md`](GLOSSARY.md).

---

## 3. Common requirements

### 3.1 Cryptographic primitives

Conforming implementations **MUST** use the following primitives. Other primitives are reserved for future versions and **MUST NOT** be substituted at this version.

| Purpose | Algorithm |
|---|---|
| Public-key signatures | Ed25519 ([RFC 8032](https://www.rfc-editor.org/info/rfc8032)) |
| Public-key sealing | libsodium `crypto_box` (X25519 + XSalsa20-Poly1305) |
| Hashing | SHA-256 ([FIPS 180-4](https://csrc.nist.gov/publications/detail/fips/180/4/final)) |
| Constant-time MAC | HMAC-SHA256 ([RFC 2104](https://www.rfc-editor.org/info/rfc2104)) |
| Random material | A CSPRNG seeded by the host OS entropy source |

### 3.2 Encodings

- All payloads are UTF-8 encoded JSON conforming to [RFC 8259](https://www.rfc-editor.org/info/rfc8259).
- Binary fields (public keys, signatures, nonces, sealed bodies, certificate fingerprints) **MUST** be encoded as URL-safe Base64 without padding (`base64url`, per [RFC 4648](https://www.rfc-editor.org/info/rfc4648) §5).
- Ed25519 and X25519 public keys **MUST** carry the textual prefix `ed25519:` and `x25519:` respectively, followed by the base64url encoding of the 32-byte key. Signatures **MUST NOT** carry a prefix and are 64 bytes encoded.
- Certificate fingerprints **MUST** carry the prefix `sha256:` followed by the hex (lowercase) encoding of the digest of the certificate's DER encoding.
- Timestamps **MUST** be integer seconds since the Unix epoch (UTC).

### 3.3 Canonicalization for signing

Every field of a signed structure other than `sig` itself **MUST** be present and serialized via [RFC 8785](https://www.rfc-editor.org/info/rfc8785) Canonical JSON before signature computation or verification. Implementations **MUST NOT** accept signed payloads that fail to round-trip through canonicalization.

### 3.4 Unknown fields

Receivers **MUST** ignore unknown top-level fields they do not recognize, subject to the rule that any field whose name is reserved by a future version **MAY** be promoted to `MUST`-process semantics in that version. Senders **MUST NOT** rely on receivers acting on unknown fields.

This rule does **NOT** apply to fields included under a signature: a verifier **MUST** include every present field in the canonical input over which `sig` is checked, including unknown ones. This prevents stripping attacks.

### 3.5 Version identifier

Every payload defined by this document carries a `spec` field whose value identifies both the protocol and the version, e.g. `"mcp-pair/v0.1"`. Receivers **MUST** reject any payload whose `spec` value does not exactly match a version they implement.

---

## 4. `mcp-pair/v0.1` — pairing protocol

The pairing protocol establishes a Server Pin between exactly one Origin and exactly one Resolver. It is a single-shot exchange over an out-of-band channel (QR scan, deeplink, or file).

### 4.1 Directions

Two directions are defined. Implementations **MUST** support `resolver_offered`. Support for `origin_offered` is **OPTIONAL** but **RECOMMENDED**.

| Direction | Initiator | Channel | Use case |
|---|---|---|---|
| `resolver_offered` (Direction B) | Resolver shows QR; phone scans | Phone camera, then HTTPS POST | Default; phone has camera |
| `origin_offered` (Direction A) | Origin shows QR; Resolver scans | Computer webcam | Phone and computer not on same LAN |

The Resolver and Origin **MUST** treat any payload whose `direction` field does not match the active flow as invalid.

### 4.2 Resolver invite (Direction B)

The Resolver constructs an invite, displays it as a QR code, and listens on `resolver.lan_addr` for the sealed pair payload.

```jsonc
{
  "spec": "mcp-pair/v0.1",
  "direction": "resolver_offered",
  "resolver": {
    "pubkey": "ed25519:<base64url>",
    "display_name": "<UTF-8 string, 1-64 grapheme clusters>",
    "sas": "<lowercase-word>-<lowercase-word>-<lowercase-word>-<lowercase-word>",
    "lan_addr": "https://<host>:<port>/pair"
  },
  "nonce": "<base64url 16 bytes>",
  "uri": "mcp-pair://<percent-encoded form of the JSON above>"
}
```

Field rules:

- `resolver.pubkey` **MUST** be the Resolver's long-lived Ed25519 identity public key.
- `resolver.display_name` **MUST** be a string the user can reasonably recognize as their device. Resolvers **SHOULD** default to the OS hostname.
- `resolver.sas` **MUST** be exactly four lowercase ASCII words separated by `-`, derived by:
  1. Computing `H = SHA-256(resolver.pubkey-bytes || nonce-bytes)` over the raw 32-byte key and 16-byte nonce.
  2. Selecting four indices from the first 8 bytes of `H` as four big-endian 16-bit integers modulo 2048.
  3. Looking the indices up in the **MCP-Bridge SAS wordlist v1** (a 2048-word lowercase ASCII-only wordlist; the canonical fixture lives at `test-vectors/sas-wordlist-v1.txt` and is locked at v0.1).
- `resolver.lan_addr` **MUST** be a `https://` URL with an IP literal or RFC 6762 `.local` hostname; **MUST NOT** be a public DNS name. The Resolver's TLS certificate **MAY** be self-signed for this address (the seal authenticates the payload independently of TLS).
- `nonce` **MUST** be 16 cryptographically random bytes. Each invite uses a fresh nonce. The Resolver **MUST** record consumed nonces for the invite-lifetime window.
- `uri` is **OPTIONAL** and, when present, **MUST** be the universal-link form (`mcp-pair://`) of the same invite, so a tap on the phone opens the host app with the invite pre-loaded.

### 4.3 Invite lifetime

An invite **MUST** be accepted by the Resolver only within 60 seconds of generation. The Resolver **MUST** reject pair payloads whose `nonce` corresponds to an expired or already-consumed invite.

### 4.4 Pair payload (sealed, both directions converge)

After confirming the SAS on the phone, the Origin builds the pair payload, signs it, and seals it to `resolver.pubkey`.

```jsonc
{
  "spec": "mcp-pair/v0.1",
  "direction": "resolver_offered",
  "origin": {
    "name": "<UTF-8 string, 1-64 grapheme clusters>",
    "pubkey": "ed25519:<base64url>",
    "logical_id": "<ASCII, 1-128 chars, [a-z0-9_-]>"
  },
  "backend": {
    "url": "https://<host-or-ip>:<port>/<path>",
    "fp": "sha256:<hex>",
    "ca": "<PEM, OPTIONAL>"
  },
  "auth": {
    "type": "bearer",
    "value": "<token string, 1-2048 chars>"
  },
  "scope": ["tools", "resources"],
  "nonce": "<echoes invite nonce>",
  "target_resolver_pubkey": "ed25519:<base64url>",
  "sig": "<base64url 64 bytes>"
}
```

Field rules:

- `origin.pubkey` is the Origin's long-lived Ed25519 identity. Once the Resolver accepts this payload, this key is **pinned** for the LID; future announces for this LID **MUST** verify against this key.
- `origin.logical_id` **MUST** be stable across IP, port, token, and certificate rotation. It **MUST NOT** be the backend URL.
- `backend.url` **MUST** be a `https://` URL. `http://` backends **MUST** be rejected by the Resolver.
- `backend.fp` **MUST** be the SHA-256 of the DER-encoded leaf certificate the Origin currently presents. Subsequent rotations are governed by §5.6.
- `backend.ca` is **OPTIONAL** and, when present, **MUST** be the PEM encoding of a CA certificate the Resolver should trust when validating the leaf chain. When absent, the Resolver **MUST** pin solely on `fp`.
- `auth.type` **MUST** be one of: `"bearer"`, `"none"`. Future authentication types are reserved.
- `scope` **MUST** be a JSON array containing zero or more of: `"tools"`, `"resources"`, `"prompts"`. Other values are reserved.
- `nonce` (Direction B): **MUST** equal the `nonce` from the invite the Resolver issued.
- `target_resolver_pubkey` (Direction B): **MUST** equal `resolver.pubkey` from the invite. In `origin_offered` direction this field **MUST** be omitted (there is no invite to bind to).
- `sig` is computed by:
  1. Removing the `sig` field.
  2. Canonicalizing the remaining structure per §3.3.
  3. Computing `Ed25519_Sign(origin.pubkey-private, canonical-bytes)`.

In `resolver_offered` direction, the entire signed payload **MUST** then be sealed via `crypto_box` using the Origin's ephemeral X25519 keypair as sender and the X25519 public key derived from `resolver.pubkey` as receiver. Per [RFC 7748](https://www.rfc-editor.org/info/rfc7748) and the libsodium contract, an Ed25519 public key is converted to its X25519 equivalent via the documented Edwards-to-Montgomery map before use as a `crypto_box` receiver. The sealed body is POSTed to `resolver.lan_addr` as `application/octet-stream`.

In `origin_offered` direction, the signed payload travels in cleartext inside the QR — the QR itself is the OOB channel — and **MUST NOT** be sealed.

### 4.5 Resolver acceptance rules

The Resolver **MUST** accept a pair payload only when **all** of the following hold. Implementations **MUST** evaluate each check; partial acceptance is forbidden.

Direction-independent:

1. `spec == "mcp-pair/v0.1"`.
2. `direction` matches the active flow.
3. `origin.pubkey` parses as a valid Ed25519 public key.
4. `sig` validates against `origin.pubkey` over the canonical form of all fields other than `sig`.
5. `backend.url` is a syntactically valid `https://` URL.
6. `backend.fp` matches the leaf certificate the Origin actually presents on a TLS handshake to `backend.url` initiated within the pairing flow.

Direction B (`resolver_offered`) additionally:

7. The outer `crypto_box` seal opens with the Resolver's X25519 private key.
8. `target_resolver_pubkey` exactly equals the Resolver's own `resolver.pubkey`.
9. `nonce` matches the currently active, unexpired, unconsumed invite nonce.
10. The POST was received on the IP/port advertised in `resolver.lan_addr`.

Failure of any rule **MUST** result in rejection with no Server Pin state persisted. The Resolver **SHOULD** log the failure category locally (without payload bodies) and **MUST NOT** retry on the receiver side.

### 4.6 Server Pin formation

On acceptance, the Resolver creates a Server Pin keyed by `(origin.logical_id, origin.pubkey)`. Re-pair with the same `(LID, pubkey)` **MUST** update the existing Pin in place. Re-pair with the same `LID` and a *different* `pubkey` **MUST** be treated as a new Pin and **MUST** require explicit user confirmation in the Resolver UI, surfacing the previous fingerprint.

---

## 5. `mcp-announce/v0.1` — identity refresh protocol

The announce protocol refreshes backend identity (URL, port, certificate fingerprint, token-rotation marker) on an already-paired Server Pin. It is sent by the Origin and consumed by the Resolver.

### 5.1 Carriers

An announce **MUST** be delivered over exactly one of the following carriers:

| Carrier | Use |
|---|---|
| **mDNS (Multicast DNS / Bonjour)** | Default when Origin and Resolver share a LAN |
| **HTTP POST** | Used when the Origin learns of a new Resolver address via a paired control channel (RECOMMENDED for cellular Origins) |

### 5.2 mDNS carrier

When using the mDNS carrier:

- The service type **MUST** be `_mcp-bridge-<HMAC>._tcp.local`, where `HMAC = base64url(HMAC-SHA256(resolver_pubkey-bytes, daily_salt)[..8])` and `daily_salt = "mcp-bridge:" || YYYYMMDD`, where `YYYYMMDD` is the current UTC date. Resolvers **MUST** subscribe to the service type for the current day and for the previous day (clock-skew tolerance window).
- The TXT record **MUST** carry exactly one `body=<base64url>` entry whose decoded bytes are the libsodium `crypto_box`-sealed announce payload (§5.4), with the Origin's ephemeral X25519 keypair as sender and the X25519 form of `resolver.pubkey` as receiver. Senders **SHOULD NOT** publish other TXT keys; receivers **MUST** ignore them.
- The advertised port and host **MUST** be zero/unused at this protocol version. Reachability is conveyed entirely inside the sealed body. (Origins **MAY** still publish a non-zero port for compatibility with mDNS implementations that reject zero ports; receivers **MUST** ignore the value.)

### 5.3 HTTP POST carrier

When using the HTTP POST carrier:

- The Origin **MUST** POST `application/octet-stream` bearing the libsodium `crypto_box`-sealed announce payload to a Resolver endpoint advertised out-of-band (typically the same `/pair` endpoint with path `/announce`, or a Resolver-published `/announce` address learned during pairing).
- The Resolver **MUST** respond `204 No Content` on acceptance and `400` with no body on rejection (no error detail is leaked to network observers).

### 5.4 Announce payload (inner, after unseal)

```jsonc
{
  "spec": "mcp-announce/v0.1",
  "lid": "<echoes origin.logical_id>",
  "backend": {
    "url": "https://<host-or-ip>:<port>/<path>",
    "fp": "sha256:<hex>"
  },
  "auth_rotated_at": <unix-ts, OPTIONAL>,
  "cert_rotated_at": <unix-ts, OPTIONAL>,
  "seq": <integer, strictly increasing per lid>,
  "exp": <unix-ts>,
  "sig": "<base64url 64 bytes>"
}
```

Field rules:

- `seq` **MUST** be strictly greater than the highest `seq` the Resolver has accepted for this `lid`. Equal or lower values **MUST** be rejected.
- `exp` **MUST** satisfy `now - 60 ≤ exp ≤ now + 60`, where `now` is the Resolver's wall-clock time in seconds. Values outside this window **MUST** be rejected.
- `auth_rotated_at`, when present, **MUST** be greater than or equal to the previously accepted value for this Pin. A strictly greater value signals the Resolver to re-fetch the bearer token via the existing pinned backend connection (see §5.7).
- `cert_rotated_at`, when present, **MUST** be greater than or equal to the previously accepted value for this Pin. The new `backend.fp` **MUST** be accepted only when `cert_rotated_at` is strictly greater than the previous value.
- `sig` covers every other field, canonicalized per §3.3, signed with the private key whose public counterpart is pinned for `lid` in the Server Pin.

### 5.5 Resolver acceptance rules

The Resolver **MUST** accept an announce only when **all** hold:

1. `spec == "mcp-announce/v0.1"`.
2. The Server Pin keyed by `lid` exists. Unknown LIDs **MUST** be ignored. The Resolver **MUST NOT** auto-create Pins from announces.
3. `sig` validates against the pinned `origin.pubkey` for `lid`.
4. `seq > last_seen_seq` for this Pin.
5. `exp` is within the clock-skew window (§5.4).
6. `backend.url` is a syntactically valid `https://` URL.
7. If `backend.fp` differs from the Pin's current `fp`, then `cert_rotated_at` is present and is strictly greater than the Pin's current `cert_rotated_at`.

On acceptance, the Resolver **MUST** update `last_seen_seq` atomically together with any other field changes implied by the announce.

### 5.6 Rate limiting (REQUIRED)

To prevent signature-verification flood, the Resolver **MUST** enforce the following pre-signature drops:

| Carrier | Per source IP | Per LID |
|---|---|---|
| mDNS | ≤ 4 verifications/sec | ≤ 1 verification/sec |
| HTTP POST | ≤ 8 verifications/sec | ≤ 1 verification/sec |

Records exceeding the budget **MUST** be dropped without verification and **SHOULD NOT** be logged at default verbosity.

### 5.7 Token rotation

When an accepted announce carries an `auth_rotated_at` value strictly greater than the Pin's recorded value, the Resolver **MUST**:

1. Open a control call over the existing pinned backend TLS connection to fetch the new bearer token.
2. Atomically replace the stored token.
3. Close all pooled Origin Connector connections and re-create them with the new credential.

The control-call shape and authentication is **out of scope** for this version and is governed by the Origin host application's API.

### 5.8 Certificate rotation

When an accepted announce carries a `cert_rotated_at` value strictly greater than the Pin's recorded value and a `backend.fp` different from the Pin's recorded value, the Resolver **MUST** update both fields atomically. The next request to the Pin's backend **MUST** validate against the new fingerprint.

The Resolver **MUST NOT** trust on first use beyond the original pair payload — every subsequent fingerprint change requires the `cert_rotated_at` ratchet, signed by the pinned Origin key.

---

## 6. Loopback face (Resolver → Consumer)

This section describes the protocol surface a Resolver exposes to Consumers. Conforming Resolvers **MUST** implement this surface so that Consumers — which do not implement any Bridge-specific protocol — can rely on it.

### 6.1 Address shape

Each Server Pin is exposed at:

```
http://127.0.0.1:<port>/<logical_id>?key=<256-bit base64url>
```

Where:

- The bind address **MUST** be `127.0.0.1` (loopback). IPv6 loopback (`::1`) **MAY** additionally be bound.
- `<port>` is a Resolver-chosen TCP port. Resolvers **SHOULD** default to `8765` and fall back to a free port on collision.
- `<logical_id>` is the URL-path segment matching the Pin's `origin.logical_id`.
- `?key=<…>` is a per-`(Pin, Consumer)` random secret, 32 bytes, base64url, generated at pair time.

### 6.2 Required access checks

For every request, in order:

1. **Bind check** — the Resolver **MUST** only listen on `127.0.0.1` (and optionally `::1`).
2. **Host header check** — the `Host:` header **MUST** match `127.0.0.1:<port>` or `localhost:<port>` (or the IPv6 equivalents). Requests with any other `Host` value **MUST** receive `421 Misdirected Request` with no body, before any further processing. This defeats DNS-rebinding attacks from browser tabs.
3. **Key check** — the `?key=` parameter **MUST** be compared in constant time to the stored value for `(logical_id, Consumer)`. Mismatch or absence **MUST** return `401 Unauthorized` with no body.
4. **Pin state** — if the Pin is `Revoked`, the Resolver **MUST** return `410 Gone`. If the Pin is `Unreachable` to the backend, the Resolver **SHOULD** return `503 Service Unavailable` with the header `X-MCP-Bridge-Reason: origin-unreachable`.

### 6.3 MCP transparency

Once the access checks pass, the Resolver **MUST** forward the MCP protocol exchange to the pinned backend without rewriting, omitting, or synthesizing protocol-level messages. The Resolver **MUST NOT** intercept or modify tool-consent flows.

### 6.4 Transports

At this protocol version, only **HTTP+SSE** ([Model Context Protocol HTTP+SSE transport](https://modelcontextprotocol.io)) is REQUIRED. WebSocket and stdio transports are reserved for future versions.

---

## 7. Versioning rules

### 7.1 Version-number semantics

The `spec` field carries `<protocol-name>/v<MAJOR>.<MINOR>`:

- An incompatible change to the wire grammar, signature scope, or acceptance rules **MUST** increment `MAJOR`. Receivers of a major-newer message **MUST** reject it.
- A backwards-compatible addition (new optional field, new enum value in a `reserved` slot) **MUST** increment `MINOR` only. Receivers of a minor-newer message **SHOULD** accept and ignore unknown additions per §3.4.
- During the pre-1.0 phase, `MAJOR == 0` and breaking changes increment `MINOR`. Receivers therefore **MUST** require an exact `spec` match.

### 7.2 Negotiation

There is no in-band version negotiation. Senders and receivers either share a `spec` value or do not interoperate. Out-of-band — typically via SDK version pinning and Resolver release notes — implementers coordinate compatibility.

### 7.3 Breaking-change process

Changes to any normative requirement in this document **MUST** follow the wire-protocol-change process in [`CONTRIBUTING.md`](CONTRIBUTING.md) §3.4: RFC, two-week comment window, maintainer approval, and updated conformance test vectors before code lands.

---

## 8. Registries

The following lists are normative for v0.1. Additions before v0.2 require an RFC per §7.3. After v1.0, additions follow an IANA-style first-come, first-served process under maintainer review.

### 8.1 `auth.type` values

| Value | Meaning |
|---|---|
| `bearer` | A bearer token in the `Authorization: Bearer <token>` header |
| `none` | No authentication; the backend is publicly reachable on the LAN |

Reserved for future versions: `oauth2`, `mtls`, `hmac-sig`.

### 8.2 `scope` values

| Value | Meaning |
|---|---|
| `tools` | The Pin exposes MCP tool calls |
| `resources` | The Pin exposes MCP resource reads |
| `prompts` | The Pin exposes MCP prompt templates |

### 8.3 Reason headers

The loopback face uses a single `X-MCP-Bridge-Reason` header for machine-readable failure categorization. Defined values:

| Value | Meaning |
|---|---|
| `origin-unreachable` | The backend is currently unreachable; Pin is still valid |
| `origin-cert-changed` | The backend presented an unexpected certificate; user review required |
| `origin-revoked` | The Pin has been revoked by the user |
| `origin-unknown` | No Pin exists for this path |

Reserved for future versions: `origin-throttled`, `origin-degraded`.

### 8.4 SAS wordlist

The wordlist used by §4.2 is the **BIP39 English wordlist** adopted verbatim. The canonical fixture lives at [`test-vectors/sas-wordlist-v1.txt`](../test-vectors/sas-wordlist-v1.txt); see [`test-vectors/README.md`](../test-vectors/README.md) for sourcing, license, and adoption rationale. Properties:

- Exactly 2048 entries.
- All entries are lowercase ASCII, 3–8 characters, no homoglyph pairs, distinct phonetic profiles.
- SHA-256 of the canonical fixture: `2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda`.
- Stable across patches; a new wordlist requires a new `mcp-pair` minor version.

---

## 9. Conformance test vectors

Conforming implementations **MUST** pass the canonical test vectors maintained at `test-vectors/` in the project repository. The fixture set includes, at minimum:

1. A valid Direction-B invite, its associated valid sealed pair payload, and the expected Server Pin state on acceptance.
2. An invalid pair payload for each of the rules 1–10 in §4.5, each demonstrating the precise rejection category.
3. A valid sequence of announces over both carriers, including legitimate token-rotation and certificate-rotation ratchets.
4. A replay attempt (`seq` equal to or lower than the previous), a stale-clock attempt (`exp` outside the window), and a wrong-key attempt — all expected to be rejected.
5. Round-trip serialization fixtures verifying RFC 8785 (JCS) canonicalization on at least one payload per protocol.

PRs that change wire behavior **MUST** include updated test vectors and a rationale per [`CONTRIBUTING.md`](CONTRIBUTING.md) §3.4.

---

## 10. Security considerations

This section is informative. The consolidated threat model lives in [`THREAT-MODEL.md`](THREAT-MODEL.md); the Resolver-side trust model is in [`ARCHITECTURE.md`](ARCHITECTURE.md) §6; the privacy charter is in [`PRIVACY.md`](PRIVACY.md).

Notable properties of this version:

- The pair payload is authenticated end-to-end by `origin.pubkey` and bound to a specific Resolver by `target_resolver_pubkey` plus the outer seal. An attacker who captures the QR but cannot reach the Resolver's IP cannot consume the nonce; an attacker who can reach the IP but cannot mint a valid `crypto_box`-sealed envelope to the Resolver's public key cannot pair.
- Announces are pinned-key-only — there is no first-use auto-discovery from announces alone. An attacker on the LAN can replay sealed envelopes but cannot bypass the strictly-increasing `seq` rule.
- Certificate rotation requires the ratchet rule (§5.8): a network attacker who can present a different certificate cannot upgrade the Pin without a signed announce from the pinned key.
- The loopback face requires both the `Host:` header check and the `?key=` parameter; either alone is insufficient.

Implementations **MUST NOT** weaken any of the above properties without a `MAJOR` version increment and corresponding documentation in [`THREAT-MODEL.md`](THREAT-MODEL.md).

### 10.1 Residual risks

- An on-network observer can see that `_mcp-bridge-<hmac>._tcp.local` traffic exists, even with sealed bodies, until Bonjour is paused (informative — see [`ARCHITECTURE.md`](ARCHITECTURE.md) §6 and [`PRIVACY.md`](PRIVACY.md) §13).
- An attacker with arbitrary local-process execution on the user's computer can read the loopback key from the Consumer's config file; this is acknowledged as out-of-scope and is documented in [`PRIVACY.md`](PRIVACY.md) §10.

---

## 11. IANA considerations

This document does not currently request any IANA allocations. Future versions **MAY** request a media-type registration for the sealed pair-payload body (`application/vnd.mcp-bridge.pair+sealed`) and a multicast service-type registration for the announce carrier.

---

## 12. Change log

| Version | Date | Change |
|---|---|---|
| 0.1 (Draft) | 2026-05-24 | Initial extraction from [`ARCHITECTURE.md`](ARCHITECTURE.md) §4 into a normative document. |

---

## 13. References

Normative:

- [RFC 2119](https://www.rfc-editor.org/info/rfc2119), [RFC 8174](https://www.rfc-editor.org/info/rfc8174) — Conformance keywords.
- [RFC 4648](https://www.rfc-editor.org/info/rfc4648) — base64url encoding.
- [RFC 8032](https://www.rfc-editor.org/info/rfc8032) — Ed25519 signature scheme.
- [RFC 7748](https://www.rfc-editor.org/info/rfc7748) — Elliptic curves (X25519).
- [RFC 8259](https://www.rfc-editor.org/info/rfc8259) — JSON.
- [RFC 8785](https://www.rfc-editor.org/info/rfc8785) — JSON Canonicalization Scheme.
- [RFC 6762](https://www.rfc-editor.org/info/rfc6762) — Multicast DNS.
- [RFC 6763](https://www.rfc-editor.org/info/rfc6763) — DNS-Based Service Discovery.

Informative:

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — design rationale, sequence diagrams, trust model.
- [`DAEMON.md`](DAEMON.md) — Resolver implementation.
- [`MOBILE.md`](MOBILE.md) — reference Origin (Bridge Peer SDK) implementation.
- [`PRIVACY.md`](PRIVACY.md) — privacy charter and threat model.
- [Model Context Protocol specification](https://modelcontextprotocol.io) — the upstream protocol whose servers and clients this bridge connects.
