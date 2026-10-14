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
- The primitives behind arranging pull requests into a stack: detached
  scratch worktrees, a rebase of exactly a pull request's own commits onto
  another branch (`rerere` on), compare-and-swap branch moves, and the
  leased push. See "Stack rewrites" below and `docs/features/stacks.md`.
- **Push and Push all**: publishing a checked-out pull request branch to its
  head repository — a fast-forward as is, a diverged branch only with an
  explicit confirmation and `--force-with-lease` on the last-fetched remote
  oid. See "Pushing a pull request branch" below.

## Non-scope

- **Pushing anything but those two paths.** Exactly two things write to a
  remote, both through `Repo::push_branch`: the user's Push / Push all (a
  fast-forward, or a confirmed leased rewrite) and the stack rewrite pipeline
  (Arrange and Add to stack, below; always leased). Nothing pushes on its own
  after a pull, merge or rebase — the ahead count is the cue, the button is
  the act. Never a bare `--force`, a `+` refspec, `--no-verify`, a tag, a
  submodule, a deletion, or a branch rostrum was not pointed at.
- **Choosing a remote.** A push goes to the branch's configured upstream or
  nowhere; a branch without one is skipped, not pushed to `origin` by guess.
  Creating a remote branch (`push -u`) is not offered.
- **Continuing a conflicted operation by hand.** The only `--continue` in this
  crate is the stack rebase's, and only for a stop `rerere` resolved entirely
  from a recorded resolution. Continuing requires staged resolutions, which requires something that
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

## Stack rewrites

Arranging pull requests into a stack rebases each branch onto the one below
and publishes the result (`docs/features/stacks.md`). Doing that in the
user's worktrees would move their checkouts under them, so
`crates/rostrum-git/src/repo/rewrite.rs` works beside them:

| Function | What it runs | Guarantee |
|---|---|---|
| `resolve(rev)` | `rev-parse --verify --quiet --end-of-options <rev>^{commit}` | `None` for an absent ref, never a guess |
| `is_ancestor(a, b)` | `merge-base --is-ancestor` | exit 1 is "no", not an error |
| `add_scratch_worktree(path, oid)` | `worktree add --detach -- <path> <oid>` | No branch is checked out, so none can move |
| `rebase_scratch(onto, upstream)` | `-c rerere.enabled=true -c rerere.autoUpdate=true rebase --no-autostash --no-fork-point --empty=drop --no-update-refs --onto <onto> --end-of-options <upstream>` | Refuses (`GitError::NotScratch`) anything but a clean detached HEAD; replays exactly `upstream..HEAD`; continues by itself only when rerere resolved every path and progress is being made; otherwise the `ConflictPolicy` applies |
| `remove_scratch_worktree(path)` | `worktree remove --force` | Only ever called on a worktree this crate added |
| `set_branch(branch, oid, expected)` | `update-ref -m "rostrum: stack" --no-deref -- refs/heads/<b> <new> <old>` | A compare-and-swap (`RefExpectation::Absent` or `At(oid)`); refuses (`GitError::CheckedOut`) a branch any worktree has checked out |
| `push_with_lease(remote, branch, new, expected)` | `push_branch` with `PushMode::Lease { expected }`: `-c push.followTags=false -c push.recurseSubmodules=no -c credential.interactive=false push --porcelain --force-with-lease=refs/heads/<b>:<expected> -- <remote> <new>:refs/heads/<b>` | One ref; a refused lease is `Ok(PushOutcome::Rejected(PushRejection::StaleLease))`, the remote unchanged |

Continuing a rerere-resolved rebase asks git for an editor to confirm the
original message. `command::run_keeping_messages` is the one runner variant
that answers with `GIT_EDITOR=:` (keep the prepared message); every other call
keeps `GIT_EDITOR=false`.

