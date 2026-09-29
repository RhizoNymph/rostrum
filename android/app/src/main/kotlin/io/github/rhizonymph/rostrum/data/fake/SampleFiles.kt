package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.FileStatus

/** Changed files for the sample pull requests. */
internal object SampleFiles {
    /** RhizoNymph/rostrum#10, in GitHub's patch (path) order. */
    val diffOverview: List<FakeFile> by lazy { listOf(
        FakeFile(
            path = "crates/rostrum-diff/src/lib.rs",
            status = FileStatus.Modified,
            additions = 2,
            deletions = 0,
            hunks = listOf(
                hunk(8, 8, "pub mod model;") {
                    ctx("pub mod highlight;")
                    ctx("pub mod model;")
                    add("pub mod overview;")
                    add("")
                    ctx("pub use model::{DiffFile, DiffRow, FileStatus};")
                },
            ),
        ),
        addedFile("crates/rostrum-diff/src/overview.rs", 371, overviewRs),
        FakeFile(
            path = "crates/rostrum/src/detail.rs",
            status = FileStatus.Modified,
            additions = 36,
            deletions = 0,
            hunks = listOf(
                hunk(140, 140, "impl PrDetail {") {
                    ctx("    fn files_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {")
                    ctx("        let files = self.files.clone();")
                    add("        let overview = cx.new(|cx| OverviewView::new(files.clone(), cx));")
                    add("        cx.subscribe(&overview, |this, _, event: &JumpToFile, cx| {")
                    add("            this.select_file(event.index, cx);")
                    add("        })")
                    add("        .detach();")
                    add("        self.overview = Some(overview);")
                    ctx("        self.files_view = Some(cx.new(|cx| FilesView::new(files, window, cx)));")
                    ctx("    }")
                },
            ),
        ),
        FakeFile(
            path = "crates/rostrum/src/detail/files.rs",
            status = FileStatus.Modified,
            additions = 78,
            deletions = 8,
            hunks = listOf(
                hunk(262, 262, "impl FilesView {") {
                    ctx("    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {")
                    ctx("        let mode = self.mode;")
                    del("        h_flex()")
                    del("            .gap_2()")
                    del("            .child(self.diff_button(cx))")
                    add("        let toggle = segmented(")
                    add("            [(\"Diff\", FilesMode::Diff), (\"Overview\", FilesMode::Overview)],")
                    add("            mode,")
                    add("        );")
                    add("        h_flex()")
                    add("            .gap_2()")
                    add("            .child(toggle.on_select(cx.listener(|this, mode, _, cx| {")
                    add("                this.mode = *mode;")
                    add("                cx.notify();")
                    add("            })))")
                    ctx("            .child(self.stats_label(cx))")
                    ctx("    }")
                },
                hunk(410, 417, "impl Render for FilesView {") {
                    ctx("        match self.mode {")
                    ctx("            FilesMode::Diff => self.render_diff(window, cx).into_any_element(),")
                    del("            FilesMode::Overview => div().into_any_element(),")
                    add("            FilesMode::Overview => {")
                    add("                self.render_overview(window, cx).into_any_element()")
                    add("            }")
                    ctx("        }")
                    ctx("    }")
                },
            ),
        ),
        addedFile("crates/rostrum/src/detail/overview.rs", 320, detailOverviewRs),
        FakeFile(
            path = "docs/OVERVIEW.md",
            status = FileStatus.Modified,
            additions = 6,
            deletions = 1,
            hunks = listOf(
                hunk(96, 96, "Features Index:") {
                    ctx("  diff_review:")
                    ctx("    description: Diff parsing, highlighting, inline comments, review batching.")
                    del("    entry_points: [crates/rostrum/src/detail/files.rs]")
                    add("    entry_points: [crates/rostrum/src/detail/files.rs, crates/rostrum-diff/src/lib.rs]")
                    add("  diff_overview:")
                    add("    description: Visual overview of a diff — change map and ranked churn list.")
                    add("    entry_points: [crates/rostrum/src/detail/overview.rs]")
                    add("    depends_on: [ui_foundation, diff_review]")
                    add("    doc: docs/features/diff_overview.md")
                    ctx("  github_sync:")
                },
            ),
        ),
        addedFile("docs/features/diff_overview.md", 87, diffOverviewMd),
    ) }

