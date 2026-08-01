# rostrum

Open pull requests across many GitHub repositories, in one native feed.

Built in Rust on [GPUI](https://gpui.rs). Repositories are stacked vertically,
each in its own container, in a single continuous scroll.

## Status

Phases 1–4 are complete: the multi-repo feed, the conversation timeline with
markdown, the syntax-highlighted diff, inline comments with pending-review
batching, review submission, and merge/close. See `docs/OVERVIEW.md` for what is
deliberately still missing (offline cache, character-level text selection,
multi-line inline comments).

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
  "prs_per_repo": 25
}
```

Repositories may be given as `owner/name` or pasted as a GitHub URL. Malformed
and duplicate entries are reported in the app rather than silently dropped.

## Keys

| Key | Action |
|---|---|
| `ctrl-r` | Refresh repositories and the open pull request |
| `ctrl-q` | Quit |
| `ctrl-enter` | Submit from a composer |
| `enter` | Newline in a composer |

## Reviewing

Click a pull request, then use the tabs:

- **Conversation** — description, comments, reviews, inline threads with
  replies, and timeline events, all rendered as markdown.
- **Files** — the diff, syntax highlighted. Click `+` on any line to draft an
  inline comment; drafts accumulate into a pending review.
- **Checks** — CI results for the head commit.

The buttons at the bottom post a comment, submit the pending review as
**Approve** or **Request changes**, or merge/close. Merge and close ask for
confirmation first, and merge is disabled while GitHub reports conflicts or is
still computing the merge state.

## Layout

| Crate | gpui? | Responsibility |
|---|---|---|
| `rostrum-core` | no | Domain types, feed flattening, conversation model |
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
