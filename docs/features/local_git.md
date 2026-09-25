# Feature: local_git

Reading and acting on a local clone of a watched repository, by driving the
`git` command line.

## Scope

- Reporting a local branch's state: which branch is checked out, whether the
  worktree is dirty, whether an operation is already under way.
- Divergence between a local branch and its remote-tracking counterpart, and
  between a branch and a base — the latter as an offline substitute for
  GitHub's answer.
- Listing worktrees and finding the one a branch is checked out in.
- Fetching one ref.
- Pull (rebase) and merge on the local clone, with or without `--autostash`.
- Deciding, before a button is drawn, whether an operation could run at all.

## Non-scope

- **Pushing.** Nothing in this crate writes to a remote. A local merge or rebase
  leaves the clone ahead of `origin`, and the ahead count is what tells the user
  to push; rostrum never force-pushes a branch.
- **Continuing a conflicted operation.** There is no `--continue` in this
  crate. Continuing requires staged resolutions, which requires something that
  can edit; the `conflict_handoff` feature hands the worktree to such a thing,
  and this crate only ever leaves the state in place or aborts it.
- **Cloning, branch creation, checkout, commit.** The clone is the user's; this
  crate reads it and moves one branch it was pointed at.
- **Submodules.** Explicitly neutralised — see the flags below.

## The invariant everything follows

> `Err` means the repository is exactly as it was before the call.
> `Ok` means rostrum changed it and is reporting the new state.

Three consequences:

1. **A conflict is not an error.** A rebase that stops on a conflict did what it
   was asked, and arrives as `Outcome::Conflicted` carrying git's own message.
2. **A refusal is not an error either.** A dirty worktree, or a rebase already
   under way, is a `Blocker` read from `Repo::preflight` *before* a button is
   drawn. It becomes `GitError::Refused` only for a caller that asked anyway.
3. **`GitError::Timeout` is the single exception.** A killed rebase can leave
   sequencer state behind, so it carries `may_have_written()` to say so.

## Verdicts come from repository state, not exit codes

A rebase whose autostash pop conflicts exits **zero** while leaving unmerged
paths. A merge that conflicts exits **one** having done exactly what was asked.
Neither exit code means what it appears to.

So every write re-reads the repository afterwards, and `classify_run` derives
the verdict from that:

| `in_progress` | conflicted paths | HEAD moved | ⇒ |
|---|---|---|---|
| `Some(Rebase)` | – | – | `Conflict::Rebase` |
| `Some(Merge)` | – | – | `Conflict::Merge` |
| `None` | > 0 | – | `Conflict::AutostashPop` |
| `None` | 0 | no | exit 0 → `AlreadyUpToDate`; else a refusal |
| `None` | 0 | yes | `Completed { from, to }` |

The third row is the payoff: an autostash pop that conflicts is exactly "the
operation finished — no sequencer state remains — yet the index has unmerged
entries". Detecting it structurally is what avoids parsing git's English.

Any `in_progress` takes the conflict branch regardless of the conflicted count,
because a rebase can also stop with none (a failed `--exec`, `--empty=stop`) and
the user's situation is identical: unfinished, with state to abort.

## Conflict policy: abort, or leave and hand off

`Repo` carries a `ConflictPolicy`, set at open time and threaded exactly like
`Timeouts`:

- **`Abort`** (the default): on a rebase or merge conflict the matching
  `--abort` runs and the conflict is reported with git's message. The clone is
  left as it was found.
- **`Leave`**: the conflict is returned untouched — `abort_target()` stays
  `Some`, the sequencer state stays on disk — for someone else to finish. The
  app selects this when a `conflict_handler` is configured; see
  `docs/features/conflict_handoff.md`. Under `Leave`, `Ok(Conflicted { aborted:
  false })` guarantees: sequencer state on disk; `refs/heads/<branch>` still at
  the pre-rebase tip (git moves it only on completion — verified live); an
  enabled autostash *held* by git, not popped, so nobody should `git stash pop`
  by hand.

`classify_run` is unchanged by the policy: it only ever decides what happened,
and `on_conflict` — one named place — decides what to do about it.

`Conflict::AutostashPop` is never aborted under either policy. There the
operation already succeeded, no sequencer state exists, `--abort` would fail,
and the user's changes are safe in the stash. `InProgress::abort_target()`
returns `None` for it, so the pairing is enforced by the type rather than by a
comment.

A foreign operation — a `git am`, cherry-pick, revert, or bisect the user
started — is never aborted either. `git rebase --abort` does not clear a real
`git am`, so guessing would destroy work rostrum did not start. Reaching that
state means preflight was bypassed, which is what `GitError::Refused` means.

The cost of `Abort` is that partially-applied commits and `rerere` resolutions
are discarded; the cost of `Leave` is a worktree the user must finish in a
terminal if the handler does not. Both are stated in the UI.

## Worktrees