    /** Plausible files for any other pull request, sized to its stats. */
    fun generic(changedFiles: Int, additions: Int, deletions: Int, seed: Int): List<FakeFile> {
        val pool = listOf(
            "src/lib.rs", "src/panel.rs", "src/status.rs", "src/view/render.rs", "tests/status.rs",
            "Cargo.toml", "docs/usage.md", "src/config.rs", "src/git/refresh.rs", "src/settings.rs",
        )
        val count = changedFiles.coerceIn(1, 6)
        val weights = (0 until count).map { count - it }
        val totalWeight = weights.sum()
        return (0 until count).map { i ->
            val path = pool[(seed + i * 3) % pool.size]
            val add = (additions * weights[i] / totalWeight).coerceAtLeast(if (additions > 0) 1 else 0)
            val del = (deletions * weights[i] / totalWeight)
            val start = 20 + (seed % 7) * 11 + i * 17
            FakeFile(
                path = path,
                status = FileStatus.Modified,
                additions = add,
                deletions = del,
                hunks = listOf(
                    hunk(start, start, "fn update(&mut self) {") {
                        ctx("    let started = Instant::now();")
                        if (del > 0) del("    let entries = self.repo.statuses()?.collect::<Vec<_>>();")
                        add("    let mut entries = std::mem::take(&mut self.scratch);")
                        add("    entries.clear();")
                        add("    entries.extend(self.repo.statuses()?);")
                        ctx("    self.apply(&entries);")
                        ctx("    log::debug!(\"refresh took {:?}\", started.elapsed());")
                    },
                ),
            )
        }.distinctBy { it.path }
    }

    private val overviewRs = listOf(
        "//! The diff overview: where the bulk of a change is, before reading it.",
        "//!",
        "//! Pure layout: no rendering, no I/O. The Files tab draws what this returns.",
        "",
        "use std::cmp::Reverse;",
        "use std::path::Path;",
        "",
        "use itertools::Itertools;",
        "use std::collections::BTreeMap;",
        "",
        "use crate::model::{DiffFile, FileStatus};",
        "",
        "/// Whole-diff totals for the overview's summary strip.",
        "#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]",
        "pub struct OverviewStats {",
        "    pub files: usize,",
        "    pub additions: u64,",
        "    pub deletions: u64,",
        "    pub added_files: usize,",
        "    pub removed_files: usize,",
        "    pub renamed_files: usize,",
        "    pub modified_files: usize,",
        "}",
        "",
        "impl OverviewStats {",
        "    /// Count every file once, by its status.",
        "    pub fn of(files: &[DiffFile]) -> Self {",
        "        let mut stats = Self::default();",
        "        for file in files {",
        "            stats.files += 1;",
        "            stats.additions += u64::from(file.additions);",
        "            stats.deletions += u64::from(file.deletions);",
        "            match file.status {",
        "                FileStatus::Added => stats.added_files += 1,",
        "                FileStatus::Removed => stats.removed_files += 1,",
        "                FileStatus::Renamed => stats.renamed_files += 1,",
        "                _ => stats.modified_files += 1,",
        "            }",
        "        }",
        "        stats",
        "    }",
        "}",
        "",
        "/// Columns taller than this fold their smallest tiles into `+N more`.",
        "pub const MAX_TILES_PER_COLUMN: usize = 12;",
        "",
        "/// One directory's column in the change map.",
        "#[derive(Clone, Debug, PartialEq)]",
        "pub struct MapColumn {",
        "    pub label: String,",
        "    pub share: f32,",
        "}",
        "/// Total churn of a file as GitHub counts it.",
        "pub fn churn(file: &DiffFile) -> u64 {",
        "    u64::from(file.additions) + u64::from(file.deletions)",
        "}",
        "",
        "/// Layout weight: like [`churn`], but a zero-churn file (a pure rename, a mode",
        "/// change) still occupies a visible sliver instead of vanishing from the map.",
        "fn weight(file: &DiffFile) -> u64 {",
        "    churn(file).max(1)",
        "}",
    )

    private val detailOverviewRs = listOf(
        "//! The Files tab's overview: the change map and the ranked list.",
        "",
        "use gpui::{div, prelude::*, px, Context, Entity, EventEmitter, Window};",
        "use rostrum_diff::overview::{layout, Overview};",
        "",
        "/// Emitted when a tile or a row is clicked.",
        "pub struct JumpToFile {",
        "    pub index: usize,",
        "}",
        "",
        "pub struct OverviewView {",
        "    overview: Overview,",
        "    hovered: Option<usize>,",
        "}",
        "",
        "impl EventEmitter<JumpToFile> for OverviewView {}",
        "",
        "impl OverviewView {",
        "    pub fn new(files: Vec<DiffFile>, _cx: &mut Context<Self>) -> Self {",
        "        Self { overview: layout(&files), hovered: None }",
        "    }",
        "}",
    )

    private val diffOverviewMd = listOf(
        "# Feature: diff_overview",
        "",
        "A visual overview of a pull request's diff, shown before the line-by-line",
        "view: where is the bulk of this change?",
        "",
        "## Scope",
        "",
        "- The change map: a column per directory, a tile per file, sized by churn.",
        "- The ranked list: every file, largest change first, with a +/− bar.",
        "- Jumping from a tile or a row to that file in the diff.",
        "",
        "## Invariants",
        "",
        "- Column shares sum to 1; tile shares within a column sum to 1.",
        "- A zero-churn file still gets a visible sliver (weight 1).",
    )
}
