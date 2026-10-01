# Feature: stacks

Stacks of pull requests: a chain where the bottom pull request targets a trunk
branch and each one above targets the head branch of the one below. Rostrum
shows stacks together in the feed, sorts a stack as one unit, makes stacks
(from a chain that already exists, or by arranging arbitrary pull requests),
and merges a whole stack at once. GitHub — through the `gh stack` extension
(`github/gh-stack` v0.1.0) and its Stacks REST API — is the source of truth.

## Scope

- **Model.** `Stack { repo, number: Option<StackNumber>, trunk: RefName,
  members: StackMembers }`, bottom first. `number` is GitHub's stack number;
  `None` is a chain rostrum detected.
- **Detection.** A pure function finding chains of open same-repository pull
  requests whose bases chain, defensive against forks, ambiguity, branching
  and cycles.
- **Reading GitHub's stacks** for every watched repository, without a clone,
  through `GET /repos/{owner}/{repo}/stacks`, cached in SQLite so a cold start
  paints grouped.
- **Feed display.** A stack header row ("Stack 7 · 3 PRs", the trunk, a
  merge-state rollup, actions) above the stack's visible members, bottom
  first, each with a chain glyph, inside the repository's container.
- **Sorting** a stack as one unit through feed-sort's group hook.
- **Make stack** on a detected chain: `gh stack link` + `gh stack init` from
  the clone. Nothing is pushed.
- **Arrange PRs**: pick pull requests of one repository, order them, choose a
  trunk, confirm; rostrum rebases each branch onto the one below in scratch
  worktrees, pushes each rewrite with `--force-with-lease`, then links and
  tracks the stack. Conflicts stop before anything is pushed and go to the
  conflict handler when one is configured.
- **Add to stack**: put open pull requests on top of an existing GitHub
  stack, in a chosen order — a plain `gh stack link <stack> <pr>...` when
  they already chain off the top, otherwise the Arrange path (rebase onto the
  top in scratch worktrees, leased push, then link). The stack's existing
  members are never touched. A line of pull requests already built on a
  stack's top is offered as "Extend with #a ← #b".
- **Merge stack**: GitHub's atomic, all-or-nothing stack merge, with a
  confirmation listing every member's merge state and the method.
- **Unstack**, with confirmation.

## Non-scope

- **Android UI.** The phone's feed gets stack members in the same contiguous
  order but no header and no actions (`rostrum-ffi` ignores
  `FeedRow::StackHeader`). The *operations* are reachable from a paired phone
  through `rostrumd` (below); a phone screen for them is not built yet.
- **Editing a stack in place** beyond adding to its top (`gh stack modify`,
  reordering or removing members, `gh stack sync`/`rebase`). Unstack and
  arrange again instead.
- **Tracking an extension locally.** gh-stack has no command that adopts
  branches onto an already-tracked stack; an extension is made on GitHub
  only, and `gh stack sync` in the clone pulls the additions into local
  tracking (the outcome says so).
- **Stack actions in the repo view.** It shows stack headers read-only; the
  actions live in the feed.
- **Creating pull requests.** Every member already has one; `gh stack submit`
  is never run, because it pushes on rostrum's behalf.
- **Forks.** A pull request from a fork is never detected into a chain and
  cannot be arranged: its branch is not in the clone's `origin`.
- **Resuming an arrangement in place.** A handed-off conflict is finished in
  its scratch worktree and the arrangement is run again; see below.

## What `gh stack` stores, and where rostrum reads stacks from