A clone in this app is any worktree of a repository — the configured path is
typically `main`, and in a one-worktree-per-branch layout every pull request is
checked out somewhere else. `Repo::worktrees()` lists them via `git worktree
list --porcelain` (records of `worktree`, `HEAD`, `branch` / `detached` /
`bare`, separated by blank lines; `locked` and `prunable` are parsed and
ignored), and `Repo::worktree_for(branch)` opens a `Repo` at the one that has
the branch checked out, carrying over timeouts and policy. `None` means the
branch is not checked out anywhere, which the detail pane renders as one quiet
line rather than a refusal.

Every local operation resolves the worktree this way first. Running `pull` in
the configured path when the branch lives elsewhere would have been the
"wrong branch" refusal on every pull request — which is what the first version
of this feature did.

## Why the command line rather than libgit2

`git` is already a hard dependency of the workflow this app serves, and shelling
out inherits the user's credential helpers, `ssh` agent, hooks, `rerere`, and
config for free. Binding libgit2 would mean reimplementing credential
negotiation, and its rebase is a notoriously partial substitute for the real
one. The precedent already existed: `rostrum-github/src/auth.rs` shells out to
`gh`.

## Flags that are correctness fixes, not preferences

Common prefix on every invocation:

```
git -C <abs root> --no-optional-locks --no-pager -c color.ui=false -c submodule.recurse=false
```

`--no-optional-locks` must precede the subcommand — `git status
--no-optional-locks` is rejected. It matters because `status` rewrites the index
by default, and a GUI polling it would collide with the user's own terminal.

| Where | Flag | Why |
|---|---|---|
| `fetch` | `+refs/heads/X:refs/remotes/origin/X` | **The most important flag here.** `git fetch origin X` writes only `FETCH_HEAD` and does *not* update the tracking ref, so every count computed afterwards would be stale. `+` forces it, needed because PR branches are force-pushed routinely |
| `fetch` | `--porcelain --verbose` | Machine-readable, on stdout. Without `--verbose`, up-to-date refs print nothing and "unchanged" is indistinguishable from "refspec never processed" |
| `status` | `--porcelain=v2 --branch -z` | v1's branch line has no stable grammar and cannot express an unborn branch. `-z` because `core.quotePath` C-quotes non-ASCII paths and a path containing a newline would split a record |
| `status` | `-c status.aheadBehind=true` | A user with it disabled gets `+? -?`, which parses to "unknown" rather than a guess |
| `status` | `--ignore-submodules=dirty` | A submodule with untracked files would otherwise mark the superproject dirty and grey out a button for something that does not block a rebase |
| state files | `rev-parse --git-path` | These are **per-worktree**. Hard-coding `root/.git/MERGE_HEAD` reports "no merge in progress" forever in a linked worktree — and this repository is one |
| `rev-list` | fully-qualified refs | A bare name resolves `refs/tags/` *before* `refs/heads/` |
| `rebase` | `--no-update-refs` | `rebase.updateRefs=true` would move every *other* local branch pointing at a rebased commit |
| `rebase` | `--no-fork-point` | `--fork-point` reads the reflog and gives different answers on different machines |
| `rebase`/`merge` | `--autostash` *or* `--no-autostash`, always | Omitting it would let a user's `rebase.autoStash=true` stash their work with the checkbox unticked — precisely the surprise a checkbox prevents |
| `rebase`/`merge` | `--end-of-options` | Neither accepts `--` for this purpose |
| all writes | `-c gc.auto=0` | A fetch tripping auto-gc forks a background `git gc` that holds locks and collides with the rebase about to run |

`pull_rebase` is fetch-then-rebase, never `git pull`: `git pull --rebase origin X`
rebases onto `FETCH_HEAD` and leaves the tracking ref stale, depends on
`branch.*.merge` being configured, and re-reads four other config knobs.

## Subprocess safety

`-C` does **not** isolate the child. `GIT_DIR` in the environment overrides
repository discovery entirely, so `git -C <clone> status` can report on a
completely different repository. The environment is therefore built with
`env_clear()` plus an allowlist:

`PATH`, `HOME`, `USER`, `LOGNAME`, `TMPDIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`,
`SSH_AUTH_SOCK`.

Two entries are load-bearing by their absence or presence:

- `DISPLAY` and `WAYLAND_DISPLAY` are **omitted**, so `SSH_ASKPASS` cannot open
  a graphical password dialog from a process the user did not start.
- `SSH_AUTH_SOCK` is **kept**, because agent auth is how fetches actually
  succeed; dropping it would break every `git@` fetch for no gain.

Then forced: `GIT_TERMINAL_PROMPT=0`, `GIT_OPTIONAL_LOCKS=0`, `GIT_EDITOR=false`,
`GIT_SEQUENCE_EDITOR=false`, `GIT_PAGER=cat`, `SSH_ASKPASS_REQUIRE=never`,
`LC_ALL=C`, `LANG=C`, `LANGUAGE=`, `TERM=dumb`.

`GIT_EDITOR=false` rather than `true`: `false` exits non-zero, so git aborts
loudly instead of silently accepting an unreviewed commit message.

