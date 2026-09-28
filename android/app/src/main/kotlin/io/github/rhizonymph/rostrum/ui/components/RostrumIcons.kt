package io.github.rhizonymph.rostrum.ui.components

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/**
 * The mockups' line icons (24-unit viewBox, round caps and joins), drawn in
 * black and tinted by `Icon(tint = …)`. Built once from their SVG path data;
 * circles and rounded rectangles are converted to paths.
 */
object RostrumIcons {
    val Search by lazy { icon("search", 1.8f, circle(11f, 11f, 6.5f), "M20 20l-4.3-4.3") }
    val Filter by lazy { icon("filter", 1.8f, "M4 7h16M7 12h10M10 17h4") }
    val Check by lazy { icon("check", 2.2f, "M5 12.5l4.5 4.5L19 7") }
    val CheckBold by lazy { icon("check_bold", 3f, "M5 12.5l4.5 4.5L19 7") }
    val ChevronDown by lazy { icon("chevron_down", 2f, "M6 9l6 6 6-6") }
    val ChevronUp by lazy { icon("chevron_up", 2f, "M6 15l6-6 6 6") }
    val ChevronRight by lazy { icon("chevron_right", 2f, "M9 6l6 6-6 6") }
    val ChevronLeft by lazy { icon("chevron_left", 2f, "M15 6l-6 6 6 6") }
    val Close by lazy { icon("close", 2f, "M6 6l12 12M18 6L6 18") }
    val ArrowBack by lazy { icon("arrow_back", 1.8f, "M19 12H5M11 18l-6-6 6-6") }
    val Desktop by lazy { icon("desktop", 1.8f, rect(3f, 4f, 18f, 12f, 2f), "M8 20h8M12 16v4") }
    val Feed by lazy { icon("feed", 1.8f, rect(4f, 4f, 16f, 7f, 2f), rect(4f, 13f, 16f, 7f, 2f)) }
    val Settings by lazy {
        icon("settings", 1.8f, "M4 7h9M17 7h3M4 17h3M11 17h9", circle(15f, 7f, 2f), circle(9f, 17f, 2f))
    }
    val OpenInBrowser by lazy {
        icon("open_in_browser", 1.8f, "M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5")
    }
    val Plus by lazy { icon("plus", 2.2f, "M12 5v14M5 12h14") }
    val Copy by lazy { icon("copy", 1.8f, rect(9f, 9f, 11f, 11f, 2f), "M5 15V6a2 2 0 0 1 2-2h9") }
    val Terminal by lazy { icon("terminal", 1.8f, rect(3f, 4f, 18f, 16f, 2f), "M7 9l3 3-3 3M13 15h4") }
    val Prompt by lazy { icon("prompt", 2f, "M4 17l6-5-6-5M12 19h8") }
    val Merge by lazy {
        icon("merge", 2f, circle(6f, 5f, 2f), circle(6f, 19f, 2f), circle(18f, 12f, 2f), "M6 7v10M6 7c0 4 4 5 10 5")
    }
    val Rebase by lazy {
        icon("rebase", 1.8f, circle(6f, 19f, 2f), circle(18f, 5f, 2f), "M6 17V10a4 4 0 0 1 4-4h6", "M13 3l3 3-3 3")
    }
    val Pull by lazy { icon("pull", 1.8f, "M12 4v11M7 10l5 5 5-5M5 20h14") }
    val Commit by lazy { icon("commit", 2f, circle(12f, 12f, 3.5f), "M3 12h5.5M15.5 12H21") }
    val SoftWrap by lazy { icon("soft_wrap", 1.8f, "M4 6h16M4 12h13a3 3 0 0 1 0 6h-5M4 18h4", "M14 15.5L11.5 18 14 20.5") }
    val Edit by lazy { icon("edit", 1.8f, "M4 20h4L19 9l-4-4L4 16v4zM14 6l4 4") }
    val Comment by lazy { icon("comment", 1.8f, "M5 5h14v10H10l-5 4z") }
    val Eye by lazy { icon("eye", 2f, "M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z", circle(12f, 12f, 3f)) }
    val Alert by lazy { icon("alert", 2f, circle(12f, 12f, 9f), "M12 7.5v5.5M12 16.5v.01") }
    val CircleCheck by lazy { icon("circle_check", 2f, circle(12f, 12f, 9f), "M8 12.5l2.7 2.7L16 10") }
    val Unfold by lazy { icon("unfold", 2f, "M7 9l5-5 5 5M7 15l5 5 5-5") }
    val File by lazy { icon("file", 2f, "M14 3H6a1 1 0 0 0-1 1v16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8z", "M14 3v5h5") }
    val Tap by lazy {
        icon(
            "tap", 2f,
            "M9 11V6a2 2 0 0 1 4 0v5M13 10a2 2 0 0 1 4 0v3a6 6 0 0 1-6 6h-1a5 5 0 0 1-4-2l-2.5-3.5a1.5 1.5 0 0 1 2.4-1.8L7 13",
        )
    }
    val Refresh by lazy { icon("refresh", 1.8f, "M20 12a8 8 0 1 1-2.34-5.66M20 4v5h-5") }
    val Link by lazy {
        icon(
            "link", 1.8f,
            "M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1",
        )
    }
    val MoreVert by lazy { filledIcon("more_vert", circle(12f, 5f, 1.6f), circle(12f, 12f, 1.6f), circle(12f, 19f, 1.6f)) }

    // --- construction ----------------------------------------------------------

    private fun circle(cx: Float, cy: Float, r: Float): String =
        "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0z"

    private fun rect(x: Float, y: Float, w: Float, h: Float, rx: Float): String =
        "M${x + rx} ${y}h${w - 2 * rx}a$rx $rx 0 0 1 $rx ${rx}v${h - 2 * rx}a$rx $rx 0 0 1 ${-rx} ${rx}" +
            "h${-(w - 2 * rx)}a$rx $rx 0 0 1 ${-rx} ${-rx}v${-(h - 2 * rx)}a$rx $rx 0 0 1 $rx ${-rx}z"

    private fun icon(name: String, strokeWidth: Float, vararg paths: String): ImageVector =
        ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
            for (d in paths) {
                addPath(
                    pathData = addPathNodes(d),
                    fill = null,
                    stroke = SolidColor(Color.Black),
                    strokeLineWidth = strokeWidth,
                    strokeLineCap = StrokeCap.Round,
                    strokeLineJoin = StrokeJoin.Round,
                )
            }
        }.build()

    private fun filledIcon(name: String, vararg paths: String): ImageVector =
        ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
            for (d in paths) addPath(pathData = addPathNodes(d), fill = SolidColor(Color.Black))
        }.build()
}
