# rostrum

Open pull requests across many GitHub repositories, in one native feed.

Built in Rust on [GPUI](https://gpui.rs). Repositories are stacked vertically,
each in its own container, in a single continuous scroll.

## Status

Phase 1: the feed works against the live GitHub API. Selecting a pull request
shows its header in the detail pane. The conversation timeline, diff viewer, and
review actions are not implemented yet — see `docs/OVERVIEW.md`.

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
cargo run -p rostrum-github --example fetch -- zed-industries/zed
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
| `ctrl-r` | Refresh all repositories |
| `ctrl-q` | Quit |

## Layout

| Crate | gpui? | Responsibility |
|---|---|---|
| `rostrum-core` | no | Domain types, feed flattening |
| `rostrum-github` | no | GraphQL reads, auth, errors |
| `rostrum-ui` | yes | Theme and components |
| `rostrum` | yes | Bootstrap, store, views |

Non-UI logic is kept free of `gpui` so it tests with plain `cargo test`.

```sh
cargo test --workspace
```

## Licence

Apache-2.0. Depends on `gpui` and `gpui_platform` (Apache-2.0) but deliberately
**not** on Zed's `ui`, `theme`, or `syntax_theme` crates, which are
GPL-3.0-or-later.
