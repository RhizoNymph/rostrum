# Feature: rostrumd

A headless desktop daemon, run as a systemd **user** service, that lets a
paired Android phone see and run *local* git operations on this machine's
clones — the same `rostrum-local` jobs the desktop app's buttons run. It also
serves a web page on the LAN and tailnet where the user downloads the Android
APK and, from this computer or over the tailnet, generates pairing codes and
revokes paired phones.

## Scope

- **The page server** (plain HTTP, `http_port`, default 8484): one
  self-contained page (inline CSS/JS, no external requests), the APK
  download, pairing-code generation with a QR code and a `rostrum://pair`
  link, the paired-device list, and revoking — the last three gated.
- **The API** (HTTPS, `https_port`, default 8485): every route in
  `rostrum_remote::routes`, exactly as `docs/features/remote_protocol.md`
  documents them.
- The pairing-code lifecycle (TTL, single use, five strikes, per-address
  throttle), device tokens and how they are stored.
- The self-signed TLS identity, generated once and kept.
- The addresses a pairing link advertises.
- Mutual exclusion on clones, sync-all runs, and the record of conflicts
  handed to tmux.
- The systemd unit and its installer.

## Non-scope

- **The protocol itself** — wire types, the route table, the phone's pinned
  client. That is `rostrum-remote` (`remote_protocol.md`); this crate
  implements it and changes none of it.
- **Git.** Every operation is `rostrum_local::{local_state, run_local_job,
  abort_in_progress}` (`local_git.md`); conflict handoffs are
  `rostrum-handoff` (`conflict_handoff.md`).
- **Pushing.** Nothing here writes to a remote, ever.
- **Writing rostrum's `config.json`**, except a phone's settings push, which
  replaces only the shareable keys. Otherwise it is read, on every request
  that needs it; when it does not exist the defaults are used and nothing is
  created.
- **Building or publishing the APK** — `android/scripts/publish-apk.sh`. The
  JSON it writes beside the APK is a contract documented below.
- **Firewalls.** `ufw` needs root; opening 8484/8485 to the LAN is the user's
  call (the tailnet normally passes through Tailscale's own rules).
- **Reaching the desktop from outside the LAN and tailnet.** No relay, no port
  forwarding.

## Data and control flow

```
browser (desktop, or phone over the tailnet)
  GET /                       ── web::page::index ── gate::Access ── render(PageModel)
  POST /pairing-codes         ── gate::Admin
        │  NetworkView::advertised_hosts (watch channel, re-probed every 30 s)
        │  Endpoint::new(hosts, https_port, fingerprint)
        │  Registry::issue_code ── CodeBook::issue (actor)
        ▼
  CodeOffer {code "XXXX-XXXX", uri = PairingOffer::to_uri, qr_svg, expires_at, fingerprint, hosts}
        │  shown large, as a QR code of the uri, and as an "Open in Rostrum" link
        ▼
phone scans / taps ── PairingOffer::from_uri ── RemoteClient pinned to the fingerprint
  POST /api/v1/pair {code, device_name}
        │  Registry::pair (actor): CodeBook::redeem → mint DeviceToken + DeviceId
        │  → DeviceBook::add (devices.json, hash only, 0600, atomic)
        │  GhHandover::handover (gh auth token / $GITHUB_TOKEN, fresh)
        ▼
  PairResponse {device, token, machine: MachineInfo, github}
        │
phone ── Authorization: Bearer <token> on every later call
  api::auth::AuthedDevice ── Registry::authenticate (constant-time, touches last_seen/last_ip)
  POST /api/v1/local/status|job|abort
        │  RostrumConfig::load (fresh)  → clone path, handler, autostash
        │  convert::branch (BranchName::new; invalid → 400)
        │  CloneKey::resolve (the repository's common git dir)
        │  Jobs::exclusive / Jobs::run_job ── lease from the coordinator (busy → 409)
        │  tokio::spawn ── rostrum-local ── git (the task outlives the request)
        ▼
  convert::{local_status, job_outcome} → JSON
```

### The page

`GET /` always renders the download card. The pairing half depends on
[`gate::Access`](../../crates/rostrumd/src/web/gate.rs):

- **Admin** (loopback or tailnet, trusted `Host`): the *Pair a phone* card
  with a *Generate pairing code* button, the certificate fingerprint (short
  and full) for comparing by eye, and the *Paired devices* list with Revoke
  buttons.
- **Visitor** (anyone else): "Pairing codes can only be generated from this
  computer or over the tailnet", and the page's tailnet URLs (MagicDNS name,
  then tailnet IPv4) — or `http://localhost:<port>/` when there is no
  tailnet.

