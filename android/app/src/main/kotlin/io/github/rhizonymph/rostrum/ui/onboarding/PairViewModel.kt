package io.github.rhizonymph.rostrum.ui.onboarding

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.rhizonymph.rostrum.data.BackendError
import io.github.rhizonymph.rostrum.data.Outcome
import io.github.rhizonymph.rostrum.data.RostrumBackend
import io.github.rhizonymph.rostrum.data.RostrumLog
import io.github.rhizonymph.rostrum.data.model.DesktopProbe
import io.github.rhizonymph.rostrum.data.model.PairingPreview
import io.github.rhizonymph.rostrum.data.session.SessionRepository
import io.github.rhizonymph.rostrum.ui.common.ActionState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** A `rostrum://pair?…` link, read before anything is contacted. */
sealed interface LinkState {
    /** The core is reading the link. */
    data object Reading : LinkState

    data class Preview(val preview: PairingPreview, val pairing: ActionState) : LinkState

    data class Invalid(val error: BackendError) : LinkState
}

/** Where manual pairing is: typing, checking the address, comparing, pairing. */
sealed interface ManualStep {
    data object Editing : ManualStep

    data object Probing : ManualStep

    data class ProbeFailed(val error: BackendError) : ManualStep

    /** The desktop answered; show its fingerprint for comparison. */
    data class Probed(val probe: DesktopProbe) : ManualStep

    data class Pairing(val probe: DesktopProbe) : ManualStep

    data class PairFailed(val probe: DesktopProbe, val error: BackendError) : ManualStep
}

data class ManualForm(
    val host: String = "",
    val port: String = DEFAULT_PAIRING_PORT.toString(),
    val code: String = "",
    val step: ManualStep = ManualStep.Editing,
) {
    /** The desktop that answered the probe, while it still applies. */
    val probe: DesktopProbe?
        get() = when (step) {
            is ManualStep.Probed -> step.probe
            is ManualStep.Pairing -> step.probe
            is ManualStep.PairFailed -> step.probe
            ManualStep.Editing, ManualStep.Probing, is ManualStep.ProbeFailed -> null
        }

    val canProbe: Boolean get() = host.isNotBlank() && step != ManualStep.Probing && step !is ManualStep.Pairing

    val canPair: Boolean
        get() = probe?.compatible == true && isCompletePairingCode(code) && step !is ManualStep.Pairing
}

data class PairUiState(
    /** `null` when the screen was opened without a link. */
    val link: LinkState?,
    val manualOpen: Boolean,
    val manual: ManualForm = ManualForm(),
    /** Pairing finished; the route leaves the screen. */
    val paired: Boolean = false,
)

interface PairActions {
    fun pairWithLink()
    fun openManual()
    fun onHostChange(host: String)
    fun onPortChange(port: String)
    fun onCodeChange(code: String)
    fun probe()
    fun pairManual()
}

object NoPairActions : PairActions {
    override fun pairWithLink() = Unit
    override fun openManual() = Unit
    override fun onHostChange(host: String) = Unit
    override fun onPortChange(port: String) = Unit
    override fun onCodeChange(code: String) = Unit
    override fun probe() = Unit
    override fun pairManual() = Unit
}

/**
 * Pairing with the desktop: from a deep link (preview, then pair), or by hand
 * (address and code, a probe to show the certificate fingerprint for the user
 * to compare, then pair pinned to it). Pairing also hands over the desktop's
 * GitHub token when the phone has none, which signs the app in.
 */
