# Feature: repo_view

One repository on its own screen. The left pane stops being the feed across
every repository and becomes that repository's pull requests (top) and issues
(bottom). With nothing selected, the right pane shows the **branch tree**:
each trunk with its distance from the default branch, the pull requests based
on it beneath, and pull requests stacked on those beneath them. Desktop only.

## Scope

- Navigation between the feed and a repository's view, and what happens to
  the selection on the way in and out.
- The sidebar: header (name, stars, *Open on GitHub*, *Branches*, the trunk
  editor), and two independently scrolling virtualized lists.
- The branch tree: trunk resolution, the comparison batch, tree building and
  its placement rules, and drawing it.
- Per-repository trunk configuration in `config.json`.

## Non-scope

- Fetching pull requests and issues. The view reads `RepoState::prs` and
  `RepoState::issues` from the store exactly as the feed does; issues are
  fetched by `feat/issues` (until the desktop store does so, the issues half
  says "not loaded yet").
- The detail panes. Selecting a pull request opens the same `PrDetail` the
  feed opens. There is no desktop issue detail yet; selecting an issue shows
  a placeholder in the right pane.
- Local worktree drift in the tree. `rostrum_local::local_state` can answer
  it per branch, but it fetches per branch; see *Deferred*.
- The Android app.

## Navigation

`rostrum_core::Screen` is the left pane's screen: `Feed` or
`Repo(RepoScreen)`. It lives on the desktop `Workspace`, not in the feed's
rows, and it is pure, so its rules are unit tested:

| Transition | Selection |
|---|---|
| Enter a repository from the feed | Kept if it is in that repository (pull request or issue); otherwise cleared, so the branch tree shows. The feed's selection is remembered either way. |
| Enter another repository from a repository | Same rule; the selection remembered on leaving the feed is kept, not replaced. |
| Enter the repository already shown | No-op. |
| Back, with something picked in the view | Kept — it is a pull request the feed lists too. |
| Back, with nothing picked | The remembered feed selection comes back. |

The feed's scroll position needs no help: the `FeedView` entity, and with it
its `ListState`, is kept while the repository is open — merely not drawn.

Ways in: click a repository header's **name** (the rest of the header still
collapses), its **open ›** affordance, or press **`o`** in the feed, which
opens the selected pull request's repository. Ways out: **‹ Feed** in the
header, or **`escape`** / **`backspace`** in the view (escape closes the
trunk editor first if it is open).

Keys in the view (`RepoView` context, on the lists only): `j`/`k`/`↓`/`↑`
move through pull requests and on into issues as one sequence, without
wrapping (`repo_view::nav::step`, unit tested); `g g`/`shift-g` jump to the
ends; `enter` focuses the detail pane; `b` clears the selection to show the
branch tree.

Entering a repository creates three entities — `RepoBranches` (data),
`RepoView` (left pane) and `BranchesPane` (right pane) — held together in
`Workspace::repo`. Leaving drops them, which cancels any fetch in flight.

## Data flow: the branch tree

```
config.json "trunks"  ──► TrunkChoice ──► names_to_probe()
                                              │
                        GitHubClient::repo_branch_meta (1 request)
                                              ▼
                                          RepoMeta { url, stars, default_branch, existing }
                                              │
                       Trunks::resolve(default, choice, existing)
                                              ▼
              ComparePlan::new(trunks, repo.prs)   ──►  pairs: [(base, head)]
                                              │
                       GitHubClient::divergences (1 aliased request)
                                              ▼
                       plan.answer(answers) ──► BranchCounts
                                              │
         build_tree(trunks, current prs, counts) ──► BranchTree ──► rows()
```

1. **Lazily on entering.** `RepoBranches::new` fetches immediately.
2. **Meta.** `build_branch_meta_query(n)` asks for `url`, `stargazerCount`,
   `defaultBranchRef { name }` and one aliased `rN: ref(qualifiedName: $rN)`
   per probed name; names go as `refs/heads/<name>` variables, never into the
   document text. A missing branch answers `null` with no error.
