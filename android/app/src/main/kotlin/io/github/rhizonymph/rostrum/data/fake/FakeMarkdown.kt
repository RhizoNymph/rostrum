package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.MdBlock
import io.github.rhizonymph.rostrum.data.model.MdBlockKind
import io.github.rhizonymph.rostrum.data.model.MdSpan

/**
 * A small markdown reader for the fake backend: paragraphs, headings, fenced
 * code, bullet/numbered/task lists, quotes, rules, and inline code, bold,
 * italic, strike and links. The real core parses with `rostrum-md`.
 */
internal object FakeMarkdown {
    fun parse(source: String): List<MdBlock> {
        val blocks = mutableListOf<MdBlock>()
        val lines = source.replace("\r\n", "\n").split('\n')
        var i = 0
        val paragraph = mutableListOf<String>()
        fun flush(quoteDepth: Int = 0) {
            if (paragraph.isNotEmpty()) {
                blocks += MdBlock(MdBlockKind.Paragraph, inline(paragraph.joinToString(" ")), quoteDepth = quoteDepth)
                paragraph.clear()
            }
        }
        while (i < lines.size) {
            val raw = lines[i]
            val line = raw.trimEnd()
            val trimmed = line.trimStart()
            when {
                trimmed.startsWith("```") -> {
                    flush()
                    val language = trimmed.removePrefix("```").trim().ifEmpty { null }
                    val code = mutableListOf<String>()
                    i++
                    while (i < lines.size && !lines[i].trimStart().startsWith("```")) code += lines[i++]
                    blocks += MdBlock(MdBlockKind.Code(language, code.joinToString("\n")), emptyList())
                }
                trimmed.isEmpty() -> flush()
                trimmed.matches(Regex("^(-{3,}|\\*{3,})$")) -> {
                    flush()
                    blocks += MdBlock(MdBlockKind.Rule, emptyList())
                }
                trimmed.startsWith("#") -> {
                    flush()
                    val level = trimmed.takeWhile { it == '#' }.length.coerceIn(1, 6)
                    blocks += MdBlock(MdBlockKind.Heading(level), inline(trimmed.drop(level).trim()))
                }
                trimmed.startsWith("> ") || trimmed == ">" -> {
                    flush()
                    blocks += MdBlock(MdBlockKind.Paragraph, inline(trimmed.removePrefix(">").trim()), quoteDepth = 1)
                }
                Regex("^[-*+] ").containsMatchIn(trimmed) -> {
                    flush()
                    var text = trimmed.drop(2)
                    val checked = when {
                        text.startsWith("[ ] ") -> false.also { text = text.drop(4) }
                        text.startsWith("[x] ", ignoreCase = true) -> true.also { text = text.drop(4) }
                        else -> null
                    }
                    val depth = 1 + (line.length - trimmed.length) / 2
                    blocks += MdBlock(MdBlockKind.ListItem(false, 0, checked), inline(text), listDepth = depth)
                }
                Regex("^\\d+[.)] ").containsMatchIn(trimmed) -> {
                    flush()
                    val number = trimmed.takeWhile { it.isDigit() }.toLong()
                    val text = trimmed.dropWhile { it.isDigit() }.drop(2)
                    blocks += MdBlock(MdBlockKind.ListItem(true, number, null), inline(text), listDepth = 1)
                }
                else -> paragraph += trimmed
            }
            i++
        }
        flush()
        return blocks
    }

    /** Inline spans: `code`, **bold**, *italic*, ~~strike~~, [text](url). */
    fun inline(text: String): List<MdSpan> {
        val spans = mutableListOf<MdSpan>()
        val plain = StringBuilder()
        var bold = false
        var italic = false
        var strike = false
        fun flush() {
            if (plain.isNotEmpty()) {
                spans += MdSpan(plain.toString(), bold = bold, italic = italic, strike = strike)
                plain.clear()
            }
        }
        var i = 0
        while (i < text.length) {
            val c = text[i]
            when {
                c == '`' -> {
                    val end = text.indexOf('`', i + 1)
                    if (end < 0) {
                        plain.append(c); i++
                    } else {
                        flush()
                        spans += MdSpan(text.substring(i + 1, end), code = true)
                        i = end + 1
                    }
                }
                text.startsWith("**", i) -> { flush(); bold = !bold; i += 2 }
                text.startsWith("~~", i) -> { flush(); strike = !strike; i += 2 }
                c == '*' || (c == '_' && (i == 0 || !text[i - 1].isLetterOrDigit())) -> { flush(); italic = !italic; i++ }
                c == '[' -> {
                    val close = text.indexOf("](", i)
                    val end = if (close >= 0) text.indexOf(')', close) else -1
                    if (close < 0 || end < 0) {
                        plain.append(c); i++
                    } else {
                        flush()
                        spans += MdSpan(text.substring(i + 1, close), bold = bold, italic = italic, link = text.substring(close + 2, end))
                        i = end + 1
                    }
                }
                else -> { plain.append(c); i++ }
            }
        }
        flush()
        return spans
    }
}