gh-stack keeps every locally tracked stack in **one JSON file, `gh-stack`, in
the git directory** of the worktree it ran in (`git rev-parse --git-dir`:
`<clone>/.git/gh-stack` for a clone's main worktree), with a `gh-stack.lock`
beside it serialising writers. There is no git config or ref involved. Schema
version 1:

```json
{ "schemaVersion": 1, "repository": "github.com:owner/repo",
  "stacks": [ { "id": "...", "number": 7,
                "trunk": { "branch": "main", "head": "<sha>" },
                "branches": [ { "branch": "feat/a", "base": "<sha>",
                                "pullRequest": { "number": 41, "merged": false } } ] } ] }
```

`rostrum_stack::LocalStacks` reads it (never writes it), and only to decide
whether `gh stack init` would duplicate tracking that already exists. A newer
schema is reported, not guessed at.

GitHub exposes stacks over REST, which is what the feed displays:

- `GET /repos/{owner}/{repo}/stacks` — open and closed stacks, newest first,
  each `{ id, number, node_id, url, base: { ref, sha }, open, created_at,
  pull_requests: [{ number, state, draft, merged_at, head: { ref, sha } }] }`,
  pull requests bottom first. **404 means stacked pull requests are not
  enabled for the repository.**
- `GET /repos/{owner}/{repo}/stacks?pull_request=N`, `GET …/stacks/{number}`
  exist too; rostrum does not need them.
- The write endpoints (`POST …/stacks`, `…/stacks/{n}/add`,
  `…/stacks/{n}/unstack`) are used only by `gh stack`; rostrum never calls
  them directly.

So the Stacks API is the source of truth for display; the local file is a
detail of `gh stack init`; rostrum's SQLite row (`cache_stack`) is a cache of
the API for cold starts.

## Data and control flow

### Reading

1. `SyncEngine`'s refresh of a repository lands (`Store::apply_refresh`).
2. `Store::fetch_stacks` (in `crates/rostrum/src/sync/stacks.rs`) issues
   `GitHubClient::stacks` — skipped for a repository with no open pull
   requests, and for an hour after a 404.
3. `rostrum_github::parse_stacks` decodes the page into `Stack`s, dropping
   closed stacks and skipping (with a warning) any it cannot represent.
4. `Store::apply_stacks` replaces `RepoState::stacks` if it changed, notifies,
   and writes the list to `cache_stack`. A failed read keeps the last answer.
5. On startup, `Store::open_database` loads `cache_stack` beside the cached
   pull requests and `hydrate_stacks` fills `RepoState::stacks` if nothing
   fresher has arrived.

### Grouping and display

`rostrum_core::flatten_in` calls `push_units` per repository:

1. `stack_groups(repo)` — GitHub's stacks first, each with its members present
   in `repo.prs` (bottom first; dropped if none are open), then
   `detect_chains` over everything not already claimed (two or more members).
   A pull request belongs to at most one group.
2. `units(visible, &groups)` — the visible pull requests (already filtered and
   in item order) become `FeedUnit::Single` or `FeedUnit::Stack`, a group
   taking the place of its first visible member.
3. Under `FeedOrder::Sorted`, units are stably sorted with feed-sort's
   `compare_groups` over each unit's members: text keys by the bottom member,
   time keys by the newest member (descending) or the oldest (ascending).
4. Each stack unit becomes a `FeedRow::StackHeader { repo, stack: StackIx }`
   followed by `FeedRow::PrRow { stack: Some(StackSlot { stack, place }) }`
   for each visible member. `Feed::stack(ix)` returns the `FeedStack` (repo
   index + `StackGroup`) the header renders.

The desktop's `render_stack_header` (`crates/rostrum/src/feed/stacks.rs`)
shows the title, trunk, "N not open" for merged/closed members, the
`MergeRollup` chip (`2/3 ready · conflict`), and the actions: **Merge stack**
and **Unstack** for a GitHub stack, **Make stack** for a detected chain.

Member rows are the feed's ordinary pull request rows (`feed/rows.rs`,
sharing `pr_row_content` with the repository view), with the chain glyph, the
stack indent and, while picking for an arrangement, the pick badge in a
column to the left of the body.

**Pull requests only.** Grouping happens on the Pull requests tab;
`flatten_tab` on the Issues tab never emits a stack row, and `Arrange PRs` is
not offered there (switching to Issues ends any picking).

