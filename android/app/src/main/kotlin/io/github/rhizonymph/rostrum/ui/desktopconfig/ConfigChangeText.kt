package io.github.rhizonymph.rostrum.ui.desktopconfig

import io.github.rhizonymph.rostrum.data.model.ConfigChange
import io.github.rhizonymph.rostrum.data.model.ConfigField

/** One [ConfigChange] as the preview draws it. */
sealed interface ConfigChangeView {
    val label: String

    /** A list setting (repositories, authors, trunks): what it gains and loses, or only a new order. */
    data class ListDiff(
        override val label: String,
        val added: List<String>,
        val removed: List<String>,
        val reordered: Boolean,
    ) : ConfigChangeView

    /** Any other setting: its value before and after. */
    data class Value(override val label: String, val before: String, val after: String) : ConfigChangeView
}

/** The core's change values (`a, b`, `(none)`, `true`, `(unset)`) in the preview's words. */
object ConfigChangeText {
    private val listFields = setOf(ConfigField.Repos, ConfigField.Authors, ConfigField.Trunks)

    fun view(change: ConfigChange): ConfigChangeView {
        if (change.field !in listFields) {
            return ConfigChangeView.Value(change.label, value(change.before), value(change.after))
        }
        val before = items(change.before)
        val after = items(change.after)
        val added = after.filter { it !in before }
        val removed = before.filter { it !in after }
        return ConfigChangeView.ListDiff(
            label = change.label,
            added = added,
            removed = removed,
            reordered = added.isEmpty() && removed.isEmpty() && before != after,
        )
    }

    private fun items(raw: String): List<String> =
        if (raw == "(none)" || raw.isBlank()) emptyList() else raw.split(", ").map { it.trim() }

    private fun value(raw: String): String = when (raw) {
        "true" -> "on"
        "false" -> "off"
        "(unset)" -> "not set"
        "(none)" -> "none"
        else -> raw
    }
}

/** The words of "send settings to the desktop", shared by the Desktop tab and Settings. */
object PushConfigText {
    const val CHANGED_SINCE = "The desktop's settings changed since you looked"

    fun sheetTitle(machine: String) = "Send settings to $machine"

    fun rowCaption(machine: String) = "Replace $machine's repositories, filters, sorts and trunks with this profile's"

    fun body(machine: String) =
        "This replaces $machine's repositories, pull requests and issues per repository, feed filters, sorts, trunks and stash default with this profile's."

    fun changedSince(machine: String) = "This is what sending would change on $machine now."

    fun nothingToSend(machine: String) = "Nothing to send: $machine already has this profile's settings."

    fun sent(machine: String) = "Sent settings to $machine"
}
