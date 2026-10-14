# Feature: open_links

Opening one pull request or one issue from a link another app hands over —
Orchestral's "Open in Rostrum" — on the Linux desktop and on Android.

```text
rostrum://pr?repo=<owner>/<name>&number=<n>
rostrum://issue?repo=<owner>/<name>&number=<n>
```

## Scope

- The link grammar, parsed and validated once per platform: `rostrum_core::link`
  (desktop) and `ItemLinks` (Android), tested against the same cases.
- Desktop: the `x-scheme-handler/rostrum` desktop entry and the script that
  installs and registers it; `rostrum <link>` on the command line; handing a
  link to the window already running instead of opening a second one; opening
  the item in that window.
- Android: the `rostrum://pr` and `rostrum://issue` intent filters; opening the
  item on its own screen, on a cold start and while the app is running.
- Items outside the feed: a repository that is not watched, a pull request past
  `prs_per_repo`, or one merged or closed since. They open all the same.

## Non-scope

- Producing links. Orchestral builds them; rostrum has no "copy link" action.
- `rostrum://pair` (pairing with rostrumd), which predates this and is handled
  by the Android pairing flow; the desktop app does not accept it.
- Adding the item's repository to the watched list. A link opens one item; it
  never changes `config.json`.
- Choosing a profile on Android. A link opens in the active profile; there is
  no profile parameter.
- macOS / Windows URL registration. The desktop half is Linux-only (a Unix
  socket and a freedesktop desktop entry).
- Refreshing a looked-up pull request on the poll (see *Items outside the
  feed*).

## The grammar

- Scheme `rostrum://`, host `pr` or `issue`, both case-insensitive; an
  optional `/` after the host; nothing else in the path.
- `repo` and `number` are required, once each (a repeat is refused as
  ambiguous). Keys and values may be percent-encoded (`owner%2Fname` is the
  canonical form); `+` is not a space. Unknown parameters, parameters without
  `=`, and a `#fragment` are ignored. An unknown parameter is never decoded,
  so a broken escape there does not matter.
- `repo` must satisfy GitHub's own rules, not `RepoId`'s lenient parser: owner
  1–39 of `[A-Za-z0-9-]`, no leading, trailing or doubled hyphen; name 1–100
  of `[A-Za-z0-9._-]`, never `.` or `..`. It arrives from outside and becomes
  part of API paths, so nothing else gets through.