The script (`web/page/page.js`) then only adds behaviour: it POSTs to
`/pairing-codes`, shows the code, the server-rendered QR SVG and the link, runs
a live countdown to `expires_at`, and while a code is live polls `/devices`
every three seconds so the page says "Paired <name>" the moment the phone
finishes. Revoke confirms, POSTs `/devices/{id}/revoke`, and re-renders the
list with DOM APIs (`textContent`, never markup).

### Pairing

1. `POST /pairing-codes` (gated) reads the current `NetworkView`, builds an
   `Endpoint` (failing *before* minting a code if there is no address to
   advertise), asks the registry for a code, and answers
   `{code, uri, qr_svg, expires_at, fingerprint_short, fingerprint_hex, hosts}`.
2. The phone opens the link, pins the fingerprint, and posts `PairRequest`.
3. The registry actor redeems the code and records the device in one step, so
   no other request can interleave. The device name is cleaned (control
   characters dropped, whitespace collapsed, 64 characters) — and escaped
   wherever the page shows it.
4. The answer carries the token, `MachineInfo` built from rostrum's config as
   it is now, and a GitHub handover when `resolve_token()` finds one
   (`source` is `"<TokenSource> on <machine>"`, e.g. `gh auth token on
   framework`).

### Re-pairing replaces the old entry

Pairing the same phone again must not leave its old record listed beside the
new one. When a pairing succeeds, in the same registry command and the same
single write of `devices.json` that inserts the new device
(`DeviceBook::pair`):

1. **By token.** If `PairRequest::replaces` is present and its hash matches a
   stored device, that device — and only that one — is dropped. Holding the
   token proves it is the same phone, whatever it is called now.
2. **By name.** Otherwise, every device whose name is exactly the new
   device's (after the same cleaning, so `" Pixel 9 "` matches `"Pixel 9"`)
   is dropped. This covers a reinstalled app whose token was wiped with its
   data, and also sweeps up duplicates left by pairings made before this rule.
3. Otherwise nothing is dropped. A `replaces` token that matches no device is
   ignored, not an error; the name rule then still applies.

Each replacement is logged at `info` with the new and the old device id. The
code is redeemed first: a request whose code is wrong, expired, burned or
throttled removes nothing, even if it carries a valid `replaces` token and a
matching name. If the write fails, nothing is dropped and nothing is added.