**The repository view** lays its pull request half out with
`repo_pull_rows`, the same `push_units` for one repository, unfiltered: a
stack is contiguous and sorts as one unit, with its header drawn on the bottom
member's row. Its branch tree marks members with a `stack N` or `chain` chip.
See `docs/features/repo_view.md`.

### Make stack and Arrange (`rostrum_stack::run_stack_job`)

Both start from a `StackPlan`, validated by `rostrum_core::plan_stack`: two or
more open, same-repository, non-fork pull requests, no duplicates, no shared
head, a trunk that is no member's head, and none already in a GitHub stack.
`StackPlan::needs_rewrite()` is true when any member's base is not the branch
below it (or the trunk for the bottom): that is the difference between Make
and Arrange. The job:

1. **Preflight the handler** (Arrange only, when one is configured): template
   usable, no member's tmux session already running.
2. **Fetch** the trunk and every head and base into the clone, and record
   each head's remote oid — the **lease** — and each old base's oid.
3. **Rebase** (Arrange only), bottom up. A member is skipped unless its base
   changes or the member below was rewritten, and also skipped when its old
   base is contained in the new parent *and* the new parent is contained in
   it (already arranged — a rerun). Otherwise: a detached **scratch
   worktree** at the remote oid, `git rebase --onto <new parent>
   <old base oid>` with `rerere` on, so exactly the pull request's own
   commits move. No branch ref moves; nothing is pushed.
   - Conflict, no handler: abort, remove the worktree, return
     `StackOutcome::Conflicted`. GitHub is unchanged.
   - Conflict, handler: keep the worktree, gather the conflict context, start
     the handler in tmux, return `StackOutcome::HandedOff`. Nothing pushed.
4. **Push** each rewritten branch with `Repo::push_with_lease` against the
   lease. A refusal returns `StackOutcome::PushRejected { pushed, number }`.
5. **Align local branches**: create missing ones at the (rewritten) tip so
   `gh stack init` can adopt them; move ones that were exactly at the old
   remote tip, by compare-and-swap; leave checked-out or diverged ones and
   say so in `LocalNote`s.
6. **Link** on GitHub: `gh stack link --base <trunk> <pr>...` from the clone.
   Pull request *numbers*, never branch names — `link` pushes branch
   arguments. `link` also retargets each base to the branch below. A failure
   after pushes is `StackOutcome::LinkFailed`; with nothing pushed it is an
   error and nothing changed.
7. **Track** in the clone: `gh stack init --base <trunk> -- <branch>...` and
   then `gh stack view --json` to confirm, unless the stack is already
   tracked, a member is in another local stack, the clone is dirty or mid-
   operation, or the top branch is checked out in another worktree (init
   checks the top branch out in the clone). Skipping is reported in
   `LocalTracking::Skipped(reason)`; the stack still exists on GitHub.

After any finish the store refreshes the repository, which re-reads its
stacks, so the feed shows GitHub's answer.

**Re-running is the recovery path for every stop.** After `LinkFailed`, the
rewritten branches are already on their parents and are not rebased again.
After a handed-off conflict, the user finishes the rebase in the scratch
worktree (`git rebase --continue`); `rerere` (whose `rr-cache` the first run
created, which turns it on for later commands) records the resolution, and
the next run's rebase replays it and continues by itself. Stale scratch
worktrees with no operation in progress are removed at the start of each run.

### Add to stack (`rostrum_stack::run_extend_job`)

The header of every GitHub stack offers **Add to stack**, which starts
picking pull requests of that repository only (`PickTarget::Extend`; clicks
in another repository are ignored), and — when `rostrum_core::continuations`
finds a line of open pull requests already built on the stack's top — an
**Extend with #a ← #b** button that opens the panel with that line ordered.

