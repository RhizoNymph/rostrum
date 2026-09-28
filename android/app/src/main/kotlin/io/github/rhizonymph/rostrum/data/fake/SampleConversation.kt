package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.CheckRunView
import io.github.rhizonymph.rostrum.data.model.CheckState
import io.github.rhizonymph.rostrum.data.model.Chip
import io.github.rhizonymph.rostrum.data.model.ColorRole
import io.github.rhizonymph.rostrum.data.model.ReviewState
import io.github.rhizonymph.rostrum.data.model.ReviewThreadView
import io.github.rhizonymph.rostrum.data.model.Side
import io.github.rhizonymph.rostrum.data.model.ThreadCommentView
import io.github.rhizonymph.rostrum.data.model.TimelineEntry
import io.github.rhizonymph.rostrum.data.model.TimelineEvent
import io.github.rhizonymph.rostrum.data.model.TimelineKind
import io.github.rhizonymph.rostrum.data.model.UserRef
import java.time.Duration
import java.time.Instant

/** A pull request's conversation as the fake keeps it; mutated by comments and reviews. */
internal class FakeConversation(
    val timeline: MutableList<TimelineEntry>,
    val threads: MutableList<ReviewThreadView>,
    val checks: List<CheckRunView>,
    val files: List<FakeFile>,
)

internal object SampleConversation {
    fun diffOverview(pull: FakePull, now: Instant): FakeConversation {
        fun ago(minutes: Long) = now.minus(Duration.ofMinutes(minutes))
        val adaLin = UserRef("ada-lin")
        val mkowal = UserRef("mkowal")
        val files = SampleFiles.diffOverview
        val overviewRs = files.first { it.path == "crates/rostrum-diff/src/overview.rs" }
        val thread = ReviewThreadView(
            id = "thread-1",
            path = overviewRs.path,
            line = 60,
            originalLine = 60,
            side = Side.Right,
            resolved = false,
            outdated = false,
            location = "${overviewRs.path}:60",
            comments = listOf(
                ThreadCommentView(
                    id = "c-1", author = mkowal, createdAt = ago(60),
                    body = FakeMarkdown.parse(MKOWAL_QUESTION), source = MKOWAL_QUESTION,
                ),
                ThreadCommentView(
                    id = "c-2", author = adaLin, createdAt = ago(45),
                    body = FakeMarkdown.parse(ADA_ANSWER), source = ADA_ANSWER,
                ),
            ),
            canReply = true,
        )
        val timeline = mutableListOf(
            TimelineEntry(
                id = "desc", author = adaLin, createdAt = pull.createdAt,
                kind = TimelineKind.Description(FakeMarkdown.parse(DESCRIPTION), DESCRIPTION),
            ),
            TimelineEntry(
                id = "ev-review-requested", author = adaLin, createdAt = ago(118),
                kind = TimelineKind.Event(TimelineEvent.ReviewRequested(SamplePulls.VIEWER), "requested your review"),
            ),
            TimelineEntry(
                id = "review-1", author = mkowal, createdAt = ago(60),
                kind = TimelineKind.Review(
                    state = ReviewState.Commented,
                    chip = Chip("reviewed", ColorRole.Neutral),
                    body = emptyList(),
                    source = "",
                    threadIds = listOf(thread.id),
                ),
            ),
            TimelineEntry(
                id = "ev-pushed", author = adaLin, createdAt = ago(40),
                kind = TimelineKind.Event(TimelineEvent.Other("committed"), "pushed 2 commits"),
            ),
        )
        return FakeConversation(timeline, mutableListOf(thread), diffOverviewChecks, files)
    }

    fun generic(pull: FakePull, now: Instant): FakeConversation {
        val description = "This pull request ${pull.title.replaceFirstChar { it.lowercase() }}.\n\n" +
            "- Touches ${pull.changedFiles} file${if (pull.changedFiles == 1) "" else "s"}\n" +
            "- Keeps the existing behaviour behind the same settings\n\n" +
            "Tested locally with `cargo test`."
        val timeline = mutableListOf(
            TimelineEntry(
                id = "desc", author = UserRef(pull.author), createdAt = pull.createdAt,
                kind = TimelineKind.Description(FakeMarkdown.parse(description), description),
            ),
        )
        if (pull.comments > 0) {
            val body = "Thanks! One question before this goes in: does it need a changelog entry?"
            timeline += TimelineEntry(
                id = "comment-1", author = UserRef("ada-lin"),
                createdAt = pull.updatedAt.minus(Duration.ofMinutes(5)),
                kind = TimelineKind.Comment(FakeMarkdown.parse(body), body),
            )
        }
        val checks = genericChecks(pull.checks)
        val files = SampleFiles.generic(pull.changedFiles, pull.additions, pull.deletions, pull.number)
        return FakeConversation(timeline, mutableListOf(), checks, files)
    }

    private fun genericChecks(state: CheckState?): List<CheckRunView> {
        if (state == null) return emptyList()
        fun run(name: String, s: CheckState?, text: String) =
            CheckRunView(name, s, FakePull.checksRole(s), text, "https://github.com/checks/$name")
        return listOf(
            run("ci / test", if (state == CheckState.Failure) CheckState.Failure else state, if (state == CheckState.Pending) "running" else "3m 40s"),
            run("ci / clippy", CheckState.Success, "1m 12s"),
            run("ci / fmt", CheckState.Success, "9s"),
        )
    }

    private val diffOverviewChecks = listOf(
        CheckRunView("ci / test (ubuntu-latest)", CheckState.Success, ColorRole.Success, "4m 12s", "https://github.com/RhizoNymph/rostrum/actions/runs/1"),
        CheckRunView("ci / clippy", CheckState.Success, ColorRole.Success, "1m 48s", "https://github.com/RhizoNymph/rostrum/actions/runs/2"),
        CheckRunView("ci / fmt", CheckState.Success, ColorRole.Success, "12s", "https://github.com/RhizoNymph/rostrum/actions/runs/3"),
        CheckRunView("ci / build (wayland)", CheckState.Success, ColorRole.Success, "6m 03s", "https://github.com/RhizoNymph/rostrum/actions/runs/4"),
        CheckRunView("ci / docs", CheckState.Success, ColorRole.Success, "58s", "https://github.com/RhizoNymph/rostrum/actions/runs/5"),
        CheckRunView("ci / audit", CheckState.Success, ColorRole.Success, "21s", "https://github.com/RhizoNymph/rostrum/actions/runs/6"),
        CheckRunView("ci / build (macos)", null, ColorRole.Neutral, "skipped", "https://github.com/RhizoNymph/rostrum/actions/runs/7"),
    )

    private const val DESCRIPTION =
        "Adds a `Diff | Overview` switcher to the Files tab. The overview answers “where is the bulk of " +
            "this change?” before you read every line.\n\n" +
            "- **Change map:** a column per directory, a tile per file, sized by churn\n" +
            "- **Largest changes:** every file ranked, with a +/− bar\n" +
            "- Tap a tile or row to jump to that file in the diff"

    private const val MKOWAL_QUESTION =
        "Should a zero-churn file really weigh 1? A 3,000-file rename would turn the map into slivers."

    private const val ADA_ANSWER =
        "Columns cap at 12 tiles and fold the rest into `+N more`, so it stays legible."
}
