# Feature: issues

Open issues across the watched repositories, beside the pull requests: a
second tab in the feed, a detail pane for one issue, and a form that opens a
new one.

## Scope

- The feed's **Pull requests | Issues** tab bar, with per-tab counts, the
  `[`/`]` keys, and the selected tab persisted in the config.
- Fetching each repository's open issues in the same poll cycle as its pull
  requests, caching them, and painting them on a cold start.
- Flattening issues into the feed with the same container layout, filters,
  hide-empty and collapse rules as pull requests.
- Selection as an enum over pull request and issue, by identity.
- The issue pane: header (state, author, milestone, editable labels and
  assignees), timeline (body, comments, events), composer, and actions —
  comment, close as completed or not planned, reopen, label and assignee
  changes.
- Creating an issue: repository, title, markdown body with Write/Preview,
  labels, assignees.

## Non-scope

- The Android app. The model, decoding, requests, cache and draft rules all
  live in gpui-free crates so `rostrum-ffi` can reuse them, but the phone has
  no issue list yet; `rostrum-ffi` maps the new timeline events onto its
  existing types and ignores `FeedRow::IssueRow`.
- Notifications for new issues. `Baseline` watches pull requests only.
- Editing an issue's title or body, milestones, issue types, sub-issues,
  reactions, pinning, locking, transferring, and marking duplicates. GitHub's
  third close reason, `duplicate`, is shown when GitHub reports it but cannot
  be chosen here — it is set by marking a duplicate, not by closing.