3. **Trunks.** The default branch is always first. With no configuration the
   others are whichever of `main`, `master`, `staging`, `develop` exist, in
   that order; configured, they are the configured names in order, existing
   or not (a missing one shows a `missing` chip and is not compared).
4. **Comparisons — one aliased document.** `ComparePlan` lists every
   existing non-default trunk against the default branch, then every open
   pull request's head against its base, and hands the pairs to the same
   `divergences` batch the feed uses (`pN: ref(qualifiedName: $bN) {
   compare(headRef: $hN) }`). A cross-fork head answers `null` with a
   per-alias `NOT_FOUND` the batch tolerates, so it files as unknown, never
   as an error. `ComparePlan::answer` files answers by identity
   (`CompareKey::Trunk`/`Pull`) and refuses a misaligned batch outright.
5. **Tree.** Built on every paint from the latest pull requests and the
   latest counts, which are keyed by name/number, so a poll that reorders the
   list cannot misattribute a count. A pull request's count falls back to its
   feed-batch `base_divergence` until the branch batch answers.
6. **Refresh.** `RepoBranches` observes the store; whenever this repository's
   `LoadState::Loaded { at }` changes (the feed poll, or `ctrl-r`), it fetches
   again. A failed fetch keeps the previous snapshot and says so in the pane
   header. **Refresh** in the pane forces a fetch.
7. **Trunk edits** go through `RepoBranches::set_choice` →
   `Store::set_trunk_choice` (config written immediately) → fetch.

### Placement rules (`build_tree`)

In precedence order:

