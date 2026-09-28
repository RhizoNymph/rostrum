package io.github.rhizonymph.rostrum.ui.review

import androidx.compose.runtime.Composable
import io.github.rhizonymph.rostrum.data.model.PrRef
import io.github.rhizonymph.rostrum.ui.components.EmptyView
import io.github.rhizonymph.rostrum.ui.components.RostrumBottomSheet

/** Placeholder until the submit-review sheet is built. */
@Composable
fun SubmitReviewSheet(
    pr: PrRef,
    onDismiss: () -> Unit,
    onSubmitted: () -> Unit,
) {
    RostrumBottomSheet(onDismiss = onDismiss) {
        EmptyView(title = "Finish your review")
    }
}
