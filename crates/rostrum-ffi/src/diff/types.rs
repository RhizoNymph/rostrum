//! The Files tab: an overview of where the change falls, and one file's diff
//! at a time as render-ready rows.

use crate::{
    detail::ReviewThreadView,
    review::ReviewDraft,
    types::Side,
};

/// The Files tab's landing view.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FilesOverview {
    /// The commit these files describe.
    pub head_sha: String,
    pub stats: DiffStats,
    /// Directories as columns, sized by their share of the churn.
    pub change_map: Vec<MapColumn>,
    /// Every file, largest change first.
    pub ranked: Vec<RankedFile>,
    /// Every file in patch order; `index` is what `file_diff` takes.
    pub files: Vec<ChangedFile>,
}

/// Totals for the summary strip.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DiffStats {
    pub files: u32,
    pub additions: u64,
    pub deletions: u64,
    pub added_files: u32,
    pub removed_files: u32,
    pub renamed_files: u32,
    pub modified_files: u32,
}

/// One column of the change map: a directory, `(root)`, or the `(other)`
/// column the overflow folds into.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MapColumn {
    pub label: String,
    pub additions: u64,
    pub deletions: u64,
    pub file_count: u32,
    /// Fraction of the map's width; columns sum to 1.
    pub share: f32,
    pub tiles: Vec<MapTile>,
}

/// One tile of a column: a file, or the `+N more` aggregate.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MapTile {
    /// The file to open on tap; `None` for the aggregate tile.
    pub file_index: Option<u32>,
    /// The file name, or `+N more`.
    pub label: String,
    pub additions: u64,
    pub deletions: u64,
    /// Fraction of the column's height; tiles sum to 1.
    pub share: f32,
    pub heat: TileHeat,
}

/// How to colour a tile: blend the success (added) and danger (removed)
/// colours by `removed_ratio`, at `alpha` opacity.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct TileHeat {
    /// 0 is all additions, 1 all deletions; `None` for a file with no line
    /// changes (a pure rename), which renders in the neutral colour.
    pub removed_ratio: Option<f32>,
    /// Stronger for the files carrying more of the diff.
    pub alpha: f32,
}

/// One row of the "largest changes" list.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RankedFile {
    pub file_index: u32,
    pub path: String,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    /// Widths of the stacked bar's green and red parts, as fractions of the
    /// largest file's churn.
    pub additions_share: f32,
    pub deletions_share: f32,
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ChangedFile {
    pub index: u32,
    pub path: String,
    /// The old path, for renames and copies.
    pub previous_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub availability: DiffAvailability,
    /// Inline threads anchored to lines of this file's diff.
    pub threads: u32,
    /// Your pending drafts on this file.
    pub drafts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FileStatus {
    Added,
    Removed,
    Modified,
    Renamed,
    Copied,
    Changed,
    Unchanged,
}

/// Whether a file's diff can be shown, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DiffAvailability {
    /// Lines to show.
    Text,
    /// A binary file: GitHub sends no patch.
    Binary,
    /// GitHub withheld the patch because it is too large.
    TooLarge,
    /// GitHub sent a patch that could not be read.
    Unparseable,
    /// Renamed, copied or mode-changed with no line changes.
    NoTextChanges,
}

/// One file's diff.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FileDiff {
    pub file: ChangedFile,
    pub head_sha: String,
    pub body: FileDiffBody,
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum FileDiffBody {
    /// Hunk headers, lines, and the threads and drafts anchored to them.
    Rows { rows: Vec<DiffRow> },
    /// Nothing to show; `file.availability` says why.
    Unavailable,
}

/// One row of a file's diff, in display order.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum DiffRow {
    /// A hunk's `@@ -a,b +c,d @@ context` line.
    Hunk { index: u32, header: String },
    Line { line: DiffLineView },
    /// An existing thread, placed right after the line it is anchored to.
    Thread { thread: ReviewThreadView },
    /// One of your pending drafts, placed right after its (last) line.
    Draft { draft: ReviewDraft },
}

/// What a line does to the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// One line of code.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DiffLineView {
    pub kind: LineKind,
    /// Line number in the old file; `None` for added lines.
    pub old_line: Option<u32>,
    /// Line number in the new file; `None` for removed lines.
    pub new_line: Option<u32>,
    /// The line's text in styled runs; concatenated they are the whole line
    /// (without the `+`/`-` marker).
    pub segments: Vec<CodeSegment>,
    /// Where a comment on this line attaches, or `None` if it cannot be
    /// commented on. Pass it back to `add_draft` unchanged: anchors are only
    /// ever computed here.
    pub anchor: Option<CommentAnchor>,
    /// GitHub's `\ No newline at end of file` applies to this line.
    pub no_newline_at_eof: bool,
}

/// A run of code with one style.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CodeSegment {
    pub text: String,
    /// Syntax colour, opaque ARGB.
    pub color: u32,
    pub bold: bool,
    pub italic: bool,
    /// Part of the words that changed between this line and its removed or
    /// added counterpart: draw a stronger background.
    pub emphasized: bool,
}

/// Where a review comment attaches: GitHub's `path` + `line` + `side`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct CommentAnchor {
    pub path: String,
    pub line: u32,
    pub side: Side,
}