`rostrum_core::plan_extend(repo, stack, order)` validates, as typed
`ExtendError`s: the stack is one of the repository's GitHub stacks, its top
member is open (its branch is what the additions build on), there is at least
one addition, and every addition is open, same-repository (not a fork),
listed once, not already in this stack, in no other stack, and not heading
one of the stack's own branches or sharing a head with another addition.

`ExtendPlan::is_chained(ix)` asks whether addition `ix` already targets what
it must (the top's head for the first, the previous addition's head for the
rest). `needs_rewrite()` is any addition not chained; `rewrites()` is the
first unchained addition and every one above it — exactly the branches the
panel names in its confirmation, since a rebased branch moves everything on
it.

`run_extend_job` runs the same pipeline as Make/Arrange (`run_chain`), rooted
at the stack's top branch instead of a trunk: fetch the top and every
addition's head and base, rebase what needs it onto the one below (the first
onto the top's remote tip) in scratch worktrees, lease-push each rewrite,
align local branches, then `gh stack link <stack> <pr>...` — numbers only, so
`link` pushes nothing, and it retargets the additions' bases. The top branch
and the stack's existing members are never rebased or pushed. The outcome is
`StackOutcome::Extended { stack, report }`, or the same stop outcomes as
Arrange (conflict, hand-off, rejected lease, link failed), each re-runnable:
a rerun skips additions already on their new parent.

### Merge stack

`rostrum_stack::merge_stack` runs `gh stack merge <number> --yes
--merge-method <merge|squash|rebase>` with `GH_REPO` pinned, from the clone or
— since a merge by number reads nothing local — from the scratch directory
when there is no clone. GitHub merges every open member or none; a refusal
(branch protection, a draft partway up) comes back as gh's own message. The
confirmation panel lists every member with its merge state, the method, and
the all-or-nothing note.

### Unstack

`gh stack unstack <number>` from the clone (it looks the number up locally
before going to GitHub, and fails outside a repository, so it needs a clone).
The pull requests stay open with their current bases.

### From a paired phone (`rostrumd`)

The phone cannot run `gh`. `rostrumd` exposes Make stack, Arrange, Add to
stack, Merge stack and Unstack as authenticated routes
(`docs/features/remote_protocol.md`, "Stacks from a phone") and runs them
with the functions above, unchanged: `run_stack_job`, `run_extend_job`,
`merge_stack`, `unstack`, over `GhCli`, with the repository's configured
clone and conflict handler and **the desktop's own scratch-worktree
directory** (`~/.cache/rostrum/stack-worktrees`), so a conflict handed off
from the phone can be finished, and the arrangement re-run, from either
side.

Validation is the desktop's: rostrumd fetches the repository's open pull
requests and GitHub's stacks for the request and calls `plan_stack` /
`plan_extend`. For a rewrite, the request's `confirm_rewrite` must equal
`StackPlan::rewrites()` / `ExtendPlan::rewrites()` as a set — the same list
the desktop's Arrange and Add to stack panels name — or nothing runs (409
`rewrite_not_confirmed`); Make stack on a chain that would need a rewrite is
refused the same way. A dry-run route returns that list for the phone to show.

Each operation is a job the phone polls, holding the clone's lease in
rostrumd's job coordinator: one job of any kind per clone at a time. (The
desktop's own "one stack operation at a time" is per process; the desktop and
the daemon do not coordinate, and git's own locks and the leased pushes are
what keep a simultaneous run from both safe.)

## The push exception

Rostrum's standing rule is that it never writes to a remote (OVERVIEW,
foundational decisions). Arrange is the one, deliberate exception, because a
rebase onto another branch is invisible to the pull request until it is
pushed. It is kept as narrow as possible:

- One function pushes: `rostrum_git::Repo::push_with_lease`. Its argv is
  `push --porcelain --force-with-lease=refs/heads/<b>:<expected-oid> --
  origin <new-oid>:refs/heads/<b>`, built by the pure `push_args`. There is
  no bare force, no lease without an expected oid, no `--no-verify`, one ref
  per call.
