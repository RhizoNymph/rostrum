package io.github.rhizonymph.rostrum.ui.profiles

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.common.rostrumViewModel
import io.github.rhizonymph.rostrum.ui.common.running
import io.github.rhizonymph.rostrum.ui.components.BackTopBar
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * "Add a GitHub token profile": a name (optional) and a github.com token.
 * On success the app switches to the new profile, which rebuilds the graph,
 * so this screen needs no "done" navigation.
 */
@Composable
fun AddTokenProfileRoute(onBack: () -> Unit, modifier: Modifier = Modifier) {
    val viewModel = rostrumViewModel { container -> AddTokenProfileViewModel(container.profiles, container.appMessages) }
    val state by viewModel.state.collectAsStateWithLifecycle()
    AddTokenProfileScreen(state, viewModel, onBack, modifier)
}

@Composable
fun AddTokenProfileScreen(
    state: AddTokenProfileState,
    viewModel: AddTokenProfileViewModel?,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    val failure = (state.submit as? ActionState.Failed)?.error
    Column(modifier.fillMaxSize().background(colors.bg).imePadding()) {
        BackTopBar(onBack = onBack) {
            Text("Add a GitHub token profile", style = RostrumText.sheetTitle, color = colors.text, modifier = Modifier.semantics { heading() })
        }
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(
                "A profile with no desktop: its own repositories, filters and drafts, for another GitHub account.",
                style = RostrumText.body,
                color = colors.textSecondary,
            )
            RostrumTextField(
                value = state.label,
                onValueChange = { viewModel?.onLabelChange(it) },
                placeholder = "Name (defaults to the GitHub login)",
                accessibilityLabel = "Profile name",
            )
            RostrumTextField(
                value = state.token,
                onValueChange = { viewModel?.onTokenChange(it) },
                placeholder = "ghp_… or github_pat_…",
                accessibilityLabel = "Token",
                mono = true,
                minHeight = 56.dp,
                isError = failure != null,
                errorText = failure?.describe(),
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false, imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { viewModel?.add() }),
            )
            Text(
                "A github.com token with repo and read:org. Stored in the Android Keystore; never leaves this phone.",
                style = RostrumText.caption,
                color = colors.textSubtle,
            )
            PrimaryButton(
                "Add profile",
                onClick = { viewModel?.add() },
                modifier = Modifier.fillMaxWidth(),
                enabled = state.canSubmit,
                busy = state.submit.running,
                height = 48.dp,
            )
        }
    }
}
