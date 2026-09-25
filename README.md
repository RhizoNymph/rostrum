# rostrum

Open pull requests across many GitHub repositories, in one native feed.

Built in Rust on [GPUI](https://gpui.rs). Repositories are stacked vertically,
each in its own container, in a single continuous scroll.

## Status

Complete: the multi-repo feed, the conversation timeline with markdown, the
syntax-highlighted diff, inline comments (single- and multi-line) with
pending-review batching, review submission, merge/close, draft conversion in
both directions, branch divergence with merge/rebase from the base, local clone
sync, a local SQLite cache, text selection, keyboard navigation, filtering, and
optional desktop notifications. `docs/OVERVIEW.md` lists what is deliberately
still missing.

## Requirements

- Rust nightly (edition 2024)
- A local Zed checkout at `/home/nymph/Code/devtools/zed` (see "Dependency
  sourcing" in `docs/OVERVIEW.md` for the portable git-dependency form)
- [`gh`](https://cli.github.com/) authenticated (`gh auth login`), or
  `GITHUB_TOKEN` set
- Wayland. X11 additionally needs `sudo apt install libxkbcommon-x11-dev`

## Run

```sh
cargo run -p rostrum
```

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

You do not have to edit this file by hand — the **repos** button in the feed
opens a panel to add and remove repositories, and changes are saved
immediately. `hide_empty_repos` is on by default and hides repositories that
have loaded with no open pull requests; one that is still loading or that failed
always stays visible.

## Keys

| Key | Action |
|---|---|
| `j` / `k` | Next / previous pull request |
| `g g` / `shift-g` | First / last |
| `enter` | Focus the detail pane |
| `/` | Focus the filter box |
| `escape` | Clear the filter |
| `c` | Collapse the selected repository |
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