class PairViewModel(
    private val backend: RostrumBackend,
    private val session: SessionRepository,
    private val link: String?,
) : ViewModel(), PairActions {
    private val _state = MutableStateFlow(
        PairUiState(link = if (link == null) null else LinkState.Reading, manualOpen = false),
    )
    val state: StateFlow<PairUiState> = _state.asStateFlow()

    init {
        if (link != null) viewModelScope.launch { readLink(link) }
    }

    private suspend fun readLink(uri: String) {
        val next = when (val parsed = backend.parsePairingLink(uri)) {
            is Outcome.Ok -> PairUiState(LinkState.Preview(parsed.value, ActionState.Idle), manualOpen = false)
            is Outcome.Err -> {
                RostrumLog.w(TAG, "pair_link_invalid", "error" to parsed.error::class.simpleName)
                PairUiState(LinkState.Invalid(parsed.error), manualOpen = true)
            }
        }
        _state.update { current -> next.copy(manual = current.manual, manualOpen = current.manualOpen || next.manualOpen) }
    }

    override fun pairWithLink() {
        val uri = link ?: return
        val preview = _state.value.link as? LinkState.Preview ?: return
        if (preview.pairing == ActionState.Running) return
        setLink(preview.copy(pairing = ActionState.Running))
        viewModelScope.launch {
            when (val result = session.pairWithLink(uri)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "paired_by_link", "machine" to result.value.machine.name)
                    setLink(preview.copy(pairing = ActionState.Idle))
                    _state.update { it.copy(paired = true) }
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "pair_by_link_failed", "error" to result.error::class.simpleName)
                    setLink(preview.copy(pairing = ActionState.Failed(result.error)))
                }
            }
        }
    }

    private fun setLink(link: LinkState) {
        _state.update { it.copy(link = link) }
    }

    override fun openManual() {
        _state.update { it.copy(manualOpen = true) }
    }

    private fun updateManual(transform: (ManualForm) -> ManualForm) {
        _state.update { it.copy(manual = transform(it.manual)) }
    }

    override fun onHostChange(host: String) {
        updateManual { it.copy(host = host, step = ManualStep.Editing) }
    }

    override fun onPortChange(port: String) {
        updateManual { it.copy(port = port.filter(Char::isDigit).take(5), step = ManualStep.Editing) }
    }

    override fun onCodeChange(code: String) {
        updateManual { form ->
            val step = form.step
            form.copy(
                code = formatPairingCode(code),
                step = if (step is ManualStep.PairFailed) ManualStep.Probed(step.probe) else step,
            )
        }
    }

    override fun probe() {
        val form = _state.value.manual
        if (!form.canProbe) return
        val port = parsePort(form.port)
        if (port == null) {
            updateManual { it.copy(step = ManualStep.ProbeFailed(BackendError.InvalidInput("Ports run from 1 to 65535"))) }
            return
        }
        updateManual { it.copy(step = ManualStep.Probing) }
        viewModelScope.launch {
            val step = when (val result = backend.probeDesktop(form.host.trim(), port)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "desktop_probed", "machine" to result.value.machine, "compatible" to result.value.compatible)
                    ManualStep.Probed(result.value)
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "desktop_probe_failed", "error" to result.error::class.simpleName)
                    ManualStep.ProbeFailed(result.error)
                }
            }
            updateManual { it.copy(step = step) }
        }
    }

    override fun pairManual() {
        val form = _state.value.manual
        val probe = form.probe ?: return
        if (!form.canPair) return
        updateManual { it.copy(step = ManualStep.Pairing(probe)) }
        viewModelScope.launch {
            when (val result = session.pairManual(probe.host, probe.port, probe.fingerprint, form.code)) {
                is Outcome.Ok -> {
                    RostrumLog.i(TAG, "paired_by_hand", "machine" to result.value.machine.name)
                    updateManual { it.copy(step = ManualStep.Probed(probe)) }
                    _state.update { it.copy(paired = true) }
                }
                is Outcome.Err -> {
                    RostrumLog.w(TAG, "pair_by_hand_failed", "error" to result.error::class.simpleName)
                    updateManual { it.copy(step = ManualStep.PairFailed(probe, result.error)) }
                }
            }
        }
    }

    private companion object {
        const val TAG = "RostrumPair"
    }
}
