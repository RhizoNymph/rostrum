package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.describe
import io.github.rhizonymph.rostrum.ui.common.ActionState
import io.github.rhizonymph.rostrum.ui.components.FieldError
import io.github.rhizonymph.rostrum.ui.components.PrimaryButton
import io.github.rhizonymph.rostrum.ui.components.RostrumIcons
import io.github.rhizonymph.rostrum.ui.components.RostrumTextField
import io.github.rhizonymph.rostrum.ui.components.TonalButton
import io.github.rhizonymph.rostrum.ui.theme.RostrumFonts
import io.github.rhizonymph.rostrum.ui.theme.RostrumText
import io.github.rhizonymph.rostrum.ui.theme.RostrumTheme

/**
 * First run: pairing with the desktop is the main path (it hands over the
 * desktop's GitHub sign-in); a personal access token is the fallback.
 */
@Composable
fun SignInScreen(
    state: SignInUiState,
    actions: SignInActions,
    onPairDesktop: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = RostrumTheme.colors
    Column(
        modifier
            .fillMaxSize()
            .background(colors.bg)
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(start = 20.dp, end = 20.dp, top = 40.dp),
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        Column(Modifier.padding(horizontal = 4.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("rostrum", style = RostrumText.wordmark, color = colors.text, modifier = Modifier.semantics { heading() })
            Text(
                "Open pull requests across your repositories, in one feed.",
                style = RostrumText.body.copy(fontSize = 16.sp, lineHeight = 24.sp),
                color = colors.textSecondary,
            )
        }
        state.notice?.let { NoticeBanner(it) }
        PairCard(onPairDesktop)
        TokenSection(state, actions)
        Text(
            "Rostrum only talks to GitHub and, if you pair one, your own desktop.",
            style = RostrumText.caption.copy(lineHeight = RostrumText.meta.lineHeight),
            color = colors.textSubtle,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 28.dp),
        )
    }
}

@Composable
private fun PairCard(onPairDesktop: () -> Unit) {
    val colors = RostrumTheme.colors
    OnboardingCard {
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("Pair with your desktop", style = RostrumText.meta.copy(fontWeight = RostrumText.label.fontWeight), color = colors.textMuted)
            Text(
                "Pairing also signs this phone in with the desktop's GitHub account, so there is nothing else to set up.",
                style = RostrumText.rowTitle.copy(fontWeight = RostrumText.body.fontWeight, lineHeight = RostrumText.body.lineHeight),
                color = colors.text,
            )
        }
        PrimaryButton("Pair with your desktop", onPairDesktop, modifier = Modifier.fillMaxWidth(), height = 48.dp)
        Text(
            pairingPageSentence(
                "Rostrum on your desktop serves a pairing page at ",
                ". Open it on this phone and tap Open in Rostrum, or scan its QR code with the camera app.",
            ),
            style = RostrumText.caption.copy(lineHeight = RostrumText.meta.lineHeight),
            color = colors.textMuted,
        )
    }
}

@Composable
private fun TokenSection(state: SignInUiState, actions: SignInActions) {
    val colors = RostrumTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        val expansion = if (state.tokenFormOpen) "expanded" else "collapsed"
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                .clickable(role = Role.Button, onClick = actions::toggleTokenForm)
                .semantics { stateDescription = expansion }
                .padding(horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                "Use a personal access token instead",
                style = RostrumText.rowTitle,
                color = colors.accentText,
                modifier = Modifier.weight(1f),
            )
            Icon(
                if (state.tokenFormOpen) RostrumIcons.ChevronUp else RostrumIcons.ChevronDown,
                contentDescription = null,
                tint = colors.accentText,
                modifier = Modifier.size(18.dp),
            )
        }
        if (state.tokenFormOpen) TokenForm(state, actions)
    }
}

@Composable
private fun TokenForm(state: SignInUiState, actions: SignInActions) {
    val colors = RostrumTheme.colors
    val failure = (state.submit as? ActionState.Failed)?.error
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        FieldLabel("Token")
        RostrumTextField(
            value = state.token,
            onValueChange = actions::onTokenChange,
            placeholder = "ghp_… or github_pat_…",
            accessibilityLabel = "Token",
            mono = true,
            minHeight = 56.dp,
            isError = failure != null,
            errorText = failure?.describe(),
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false, imeAction = ImeAction.Next),
        )
    }
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        FieldLabel("Host")
        RostrumTextField(
            value = state.host,
            onValueChange = actions::onHostChange,
            accessibilityLabel = "GitHub host",
            mono = true,
            minHeight = 56.dp,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { actions.signIn() }),
        )
        Text(
            "Change for GitHub Enterprise Server",
            style = RostrumText.caption,
            color = colors.textSubtle,
            modifier = Modifier.padding(horizontal = 4.dp),
        )
    }
    Text(
        buildAnnotatedString {
            append("Needs ")
            withStyle(SpanStyle(fontFamily = RostrumFonts.Mono)) { append("repo") }
            append(" and ")
            withStyle(SpanStyle(fontFamily = RostrumFonts.Mono)) { append("read:org") }
            append(". Stored in the Android Keystore; never leaves this phone.")
        },
        style = RostrumText.caption.copy(lineHeight = RostrumText.meta.lineHeight),
        color = colors.textSubtle,
        modifier = Modifier.padding(horizontal = 4.dp),
    )
    failure?.let { FieldError(it.describe()) }
    TonalButton(
        "Sign in with token",
        actions::signIn,
        modifier = Modifier.fillMaxWidth(),
        enabled = state.canSubmit,
        busy = state.submit == ActionState.Running,
        height = 48.dp,
    )
}

@Composable
internal fun FieldLabel(text: String) {
    Text(
        text,
        style = RostrumText.meta.copy(fontWeight = RostrumText.label.fontWeight),
        color = RostrumTheme.colors.textMuted,
    )
}

// --- previews ----------------------------------------------------------------

@Preview(widthDp = 412, heightDp = 915)
@Composable
private fun SignInPreview() {
    RostrumTheme {
        SignInScreen(SignInUiState(), NoSignInActions, onPairDesktop = {})
    }
}

@Preview(widthDp = 412, heightDp = 1100)
@Composable
private fun SignInTokenErrorPreview() {
    RostrumTheme {
        SignInScreen(
            SignInUiState(
                notice = "Your saved sign-in couldn't be read. Sign in again.",
                tokenFormOpen = true,
                token = "ghp_wrong",
                submit = ActionState.Failed(BackendError.GitHubAuthFailed("Bad credentials")),
            ),
            NoSignInActions,
            onPairDesktop = {},
        )
    }
}
