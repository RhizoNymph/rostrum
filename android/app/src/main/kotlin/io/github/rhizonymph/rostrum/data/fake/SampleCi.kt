package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.CiAnnotation
import io.github.rhizonymph.rostrum.data.model.CiAnnotationLevel
import io.github.rhizonymph.rostrum.data.model.CiCheckKey
import io.github.rhizonymph.rostrum.data.model.CiCheckOutput
import io.github.rhizonymph.rostrum.data.model.CiColumn
import io.github.rhizonymph.rostrum.data.model.CiJobLog
import io.github.rhizonymph.rostrum.data.model.CiLineKind
import io.github.rhizonymph.rostrum.data.model.CiLogGroup
import io.github.rhizonymph.rostrum.data.model.CiLogLine
import io.github.rhizonymph.rostrum.data.model.CiLogStep
import io.github.rhizonymph.rostrum.data.model.CiStatus
import java.time.Duration

/**
 * The checks the fake's CI grid shows: each repository's columns and, for each
 * open pull request, a status per column that is the same on every run (so
 * tests can name them), plus a parsed job log and another app's output.
 */
internal object SampleCi {
    /** How a column's check is produced. */
    enum class Kind { Actions, App, Status }

    data class Check(val column: CiColumn, val kind: Kind)

    private fun actions(job: String) = Check(CiColumn(CiCheckKey("CI", job), "CI / $job"), Kind.Actions)

    fun columns(repo: String): List<Check> = when (repo) {
        SamplePulls.ROSTRUM -> listOf(
            actions("build"),
            actions("test"),
            actions("clippy"),
            Check(CiColumn(CiCheckKey(null, "Coverage"), "Coverage"), Kind.App),
            Check(CiColumn(CiCheckKey(null, "deploy/preview"), "deploy/preview"), Kind.Status),
        )
        else -> listOf(actions("build"), actions("test"))
    }

    /** One pull request's check in column [column]; `null` is "not run". */
    data class Seed(val status: CiStatus, val age: Duration)

    /**
     * rostrum #10 fails `test`, #9 is still running `clippy` with `Coverage`
     * queued, #11 skipped the preview, and everything else passes.
     */
    fun seed(repo: String, number: Int, column: Int): Seed? {
        val minutes = (number * 7L + column * 3L) % 50 + 2
        val done = Seed(CiStatus.Success, Duration.ofMinutes(minutes))
        if (repo != SamplePulls.ROSTRUM) return if (number % 5 == 0 && column == 1) Seed(CiStatus.Failure, Duration.ofMinutes(minutes)) else done
        return when {
            number == 10 && column == 1 -> Seed(CiStatus.Failure, Duration.ofMinutes(12))
            number == 9 && column == 2 -> Seed(CiStatus.InProgress, Duration.ofSeconds(192))
            number == 9 && column == 3 -> Seed(CiStatus.Queued, Duration.ofSeconds(45))
            number == 11 && column == 4 -> Seed(CiStatus.Skipped, Duration.ofMinutes(30))
            number == 12 && column == 4 -> null
            else -> done
        }
    }

    const val LOG_LINES_FULL = 160
    const val LOG_LINES_TAIL = 40

    /** A failing `cargo test` job's log; the tail only unless [full]. */
    fun jobLog(failed: Boolean, full: Boolean): CiJobLog {
        val all = buildList {
            add("Set up job" to CiLineKind.GroupHeader)
            add("Current runner version: '2.319.1'" to CiLineKind.Plain)
            add("Operating System: Ubuntu 24.04" to CiLineKind.Plain)
            add("Run actions/checkout@v4" to CiLineKind.GroupHeader)
            add("[command]/usr/bin/git fetch --depth=1 origin" to CiLineKind.Command)
            repeat(LOG_LINES_FULL - 20) { add("   Compiling crate-$it v0.1.0" to CiLineKind.Plain) }
            add("Run cargo test --workspace" to CiLineKind.GroupHeader)
            add("[command]cargo test --workspace --locked" to CiLineKind.Command)
            add("running 214 tests" to CiLineKind.Plain)
            add("test feed::sort::keeps_ties_stable ... ok" to CiLineKind.Plain)
            add("warning: unused variable: `stale`" to CiLineKind.Warning)
            if (failed) {
                add("test ci::grid::orders_columns_by_first_seen ... FAILED" to CiLineKind.Plain)
                add("thread 'ci::grid::orders_columns_by_first_seen' panicked at crates/rostrum-core/src/ci/grid.rs:212:9:" to CiLineKind.Error)
                add("assertion `left == right` failed" to CiLineKind.Error)
                add("  left: [\"test\", \"build\"]" to CiLineKind.Plain)
                add(" right: [\"build\", \"test\"]" to CiLineKind.Plain)
                add("test result: FAILED. 213 passed; 1 failed" to CiLineKind.Plain)
                add("Process completed with exit code 101." to CiLineKind.Error)
            } else {
                add("test result: ok. 214 passed; 0 failed" to CiLineKind.Plain)
            }
            add("Post job cleanup." to CiLineKind.GroupHeader)
            add("Cleaning up orphan processes" to CiLineKind.Plain)
        }
        val keep = if (full) all.size else LOG_LINES_TAIL
        val dropped = all.size - keep
        val kept = all.drop(dropped)
        val lines = kept.mapIndexed { index, (text, kind) -> CiLogLine(dropped + index + 1, text, kind) }
        val headers = lines.indices.filter { lines[it].kind == CiLineKind.GroupHeader }
        val groups = headers.mapIndexed { i, header ->
            CiLogGroup(lines[header].text, header, headers.getOrNull(i + 1) ?: lines.size)
        }
        val steps = groups.map { CiLogStep(it.title, it.header, it.end) }
        val firstError = lines.indexOfFirst { it.kind == CiLineKind.Error }.takeIf { it >= 0 }
        val failingStep = firstError?.let { error -> steps.indexOfFirst { error in it.start until it.end } }
        val collapsed = groups.indices.filter { g -> firstError == null || firstError !in groups[g].header until groups[g].end }
        return CiJobLog(lines, groups, steps, firstError, failingStep, collapsed, dropped, truncated = dropped > 0)
    }

    fun checkOutput(): CiCheckOutput = CiCheckOutput(
        title = "Coverage 81.4% (−0.6%)",
        summary = FakeMarkdown.parse("**81.4%** of lines covered, **−0.6%** against `main`."),
        text = FakeMarkdown.parse("Files with the largest drop:\n\n- `crates/rostrum-core/src/ci/grid.rs` 74%\n- `crates/rostrum-ffi/src/ci/convert.rs` 69%"),
        annotations = listOf(
            CiAnnotation(
                path = "crates/rostrum-core/src/ci/grid.rs", startLine = 198, endLine = 214,
                level = CiAnnotationLevel.Warning, title = "Not covered",
                message = "These lines are not covered by tests.", location = "crates/rostrum-core/src/ci/grid.rs:198-214",
            ),
            CiAnnotation(
                path = "crates/rostrum-ffi/src/ci/convert.rs", startLine = 40, endLine = 40,
                level = CiAnnotationLevel.Notice, title = null,
                message = "One branch is never taken.", location = "crates/rostrum-ffi/src/ci/convert.rs:40",
            ),
        ),
    )
}