**Trade-off:** the name rule cannot tell two phones apart that report the
same `device_name`. Two identical phones without distinct names will replace
each other on every pairing unless the token path applies — which it does for
any phone that still holds its previous token. Renaming one of the phones
(Android's device name) keeps both.

A refused code answers `pairing_code_invalid` (403: wrong, used, or burned),
`pairing_code_expired` (410), or `rate_limited` (429); every refusal is logged
at `warn` with the peer address and the reason — never the code.

### Authenticated requests

`AuthedDevice` reads `Authorization: Bearer`, parses a `DeviceToken`, and asks
the registry. Missing header, other scheme, malformed token, unknown token,
revoked device: all one 401 `unauthorized`. A successful lookup updates
`last_seen`/`last_ip`, written to disk only when the address changed or a
minute has passed.

- `machine`: `MachineInfo` from the config as it is now.
- `config`: the part of rostrum's `config.json` a phone may copy, as a
  `DesktopConfig` built from the file as it is now — see *Copying the
  desktop's config* below.
- `github-token`: a fresh handover, or 404 `not_found`.
- `local/status`: no clone → `NotConfigured`. Otherwise `local_state(clone,
  branch, config.autostash, session)` where `session` is
  `Some(session_name(repo, number))` exactly when a conflict handler is
  configured.
- `local/job`: `JobRequest` → `LocalJob` (the `PrMeta` from the `PrRef`,
  autostash from the request, the handler from the config) →
  `run_local_job` → `JobOutcome`. No clone → `NotConfigured` (200). A
  `HandedOff` result is recorded with its pull request and worktree.
- `local/abort`: finds the worktree with `local_state`, then
  `abort_in_progress`. No clone or not checked out → 404; nothing in
  progress, or something rostrum does not abort (bisect, `git am`) → 400.
- `sync-all` `POST`: resolves every `PrRef` up front into a `SyncPlan`
  (no clone → `NotConfigured`; a bad branch name → that entry `Failed`, the
  rest still run), starts the run, and answers with it at once. `GET`: the
  latest run, running or finished, or `null`.
- `handoffs`: `tmux list-sessions -F '#{session_name} #{session_created}'`,
  only `rostrum-` sessions, joined with the daemon's `handoffs.json` record
  (pull request, head ref, worktree), newest first. No tmux server — or no
  tmux — is an empty list.
- `DELETE device`: forgets the caller; its token fails from the next request.

### Sharing the desktop's config

`GET /api/v1/config` (bearer-authenticated) lets a phone start from the same
repositories and feed preferences as the desktop. `rostrum_config::
desktop_config` builds the answer from an allowlist, field by field, from
`Config` loaded fresh for the request:

| `DesktopConfig` | From `Config` |
|---|---|
| `repos` | `Config::repo_ids()`: valid `owner/name` entries only (a pasted GitHub URL is accepted, as the desktop accepts it), duplicates dropped, in the file's order |
| `prs_per_repo` | `prs_per_repo` |
| `hide_drafts` | `hide_drafts` |
| `hide_empty_repos` | `hide_empty_repos` |
| `authors` | the `authors` set (logins already lowercased by `LoginKey`), empty ones skipped as `feed_filter` skips them |
| `include_involved` | `include_involved` |
| `autostash` | `autostash` |
| `issues_per_repo` | `issues_per_repo` |
| `repo_sort`, `item_sort` | `repo_sort`, `item_sort` |
| `trunks` | `trunks`, valid repositories and names only |

`GET` answers a `RevisedConfig`: those fields plus `revision`, the first 16
bytes of the SHA-256 of the shareable settings (`SharedSettings::revision`),
so a phone can push against exactly what it previewed.

**Never sent:** `clones` (paths on this machine), `conflict_handler` (a
command line that describes this machine and may carry secrets),
`refresh_secs` and `notifications` (the desktop's own habits). Because the
answer is built from an allowlist rather than by filtering the file, a
setting added to `Config` later stays on the desktop until someone decides it
should travel. A missing `config.json` answers with rostrum's defaults, as
every other route does.

**Pushed settings (`PUT /api/v1/config`, `api/config.rs`, `config_push.rs`).**
The body is a `ConfigPush`. The handler validates it (`config_push::validate`:
no duplicate repositories, `prs_per_repo` and `issues_per_repo` in 1–100,
GitHub-shaped logins, no repository's trunks listed twice; 400 otherwise),
then hands it to the **`ConfigWriter`**, the daemon's single writer of
`config.json`: one task fed by a channel, so two pushes never interleave a
read and a write. For each push the writer:

1. reads the file (`rostrum_config::document::read`); a file that is not JSON
   is refused (500) and left alone — a hand-edit with a typo is never erased;
   a missing file starts from the defaults;
2. if the push has a `base` that is not the revision of the shareable
   settings on disk, writes nothing and answers 409 `config_changed` with the
   current settings;
3. applies the push to those settings (`config_push::apply`; an `Option`
   field left `None` keeps its value) and, if anything changed, overlays only
   the shareable keys onto the document (`rostrum_config::overlay_shared`) —
   the desktop's own settings and keys this build does not know keep their
   values — and writes it atomically (temporary file, fsync, rename, keeping
   the file's permissions);
4. answers with the new settings and revision, and logs which keys changed
   with the device id.

The running desktop app notices the new file within a couple of seconds and
reloads it; see `author_filter.md` ("Which settings persist").

### Stacks

A paired phone drives stacks — make, arrange, add to stack, merge, unstack —
through `rostrum-stack`, since it cannot run `gh` itself. The protocol side is
in `remote_protocol.md` ("Stacks from a phone"); the operations themselves in
`stacks.md`. Every operation goes (`api/stacks.rs`, `stacks/`):

1. **Clone.** `RostrumConfig::load` (fresh) must have a clone for the
   repository — otherwise 404 — and its conflict handler, if any, comes along.
   The clone's lease is taken from the job coordinator **first**, so a busy
   clone is refused with 409 `busy` before GitHub is asked anything.
2. **Snapshot.** `RepoSnapshots::snapshot` fetches the repository's open pull
   requests (GraphQL, newest 100) and GitHub's stacks (Stacks REST API; a 404
   means none) with `resolve_token()`'s token, which is never logged. A
   failure is 500 with GitHub's reason.
3. **Validate** (`stacks/validate.rs`), purely: `plan_stack` / `plan_extend`
   from rostrum-core (400 with the plan's reason: a closed pull request, a
   fork, a duplicate, already stacked, …); for Arrange and Add to stack,
   `confirm_rewrite` must equal the plan's `rewrites()` heads as a set, and
   Make stack must need no rewrite — otherwise 409 `rewrite_not_confirmed`
   naming the branches. Merge and unstack need the stack to be one of the
   repository's open stacks (404 otherwise). Nothing has run yet.
4. **Job.** `Jobs::start_stack_job` records a `StackJobStatus` (id, kind,
   repo, `running`) in the coordinator's `StackJobBook` and spawns a task that
   owns the lease and runs the operation (`StackOps`: `GhStackOps(GhCli)` in
   the service) with a `Progress` channel; each step is forwarded to the
   job's record as `running { progress }`, and the final state — mapped by
   `stacks/outcome.rs` from `StackOutcome` or `StackOpError`, with
   rostrum-stack's own summary as `detail` — lands last. The `POST` answers
   with the starting status; `GET /api/v1/stacks/jobs/{id}` polls. The book
   keeps the newest 32 jobs.

`POST /api/v1/stacks/plan` runs steps 1–3 without the lease and answers with
the branches a rewrite would touch.

The work runs in its own task, like every job: a phone that hangs up does not
stop a rebase half-way. Scratch worktrees go in the desktop app's own
directory (`~/.cache/rostrum/stack-worktrees`). Merge runs `gh stack merge`
from the clone and unstack `gh stack unstack` from the clone, exactly as the
desktop does. Starting, refusing and finishing a stack job are logged at
`info` with the repository, the pull request numbers, the stack number and
the job id.

### Concurrency

No web of mutexes. Two actors own all mutable state and are fed over
channels:

- **`Registry`** (`registry/mod.rs`): the `CodeBook` and the `DeviceBook`.
  One command at a time; the devices file has one writer.
- **`Jobs`** (`jobs/actor.rs`): which clone is held and by whom, the active
  sync-all run and the latest `SyncRun`, and the `HandoffBook`.

Rules the coordinator enforces:

- **One thing per clone.** Status, job and abort each hold the clone's
  `Lease`. A second request on a held clone is refused with 409 `busy`, not
  queued — a double tap must not become a second rebase minutes later.
- **Sync-all reserves everything it will touch** when it starts (refused with
  `busy` if any of those clones is held, or if a run is already going), and
  releases each clone after the run's last entry on it. Entries run strictly
  one at a time, in request order.
- **Work outlives the request.** Git work runs in a spawned task; the handler
  only awaits its answer. A phone that hangs up mid-rebase does not kill git
  half-way (rostrum-git uses `kill_on_drop`); the lease is released when the
  work ends. `Lease` releases itself from `Drop` over the (unbounded) channel.
- **Shutdown drains.** On SIGTERM the servers stop accepting and get 10 s for
  in-flight requests; the coordinator refuses new work, stops a sync-all
  between entries, and answers once every lease is released (bounded at
  330 s, under the unit's `TimeoutStopSec=360`).

"A clone" is a repository: `CloneKey` is the canonical common git directory,
so two configured worktrees of one repository are one clone.

The network view is the third piece of shared state: a watcher task re-probes
interfaces and `tailscale status --json` every 30 s and publishes through a
`tokio::sync::watch` channel; readers never wait on a subprocess.

## Security model

- **The gate** (`web/gate.rs`), for `POST /pairing-codes`, `GET /devices`,
  `POST /devices/{id}/revoke` and the admin half of `/`:
  1. the peer is loopback (`127.0.0.0/8`, `::1`) or the tailnet
     (`100.64.0.0/10`, `fd7a:115c:a1e0::/48`), after normalising an
     IPv4-mapped IPv6 address — the owner's rule, modelled as
     `ClientClass::{Loopback, Tailnet, Other}`;
  2. the `Host` header is an IP literal, `localhost`, the hostname,
     `<hostname>.local`, or this node's MagicDNS name — defeats DNS rebinding,
     where a hostile page re-points its own name at 127.0.0.1 and would
     otherwise pass both other checks;
  3. for a POST, an `Origin` header, when present, names the same `http`
     origin as `Host` — defeats cross-site requests.
  Anyone else gets 403 `forbidden`.
- **Headers** on every page response: a CSP allowing only inline style and
  script and same-origin fetches, `nosniff`, `no-referrer`, `DENY` framing,
  `no-store`.
- **Codes**: 40 random bits, TTL `code_ttl_secs` (default 300), single use;
  every failed attempt strikes every live code and five strikes burn it; ten
  failures per address (IPv4 host or IPv6 `/64`) in ten minutes and that
  address is refused outright, even with the right code. Comparisons scan
  every code without an early exit.
- **Tokens**: 32 random bytes; only the SHA-256 is stored, compared in
  constant time against every device.
- **TLS**: a self-signed certificate the phone pins by fingerprint. `ring`
  is used explicitly (`builder_with_provider`), and also installed as the
  process default at startup.
- **Logs**: method, path, status, peer and duration per request at `debug`;
  pairing, revoking, jobs and sync runs at `info`. Never a header, a body, a
  query string, a code, a token or a GitHub token; the protocol types redact
  themselves in `Debug` as a second line of defence.

## Configuration

`~/.config/rostrum/rostrumd.json`, written with every field on first run. An
absent field takes its default; an unknown field is an error.

| Field | Default | Notes |
|---|---|---|
| `machine_name` | the hostname | Shown on the page, in `Hello` and `MachineInfo` |
| `http_port` | 8484 | The page |
| `https_port` | 8485 | The API; must differ from `http_port` |
| `bind` | `["0.0.0.0", "::"]` | Every IPv6 listener sets `IPV6_V6ONLY`, so the two never collide |
| `state_dir` | `~/.local/share/rostrum/server` | Absolute or `~`; made `0700` |
| `apk_dir` | `<state_dir>/apk` | Where the publish script puts the APK |
| `code_ttl_secs` | 300 | 30–3600 |

`~/.config/rostrum/rostrumd.env` (optional, never committed) is read by the
unit — e.g. `GITHUB_TOKEN=…` when `gh` is not logged in. `RUST_LOG`
overrides the log filter.

rostrum's own `~/.config/rostrum/config.json` supplies `clones`,
`conflict_handler` and `autostash` to the local routes, and the copyable
fields to `config`, re-read on every request that needs them.

### State on disk

```
<state_dir>/            0700
  tls/                  0700
    cert.pem            the certificate phones pin — never regenerated
    key.pem             0600
  devices.json          0600  {"devices": [{id, name, token_hash, paired_at, last_seen, last_ip}]}
  handoffs.json         0600  {"handoffs": [{session, key, head_ref, worktree, started_at}]}
  apk/                  rostrum.apk + rostrum.apk.json
```

All three JSON/PEM writes are temp-file + fsync + rename with mode `0600`. A
`devices.json` or `handoffs.json` that exists but does not parse stops the
daemon from starting rather than being silently replaced.

### The APK contract

`<apk_dir>/rostrum.apk` and, beside it, `<apk_dir>/rostrum.apk.json`:

```json
{"version_name": "0.3.0", "version_code": 12,
 "built_at": "2026-09-28T12:00:00Z",
 "sha256": "<64 hex>", "size": 12345678}
```

`built_at` is RFC 3339 (integer Unix seconds are accepted too); `sha256` is
64 hex characters; `size` must equal the APK's size. An APK whose description
is missing, malformed or stale is still offered, as `rostrum.apk`, with the
problem named on the page. The download is streamed as
`application/vnd.android.package-archive` with
`Content-Disposition: attachment; filename="rostrum-<version_name>.apk"`
(the version reduced to `[A-Za-z0-9._-]`). No APK: the page says "No build
published yet" and names `android/scripts/publish-apk.sh`; `/rostrum.apk` is
404.

### Advertised hosts

The `h` list of a pairing link, in this order, without duplicates:

1. LAN IPv4 of interfaces that are up and not virtual — skipped: loopback,
   link-local, and names starting `lo`, `docker`, `br-`, `veth`, `virbr`,
   `tailscale`, `cni`, `flannel`, `podman`, `lxcbr`, `lxdbr`, `vboxnet`,
   `vmnet`, `tun`, `tap` (via `if-addrs`);
2. the tailnet IPv4, then IPv6 (`tailscale status --json` → `.Self.TailscaleIPs`);
3. the MagicDNS name (`.Self.DNSName`; on a Headscale tailnet this is not
   under `ts.net`, so no suffix is assumed).

The CLI gets a 3 s timeout; not installed, logged out or stopped all mean
"no tailnet". With nothing to advertise, `/pairing-codes` answers 500 with a
message and mints no code.

## The service

`packaging/systemd/rostrumd.service`, a user unit:

- `ExecStart=%h/.local/bin/rostrumd`, `Restart=on-failure`.
- `Environment=PATH=%h/.local/bin:%h/.cargo/bin:/usr/local/bin:/usr/bin:/bin`
  (git, gh, tmux, tailscale).
- `EnvironmentFile=-%h/.config/rostrum/rostrumd.env`.
- `KillMode=process`: a conflict handoff can be what starts the user's tmux
  server, which would then live in this unit's cgroup. Stopping or restarting
  rostrumd must never take the user's tmux sessions down with it. A tmux
  server started this way inherits the service's environment (no `DISPLAY`);
  the handler still runs in an interactive shell whose rc files set it up.
- `TimeoutStopSec=360`: longer than the 330 s rostrumd waits for git work.
- No `NoNewPrivileges`: it would be inherited by a tmux server started from
  here and by every shell the user later opens in it.
- `WantedBy=default.target`; with lingering enabled it runs without a login.

`bash scripts/install-rostrumd.sh` builds with `~/.cargo/bin/cargo build --release
--locked -p rostrumd`, renames the binary into `~/.local/bin` (atomic; a
running daemon keeps its inode), installs the unit, `daemon-reload`s, enables
it, and starts it — or restarts it if it was running — then waits for the page
to answer.

Live check against the running service:
`cargo run -p rostrumd --example pair_live` issues a code through the page,
pairs with the real `RemoteClient`, calls `machine`, `handoffs`, `sync-all`
and `github-token`, unpairs, and scans the journal for the code and tokens it
saw — printing none of them.

## Files

| File | Role | Key exports |
|---|---|---|
| `crates/rostrumd/src/lib.rs` | Crate doc, module list | `Daemon`, `DaemonParts`, `StartupError`, `Settings`, `TlsIdentity`, `VERSION` |
| `crates/rostrumd/src/main.rs` | The binary: arguments, logging, runtime, error chain | — |
| `crates/rostrumd/src/app.rs` | Startup and shutdown in order | `Args`, `Action`, `USAGE`, `run` |
| `crates/rostrumd/src/settings.rs` | `rostrumd.json`: parse, validate, write defaults | `SettingsFile`, `Settings`, `SettingsError`, `hostname` |
| `crates/rostrumd/src/error.rs` | Startup failures | `StartupError` |
| `crates/rostrumd/src/daemon.rs` | State shared by both servers | `Daemon`, `DaemonParts`, `Inner` |
| `crates/rostrumd/src/tls.rs` | Generate-once certificate, server config | `TlsIdentity`, `Provenance`, `TlsError` |
| `crates/rostrumd/src/random.rs` | OS randomness for tokens, ids, codes | `device_token`, `device_id`, `pairing_code`, `RandomError` |
| `crates/rostrumd/src/fsutil.rs` | Private atomic writes; scratch dirs for tests | `write_private`, `ensure_private_dir`, `ScratchDir` |
| `crates/rostrumd/src/state_file.rs` | JSON state files | `StoreError`, `read_json`, `write_json` |
| `crates/rostrumd/src/boxed.rs` | The boxed-future alias | `BoxFuture` |
| `crates/rostrumd/src/convert.rs` | Wire types ↔ `rostrum-local` types | `job_outcome`, `local_status`, `in_progress_kind`, `handoff_status`, `local_op`, `autostash`, `pr_meta`, `branch` |
| `crates/rostrumd/src/rostrum_config.rs` | rostrum's `config.json`, read fresh; `MachineInfo` and the shareable `DesktopConfig` (with its revision) built from it | `RostrumConfig`, `clones`, `machine_info`, `desktop_config`, `revised_config` |
| `crates/rostrumd/src/config_push.rs` | Validating and applying a phone's settings push; the daemon's one writer of `config.json` | `validate`, `apply`, `ConfigWriter`, `PushResult`, `PushInvalid` |
| `crates/rostrumd/src/api/config.rs` | `PUT /api/v1/config` | — |
| `crates/rostrumd/src/api/config_tests.rs` | Router tests for the push | — |
| `crates/rostrumd/src/github.rs` | GitHub token handover | `HandoverSource`, `GhHandover` |
| `crates/rostrumd/src/tmux.rs` | Listing handoff sessions | `SessionLister`, `TmuxCli`, `TmuxSession`, `parse_sessions`, `is_no_server`, `handoff_sessions` |
| `crates/rostrumd/src/stacks/mod.rs` | The stack request flow; the desktop's scratch directory | `default_scratch_dir` |
| `crates/rostrumd/src/stacks/validate.rs` | Validating a stack request, the rewrite confirmation | `make`, `arrange`, `extend`, `existing_stack`, `preview`, `StackRequestError` |
| `crates/rostrumd/src/stacks/snapshot.rs` | Open pull requests and stacks from GitHub, now | `RepoSnapshots`, `GitHubSnapshots`, `SnapshotError` |
| `crates/rostrumd/src/stacks/ops.rs` | `rostrum-stack` behind a replaceable seam | `StackOps`, `GhStackOps`, `merge_method` |
| `crates/rostrumd/src/stacks/outcome.rs` | Results → the job state a phone polls | `chain_state`, `merge_state`, `unstack_state` |
| `crates/rostrumd/src/jobs/stack_jobs.rs` | The coordinator's record of stack jobs | `StackJobBook` |
| `crates/rostrumd/src/api/stacks.rs` | The stack routes | — |
| `crates/rostrumd/src/api/stack_tests.rs` | Stack router tests | — |
| `crates/rostrumd/tests/stack_job.rs` | Stack jobs end to end over TLS, real git, rostrum-stack's recording `gh` | — |
| `crates/rostrumd/src/logging.rs` | Structured logs to stdout/journal | `init`, `DEFAULT_FILTER` |
| `crates/rostrumd/src/server.rs` | Running both routers on their listeners | `start`, `serve_http`, `serve_https`, `Servers` |
| `crates/rostrumd/src/net/client.rs` | Loopback / tailnet / other | `ClientClass`, `throttle_key` |
| `crates/rostrumd/src/net/request_host.rs` | `Host` allowlist and `Origin` match | `RequestAuthority`, `TrustedNames`, `origin_matches` |
| `crates/rostrumd/src/net/interfaces.rs` | LAN addresses worth advertising | `Interface`, `lan_ipv4s`, `is_virtual`, `probe` |
| `crates/rostrumd/src/net/tailscale.rs` | The tailnet from the CLI | `Tailnet`, `parse_status`, `status`, `probe` |
| `crates/rostrumd/src/net/view.rs` | Advertised hosts, page URLs, the watcher | `NetworkView`, `probe`, `spawn_watcher` |
| `crates/rostrumd/src/net/listen.rs` | Dual-stack listeners without double-binding | `bind_all`, `bind_one`, `ListenError` |
| `crates/rostrumd/src/registry/mod.rs` | The registry actor | `Registry`, `IssuedCode`, `Paired`, `PairError`, `RegistryError` |
| `crates/rostrumd/src/registry/codes.rs` | Code lifecycle, strikes, throttle | `CodeBook`, `RedeemError`, `MAX_STRIKES`, `FAILURE_LIMIT`, `FAILURE_WINDOW` |
| `crates/rostrumd/src/registry/devices.rs` | `devices.json`, and replacing a re-paired device in the same write | `DeviceBook` (`pair`), `DeviceRecord`, `DeviceView`, `Superseded`, `clean_name` |
| `crates/rostrumd/src/jobs/mod.rs` | The coordinator's handle, leases, clone identity | `Jobs`, `Lease`, `CloneKey`, `Busy`, `JobsError`, `JobRunner`, `live_runner` |
| `crates/rostrumd/src/jobs/actor.rs` | The coordinator task | — |
| `crates/rostrumd/src/jobs/sync.rs` | Sync-all plans and the runner | `SyncPlan`, `PlannedEntry`, `EntryTarget` |
| `crates/rostrumd/src/jobs/handoffs.rs` | `handoffs.json` | `HandoffRecord`, `HandoffBook` |
| `crates/rostrumd/src/http/*.rs` | Shared: error body, JSON extractor, peer, request log | `ApiFailure`, `ApiJson`, `Peer`, `peer_ip`, `log_request` |
| `crates/rostrumd/src/api/mod.rs` | The API router | `router` |
| `crates/rostrumd/src/api/auth.rs` | Bearer authentication | `AuthedDevice`, `bearer_token` |
| `crates/rostrumd/src/api/pairing.rs` | `hello`, `pair` | — |
| `crates/rostrumd/src/api/machine.rs` | `machine`, `config`, `github-token`, `handoffs`, `DELETE device` | — |
| `crates/rostrumd/src/api/local.rs` | `local/status`, `local/job`, `local/abort` | — |
| `crates/rostrumd/src/api/sync.rs` | `sync-all` | — |
| `crates/rostrumd/src/web/mod.rs` | The page router, hardening headers | `router`, `CONTENT_SECURITY_POLICY`, `CodeOffer` |
| `crates/rostrumd/src/web/gate.rs` | Who may administer | `admit`, `Admin`, `Access`, `Refusal` |
| `crates/rostrumd/src/web/apk.rs` | The APK and its JSON contract | `ApkMeta`, `Sha256Hex`, `ApkListing`, `inspect`, `download` |
| `crates/rostrumd/src/web/codes.rs` | `POST /pairing-codes` | `CodeOffer` |
| `crates/rostrumd/src/web/devices.rs` | `GET /devices`, revoke | — |
| `crates/rostrumd/src/web/qr.rs` | The link as inline SVG | `svg` |
| `crates/rostrumd/src/web/page/mod.rs` | `GET /` | `index` |
| `crates/rostrumd/src/web/page/render.rs` | The page's markup, pure | `PageModel`, `PairingPanel`, `render` |
| `crates/rostrumd/src/web/page/format.rs` | Escaping, sizes, hex, times | `escape`, `human_size`, `group_hex`, `utc` |
| `crates/rostrumd/src/web/page/page.css`, `page.js` | Inlined into the page | — |
| `crates/rostrumd/src/testkit.rs` | Test-only: a daemon on scratch dirs with fakes | — |
| `crates/rostrumd/src/{api,web,jobs}/tests.rs` | Router and coordinator tests | — |
| `crates/rostrumd/tests/tls.rs` | `RemoteClient` against the real servers over TLS | — |
| `crates/rostrumd/tests/local_job.rs` | Real jobs over the API on a scratch origin/clone/worktree | — |
| `crates/rostrumd/tests/common/mod.rs` | The integration harness | — |
| `crates/rostrumd/examples/pair_live.rs` | End-to-end check against the running service | — |
| `packaging/systemd/rostrumd.service` | The user unit | — |
| `scripts/install-rostrumd.sh` | Build, install, (re)start | — |

## Invariants and constraints

- **A stack rewrite runs only when confirmed by name**: the request's
  `confirm_rewrite` equals the plan's rewritten branches, computed on the
  desktop from GitHub's current state.
- **Stack requests are validated against GitHub now**, with the desktop's
  rostrum-core planners, before anything runs; a busy clone is refused
  before even that.
- **Tests never run `gh stack`**: the stack operations are behind `StackOps`,
  and the end-to-end test uses rostrum-stack's recording double.

- **The gating rule.** Only loopback (`127.0.0.0/8`, `::1`) and tailnet
  (`100.64.0.0/10`, `fd7a:115c:a1e0::/48`) peers, after IPv4-mapped
  normalisation, may generate codes, list devices or revoke; the `Host` must
  be an IP literal or one of this machine's names; a POST's `Origin`, when
  present, must match. Everyone else gets 403 `forbidden`.
- **A re-pairing replaces, never duplicates** — by the presented token, else
  by exact name — atomically with the insert, and only after the code is
  accepted.
- **Hashes only.** `devices.json` holds `TokenHash`es; a token exists only in
  the pairing response and on the phone. Codes live only in memory.
- **One thing per clone.** At most one status/job/abort per repository at a
  time, and nothing else on a repository a sync-all run has reserved; the
  refusal is 409 `busy`, never a queue.
- **Git work is never cut short by the network.** It runs in its own task;
  shutdown waits for it.
- **A stable certificate.** Generated only when `cert.pem` does not exist;
  a certificate without its key is an error, not a regeneration.
- **Never pushes.** Every git operation is `rostrum-local`'s, which never
  writes to a remote.
- **rostrum's config is written only by a phone's push**, through one
  writer, replacing only the shareable keys, atomically; it is read fresh on
  every request.
- **Only the shareable config travels, either way.** `config` sends, and a
  push replaces, repositories, the per-repository counts, the feed filters
  and sorts, trunks and autostash — never clones, the conflict handler, the
  refresh interval, notifications or the open tab. A stale `base` writes
  nothing.
- **The APK JSON contract** above; a mismatched size is reported, not served
  as published.
- **Advertised-host order**: LAN IPv4, tailnet IPv4, tailnet IPv6, MagicDNS
  name; never a Docker, bridge, veth, virbr, loopback or link-local address.
- **Every failure is an `ApiError`** with its code's status; request bodies
  that do not parse are 400 `bad_request`.
- **No secret reaches the log.**
- **The protocol is `rostrum-remote`'s.** Routes, types and error codes are
  implemented as documented there, unchanged.

## Testing

`cargo test -p rostrumd` — 224 tests:

- Unit: code lifecycle (expiry, single use, five strikes, per-address throttle
  and its window, IPv6 `/64`), address classification including mapped IPv6
  and the range edges, `Host`/`Origin` parsing and matching, device store
  round trip / revoke / `0600` / no plaintext token / debounce / replacement
  by token or name in one write, settings,
  the TLS identity's reuse and refusal cases, advertised-host filtering from a
  fake interface list, `tailscale status` parsing, every wire ↔ local
  conversion, tmux output parsing and joining, the coordinator (ordering,
  one-at-a-time, busy, release after the last entry, detached work, handoff
  records, shutdown), the copyable-config mapping, the settings push (only
  the shareable keys change; unknown and machine keys survive; a stale base
  writes nothing; concurrent pushes on one base apply exactly once and
  concurrent unconditional ones leave a whole file; a no-op writes nothing; a
  file that is not JSON is left alone), page rendering and
  escaping, APK description and download headers.
- Router (`tower::ServiceExt::oneshot` on scratch directories): every route,
  bearer auth (missing, garbage, other scheme, unknown, revoked), re-pairing
  (replaced by token, by name, a different name keeps both, a bad code removes
  nothing, a bogus `replaces` is ignored, the old token is then 401), `config`
  from a file with malformed and duplicate repositories, mixed-case authors
  and every never-sent field set, pairing and
  its refusals (410 with a paused clock, 429), the gate from every kind of
  peer, cross-site and rebound requests, both page variants; every stack
  route's 401, the dry run, the rewrite confirmation (missing, partial,
  extra, wrong — 409, nothing run), plan errors (400) and unknown repository,
  stack or job (404), the job lifecycle with progress, and the busy rule
  (a second stack job and a local job on the clone are 409 while one runs);
  `PUT /config`'s 401, 400s, the stale-base 409 carrying the desktop's
  change, and a push that GET then reads back.
- Integration: the phone's real `RemoteClient` over TLS (pinning, mismatch,
  probe, pair, use including `config()`, unpair) and real
  pull/merge/rebase/abort/sync-all over the
  API against a scratch origin, clone and worktree; stack jobs through the
  real `RemoteClient`, the real pipeline and real git with rostrum-stack's
  recording `gh` (a refused over-confirmation, then an arrangement that
  rebases and lease-pushes exactly the confirmed branch, and a make); a
  settings push over TLS with `push_config`, then a stale push turned back
  with the current settings.