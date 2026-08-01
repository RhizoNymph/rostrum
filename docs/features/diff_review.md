# Feature: diff_review

Viewing a pull request's diff with syntax highlighting, and leaving line-level
review comments on it.

## Scope

- Parsing GitHub's unified-diff patches into a renderable row model.
- Preserving old/new line numbers per row — the anchor for inline comments.
- Syntax highlighting per line via tree-sitter.
- Virtualized rendering of the diff.
- Inline comment threads rendered between diff rows.
- Pending-review batching and review submission.
- Word-level highlighting within modified lines.

## Non-scope

- Editing files. This is a read-and-comment surface.
- Computing diffs locally; GitHub supplies them.
- Fetching — see `github_sync`.

## Row model

As with the feed, the whole Files tab is **one flattened row stream in one
virtualized list**, so scrolling across all files is continuous:

```rust
pub enum DiffRow {
    FileHeader  { file: FileIx },              // path, status, +/- counts
    FileCollapsed { file: FileIx },
    FileOmitted { file: FileIx, reason: PatchAvailability },
    HunkHeader  { file: FileIx, hunk: HunkIx }, // the @@ line
    Line        { file: FileIx, line: LineIx },
    Thread      { file: FileIx, thread: ThreadIx },
    Composer    { file: FileIx, anchor: CommentAnchor },
    Expander    { file: FileIx, hunk: HunkIx },  // "expand N unchanged lines"
}
```

`Line` rows are fixed height; `Thread` and `Composer` rows are not. Because
heights are mixed, this uses `list`/`ListState` rather than `uniform_list`.

If a pure-code view without threads is ever needed, `uniform_list` is the better
primitive there — fixed line height is its exact use case, and it only invokes
its range closure for visible rows.

## The critical invariant: line anchoring

Every `DiffLine` carries:

```rust
pub struct DiffLine {
    pub kind: LineKind,          // Context | Added | Removed
    pub old_line: Option<u32>,   // None for Added
    pub new_line: Option<u32>,   // None for Removed
    pub content: String,
}
```

GitHub's review-comment API anchors comments by `path` + `line` + `side`
(`RIGHT` = new file, `LEFT` = old file), with `start_line`/`start_side` for
multi-line selections. It does **not** use the older `position` field for new
comments.

Therefore:

- **A comment on an `Added` or `Context` line uses `new_line` with `side: RIGHT`.**
- **A comment on a `Removed` line uses `old_line` with `side: LEFT`.**
- **A row whose relevant line number is `None` is not commentable.** The UI must
  disable the comment affordance rather than guess.

If this mapping is wrong, comments silently land on the wrong lines of a
colleague's PR — the worst failure mode in the application. The parser is
unit-tested against captured real-world patches covering: multiple hunks, hunks
with `\ No newline at end of file`, pure additions, pure deletions, renames,
binary files, and patches where the hunk header omits a count (`@@ -1 +1 @@`).

## Parsing

The parser is hand-rolled. `diffy` was evaluated and rejected: it requires the
`---`/`+++` file headers that GitHub's per-file `patch` strings do not have, it
does not expose `\ No newline at end of file`, it returns only the stripped
function context rather than the raw `@@` line the hunk header row renders, and
it carries no line numbers — so the line-number walk, the entire high-stakes
part, would have been ours regardless.

Line numbers are reconstructed by walking each hunk from its header's start
offsets, incrementing the old counter on `Context`/`Removed` and the new counter
on `Context`/`Added`. This walk is the single source of truth for anchoring and
lives in one function with dense test coverage.

For word-level highlighting inside a modified pair (a `Removed` immediately
followed by an `Added`), `imara-diff` with `Algorithm::Histogram` produces
intra-line ranges — the same engine and algorithm Zed uses.

## Syntax highlighting

Zed's own highlighting is unusable here: `language`/`language_core` couple
tree-sitter to Zed's `Rope`, CRDT anchors, `SyntaxSnapshot` injection machinery,
and `LanguageRegistry`, and those crates are GPL-3.0-or-later.

`syntect` is used instead, with the pure-Rust `fancy-regex` engine so there is
no `onig` C dependency. It ships syntax definitions for far more languages than
would be practical to wire up as individual tree-sitter grammar crates, each of
which would have to match the `tree-sitter` ABI version.

