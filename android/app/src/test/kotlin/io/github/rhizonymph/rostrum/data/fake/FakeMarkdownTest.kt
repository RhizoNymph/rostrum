package io.github.rhizonymph.rostrum.data.fake

import io.github.rhizonymph.rostrum.data.model.MdBlockKind
import io.github.rhizonymph.rostrum.data.model.MdSpan
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertInstanceOf
import org.junit.jupiter.api.Test

class FakeMarkdownTest {
    @Test
    fun `paragraphs split on blank lines and join wrapped lines`() {
        val blocks = FakeMarkdown.parse("one\ntwo\n\nthree")
        assertEquals(2, blocks.size)
        assertEquals("one two", blocks[0].spans.joinToString("") { it.text })
    }

    @Test
    fun `inline code, bold, italic and links`() {
        val spans = FakeMarkdown.inline("a `b` **c** *d* [e](https://x.y)")
        assertEquals(MdSpan("b", code = true), spans[1])
        assertEquals(MdSpan("c", bold = true), spans[3])
        assertEquals(MdSpan("d", italic = true), spans[5])
        assertEquals("https://x.y", spans.last().link)
    }

    @Test
    fun `lists, headings, code fences and rules`() {
        val blocks = FakeMarkdown.parse("# Title\n\n- [x] done\n1. first\n\n```rust\nfn a() {}\n```\n\n---")
        assertEquals(MdBlockKind.Heading(1), blocks[0].kind)
        assertEquals(MdBlockKind.ListItem(false, 0, true), blocks[1].kind)
        assertEquals(MdBlockKind.ListItem(true, 1, null), blocks[2].kind)
        assertEquals(MdBlockKind.Code("rust", "fn a() {}"), blocks[3].kind)
        assertInstanceOf(MdBlockKind.Rule::class.java, blocks[4].kind)
    }

    @Test
    fun `quotes carry their depth`() {
        assertEquals(1, FakeMarkdown.parse("> quoted").single().quoteDepth)
    }
}
