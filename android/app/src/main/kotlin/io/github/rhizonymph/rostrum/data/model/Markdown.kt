package io.github.rhizonymph.rostrum.data.model

/**
 * Markdown flattened into a render-ready, non-recursive block list
 * (`markdown.rs`). Nesting is carried as [quoteDepth] and [listDepth].
 */
data class MdBlock(
    val kind: MdBlockKind,
    /** Inline content. Empty for Code, Rule, TableRow and Image. */
    val spans: List<MdSpan>,
    /** How many block quotes enclose this block; draw that many bars. */
    val quoteDepth: Int = 0,
    /**
     * How many lists enclose this block. A ListItem at depth 1 is a top-level
     * bullet; any other block with a non-zero depth continues the item above.
     */
    val listDepth: Int = 0,
)

sealed interface MdBlockKind {
    data object Paragraph : MdBlockKind

    /** [level] is 1..=6. */
    data class Heading(val level: Int) : MdBlockKind

    data class Code(val language: String?, val code: String) : MdBlockKind

    /** The first block of a list item; its spans are the item's first paragraph. */
    data class ListItem(
        val ordered: Boolean,
        /** The ordinal for ordered lists; 0 for bullets. */
        val number: Long,
        /** Task-list checkbox state, when the item has one. */
        val checked: Boolean?,
    ) : MdBlockKind

    data object Rule : MdBlockKind

    /** One row of a table; consecutive rows form the table. */
    data class TableRow(val cells: List<List<MdSpan>>, val header: Boolean) : MdBlockKind

    /** An image on a line of its own. */
    data class Image(val url: String, val alt: String) : MdBlockKind
}

/** A run of text with one style. */
data class MdSpan(
    val text: String,
    val bold: Boolean = false,
    val italic: Boolean = false,
    /** Inline code: render monospace. */
    val code: Boolean = false,
    val strike: Boolean = false,
    /** The span is (part of) a link to this URL. */
    val link: String? = null,
)
