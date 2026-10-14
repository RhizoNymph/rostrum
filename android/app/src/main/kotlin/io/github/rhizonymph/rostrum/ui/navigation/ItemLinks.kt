package io.github.rhizonymph.rostrum.ui.navigation

import io.github.rhizonymph.rostrum.data.model.IssueRef
import io.github.rhizonymph.rostrum.data.model.PrRef

/**
 * `rostrum://pr?repo=<owner>/<name>&number=<n>` and
 * `rostrum://issue?repo=<owner>/<name>&number=<n>`: links from other apps
 * (Orchestral's "Open in Rostrum") to one pull request or issue.
 *
 * The same grammar as the desktop's `rostrum_core::link`, checked the same
 * way: `repo` may be percent-encoded, unknown parameters and a `#fragment`
 * are ignored, a repeated `repo` or `number` is refused, the repository must
 * satisfy GitHub's own name rules, and the number must be a plain positive
 * decimal. One difference: numbers stop at [Int.MAX_VALUE] here (the app's
 * refs are `Int`), at `u32::MAX` on the desktop; GitHub's are far below both.
 * Plain values in, plain values out, so it runs on the JVM.
 */
object ItemLinks {
    const val MAX_LINK_LENGTH = 2048
    private const val SCHEME = "rostrum://"
    private const val MAX_OWNER = 39
    private const val MAX_NAME = 100

    /** Whether [uri] is a `rostrum://pr` or `rostrum://issue` link at all, well-formed or not. */
    fun isItemLink(uri: String): Boolean {
        val host = hostOf(uri.trim()) ?: return false
        return host.equals("pr", ignoreCase = true) || host.equals("issue", ignoreCase = true)
    }

    fun parse(input: String): ItemLink {
        val uri = input.trim()
        if (uri.length > MAX_LINK_LENGTH) return ItemLink.Invalid("it is longer than $MAX_LINK_LENGTH characters")
        val host = hostOf(uri) ?: return ItemLink.Invalid("it is not a rostrum:// link")
        val isPull = when {
            host.equals("pr", ignoreCase = true) -> true
            host.equals("issue", ignoreCase = true) -> false
            else -> return ItemLink.Invalid("rostrum://$host is not something rostrum opens (expected rostrum://pr or rostrum://issue)")
        }
        val afterScheme = uri.substring(SCHEME.length).substringBefore('#')
        val query = if ('?' in afterScheme) afterScheme.substringAfter('?') else ""

        var repo: String? = null
        var number: String? = null
        for (pair in query.split('&')) {
            if (pair.isEmpty()) continue
            val rawKey = pair.substringBefore('=')
            val rawValue = if ('=' in pair) pair.substringAfter('=') else ""
            val key = percentDecode(rawKey) ?: continue
            when (key) {
                "repo" -> {
                    if (repo != null) return ItemLink.Invalid("it gives `repo` more than once")
                    repo = percentDecode(rawValue) ?: return ItemLink.Invalid("its `repo` parameter is not valid percent-encoding")
                }
                "number" -> {
                    if (number != null) return ItemLink.Invalid("it gives `number` more than once")
                    number = percentDecode(rawValue) ?: return ItemLink.Invalid("its `number` parameter is not valid percent-encoding")
                }
            }
        }
        val repoValue = repo ?: return ItemLink.Invalid("it has no `repo` parameter")
        val numberValue = number ?: return ItemLink.Invalid("it has no `number` parameter")
        if (!isRepo(repoValue)) return ItemLink.Invalid("`$repoValue` is not a GitHub repository (expected owner/name)")
        val parsed = parseNumber(numberValue)
            ?: return ItemLink.Invalid("`$numberValue` is not a pull request or issue number")
        return if (isPull) ItemLink.Pull(PrRef(repoValue, parsed)) else ItemLink.Issue(IssueRef(repoValue, parsed))
    }

    /** The host of a `rostrum://` link (`pr`, `pr/` trimmed to `pr`), or `null` for any other scheme. */
    private fun hostOf(uri: String): String? {
        if (!uri.regionMatches(0, SCHEME, 0, SCHEME.length, ignoreCase = true)) return null
        val target = uri.substring(SCHEME.length).substringBefore('#').substringBefore('?')
        return target.removeSuffix("/")
    }

    /** `%XX` escapes decoded as UTF-8; `null` for a broken escape or bytes that are not UTF-8. `+` stays `+`. */
    internal fun percentDecode(input: String): String? {
        val out = java.io.ByteArrayOutputStream(input.length)
        val bytes = input.toByteArray(Charsets.UTF_8)
        var index = 0
        while (index < bytes.size) {
            val byte = bytes[index]
            if (byte == '%'.code.toByte()) {
                if (index + 2 >= bytes.size) return null
                val high = hex(bytes[index + 1]) ?: return null
                val low = hex(bytes[index + 2]) ?: return null
                out.write((high shl 4) or low)
                index += 3
            } else {
                out.write(byte.toInt())
                index += 1
            }
        }
        val decoder = Charsets.UTF_8.newDecoder()
            .onMalformedInput(java.nio.charset.CodingErrorAction.REPORT)
            .onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT)
        return try {
            decoder.decode(java.nio.ByteBuffer.wrap(out.toByteArray())).toString()
        } catch (e: java.nio.charset.CharacterCodingException) {
            null
        }
    }

    private fun hex(byte: Byte): Int? = when (val c = byte.toInt().toChar()) {
        in '0'..'9' -> c - '0'
        in 'a'..'f' -> c - 'a' + 10
        in 'A'..'F' -> c - 'A' + 10
        else -> null
    }

    /** `owner/name` under GitHub's rules. */
    internal fun isRepo(value: String): Boolean {
        val slash = value.indexOf('/')
        if (slash < 0) return false
        val owner = value.substring(0, slash)
        val name = value.substring(slash + 1)
        val ownerOk = owner.length in 1..MAX_OWNER &&
            owner.all { it.isAsciiAlphanumeric() || it == '-' } &&
            !owner.startsWith('-') && !owner.endsWith('-') && "--" !in owner
        val nameOk = name.length in 1..MAX_NAME &&
            name.all { it.isAsciiAlphanumeric() || it == '.' || it == '_' || it == '-' } &&
            name != "." && name != ".."
        return ownerOk && nameOk
    }

    private fun Char.isAsciiAlphanumeric(): Boolean = this in 'a'..'z' || this in 'A'..'Z' || this in '0'..'9'

    private fun parseNumber(value: String): Int? {
        if (value.isEmpty() || !value.all { it in '0'..'9' }) return null
        val number = value.toIntOrNull() ?: return null
        return number.takeIf { it > 0 }
    }
}

/** A parsed item link: what it opens, or why it can't be opened. */
sealed interface ItemLink {
    data class Pull(val pr: PrRef) : ItemLink

    data class Issue(val issue: IssueRef) : ItemLink

    /** [reason] completes "Rostrum can't open this link: …". */
    data class Invalid(val reason: String) : ItemLink
}