- One caller: `run_stack_job`, only after every member rebased cleanly, only
  for branches it rewrote, with the oid it fetched at the start as the lease.
- `gh stack` is never allowed to push: `submit`, `push`, `sync` and `rebase`
  have no `GhStackCommand` variant, and `link` is given numbers.
- The confirmation states it: the Arrange and Add to stack panels require
  ticking "these branches will be rebased and force-pushed (with lease)"
  (Add to stack names each branch), and reordering clears the tick.
- An extension never rewrites the stack it extends: the top branch is the
  root of the rebase, not a member of it.

## Running `gh`

Every `gh` call goes through `rostrum_stack::gh::GhRunner`; `GhCli` is the real
one. It follows rostrum-git's `command.rs` discipline — a per-command timeout
that kills the child, stdin closed, output captured — with the environment
rule of rostrum-handoff's tmux: inherit the user's environment, because `gh`
authenticates through `GH_TOKEN`, `GH_CONFIG_DIR`, the keyring and `HOME`,
then remove `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_COMMON_DIR`,
set `GH_REPO=<owner>/<name>` so a fork remote cannot redirect a merge, and
force `GH_PROMPT_DISABLED=1`, `GIT_TERMINAL_PROMPT=0` and no colour. stdout is
not a terminal, so gh-stack's interactive paths are never taken.

| Command | argv | Timeout | Cwd |
|---|---|---|---|
| `Link` | `stack link --base <trunk> <n>...` | 180 s | clone |
| `LinkExtend` | `stack link <stack> <n>...` | 180 s | clone |
| `Init` | `stack init --base <trunk> -- <branch>...` | 60 s | clone |
| `ViewJson` | `stack view --json` | 60 s | clone |
| `Merge` | `stack merge <stack> --yes --merge-method <m>` | 900 s | clone or scratch dir |
| `Unstack` | `stack unstack <stack>` | 180 s | clone |

## Files

