package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.CodeSegment

/**
 * A deliberately small tokenizer that colours sample code the way the core's
 * highlighter would, with the mockups' syntax palette. Only the fake backend
 * uses it; the real core sends coloured segments.
 */
internal object FakeHighlighter {
    const val PLAIN = 0xFFE4E7EE.toInt()
    const val KEYWORD = 0xFFC4A1FF.toInt()
    const val FUNCTION = 0xFFF0B86E.toInt()
    const val TYPE = 0xFF7EE0C3.toInt()
    const val NUMBER = 0xFFF5A97F.toInt()
    const val COMMENT = 0xFF8B94A7.toInt()
    const val STRING = 0xFFA5D6A7.toInt()
    const val HEADING = 0xFF8CBCFF.toInt()

    private val rustKeywords = setOf(
        "as", "async", "await", "break", "const", "continue", "crate", "else", "enum", "fn", "for",
        "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
        "self", "Self", "static", "struct", "super", "trait", "type", "use", "where", "while",
        "true", "false",
    )

    fun highlight(path: String, line: String): List<CodeSegment> = when {
        path.endsWith(".md") -> markdown(line)
        else -> code(line)
    }

    private fun markdown(line: String): List<CodeSegment> = when {
        line.isEmpty() -> emptyList()
        line.startsWith("#") -> listOf(CodeSegment(line, HEADING, bold = true))
        else -> listOf(CodeSegment(line, PLAIN))
    }

    private fun code(line: String): List<CodeSegment> {
        val out = mutableListOf<CodeSegment>()
        var i = 0
        val plain = StringBuilder()
        fun flushPlain() {
            if (plain.isNotEmpty()) {
                out += CodeSegment(plain.toString(), PLAIN)
                plain.clear()
            }
        }
        while (i < line.length) {
            val c = line[i]
            when {
                line.startsWith("//", i) -> {
                    flushPlain()
                    out += CodeSegment(line.substring(i), COMMENT, italic = true)
                    i = line.length
                }
                c == '"' -> {
                    flushPlain()
                    val end = line.indexOf('"', i + 1).let { if (it < 0) line.length - 1 else it }
                    out += CodeSegment(line.substring(i, end + 1), STRING)
                    i = end + 1
                }
                c.isDigit() && (i == 0 || !line[i - 1].isLetterOrDigit()) -> {
                    flushPlain()
                    var j = i
                    while (j < line.length && (line[j].isLetterOrDigit() || line[j] == '_' || line[j] == '.')) j++
                    out += CodeSegment(line.substring(i, j), NUMBER)
                    i = j
                }
                c.isLetter() || c == '_' -> {
                    var j = i
                    while (j < line.length && (line[j].isLetterOrDigit() || line[j] == '_')) j++
                    val word = line.substring(i, j)
                    val color = when {
                        word in rustKeywords -> KEYWORD
                        j < line.length && line[j] == '(' -> FUNCTION
                        word[0].isUpperCase() -> TYPE
                        word in setOf("u8", "u16", "u32", "u64", "usize", "i32", "i64", "f32", "f64", "bool", "str") -> TYPE
                        else -> null
                    }
                    if (color == null) {
                        plain.append(word)
                    } else {
                        flushPlain()
                        out += CodeSegment(word, color)
                    }
                    i = j
                }
                else -> {
                    plain.append(c)
                    i++
                }
            }
        }
        flushPlain()
        return out
    }
}
