# rostrum

Open pull requests and issues across many GitHub repositories, in one native
feed.

Built in Rust on [GPUI](https://gpui.rs). Repositories are stacked vertically,
each in its own container, in a single continuous scroll.

## Status

Complete: the multi-repo feed, the conversation timeline with markdown, the
syntax-highlighted diff, inline comments (single- and multi-line) with
pending-review batching, review submission, merge/close, draft conversion in
both directions, branch divergence with merge/rebase from the base, local clone
sync, an Issues tab with its own detail pane and issue creation, a local SQLite
cache, text selection, keyboard navigation, filtering, and optional desktop
notifications. `docs/OVERVIEW.md` lists what is deliberately
still missing.

## Requirements

- Rust nightly (edition 2024)
- [`gh`](https://cli.github.com/) authenticated (`gh auth login`), or
  `GITHUB_TOKEN` set
- Wayland. X11 additionally needs `sudo apt install libxkbcommon-x11-dev`

## Run

```sh
cargo run -p rostrum
```

GPUI comes from a pinned Zed git rev, so the first build clones Zed's history
(~500 MB) into cargo's shared git cache. Subsequent builds reuse it.

Verify the data layer alone, without opening a window:

```sh
# the feed query
cargo run -p rostrum-github --example fetch -- zed-industries/zed

# conversation + diff + anchor verification for one pull request (read-only)
cargo run -p rostrum --example review -- zed-industries/zed
cargo run -p rostrum --example review -- zed-industries/zed 62051
```

## Configure

`~/.config/rostrum/config.json`, written with defaults on first run:

```json
{
  "repos": ["zed-industries/zed", "rust-lang/rust"],
  "refresh_secs": 60,
  "prs_per_repo": 25,
  "issues_per_repo": 25,
  "feed_tab": "pull_requests",
  "notifications": false,
  "clones": {
    "zed-industries/zed": "~/Code/zed"
  },
  "autostash": false,
  "conflict_handler": {
    "command": "claude 'Resolve the conflicts described in {context}'"
  }
}
```

`clones` is optional and maps `owner/name` to any worktree of a local
checkout. A repository with one gains a local section on its pull requests —
the worktree the branch is checked out in, how far it has drifted from GitHub,
and buttons to pull, merge, or rebase it — plus a `Pull all` / `Merge base into
all` / `Rebase all onto base` row in the feed that runs across every open pull
request with a checked-out worktree. Nothing is ever pushed — after a local
merge or rebase the "ahead" count is the cue to push it yourself. `autostash`
decides whether those operations pass `--autostash`; it is also a checkbox.

`conflict_handler` is optional. Without it a local conflict is aborted and the
worktree left as it was. With it, the worktree is left mid-rebase and the
command is typed into a new tmux session (`rostrum-<owner>-<repo>-<n>`) whose
working directory is the worktree; `{context}` is replaced by the path of a
markdown bundle describing the conflict — the conflicted regions, the commits
on each side, the pull request body, and the exact commands to continue.

Set `notifications` to `true` for a desktop notification when a pull request
appears. The cache lives at `~/.local/share/rostrum/cache.db`; deleting it is
safe — it is rebuilt on the next refresh, and unsent review drafts live in a
separate table that a cache rebuild does not touch.

Repositories may be given as `owner/name` or pasted as a GitHub URL. Malformed
and duplicate entries are reported in the app rather than silently dropped.

Edits to the file while the app is running are picked up within a couple of
seconds, and the app's own saves merge with them rather than overwriting
them. A paired phone can also send its repositories and feed preferences
here, on demand (through `rostrumd`); clones, the conflict handler, the
refresh interval and notifications are never sent or replaced.

You do not have to edit this file by hand — the **repos** button in the feed
opens a panel to add and remove repositories, and changes are saved
immediately. `hide_empty_repos` is on by default and hides repositories that
have loaded with no open pull requests; one that is still loading or that failed
always stays visible.

## Phone pairing (`rostrumd`)

`rostrumd` is a small desktop daemon that lets the Android app see and run
the local pull/merge/rebase operations above on this machine's clones. It
serves a page on port 8484 — open it on the LAN or tailnet to download the
APK; from this computer (or over the tailnet) it also generates pairing codes
as a QR code and lists paired phones — and the phone's API on port 8485 over
a pinned, self-signed TLS certificate. Install it as a systemd user service:

```sh
bash scripts/install-rostrumd.sh   # build, install to ~/.local/bin, enable and start
journalctl --user -u rostrumd -f
```

Settings live in `~/.config/rostrum/rostrumd.json`; see
`docs/features/rostrumd.md`.

## Keys

| Key | Action |
|---|---|
| `j` / `k` | Next / previous pull request, or issue on the Issues tab |
| `[` / `]` | Pull requests tab / Issues tab |
| `g g` / `shift-g` | First / last |
| `enter` | Focus the detail pane |
| `/` | Focus the filter box |
| `escape` | Clear the filter |
| `c` | Collapse the selected repository |
| `shift-c` | Open the CI grid (and back to the feed from it) |
| `h` `j` `k` `l` / arrows | In the CI grid: move between cells |
| `enter` | In the CI grid: open the selected check's log |
| `r` | In the CI grid: re-run the selected check (asks first) |
| `f` | In the CI grid: show only failing or running pull requests |
| `ctrl-c` | Copy the selected diff lines, or the selected text |
| `ctrl-r` | Refresh repositories and the open pull request |
| `ctrl-q` | Quit |
| `ctrl-enter` | Submit from a composer |
| `enter` | Newline in a composer |

## Reviewing

Click a pull request, then use the tabs:

- **Conversation** — description, comments, reviews, inline threads with
  replies, and timeline events, all rendered as markdown.
- **Files** — the diff, syntax highlighted. Click `+` on a line to draft an
  inline comment, or shift-click a second line to comment on a range; drafts
  accumulate into a pending review and survive a restart. Click a line and
  shift-click another to select a run, then copy it.
- **Checks** — CI results for the head commit.

Labels are editable from the header: remove one with its `×`, or open the picker
to toggle any of the repository's labels.

## CI

**CI** in the feed's filter bar (or `shift-c`) swaps the window to a grid of
every open pull request's checks: one row per pull request, grouped by
repository and stack in the feed's order, one column per check. Each cell
shows the status and how long it has been running or how long ago it
finished; hover for the duration. `f` narrows to failing or running pull
requests. `enter` opens the check's log — collapsible sections, the failing
step highlighted, searchable — or, for another app's check, its output and
annotations. `r` re-runs: the job, the run's failed jobs or the whole run for
GitHub Actions, or re-requests another app's suite, after confirming. While
the grid is open and something is running it refreshes every 15 seconds.

## Issues

The tab bar at the top of the feed switches between **Pull requests** and
**Issues**; each tab shows how many open items the current filter lets
through, and the choice is remembered. Issues sit in the same per-repository
containers and obey the same search, author filter (where "involved" means
assigned), hidden-empty and collapse settings.

Click an issue for its pane: title, state, labels and assignees (both editable
through pickers), then the description, comments and events — closes and
reopens with their reasons, label and assignee changes, renames, and
cross-references from other issues and pull requests. Below it, comment, close
as completed, close as not planned, or reopen; none of these ask for
confirmation, since each is undone by another.

**New issue** — on the Issues tab bar, or `+ New issue` on a repository's
header — opens a form with a repository chooser, a title, a markdown body with
Write/Preview, and labels and assignees. It will not send without a title;
once created, the new issue opens in its pane.

The buttons at the bottom post a comment, submit the pending review as
**Approve** or **Request changes**, or merge/close. Merge and close ask for
confirmation first, and merge is disabled while GitHub reports conflicts or is
still computing the merge state.

## Layout

| Crate | gpui? | Responsibility |
|---|---|---|
| `rostrum-core` | no | Domain types, feed flattening, conversation model |
| `rostrum-db` | no | SQLite cache and draft persistence |
| `rostrum-diff` | no | Unified-diff parsing, comment anchoring, highlighting |
| `rostrum-github` | no | GraphQL reads, REST mutations, auth, errors |
| `rostrum-md` | no | Markdown parsing, GitHub shorthand expansion |
| `rostrum-ui` | yes | Theme, components, text input, markdown renderer |
| `rostrum` | yes | Bootstrap, store, feed and detail views |

Non-UI logic is kept free of `gpui` so it tests with plain `cargo test`.

```sh
cargo test --workspace
```

## Licence

Apache-2.0. Depends on `gpui` and `gpui_platform` (Apache-2.0) but deliberately
**not** on Zed's `ui`, `theme`, or `syntax_theme` crates, which are
GPL-3.0-or-later.
