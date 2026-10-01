# Feature: remote_protocol

The contract between a paired phone and `rostrumd` on the user's desktop:
pairing, the authenticated API, and the phone's pinned HTTPS client. One crate,
`rostrum-remote`, so the server and the client cannot disagree about a field.

## Scope

- Pairing codes, the `rostrum://pair` link that carries one, and the exchange
  of a code for a device token (and, optionally, the desktop's GitHub token).
- Wire types for every authenticated request: machine info, the copyable part
  of the desktop's config, local status, local jobs, abort, sync-all, handoff
  sessions, errors.
- The route table.
- The phone's HTTPS client (feature `client`): certificate pinning, host
  fallback, probing an unknown host, error mapping.

## Non-scope

- Serving any of it — `rostrumd` (see `docs/features/rostrumd.md`).
- Running git — `rostrum-local` and `rostrum-git`.
- Converting between wire types and `rostrum-local`'s types. That is the
  server's job; this crate does not depend on `rostrum-local`, so the phone's
  build does not pull in the desktop's git machinery.

## Trust model

`rostrumd` serves its API over HTTPS with a **self-signed certificate** it
generates once and keeps. No certificate authority is involved. Trust is the
SHA-256 of that certificate's DER, carried in the pairing link as `fp`:

```
rostrum://pair?v=1&m=nymph-desk&h=192.168.1.20,100.101.102.103,nymph-desk.tail1234.ts.net&p=8485&c=K7QXM2PD&fp=<base64url sha256>
```

The client's `PinnedVerifier` accepts exactly that certificate and still
verifies handshake signatures with the provider's algorithms — pinning replaces
the chain-of-trust check, not the proof that the peer holds the key. There is
no hostname check: the same certificate answers on the LAN address, the tailnet
address and the MagicDNS name.

A pairing code is 40 random bits in Crockford base32, shown as `XXXX-XXXX`,
single-use, short-lived and throttled by the server; parsing folds `O`→`0` and
`I`/`L`→`1`. Pairing exchanges it for a 32-byte **device token**, presented as
`Authorization: Bearer` on every later request. The server stores only the
token's SHA-256 (`TokenHash`) and compares in constant time.

A phone that pairs again with a desktop it was paired with before sends the
old token as `PairRequest::replaces` (`#[serde(default)]`, so an older phone's
request without it still parses). The desktop drops the device holding that
token once the new pairing succeeds, so a re-pair replaces the old entry rather
than listing the phone twice; a `replaces` that matches nothing is ignored.
The old token is a credential like any other and is redacted in `Debug`.

When a code is typed by hand there is no link to carry a fingerprint. The
client's `probe` connects without pinning, records the certificate it saw, and
returns it; the phone shows `CertFingerprint::short()` (`4F2A · 91C0 · 7E3B`)
to compare with the desktop page before pairing against that fingerprint.

## Data flow

```
desktop page ──PairingOffer::to_uri──▶ link / QR
phone ──PairingOffer::from_uri──▶ Endpoint{hosts, port, fingerprint} + PairingCode
phone ──RemoteClient::new(endpoint, None).pair(PairRequest)──▶ rostrumd
rostrumd ──PairResponse{device, token, machine, github?}──▶ phone (persists endpoint + token)
phone ──RemoteClient::new(endpoint, Some(token)).<call>──▶ rostrumd
```

`RemoteClient::call` tries the endpoint's hosts starting from the one that last
answered. It moves to the next host **only on a connect failure**; once a
request has been sent it is never re-sent elsewhere, because a job that timed
out on one address may still be running. A certificate mismatch on any host
is reported as `CertificateMismatch` in preference to `Unreachable`. A 401 is
always `Unauthorized` (the device was revoked), whatever the body says.

## Routes

| Route | Method | Auth | Body → Answer |
|---|---|---|---|
| `/api/v1/hello` | GET | none | → `Hello` |
| `/api/v1/pair` | POST | code | `PairRequest` → `PairResponse` |
| `/api/v1/machine` | GET | token | → `MachineInfo` |
| `/api/v1/config` | GET | token | → `DesktopConfig` |
| `/api/v1/github-token` | GET | token | → `GitHubHandover` |
| `/api/v1/local/status` | POST | token | `LocalStatusRequest` → `LocalStatus` |
| `/api/v1/local/job` | POST | token | `JobRequest` → `JobOutcome` |
| `/api/v1/local/abort` | POST | token | `AbortRequest` → `null` |
| `/api/v1/sync-all` | POST | token | `SyncAllRequest` → `SyncRun` |
| `/api/v1/sync-all` | GET | token | → `SyncRun \| null` |
| `/api/v1/handoffs` | GET | token | → `[HandoffSession]` |
| `/api/v1/device` | DELETE | token | → `null` |

Errors are `ApiError { code, message }` with `ApiErrorCode::http_status()` as
the response status.

## Files

| File | Role |
|---|---|
| `crates/rostrum-remote/src/lib.rs` | Crate doc, re-exports, `API_VERSION`, `routes` |
| `crates/rostrum-remote/src/code.rs` | `PairingCode`: Crockford base32, parse/fold, constant-time compare |
| `crates/rostrum-remote/src/fingerprint.rs` | `CertFingerprint`: SHA-256 of DER, base64url, short form |
| `crates/rostrum-remote/src/secret.rs` | `DeviceToken`, `TokenHash`, `DeviceId`, `GitHubToken` — all redacted in `Debug` |
| `crates/rostrum-remote/src/host.rs` | `Host`: IP or DNS name, URL authority |
| `crates/rostrum-remote/src/pairing.rs` | `Endpoint`, `PairingOffer` and its link, `Hello`, `PairRequest`, `PairResponse`, `GitHubHandover` |
| `crates/rostrum-remote/src/api.rs` | Authenticated request/response types, `SyncRun::summary`, `ApiError` |
| `crates/rostrum-remote/src/client.rs` | `RemoteClient`, `probe`, `PinnedVerifier`, `ClientError` |
| `crates/rostrum-remote/tests/client.rs` | The client against a real TLS listener: pinning, fallback, probe, errors |

## Copying the desktop's config

`DesktopConfig` is the part of the desktop's `config.json` a phone may adopt:
watched repositories, PRs per repository, and the feed preferences (hide
drafts, hide empty repositories, author filter, include involved) plus
`autostash`. Clone paths and the conflict-handler command are never sent —
they describe the desktop's disk, and a handler command can embed secrets —
and the refresh interval and notification switch stay per device. Malformed
repository entries are dropped server-side; logins arrive normalised as
`LoginKey`s.

## Invariants

- An `Endpoint` always has at least one host and a non-zero port; it cannot be
  constructed or deserialised otherwise.
- A `PairingCode` is always exactly eight symbols of the Crockford alphabet.
- No secret type prints its value in `Debug`.
- The server never stores a device token, only its hash.
- A request is sent to at most one host.
- The client uses the `ring` provider explicitly, so no process-wide default
  provider needs installing and nothing in the phone's build needs cmake.