`push.rs` holds the pure halves: `push_args` (the leased argv),
`fast_forward_push_args` (the same shape with no lease and no force),
`branch_push_args` choosing between them by `PushMode`, `PUSH_CONFIG` (the
`-c` pairs pinned ahead of every push, so a user's `push.followTags` or
`push.recurseSubmodules` cannot widen it) — all asserted in one test to never
contain `--force`, a lease without a value, `--force-if-includes`,
`--no-verify`, `--mirror`/`--all`/`--tags`, a deletion, or a `+` refspec, and
to carry exactly one refspec — and `classify_push`, which reads the porcelain
line for the one ref (`' '`, `'+'`, `'*'`, `'='`, `'!'`) and the rejection
reason (`stale info` is the failed lease). A push with no line for the ref
(authentication, an unreachable remote) is a `GitError::Failed` carrying
stderr.

## Pushing a pull request branch

The detail pane's **Push** and the feed's **Push all** are the second
sanctioned push path (beside the stack pipeline). Both are
`rostrum_local::push`, which `rostrumd` runs unchanged for a paired phone.

### Where a branch goes

Only to its own upstream, and only when that is the pull request's head:

1. `Repo::branch_upstream(branch)` — `for-each-ref
   --format=%(refname)%00%(upstream:remotename)%00%(upstream:remoteref)%00%(upstream)
   -- refs/heads/<b>`, parsed by the pure `upstream::parse_upstream` into
   `BranchUpstream::{None, Tracking(RemoteRef), Unusual(String)}`. A local
   upstream (`.`), a merge ref outside `refs/heads/`, a custom refspec that
   maps the tracking ref anywhere but `refs/remotes/<remote>/<branch>`, or a
   remote name this crate would not hand to git is `Unusual` — skipped.
2. The upstream's branch must equal the pull request's head ref
   (`PushSkip::UpstreamIsNotHead` otherwise).
3. The head repository: `PullRequest::head_repository_in(base)` — the base
   for a same-repository pull request, GitHub's `headRepository.
   nameWithOwner` for a fork, `None` when GitHub did not name it
   (`PushSkip::HeadRepositoryUnknown`). Every URL `git remote get-url --push
   --all <remote>` prints (after `pushurl` and `pushInsteadOf`) must name that
   `owner/name` (last two path components, case-insensitive, host ignored so
   SSH aliases and GHE hosts work; `push::url::repo_of_url`). Otherwise
   `PushSkip::RemoteIsNotHeadRepository`. No remote is ever picked by
   rostrum.

### What a push would do: `plan_push(target, refresh)`

```
worktree_for(head) ──none──▶ Skip(NotCheckedOut)
blockers(Operation::Push, status, Some(head), Autostash::Disabled)
    ──any──▶ Skip(Blocked(reason))      dirty, mid-rebase/merge, conflicts, detached, wrong branch
branch_upstream / head check / push URLs ──▶ Skip(NoUpstream | UnusualUpstream | UpstreamIsNotHead
                                                 | HeadRepositoryUnknown | RemoteIsNotHeadRepository)
[Refresh::Fetch] fetch(upstream)          failure logged; refs on disk used
resolve(refs/remotes/<r>/<b>) ──absent──▶ Skip(NotFetched)
divergence(local, upstream).relation():
    Identical          ▶ Skip(UpToDate)
    Behind(n)          ▶ Skip(Behind { n })          pull instead
    Ahead(n)           ▶ Ready(FastForward { ahead })
    Diverged{a, b}     ▶ Ready(Rewrite { ahead, behind, expected: <tracking oid> })