1. A base that is a trunk puts the pull request under that trunk — even when
   some pull request's head shares the trunk's name (a fork's `main`).
2. A base that is the head of exactly one *other* open pull request nests it
   beneath that pull request, so stacks appear without being declared.
3. Anything else goes to **Other bases**, grouped by base name and sorted:
   unknown bases, a base two heads share (`PullNote::AmbiguousBase` — forks
   reusing a branch name), and a pull request based on its own head name.
4. Loops (A based on B's head, B on A's) never reach a trunk. Each is cut at
   its lowest-numbered member, which goes to Other bases with
   `PullNote::BreaksCycle`; the rest of the loop and anything stacked on it
   nest beneath it.

Siblings are ordered by number. Every pull request appears exactly once.

### Row contents

- **Trunk**: name; `default` for the default branch, otherwise `↑a ↓b`
  against the default branch, `↑? ↓?` if unknown, or `missing`; the number of
  pull requests anywhere beneath.
- **Pull request**: indented per stacking level; head branch name; `#N` and
  title (click selects it and opens the detail); `↑a ↓b` against its base,
  `↓` in the warning colour when behind, `↑? ↓?` when unknown; `draft`; the
  merge chip (`MergeStatus::chip`, coloured and explained exactly as in the
  feed); `cycle`/`ambiguous` notes.
- **Other bases** heading, and per base a `unknown base` chip and count.

## Trunk configuration

```json
"trunks": { "owner/name": ["main", "staging"] }
```

- No entry → `TrunkChoice::Detected`.
- An array → `TrunkChoice::Configured`, in order; `[]` means the default
  branch alone, which is different from no entry.
- Names are validated by `TrunkName::parse` (trimmed, `refs/heads/` dropped,
  the parts of `git check-ref-format` a typo can break). An invalid name is
  skipped with a `Warning`; the rest of the list stands.
- Removing a repository removes its entry, like its clone path.

The **trunks** popover in the view header lists the default branch (fixed)
and the other trunks with a remove control, an input to add one, and
*Detect automatically* to drop the entry. Editing a detected choice first
turns it into the configured list it amounted to (`TrunkChoice::adding` /
`removing`), so adding `qa` to an auto-detected `staging` gives
`staging, qa`.

## Layout

The sidebar is a fixed 50/50 vertical split. Each half is a header with a
count (`—` for issues until they have loaded once) and its own `gpui::list`
over `RepoState::prs` / `RepoState::issues`, so each scrolls and virtualizes
independently. Pull request rows are drawn by `feed::pr_row_content`, the
same function the feed uses, so the two lists cannot drift apart in what a
row says.

## Invariants

- The default branch is always the first trunk and never appears twice
  (`Trunks` holds it apart from `others`).
- `ComparePlan::keys` and `pairs` are index-aligned; answers are filed by key,
  never by position, and a batch of the wrong length is rejected.
- Branch names reach GitHub only as GraphQL variables.
- An unknown count is absence (`None`/`TrunkDrift::Unknown`), never an error.
- `Workspace::repo` is `Some` exactly when `Workspace::screen` is
  `Screen::Repo`; a repository removed while open sends the view back to the
  feed.
- Each list's `ListState` item count equals its vector's length
  (`RepoView::store_changed` resets on change).

## Deferred

- **Local worktree drift** in the tree. Doing it per branch means a fetch per
  branch through `local_state`; worth a batched local comparison first.
- **Resizable split.** Fixed 50/50 for now.
- **Desktop issue detail and issue fetching**: owned by `feat/issues`.
- **Cross-fork heads that share a trunk's name** can compare against the base
  repository's branch of that name, as in the feed; telling them apart needs
  `isCrossRepository` on the feed query.

## Files

| File | Role |
|---|---|
| `crates/rostrum-core/src/navigation.rs` | `Screen`, `RepoScreen`: enter/back and the selection rules |
| `crates/rostrum-core/src/branches/mod.rs` | Module map and re-exports |
| `crates/rostrum-core/src/branches/name.rs` | `TrunkName` (validated, serde as a string), `TrunkNameError` |
| `crates/rostrum-core/src/branches/trunks.rs` | `TrunkChoice` (+ `adding`/`removing`/`names_to_probe`), `RepoMeta`, `Trunks::resolve`, `DEFAULT_TRUNK_CANDIDATES` |
| `crates/rostrum-core/src/branches/plan.rs` | `ComparePlan`, `CompareKey`, `BranchCounts`, `PlanError` |
| `crates/rostrum-core/src/branches/tree.rs` | `build_tree`, `BranchTree`, `TrunkNode`, `PullNode`, `BaseGroup`, `TrunkDrift`, `PullNote` |
| `crates/rostrum-core/src/branches/rows.rs` | `BranchTree::rows` → `BranchRow` |
| `crates/rostrum-core/src/branches/tests.rs` | Placement, stacks, loops, unknown bases, trunk order, counts |
| `crates/rostrum-config/src/lib.rs` | `Config::trunks`, `trunk_choice`, `set_trunk_choice` |
| `crates/rostrum-github/src/branches.rs` | `build_branch_meta_query`, `branch_meta_variables`, `BranchMetaData`, `GitHubClient::repo_branch_meta` |
| `crates/rostrum/src/repo_view/mod.rs` | `RepoView` (sidebar), actions and key bindings, `RepoViewEvent` |
| `crates/rostrum/src/repo_view/model.rs` | `RepoBranches` (fetch, refresh-on-poll, `set_choice`), `BranchFetchError`, `Store::set_trunk_choice` |
| `crates/rostrum/src/repo_view/branches.rs` | `BranchesPane`, the tree's rendering |
| `crates/rostrum/src/repo_view/trunk_editor.rs` | The trunks popover |
| `crates/rostrum/src/repo_view/issues.rs` | Issue rows and the issues half's empty states |
| `crates/rostrum/src/repo_view/nav.rs` | `step`: keyboard movement across both lists |
| `crates/rostrum/src/feed/pr_row.rs` | `pr_row_content`, shared by the feed and the view |
| `crates/rostrum/src/main.rs` | `Workspace::open_repo` / `close_repo`, pane switching |