| File | Role | Key exports |
|---|---|---|
| `crates/rostrum-core/src/stack/model.rs` | The types | `Stack`, `StackNumber`, `StackMembers`, `RefName`, `StackError` |
| `crates/rostrum-core/src/stack/detect.rs` | Chain detection | `detect_chains` |
| `crates/rostrum-core/src/stack/group.rs` | Groups, units, rollup | `stack_groups`, `StackGroup`, `units`, `FeedUnit`, `MergeRollup`, `StackIx` |
| `crates/rostrum-core/src/stack/plan.rs` | Validating a request; which members a rewrite touches | `plan_stack`, `StackPlan` (`rewrites`), `PlanMember`, `PlanError` |
| `crates/rostrum-core/src/stack/extend.rs` | Validating an extension, chained-vs-rewrite, lines past a top | `plan_extend`, `ExtendPlan`, `ExtendError`, `continuations`, `Continuation` |
| `crates/rostrum-core/src/stack/feed_tests.rs` | Stacks in `flatten`, including sorting | — |
| `crates/rostrum-core/src/feed.rs` | `push_units`; `FeedRow::StackHeader`, `StackSlot`, `StackPlace`, `FeedStack`; one repository's rows for the repository view | `Feed::stack`, `repo_pull_rows` |
| `crates/rostrum-core/src/sort/compare.rs` | feed-sort's group hook stacks sort through | `sort_key_for_group`, `compare_groups` |
| `crates/rostrum-github/src/stacks.rs` | Stacks API wire types and decoding | `parse_stacks`, `RepoStacks` |
| `crates/rostrum-github/src/client/stacks.rs` | The GET | `GitHubClient::stacks` |
| `crates/rostrum-db/src/stacks.rs` | The `cache_stack` table | `Db::save_stacks`, `Db::load_stacks` |
| `crates/rostrum-git/src/push.rs` | The leased push's argv and verdict | `push_args`, `classify_push`, `PushOutcome`, `PushRejection` |
| `crates/rostrum-git/src/repo/rewrite.rs` | Scratch worktrees, rebase, CAS branch moves, the push | `Repo::{resolve, is_ancestor, set_branch, add_scratch_worktree, remove_scratch_worktree, rebase_scratch, push_with_lease}`, `RefExpectation` |
| `crates/rostrum-stack/src/gh/argv.rs` | Every `gh stack` call as a type | `GhStackCommand`, `MergeMethod` |
| `crates/rostrum-stack/src/gh/runner.rs` | The one place `gh` is spawned | `GhRunner`, `GhCli`, `GhOutput` |
| `crates/rostrum-stack/src/gh/view.rs` | `gh stack view --json` | `StackView` |
| `crates/rostrum-stack/src/local_file.rs` | Reading `<git-dir>/gh-stack` | `LocalStacks`, `LocalStack` |
| `crates/rostrum-stack/src/job.rs` | Job, progress, outcomes | `StackJob`, `StackProgress`, `Progress`, `StackOutcome`, `StackReport`, `LocalTracking`, `LocalNote` |
| `crates/rostrum-stack/src/run.rs` | Make, Arrange and Add to stack, over one shared pipeline | `run_stack_job`, `run_extend_job` |
| `crates/rostrum-stack/src/remote.rs` | Merge and unstack | `merge_stack`, `unstack` |
| `crates/rostrum-stack/src/error.rs` | Typed errors | `StackOpError` |
| `crates/rostrum/src/sync/stacks.rs` | Store: reading stacks, running one operation, progress | `StackSync`, `Store::{make_stack, extend_stack, merge_stack, unstack, stack_op}` |
| `crates/rostrum/src/feed/stacks.rs` | Header row, glyphs, confirmation panels, status line | `StackUi`, `StackPanel` |
| `crates/rostrum/src/feed/rows.rs` | A member row: glyph, indent, pick badge around the shared body | — |
| `crates/rostrum/src/repo_view/order.rs` | Stacks in the repository view's pull request list | `ListOrder::{slot_at, header_at}` |
| `crates/rostrum/src/feed/arrange.rs` | Picking (for a new stack or an extension), ordering, trunk, rewrite confirmation | `Picking`, `PickTarget` |
| `crates/rostrum/src/feed/extend.rs` | The Add to stack panel | `describe` |

## Invariants and constraints

- **A pull request is in at most one group**, GitHub's stacks taking
  precedence over detected chains, and is rendered exactly once.
- **Stack rows are contiguous** inside their repository's run: header, then
  visible members bottom first. Container chrome is unaffected — a header is a
  repository row like any other.
- **A stack sorts as one unit** and its members never reorder among
  themselves.
- **Detection never groups a fork, an ambiguous head, a branch point, or a
  cycle.**
- **Nothing is pushed until every member has rebased**, and every push is
  leased against the oid fetched at the start.
- **No branch ref a worktree has checked out is moved**, and every other move
  is a compare-and-swap.
- **Every stop is re-runnable**: rerunning skips what is already arranged and
  `rerere` replays recorded resolutions.
- **Only `gh stack link`, `init`, `view`, `merge` and `unstack` are ever
  run.** Tests never run a mutating `gh stack` command: the runner is a trait
  and the tests assert the exact argv a double receives.
- **One stack operation at a time**, across all repositories, in the desktop
  app; one job per clone in `rostrumd`.
- **A rewrite is confirmed by name.** The desktop's panels list
  `rewrites()` and require the tick; a phone must send exactly that list as
  `confirm_rewrite`.
- **Callable without the UI.** `run_stack_job`, `run_extend_job`,
  `merge_stack` and `unstack` take plain inputs (a clone path, a validated
  plan or a stack number, an optional handler, a scratch directory, a
  `GhRunner`) so `rostrumd` can expose them to a paired phone.