- Mentions as involvement. See [Involvement](#involvement).
- Issue templates and forms. The body is free markdown.

## Data model

`crates/rostrum-core/src/issue.rs`:

```rust
pub struct IssueNumber(pub u32);              // distinct from PrNumber
pub enum CloseReason { Completed, NotPlanned, Duplicate }
pub enum IssueState { Open, Closed(Option<CloseReason>) }
pub struct Issue {
    number, node_id, title, url, state, created_at, updated_at,
    author: Option<User>, assignees: Vec<User>, labels: Vec<Label>,
    comment_count: u32, milestone: Option<Milestone>,
}
pub struct IssueDetail { issue: Issue, conversation: Conversation }
pub struct IssueTitle(String);                // non-blank, trimmed
```

- **A close reason lives inside `Closed`.** GitHub reports `state` and
  `stateReason` as two fields, and `REOPENED` as the reason of an *open*
  issue. Collapsing them into one enum makes "open with a close reason"
  unrepresentable; `issue_state` in the wire layer does the folding, and
  `REOPENED` is history (it shows in the timeline), not state.
- **`IssueNumber` is not `PrNumber`.** The two share a number space per
  repository and GitHub's issues REST endpoints accept either, but which pane
  opens and which query runs depend on the kind, so the type carries it.
- **`IssueDetail` carries the issue as well as the timeline.** Closing an
  issue drops it from the open list on the next poll; the pane must still show
  it, and offer Reopen, so it does not read the header from the feed once the
  detail query has answered.
- **`IssueTitle` cannot be blank.** GitHub answers a blank title with a 422;
  validating at construction means the create request cannot be built without
  one.

`crates/rostrum-core/src/state.rs`:

```rust
pub struct RepoState { …, issues: Vec<Issue>, issues_load: LoadState, … }
pub enum Selection {
    PullRequest { repo: RepoId, number: PrNumber },
    Issue { repo: RepoId, number: IssueNumber },
}
pub struct AppState { …, tab: FeedTab, selection: Option<Selection> }
```

Issues have their own `LoadState`: the two lists are separate requests, one can
fail while the other lands, and each tab reports its own state.

`crates/rostrum-core/src/tabs.rs`: `FeedTab { PullRequests, Issues }` (serde
`snake_case`, default pull requests), `TabCounts`, `tab_counts`.

`crates/rostrum-core/src/feed.rs`: `FeedRow::IssueRow { repo, issue: IssueIx }`,
`FeedRow::is_item`, `FeedFilter::accepts_issue`, `flatten_tab`, and `Feed::tab`.

`crates/rostrum-core/src/timeline.rs`: `EventKind` gains `Unassigned`,
`ClosedAs(CloseReason)` and `CrossReferenced { source, title }`, and
`EventKind::describe` — the one phrasing of every event, used by the desktop
and the phone.

## Control flow

### Fetching

1. `Store::refresh_all` (startup, the poll timer, `ctrl-r`) calls
   `refresh_repo` **and** `refresh_issues` for every repository; `add_repo`
   does both for the new one.
2. `refresh_issues` (`crates/rostrum/src/sync/issues.rs`) is guarded by
   `pending_issues: HashMap<RepoId, Task<()>>`, separate from the pull request
   guard, so a merge probe re-issuing the pull request query neither waits on
   nor cancels an issue fetch. A spinner shows only on first load.
3. `GitHubClient::open_issues` posts `OPEN_ISSUES`
   (`crates/rostrum-github/src/issues/wire.rs`):
   `issues(states: OPEN, first: $first, orderBy: UPDATED_AT DESC)` with the
   number, title, url, state, stateReason, timestamps, author, assignees and
   labels (ten each), comment count and milestone. GraphQL `issues` never
   returns pull requests, so nothing is filtered. Captured live at cost 1.
4. `apply_issue_refresh` replaces `repo.issues`, sets `issues_load`, logs the
   rate limit, and writes the list to the cache fire-and-forget. A failure sets
   `issues_load = Failed` and keeps any issues already held.

Why a second request rather than a selection on the pull request query:
separate load states, no coupling to the merge-probe timer, and no conflict
with the pull request query's own evolution. The price is one more request per
repository per poll, each costing one point.

### Cold start

`Store::open_database` loads each repository's cached pull requests **and**
issues; `hydrate` fills each list only where the network has not already
delivered one.

### Flattening and the tab

`FeedView::build` calls `flatten_tab(repos, filter, state.tab)`. Both tabs
share every rule — one header per repository, contiguous runs, `Spacer`
between, hide a repository only once its list has *loaded* empty, collapse
contributes header and spacer — and differ only in which list and which load
state they read. `Feed` remembers its tab and compares by it, so switching
between two streams of identical notice rows still rebuilds.

The feed's sort applies on this tab too (see `docs/features/feed_sort.md`):
repositories in the repository sort, issues within each in the **item** sort
the pull requests use (`order_issues`). Issues have no branch, so the
**pushed** key orders them by `updatedAt`. Repository "updated" counts open
issues as well as pull requests.

### In a repository's view

A repository's own view (`docs/features/repo_view.md`) lists the same
`RepoState::issues` in the bottom half of its sidebar — unfiltered, in the
same item sort, drawn by the same `issue_row_content` the feed's issue row
uses. Selecting one sets the same `Selection::Issue`, so the workspace opens
the same `IssuePane`; its **+ New issue** opens the same form preset to that
repository.

`FeedFilter::accepts_issue` applies the search (title, number, author, labels,
milestone) and the author selection; `hide_drafts` does not apply and the
drafts button is hidden on the Issues tab. The filter bar's "N of M shown",
the tab badges (`tab_counts`), and the authors popover (`issue_roster`) all
follow the active tab.

`Store::set_tab` writes `config.feed_tab` and saves. `Config::feed_tab` reads
leniently: an unknown value falls back to pull requests instead of discarding
the file.

### Involvement

With *include involved in* checked, an issue matches a selected person when
they opened it or are assigned to it. Mentions are not counted: the feed query
has no bounded way to ask who an issue mentions short of reading every comment
body, and assignment is the signal that means "waiting on you".

### Selection and navigation

Clicking a row stores `Selection::Issue { repo, number }`, resolved from the
row's indices at that moment (`selection_for` in `feed/mod.rs`). `nav.rs`
resolves a selection to a row of the *current* feed: a pull request selection
has no row in the Issues feed, so `j`/`k` stay within the active tab and enter
it from its end. The selection survives a tab switch, so the detail pane keeps
what was open.

### The detail pane

`Workspace::sync_detail` (`main.rs`) holds a `DetailPane`:

```rust
enum DetailPane {
    PullRequest(Entity<PrDetail>),
    Issue(Entity<IssuePane>),
    NewIssue { form: Entity<NewIssueForm>, _cancelled: Subscription },
}
```

A selection that matches what is shown leaves the entity alone; any other
replaces it, which drops the old one and cancels its requests. The form is not
a selection — opening it clears the selection, and a store change with nothing
selected leaves the form up.

`IssuePane::new` (`crates/rostrum/src/issue/mod.rs`) paints from the cache
(`Db::load_issue_detail`, gap-filling only) and fetches `ISSUE_DETAIL`: the
summary fields plus the body, the first 100 comments, and the first 100
timeline events of the types it renders — `CLOSED_EVENT` (with `stateReason`),
`REOPENED_EVENT`, `LABELED_EVENT`, `UNLABELED_EVENT`, `ASSIGNED_EVENT`,
`UNASSIGNED_EVENT`, `RENAMED_TITLE_EVENT`, `CROSS_REFERENCED_EVENT`. The
decode reuses the pull request conversation's `IssueCommentNode` and
`TimelineEventNode` (now taught the issue events), sorts body-first then
chronologically, and is cached by `Db::save_issue_detail`.

Until the detail answers, the header comes from the feed's copy of the issue,
so the pane paints at once.

The timeline is drawn by `detail::conversation::render_plain_item`, the same
function the pull request pane uses for its body, comments and events, so the
two read identically.

### Mutations

Every change is an `IssueMutation` (`crates/rostrum-github/src/issues/rest.rs`)
that describes its own REST call:

| Mutation | Request |
|---|---|
| `Comment(CommentBody)` | `POST /repos/{o}/{r}/issues/{n}/comments` `{"body"}` |
| `SetState(Close(Completed))` | `PATCH /repos/{o}/{r}/issues/{n}` `{"state":"closed","state_reason":"completed"}` |
| `SetState(Close(NotPlanned))` | same, `"state_reason":"not_planned"` |
| `SetState(Reopen)` | same, `{"state":"open","state_reason":"reopened"}` |
| `AddLabels(AddLabels)` | `POST …/issues/{n}/labels` `{"labels":[…]}` |
| `RemoveLabel(name)` | `DELETE …/issues/{n}/labels/{percent-encoded name}` |
| `AddAssignees(Assignees)` | `POST …/issues/{n}/assignees` `{"assignees":[…]}` |
| `RemoveAssignees(Assignees)` | `DELETE …/issues/{n}/assignees` `{"assignees":[…]}` |

`GitHubClient::mutate_issue` sends nothing for a no-op (an empty list) and
treats a 404 on label removal as success, as the pull request pane does.

`IssuePane::mutate` follows the pull request pane's `mutate()` contract: one
mutation in flight (`busy`), the affordances drop their click handlers while it
is, failures land in the error banner, and success reloads authoritatively —
the detail query again, and `Store::refresh_issues` for the repository so a
close or reopen moves the row. Nothing is patched locally.

Close and reopen ask for no confirmation: each is undone by the other and
neither destroys anything. The buttons carry the end state they move to, fixed
at render, so a poll landing before the click can only make a request
redundant, never invert it.

The label and assignee pickers load lazily on first open
(`GitHubClient::repository_labels`, `GitHubClient::assignable_users` — `GET
/repos/{o}/{r}/assignees`, paginated) and are drawn by the shared
`crate::pickers`, which the pull request pane's label picker now uses too.

### Creating

`FeedEvent::NewIssue { repo }` — from the Issues tab bar (`repo: None`, the
form defaults to the first watched repository) or a repository header's
`+ New issue` (`repo: Some(..)`) — makes the workspace open `NewIssueForm`
(`crates/rostrum/src/issue/create.rs`) and switch the feed to Issues.

The form's rules are `IssueDraft` (`crates/rostrum-github/src/issues/draft.rs`):
the chosen repository, the picked labels and assignees, and
`IssueDraft::request(title, body)`, which fails with `DraftError::NoRepository`
or `DraftError::EmptyTitle` and otherwise yields the `CreateIssue` body.
Changing repository drops the picked labels and assignees, which belong to the
old one. The Create button is disabled without a title, and the draft refuses
one anyway.

`GitHubClient::create_issue` posts `{"title","body","labels","assignees"}` —
empty parts omitted — and decodes the new number. On success the form
refreshes the repository's issues and calls `Store::reveal` with the new
issue's selection, which replaces the form with its pane. On failure the form
stays, with GitHub's reason, and nothing typed is lost.

## Invariants and constraints

- **A feed holds one kind of item row.** `flatten_tab` emits `PrRow`s or
  `IssueRow`s, never both; navigation and selection rely on it.
- **Contiguity and chrome rules are shared.** The repo_feed invariants hold on
  both tabs because one function builds both.
- **Selection is by identity and kind**, never by feed index; an issue
  selection never resolves against a pull request with the same number, nor
  the reverse (`a_selection_resolves_only_against_its_own_kind`).
- **Each list has its own load state**, and only a `Loaded` list can hide its
  repository.
- **Tab counts ignore collapse**; they count open items the filter accepts.
- **One issue mutation in flight**, then an authoritative reload; no local
  patching.
- **No blank titles or comments reach GitHub** — `IssueTitle` and
  `CommentBody` cannot be built blank.
- **`feed_tab` round-trips through the config** and an unknown value never
  costs the rest of the file. It is saved alongside `repo_sort`/`item_sort`,
  and the tab and the sort both survive a restart together.
- **Issues follow the item sort**; "pushed" on an issue is its "updated".
- **Cache tables are additive.** `cache_issue` and `cache_issue_detail` are
  created `IF NOT EXISTS` on every open, so they appeared without a schema
  bump and the cached pull requests survived. Both are pruned like the rest of
  the cache.

## Files

| File | Role | Key exports |
|---|---|---|
| `crates/rostrum-core/src/issue.rs` | Issue domain types | `Issue`, `IssueNumber`, `IssueState`, `CloseReason`, `Milestone`, `IssueDetail`, `IssueTitle`, `EmptyTitle` |
| `crates/rostrum-core/src/tabs.rs` | The feed's tabs and their counts | `FeedTab`, `TabCounts`, `tab_counts` |
| `crates/rostrum-core/src/feed.rs` | Flattening for either tab | `FeedRow::IssueRow`, `IssueIx`, `flatten_tab`, `FeedFilter::accepts_issue` |
| `crates/rostrum-core/src/state.rs` | Issues on `RepoState`, the selection enum, the tab | `Selection`, `AppState::{tab, selected_issue, total_open_issues}` |
| `crates/rostrum-core/src/authors.rs` | Author roster from issue authors | `issue_roster` |
| `crates/rostrum-core/src/timeline.rs` | Issue events and their phrasing | `EventKind::{Unassigned, ClosedAs, CrossReferenced}`, `EventKind::describe` |
| `crates/rostrum-github/src/issues/wire.rs` | GraphQL documents and decoding | `OPEN_ISSUES`, `ISSUE_DETAIL`, `IssueNode`, `issue_state` |
| `crates/rostrum-github/src/issues/rest.rs` | REST request bodies | `IssueMutation`, `RestCall`, `IssueStateChange`, `CloseAs`, `CommentBody`, `Assignees`, `CreateIssue`, `CreatedIssue`, `AssignableUser` |
| `crates/rostrum-github/src/issues/draft.rs` | The new-issue form's rules | `IssueDraft`, `DraftError` |
| `crates/rostrum-github/src/issues/client.rs` | Client methods | `open_issues`, `issue_detail`, `mutate_issue`, `create_issue`, `assignable_users`, `RepoIssues` |
| `crates/rostrum-github/src/conversation.rs` | Shared timeline decoding, now with issue events | `TimelineEventNode`, `ReferenceSource`, `close_reason` |
| `crates/rostrum-github/fixtures/issues/*.json` | Responses captured from the live API | — |
| `crates/rostrum-db/src/issues.rs` | Issue cache | `Db::{save_issues, load_issues, save_issue_detail, load_issue_detail}` |
| `crates/rostrum-db/src/schema.rs` | `cache_issue`, `cache_issue_detail` | — |
| `crates/rostrum-config/src/lib.rs` | `feed_tab`, `issues_per_repo` | `Config::feed_tab` |
| `crates/rostrum/src/sync/issues.rs` | Issue refresh, cache write, tab, reveal | `Store::{refresh_issues, set_tab, reveal}` |
| `crates/rostrum/src/feed/mod.rs` | Tab bar, tab keys, identity selection | `FeedEvent::NewIssue`, `NextTab`, `PreviousTab` |
| `crates/rostrum/src/feed/rows.rs` | Row renderers, including the issue row and the header's `+ New issue` | — |
| `crates/rostrum/src/nav.rs` | Navigation over either kind of item row | `selected_row`, `navigate` |
| `crates/rostrum/src/issue/mod.rs` | The issue pane: loading, pickers, mutations | `IssuePane` |
| `crates/rostrum/src/issue/render.rs` | Header, timeline, actions | — |
| `crates/rostrum/src/issue/create.rs` | The new-issue form | `NewIssueForm`, `NewIssueEvent` |
| `crates/rostrum/src/pickers.rs` | Label and assignee chips and pickers, shared by every pane | `label_chip`, `label_picker`, `assignee_chip`, `assignee_picker`, `Toggle`, `OpenPicker` |
| `crates/rostrum/src/loadable.rs` | `Loadable<T>`, shared by every pane | `Loadable` |
| `crates/rostrum/src/main.rs` | The detail slot | `DetailPane` |

## Testing

- **Decoding** against responses captured from the live API
  (`fixtures/issues/`): a feed page, one with assignees, one with milestones,
  and details with cross-references, an assignment, a not-planned close, and a
  close-then-reopen; plus hand-shaped nulls.
- **Flattening** (`tabs.rs`): rows and chrome on the Issues tab, per-tab load
  states, stale data on failure, hide-empty per tab, collapse, search, author
  and involvement filters, drafts not touching issues, tab counts with filters
  and collapse.
- **REST bodies** for every `IssueMutation`, for `CreateIssue`, and the
  created-issue and assignee responses.
- **Draft rules**: repository and title required, the request body, and
  switching repository.
- **Config** round trip of `feed_tab`, an older file, and an unknown value;
  the tab and the sort round-tripping together.
- **Sorting** (`sort/tests/issues.rs`): every item key over issues, "pushed"
  falling back to "updated", unknown authors last, ties by number, repository
  "updated" counting issues, and the Issues tab under both sorts.
- **Cache** round trips of issue lists and details, replacement, scoping,
  corruption-as-miss, and pruning.
- **Navigation** on the Issues tab and across tabs, and identity selection
  from rows.
