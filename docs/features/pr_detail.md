# Feature: pr_detail

The detail pane for a selected pull request: body, conversation, checks, and
PR-level actions.

## Scope

- Master/detail layout and the feed↔detail relationship.
- PR header (title, state, branches, author, labels, mergeability).
- Conversation timeline: body, issue comments, reviews, review threads, events.
- Comment composer.
- PR-level actions: merge, close, reopen, ready-for-review.
- Checks tab.

## Non-scope

- The diff itself and inline comments — see `diff_review`.
- Fetching — see `github_sync`.
- Markdown rendering internals — see `ui_foundation`.

## Layout

A master/detail split: the feed on the left at ~420px, collapsible to icons; the
detail pane fills the rest. Diff review needs horizontal room, and keeping the
feed visible makes moving between PRs across repositories fast — the central
purpose of the application.

The detail pane is tabbed: **Conversation**, **Files**, **Checks**. Tabs load
lazily; opening a PR fetches only the conversation, and the file patches are
fetched on first visit to Files.

When no PR is selected the pane shows an empty state with aggregate counts.

## Entity structure

```
AppState (Entity)
├── repos: Vec<RepoState>
├── selection: Option<(RepoId, PrNumber)>
└── detail: Option<Entity<PrDetail>>

PrDetail (Entity)
├── key: (RepoId, PrNumber)
├── header: PrHeader
├── tab: DetailTab
├── conversation: Loadable<Vec<TimelineItem>>
├── files: Loadable<Vec<DiffFile>>
├── checks: Loadable<Vec<CheckRun>>
├── composer: ComposerState
├── pending_review: Option<PendingReview>
└── refresh: Option<Task<()>>
```

Selection is stored as `(RepoId, PrNumber)`, never as feed indices — indices are
positional and invalidated by every refresh.

Changing selection replaces the `PrDetail` entity outright rather than mutating
it. Dropping the old entity cancels its in-flight `Task`s automatically, which is
the cleanest way to avoid a slow response from a previous PR landing in the
current one.

## Timeline model

```rust
pub enum TimelineItem {
    Body       { author: User, body: Markdown, created_at: DateTime<Utc> },
    Comment    { id: CommentId, author: User, body: Markdown, .. },
    Review     { id: ReviewId, author: User, state: ReviewState,
                 body: Option<Markdown>, threads: Vec<ThreadId> },
    Thread     { id: ThreadId, path: String, line: Option<u32>,
                 is_resolved: bool, comments: Vec<ThreadComment> },
    Event      { kind: EventKind, actor: User, created_at: DateTime<Utc> },
}
```

Items are merged from several GraphQL connections and **sorted by `created_at`
into a single chronological stream**, matching GitHub's own presentation.

Review threads appear in two places — inline in the diff and in the conversation
timeline. They are stored once, keyed by `ThreadId`, and referenced from both;
they are never duplicated, so resolving a thread in one view updates the other.

The timeline is variable-height and can be long, so it too is rendered with
`list`/`ListState` over a flattened row vector, consistent with the feed and the
diff view.

## Actions

| Action | Behavior |
|---|---|
| Comment | Posts, appends optimistically, reconciles on next refresh |
| Approve / Request changes / Comment review | Submits pending review if one exists, else a bodied review |
| Merge | Confirmation required; method from config (merge/squash/rebase) |
| Close / Reopen | Confirmation required |
| Ready for review | Clears draft status |
| Add / remove label | Toggles a label via the issues API |

**Merge and close require explicit confirmation.** They are outward-facing and
effectively irreversible from the app's perspective. The merge confirmation shows
the target branch, the method, and any blocking state (failing checks, requested
changes, conflicts) so the decision is made with the relevant facts visible.

Merge is disabled when `mergeable` is `CONFLICTING`; when it is `UNKNOWN`, GitHub
is still computing, and the UI says so rather than presenting a button that will
fail with a 405.

## Labels

Labels are editable from the header: each applied label carries a remove
affordance, and a picker lists the repository's palette with the applied ones
ticked.

The palette loads **lazily** — only when the picker is first opened, never when
the pull request opens — and is cached on `PrDetail.repo_labels`, so reopening
the picker costs nothing.

Both add and remove route through the same `mutate()` helper as every other
mutation, inheriting the in-flight guard, the error banner, and the
authoritative reload. While a mutation is in flight the affordances drop their
click handlers entirely rather than merely looking disabled, so nothing can be
double-submitted.

Two API details worth remembering: labels live on the **issues** endpoints even
for pull requests, and the label name is a *path segment* on delete, so it must
be percent-encoded — real labels contain spaces, slashes, and colons
(`help wanted`, `area/editor`, `status: blocked`). A delete for a label the pull
request does not have returns 404, which is treated as success: the desired end
state already holds.

## Composer

A multi-line text input with a Write/Preview toggle rendering through the same
markdown pipeline used for display. Drafts are keyed by `(repo, pr)` and persisted
to SQLite on change, debounced — losing a long review comment to a crash is
unacceptable.

Submit is `cmd-enter`/`ctrl-enter`. The composer is disabled with an explanatory
message while a submission is in flight rather than allowing double-submission.

Note that GPUI provides no built-in text input widget; the composer is built on
`EntityInputHandler` for IME support, following the pattern in gpui's
`examples/input.rs`. This is a meaningful piece of work in its own right and is
scheduled in phase 2.

## Invariants

- Exactly one `PrDetail` entity exists at a time; replacing it cancels prior tasks.
- Selection is `(RepoId, PrNumber)`, resolved to indices only at render time.
- A `ThreadId` maps to exactly one stored thread, shared by both views.
- Timeline items are ordered by `created_at` ascending.
- Destructive or outward-facing actions (merge, close) require confirmation.
- Optimistic updates are tagged and replaced wholesale by the next authoritative
  refresh; they are never merged field-by-field.

## Files

| File | Role |
|---|---|
| `crates/rostrum/src/detail/mod.rs` | `PrDetail` entity, tab state, lifecycle |
| `crates/rostrum/src/detail/header.rs` | PR header rendering |
| `crates/rostrum/src/detail/conversation.rs` | Timeline flattening and rendering |
| `crates/rostrum/src/detail/composer.rs` | Text input, drafts, submission |
| `crates/rostrum/src/detail/actions.rs` | Merge/close/review, confirmations |
| `crates/rostrum/src/detail/checks.rs` | Check run list |
| `crates/rostrum-core/src/timeline.rs` | `TimelineItem`, merge/sort logic |