`Highlighter::highlight_lines_for_path` returns one `Vec<HighlightSpan>` per
line, which the diff view converts into `TextRun`s.

Tree-sitter remains the better long-term choice — it is what Zed uses, and it
gives more accurate results — but it is a larger dependency-management problem
than this phase warranted. Swapping it in later only touches
`rostrum-diff::highlight`; the `HighlightSpan` contract stays the same.

Highlighting is applied per line, so constructs spanning multiple lines (block
comments, multi-line strings) are not tracked across a diff's discontinuities.

`TextRun` has a `len` in UTF-8 bytes and no start offset — runs are concatenated
and **must exactly cover the string**, with no gaps or overlaps. `rostrum-diff`
guarantees byte-exact coverage, and the renderer additionally falls back to a
single unstyled run if the totals ever disagree, so a highlighting bug degrades
appearance rather than panicking.

## Performance

GPUI shapes text for every element actually constructed in a frame; there is no
implicit offscreen culling. Building `StyledText` for thousands of diff lines
each frame means full glyph shaping — font fallback, kerning, bidi — for all of
them, every frame. This is precisely why the list must be virtualized: build rows
only for the range the list asks for.

Shaped lines are cached in GPUI's `LineLayoutCache`, keyed by text plus style, so
once a line has been painted, re-visiting it while scrolling is cheap.

## Review batching

Inline comments follow GitHub's pending-review model:

- The first inline comment starts a pending review held **locally**, not posted.
- Subsequent comments accumulate into `PrDetail::pending`.
- Submitting posts one `POST /pulls/{n}/reviews` with the full `comments[]` array
  and an `event` of `APPROVE`, `REQUEST_CHANGES`, or `COMMENT`.

Pending comments are tagged with the `head_sha` they were drafted against, taken
from `headRefOid` on the pull request query. If the author pushes before the
review is submitted, `drafts_are_stale` detects the mismatch: the action bar
shows a warning, and Approve and Request changes are disabled until the drafts
are discarded. Submitting stale anchors would attach comments to lines that have
moved, which is worse than making the user re-read the diff.

An unreadable sha on either side is treated as *not* stale — refusing to submit
because a field could not be read would be worse than the risk it guards
against.

## Text selection

GPUI has no built-in drag-selection primitive for read-only text; `InteractiveText`
offers click and hover only. Selecting and copying code must be hand-rolled,
following the pattern Zed's `markdown` crate uses: a `Selection { start, end,
reversed, pending, mode }` byte-range struct, manual
`on_mouse_down`/`on_mouse_move`/`on_mouse_up`, hit-testing via
`TextLayout::index_for_position`, a manually painted selection background quad
per visible row, and `cx.write_to_clipboard` (plus `cx.write_to_primary` on
Linux) on copy.

Budget roughly 300–400 lines for this. It is deferred to phase 4, but "select and
copy code from a diff" is not optional in a review tool.

## Invariants

- Every `Line` row has at least one of `old_line`/`new_line` populated.
- A comment anchor's `side` and line number are derived from the same `DiffLine`
  that was clicked — never recomputed from row position.
- `ListState` item count equals `diff_rows.len()` after every mutation; inserting
  a `Thread` or `Composer` row requires a matching `splice`.
- Highlight runs for a line exactly cover that line's bytes.
- Pending review comments are always tagged with the `head_sha` they were
  authored against.

## Files

| File | Role |
|---|---|
| `crates/rostrum-diff/src/parse.rs` | Patch → `DiffFile`/`Hunk`/`DiffLine`, line-number walk |
| `crates/rostrum-diff/src/model.rs` | `DiffFile`, `DiffLine`, `LineKind`, `CommentAnchor` |
| `crates/rostrum-diff/src/highlight.rs` | tree-sitter config registry, events → runs |
| `crates/rostrum-diff/src/word_diff.rs` | `imara-diff` intra-line ranges |
| `crates/rostrum/src/detail/files.rs` | `DiffRow` flattening, `ListState`, row renderers |
| `crates/rostrum/src/detail/review.rs` | Pending review state, submission |
| `crates/rostrum-ui/src/selection.rs` | Byte-range selection, hit-testing, clipboard |