Every call also gets `stdin(Stdio::null())` and `kill_on_drop(true)`.

### Timeouts

New to this workspace — nothing else here bounds a subprocess. Introduced
because nothing else provably stops a hook (arbitrary user code) or an `ssh`
(which can read `/dev/tty` directly, bypassing a null stdin) from hanging a GUI
thread that a click started.

| Class | Default | Covers |
|---|---|---|
| read | 10 s | `status`, `rev-parse`, `show-ref`, `rev-list` |
| network | 120 s | `fetch` |
| write | 300 s | `rebase`, `merge`, `abort` |

Hooks are **not** disabled. A `pre-rebase` hook that refuses to rebase a
protected branch is a safety mechanism the user installed on purpose, and
bypassing it from a GUI button would be worse than the button being slow. The
veto surfaces naturally: git exits non-zero, nothing changed, and the hook's own
stderr reaches the error.

## Concurrency

Two layers, for two different threats.

In-process: an `AtomicBool` taken for the duration of any write, released by a
guard's `Drop`. A second concurrent write gets `Blocker::Busy`. Deliberately not
a mutex — queueing means a user who double-clicks gets a second rebase minutes
later against a repository that has since changed.

Cross-process: the user's own terminal, their editor, a background `git gc`.
Nothing prevents it; git's own `index.lock` is the real mutex. `Unable to create
'<path>': File exists` is classified as a transient error so the UI can offer
Retry. A lock file is never deleted.

## Testing

No mocks — there are none anywhere in this workspace. Every decision is a pure
function over a string git actually printed, so the interesting cases are
literals in a test rather than a fixture repository on disk:

`parse_status_v2`, `parse_ab`, `parse_left_right_count`, `in_progress`,
`classify_run`, `classify_fetch`, `parse_fetch_porcelain`, `blockers`,
`BranchName::new`, `Oid::parse`.

The cases that most need to exist:

- `# branch.upstream` present with no `# branch.ab` ⇒ `Some(Upstream {
  divergence: None })`. "Configured but never fetched" must not collapse into
  "no upstream".
- `rebase-apply/` **with** `applying` ⇒ `Am`, not a rebase. Backwards, this puts
  a "rebase in progress" message on a user's hand-rolled `git am`.
- Dirty + `Autostash::Enabled` ⇒ no blocker; in-progress + `Enabled` ⇒ still
  blocked. Autostash rescues a dirty tree, never an unfinished operation.
- `AutostashPop` reached from exit 0 *and* exit 1, proving exit-code
  independence.

Live verification is `cargo run -p rostrum-git --example inspect -- <path>
<head> <base>`, following the `examples/` convention `rostrum-github` uses.

## Invariants

- No remote is ever written to.
- A branch name from the GitHub API is validated before it reaches an argument
  vector. A branch called `--upload-pack=...` would otherwise be read as a flag.
- Every revision handed to git is fully qualified.
- `Worktree::is_dirty()` counts staged and unstaged tracked changes only.
  Untracked files do not block a rebase and `--autostash` does not stash them,
  so counting them would grey out buttons that would have worked.
- `Repo` is cheap to clone (`Arc`-backed), like `GitHubClient`, so views hand
  out copies rather than borrowing across an await.

## Files

| File | Role |
|---|---|
| `crates/rostrum-git/src/lib.rs` | Crate doc stating the invariant; re-exports |
| `crates/rostrum-git/src/error.rs` | `GitError` |
| `crates/rostrum-git/src/refs.rs` | `BranchName`, `Remote`, `RemoteRef`, `Rev`, `Oid` |
| `crates/rostrum-git/src/status.rs` | `RepoStatus` and the porcelain-v2 parsers |
| `crates/rostrum-git/src/preflight.rs` | `Blocker`, `Operation`, `Autostash`, `blockers` |
| `crates/rostrum-git/src/outcome.rs` | `Outcome`, `Conflict`, `classify_run` |
| `crates/rostrum-git/src/fetch.rs` | `FetchOutcome` and the porcelain fetch parsers |
| `crates/rostrum-git/src/command.rs` | The one place a process is spawned; env, timeouts |
| `crates/rostrum-git/src/repo.rs` | `Repo`, `ConflictPolicy`, worktrees, and `on_conflict` |
| `crates/rostrum-git/src/repo/describe.rs` | `Repo::conflict_context` — see `conflict_handoff.md` |
| `crates/rostrum-git/src/worktree.rs` | `WorktreeEntry`, `parse_worktree_list` |
| `crates/rostrum-git/src/context.rs` | `ConflictContext` and its parsers — see `conflict_handoff.md` |
| `crates/rostrum/src/localops.rs` | `run_local_job` — one operation, shared by the detail pane and sync-all |
| `crates/rostrum/src/detail.rs` | `LocalState`, `load_local`, `run_local_op`, `abort_local`, `render_local` |
| `crates/rostrum/src/sync.rs` | `Store::sync_all`, `SyncProgress` |
| `crates/rostrum/src/config.rs` | `clones` map, `autostash` flag, `local_path` |
