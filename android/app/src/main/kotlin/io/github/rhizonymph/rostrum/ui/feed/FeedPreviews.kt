package io.github.rhizonymph.rostrum.ui.feed

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.ui.common.UiState
import io.github.rhizonymph.rostrum.ui.preview.PreviewData
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

@Composable
private fun PreviewFrame(content: @Composable () -> Unit) {
    RostrumTheme {
        Box(Modifier.background(RostrumTheme.colors.bg)) { content() }
    }
}

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun FeedScreenPreview() {
    PreviewFrame {
        FeedScreen(
            state = FeedUiState(
                feed = UiState.Loaded(PreviewData.sample { cachedFeed() }),
                desktop = DesktopPill.Connected("nymph-desk"),
                now = PreviewData.clock.instant(),
            ),
            actions = FeedActions(),
        )
    }
}

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun FeedSearchPreview() {
    PreviewFrame {
        FeedScreen(
            state = FeedUiState(
                feed = UiState.Loaded(PreviewData.sample { setQuery("soft") }),
                search = SearchState.Open("soft"),
                desktop = DesktopPill.Unreachable,
                now = PreviewData.clock.instant(),
            ),
            actions = FeedActions(),
        )
    }
}

@Preview(widthDp = 412, heightDp = 600)
@Composable
private fun FeedErrorPreview() {
    PreviewFrame {
        FeedScreen(
            state = FeedUiState(
                feed = UiState.Error(BackendError.Network("connection reset")),
                desktop = DesktopPill.NotPaired,
                now = PreviewData.clock.instant(),
            ),
            actions = FeedActions(),
        )
    }
}

@Preview(widthDp = 412, heightDp = 760)
@Composable
private fun FeedFilterPreview() {
    PreviewFrame {
        FeedFilterContent(
            preferences = PreviewData.sample { cachedFeed() }.preferences,
            sheet = FilterSheetState.Open(UiState.Loaded(PreviewData.sample { authorRoster(5) }), expanded = false),
            actions = FilterSheetActions(),
        )
    }
}