```

`Operation::Push` was added to `preflight::Operation` and guards the
worktree like a rebase does, with autostash never clearing dirt: a push from
a worktree with uncommitted work or an unfinished operation would publish a
tip that is not what the user meant.

### Running it: `run_push(target, confirmation)`

Plans again with `Refresh::OnDisk` — **never fetching**, so the lease is the
oid the user was shown — then:

| Plan now | Confirmation | Result |
|---|---|---|
| `Skip(s)` | any | `PushResult::Skipped(s)`; nothing run |
| `FastForward` | any | `push_branch(PushMode::FastForward)`: no force, no lease; the remote refuses anything but a fast-forward (`Rejected(NonFastForward)`) |
| `Rewrite { expected }` | none, or for another branch | `NotConfirmed { expected }`; nothing run |
| `Rewrite { expected }` | `ConfirmedRewrite { branch, expected: e }`, `e ≠ expected` | `ConfirmationStale { confirmed: e, current: expected }`; nothing run |
| `Rewrite { expected }` | exactly `{ branch, expected }` | `push_branch(PushMode::Lease { expected })`; someone pushed since the fetch ⇒ `Rejected(StaleLease)`, remote unchanged |

The pushed commit is the exact oid the plan read, never "whatever the branch
points at when git runs". git moves the tracking ref as it pushes, so the
local state re-read afterwards shows the branch level with its upstream.

### The confirmation model

`ConfirmedRewrite { branch, expected }` is the only way to authorize a
rewrite, and it is checked at push time against the oid on disk: agreeing to
replace one commit can never replace another. A fast-forward needs none.

- **Push (detail pane).** The plan is loaded beside `local_state` (same
  refs, no second fetch) and decides the button before it is drawn: disabled
  with the skip's reason as tooltip; **Push** for a fast-forward (pushes on
  click); **Push (rewrite)…** for a diverged branch, which opens a
  confirmation naming the branch, the upstream, the oid it replaces, how many
  remote commits that drops, and the local oid — **Force-push with lease** /
  Cancel. Any re-read of the local state clears an open confirmation.
- **Push all (feed).** `plan_push_all` fetches and plans every checked-out
  pull request branch of every repository with a clone, one at a time.
  `PushAllPlan::rewrites()` is the one list the confirmation shows; if it is
  empty the batch runs at once. Otherwise the user answers once for the batch
  (`RewriteDecision::RewriteListed` — push everything, rewriting the listed
  branches each on its own lease; `FastForwardsOnly` — the listed branches
  are reported `NotConfirmed` and nothing runs for them; or Cancel — nothing
  at all). `PushAllPlan::authorize` turns the answer into `PushStep`s, each
  `Run` carrying its own branch's `ConfirmedRewrite`, so every rewrite is
  still re-checked as it runs.

After a push the detail pane re-reads its local state and asks GitHub for
the repository again; Push all refreshes every repository that received a
push. The poll reconciles the rest.

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
| read | 10 s | `status`, `rev-parse`, `show-ref`, `rev-list`, `for-each-ref`, `remote get-url` |
| network | 120 s | `fetch`, `push` |
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

Pushes are tested only against bare repositories in a scratch directory —
never a real remote, never the user's clones:
`crates/rostrum-git/tests/branch_push.rs` (fast-forward, a fast-forward that
would rewrite being refused, the leased form, `push.followTags` not
publishing tags, upstream reading, push URLs) and
`crates/rostrum-local/tests/push.rs` (fast-forward; unconfirmed and
wrongly-confirmed rewrites; a confirmed rewrite; a stale lease after the
remote moved unfetched; a confirmation for an oid the clone has since
fetched past; up to date, behind, no upstream, tracking another branch,
never fetched, dirty, mid-rebase, not checked out; a fork pushed only to its
head repository; Push all over a mixed batch, confirmed and declined). The
origin is named `octo/repo.git` so its URL names the head repository.

Live verification is `cargo run -p rostrum-git --example inspect -- <path>
<head> <base>`, following the `examples/` convention `rostrum-github` uses.

## Invariants

- No remote is written to except by `Repo::push_branch`, called only by the
  user's Push / Push all (`rostrum_local::run_push`) and by the stack
  pipeline (`Repo::push_with_lease`, for Arrange, and Add to stack when a
  rewrite is needed). Every push is one ref, from an exact oid, with
  `credential.interactive=false`, `--porcelain`, tags and submodules pinned
  off, and never a bare force, a `+` refspec or `--no-verify`.
- A user push goes only to the branch's own upstream, only when that is the
  pull request's head branch in its head repository; never to a guessed
  remote, never creating a branch.
- An ahead-only branch is pushed as a fast-forward; a diverged branch only
  with `--force-with-lease=refs/heads/<b>:<oid>` where `<oid>` is the
  tracking ref on disk at push time *and* equals the oid the user confirmed.
  A behind, up-to-date, unconfigured, dirty or mid-operation branch is never
  pushed, and the reason is decided before the button is drawn.
- `run_push` never fetches: the lease is the last-fetched oid the user saw.
- No branch a worktree has checked out is moved by `set_branch`, and every
  move it makes is a compare-and-swap.
- A branch name from the GitHub API is validated before it reaches an argument
  vector. A branch called `--upload-pack=...` would otherwise be read as a flag.
- Every revision handed to git is fully qualified.
- `Worktree::is_dirty()` counts staged and unstaged tracked changes only.
  Untracked files do not block a rebase and `--autostash` does not stash them,
  so counting them would grey out buttons that would have worked.
- `Repo` is cheap to clone (`Arc`-backed), like `GitHubClient`, so views hand
  out copies rather than borrowing across an await.
- A local job always works against a freshly fetched target. `pull_rebase`
  fetches for itself; `run_local_job` fetches `origin/<head>` or
  `origin/<base>` before a merge or rebase, and a failed fetch fails the job.
  Without it, "Rebase onto base" measured against whatever `origin/<base>`
  happened to be on disk and could report "up to date" against a base that had
  moved on GitHub.
- The UI-free half lives in `rostrum-local`, not in the desktop crate, so the
  desktop app and `rostrumd` run the identical sequence.

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
| `crates/rostrum-git/src/repo/rewrite.rs` | Stack rewrites: `resolve`, `is_ancestor`, `set_branch` + `RefExpectation`, scratch worktrees, `rebase_scratch`, `push_with_lease` |
| `crates/rostrum-git/src/push.rs` | `PushMode`, `push_args`, `fast_forward_push_args`, `branch_push_args`, `PUSH_CONFIG`, `classify_push`, `PushOutcome`, `PushRejection` |
| `crates/rostrum-git/src/upstream.rs` | `BranchUpstream`, `UPSTREAM_FORMAT`, `parse_upstream` |
| `crates/rostrum-git/src/repo/publish.rs` | `Repo::{branch_upstream, push_urls, push_branch}` |
| `crates/rostrum-git/tests/branch_push.rs` | `push_branch`, `branch_upstream`, `push_urls` against a scratch bare origin |
| `crates/rostrum-local/src/push/mod.rs` | `PushTarget`, `Refresh`, `PushPlan`, `PlannedPush`, `PushKind`, `PushSkip`, `ConfirmedRewrite`, `PushResult`, `plan_push`, `run_push` |
| `crates/rostrum-local/src/push/batch.rs` | Push all: `plan_push_all`, `PushAllPlan` (`rewrites`, `needs_confirmation`, `pushes`, `authorize`), `PlannedEntry`, `RewriteDecision`, `PushStep` |
| `crates/rostrum-local/src/push/url.rs` | `repo_of_url`, `url_names` |
| `crates/rostrum-local/tests/push.rs` | Plans and pushes against scratch bare repositories, including forks and Push all |
| `crates/rostrum/src/detail/push.rs` | The Push button, its rewrite confirmation, `run_push_branch` |
| `crates/rostrum/src/sync/push.rs` | `PushAll` (planning → confirming → running), `PushProgress`, `PushSummary`, `Store::{push_all, answer_push_all}` |
| `crates/rostrum/src/sync/verdict.rs` | `SyncVerdict` (a row's chip from a local sync or a Push all), `VerdictTone` |
| `crates/rostrum/src/feed/push_all.rs` | The Push all button, progress line, and batch confirmation |
| `crates/rostrum-git/tests/stack_rewrite.rs` | The rewrite primitives against scratch repositories, including a stale lease and a rerere replay |
| `crates/rostrum-git/src/context.rs` | `ConflictContext` and its parsers — see `conflict_handoff.md` |
| `crates/rostrum-local/src/jobs.rs` | `run_local_job` — one operation, shared by the detail pane, sync-all, and any other caller; fetches its target first |
| `crates/rostrum-local/src/state.rs` | `local_state` (worktree, drift from `origin/<head>`, blocker, in-progress, handoff session) and `abort_in_progress` |
| `crates/rostrum-local/tests/local_state.rs` | Both against real repositories built in a scratch directory |
| `crates/rostrum/src/detail.rs` | `LocalState`, `load_local`, `run_local_op`, `abort_local`, `render_local` |
| `crates/rostrum/src/sync/mod.rs` | `Store::sync_all`, `SyncProgress`, `Store::sync_result` |
| `crates/rostrum-config/src/lib.rs` | `clones` map, `autostash` flag, `local_path` |