- `number` is plain decimal digits, positive, at most `u32::MAX` on the
  desktop and `Int.MAX_VALUE` on Android (the app's refs are `Int`).
- Whole link at most 2048 characters.
- Every refusal is a sentence completing "Rostrum can't open this link: …"
  (`LinkError`'s `Display`, `ItemLink.Invalid.reason`).

## Desktop

### Registration

`packaging/desktop/rostrum.desktop` declares
`MimeType=x-scheme-handler/rostrum;` and `Exec=rostrum %u`;
`StartupWMClass=rostrum` matches the window's Wayland `app_id`, which
`main.rs` sets. `scripts/install-rostrum.sh` builds the release binary,
installs it to `~/.local/bin/rostrum`, writes the entry to
`${XDG_DATA_HOME:-~/.local/share}/applications/rostrum.desktop` with `Exec`
rewritten to the binary's absolute path, runs `update-desktop-database`, and
makes it the default with `xdg-mime default rostrum.desktop
x-scheme-handler/rostrum`. `--entry-only` skips the build for a binary
installed some other way.

### Launch

`main` parses its arguments before GPUI starts (`link::link_from_args`): none
is an ordinary start (a launcher's `%u` expands to nothing without a URL),
one is a link, more is an error. A malformed link prints `rostrum: rostrum
can't open this link: …` to stderr and exits 2, without opening a window.

Then `link::start(link, socket)` decides, with the socket at
`$XDG_RUNTIME_DIR/rostrum/desktop.sock` (else under the cache directory):

| Situation | What happens |
|---|---|
| Link, a window is running | The link is written to the socket; the window opens it and raises itself; this process exits 0. |
| Link, the running window refuses it (or does not answer in 3 s) | `rostrum: the running rostrum …` on stderr, exit 1. No second window is opened behind its back. |
| Link, no window (no socket, or a stale one nothing answers on) | This process becomes the window: it claims the socket (replacing a stale file), opens, and opens the link once GitHub auth resolves. |
| No link, no window | An ordinary start that also claims the socket, so later links come here. |
| No link, a window is running | An ordinary second window, as before; it does not take the socket, so links keep going to the first. |
| No runtime or cache directory | The link opens here; nothing listens. |

### The socket

`link::instance`. A blocking `UnixListener` on a thread of its own
(`rostrum-links`): it spends its life in `accept`, and the hand-off side runs
before any runtime exists, so plain std sockets are the simplest correct tool.
The thread's only way into the app is a `futures` unbounded channel; a GPUI
task drains it and calls `Workspace::open_link` through the `WindowHandle`.
When the window is gone the receiver drops, the next link is refused
(`refused rostrum is closing`) and the thread ends.

The wire is one line each way: the link, then `ok` or `refused <reason>`. The
listener re-parses the line — anything else on the user's account can connect
— and refuses a malformed one with `LinkError`'s sentence. Lines are capped at
4096 bytes; a client gets 2 s to send its line. The socket's directory is
created `0700` and the socket set `0600`.

A connection that sends nothing (a second window probing in `claim`) is
dropped quietly. Two windows claiming in the same instant can both see a
stale path; the later one replaces the earlier's socket and the earlier stops
receiving links. Rare enough to leave.

### Opening

`Workspace::open_link`:

1. If auth is still `Resolving`, the link is held in `pending_link` (a newer
   one replaces it) and opened from the store observer once auth settles. The
   panes cannot load anything without a client, and a pane built before one
   exists never loads.
2. Leave the CI grid; leave a repository view of a *different* repository
   (one of the same repository stays).
3. For a pull request, `Store::look_up_pull` (below).
4. `Store::reveal(link.selection())`: select the item and switch the feed to
   its tab (which, like every tab change, is saved as `feed_tab`). The detail
   pane follows the selection as it always does, replacing a half-typed
   new-issue form like any other selection would.
5. `window.activate_window()`.

An issue needs nothing more: `IssuePane` fetches the issue and its timeline by
itself whether or not the feed holds it (and shows GitHub's error if it
can't).

### Items outside the feed

`PrDetail` draws its header from the feed's copy of the pull request. For one
the feed does not hold, `Store::look_up_pull` fetches it alone
(`GitHubClient::pull_request`, the `PULL_REQUEST` GraphQL document: the feed's
field list, any state) into `Store::linked` (`LinkedPulls`), and
`PrDetail::pull` falls back to it. Until it lands the pane says "Looking up
owner/name#N on GitHub…"; if GitHub has no such repository or number, or the
token cannot see it, it says "Couldn't open owner/name#N: pull request
owner/name#N not found (renamed, deleted, or not visible to this token)".
Without a client the failure names the auth problem.

A lookup runs once per pull request per session: a second link to it, or one
already found, sends nothing; a failed one is retried by opening the link
again. The poll never refreshes a looked-up copy — the feed's copy, when there
is one, always wins in `PrDetail::pull`. The conversation, files and checks
load as for any pull request. The local section (clone status) is computed
once when the pane opens; for a looked-up pull request in a repository with a
configured clone it stays empty until the pane is reopened.

## Android

### Intent filters

`AndroidManifest.xml` has a second VIEW/BROWSABLE filter on `MainActivity`
for scheme `rostrum` with hosts `pr` and `issue` (beside the existing `pair`
one). Callers that want to know whether Rostrum is installed query
`Intent(ACTION_VIEW, Uri.parse("rostrum://pr"))`.

### Flow

`MainActivity` (`singleTop`) reads its intent in `onCreate` (cold start, or a
fresh instance in the calling app's task) and in `onNewIntent` (already on
top). `AppLinks.parse` turns a VIEW of a `rostrum://pr`/`issue` link into
`AppLink.OpenPullRequest(pr, profile = null)`, `AppLink.OpenIssue(issue)`, or
`AppLink.Invalid(reason)`, and offers it to `AppLinkInbox`, which holds the
newest link until a graph is ready to take it. The root's `routeOf` shows all
three where they are (no profile switch). The graph's `MainScaffold` asks
`stepFor(link, signedIn, profile)`:

| Link | Signed in | Not signed in |
|---|---|---|
| `OpenPullRequest` | navigate to `Destination.PullRequest` | snackbar "Sign in to GitHub first, then open the link to owner/name#N again." |
| `OpenIssue` | navigate to `Destination.Issue` | the same sentence |
| `Invalid` | snackbar "Rostrum can't open this link: …" | the same |
| `Pair` | navigate to `Destination.Pair(uri)` (unchanged) | the same |
| `OpenPullRequest` of another profile (notifications only) | `Wait` for the root's switch | — |

then consumes it.

### Items outside the feed

The issue screen already fetched by itself (`issue_detail` never consulted
the feed). The pull request screen's `pull_detail` used to refuse one the
core had never held (`UnknownPullRequest`). It now looks it up
(`GitHubClient::pull_request`) and remembers it in `FeedState::known` via
`FeedState::remember` — never over the feed's own copy, and without adding a
row to the feed — so the header, files, merge and every other call keyed on a
known pull request work for the rest of the session. A repository or number
GitHub does not have is `RostrumError::GitHubApi { status: 404, reason:
"pull request owner/name#N not found, or not visible to this token" }`,
which the screen's error view shows with Retry. `FakeRostrumBackend` keeps
answering an unknown pull request with `UnknownPullRequest`, as a failed
lookup.

## Invariants

- One grammar: a link either parses to a well-formed `owner/name` and a
  positive number, or is refused with a sentence. No partial links.
- A link never changes the watched repositories or any other setting except
  the feed tab it selects (desktop).
- Only one process listens on the desktop socket at a time, and a link is
  never opened by two windows.
- The listener trusts nothing it reads: every line is re-parsed.
- A looked-up pull request never overrides the feed's copy, never appears in
  the feed, and is looked up at most once per session unless the lookup
  failed.
- Desktop panes are only built for a link after auth has resolved.

## Files

| File | Role |
|---|---|
| `crates/rostrum-core/src/link.rs` | `RostrumLink` (`parse`, `repo`, `selection`, canonical `Display`), `LinkError`, `MAX_LINK_LEN` |
| `crates/rostrum-github/src/graphql.rs` | `pr_fields!` (shared selection), `OPEN_PULL_REQUESTS`, `PULL_REQUEST`, `PullQueryData` |
| `crates/rostrum-github/src/graphql/single_pull_tests.rs` | The lookup selects the feed's fields and decodes the same `PullRequest` |
| `crates/rostrum-github/src/client.rs` | `GitHubClient::pull_request` |
| `crates/rostrum/src/link/mod.rs` | `link_from_args`, `ArgsError`, `start`, `Startup` |
| `crates/rostrum/src/link/instance.rs` | `socket_path`, `hand_off`/`HandOff`/`HandOffError`, `claim`/`Claim`, `Listener::serve`, `serve_one` |
| `crates/rostrum/src/link/linked.rs` | `LinkedPulls`, `Lookup`, `placeholder`, `failure_message` |
| `crates/rostrum/src/sync/linked.rs` | `Store::look_up_pull`, `Store::feed_has_pull` |
| `crates/rostrum/src/main.rs` | `main` (arguments, `start`, `app_id`), `forward_links`, `Workspace::open_link`, `pending_link` |
| `crates/rostrum/src/detail.rs` | `PrDetail::pull` falls back to `Store::linked`; the placeholder sentence |
| `packaging/desktop/rostrum.desktop` | The desktop entry: `x-scheme-handler/rostrum`, `Exec=rostrum %u` |
| `scripts/install-rostrum.sh` | Build, install, write the entry, `xdg-mime default` |
| `crates/rostrum-ffi/src/detail/mod.rs` | `pull_detail` looks up a pull request the core has never held |
| `crates/rostrum-ffi/src/feed/state.rs` | `FeedState::remember` |
| `crates/rostrum-ffi/tests/linked_pulls.rs` | Lookup once, then known; 404 names the item |
| `android/app/src/main/AndroidManifest.xml` | The `pr`/`issue` intent filter |
| `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/navigation/ItemLinks.kt` | `ItemLinks.parse`, `isItemLink`, `ItemLink` |
| `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/navigation/AppLinks.kt` | `AppLink.OpenIssue`, `AppLink.Invalid`, `AppLinks.parse`, `routeOf`, `LinkStep`, `stepFor` |
| `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/app/RostrumApp.kt` | `MainScaffold` drains the inbox through `stepFor` |
| `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/MainActivity.kt` | Intent → inbox, cold and warm |
