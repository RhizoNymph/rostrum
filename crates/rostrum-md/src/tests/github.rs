//! Tests for the GitHub shorthand post-pass run by [`crate::parse_github`].

use crate::ast::{Block, GitHubContext, Inline};
use crate::{parse, parse_github};

use super::{code, text};

fn context() -> GitHubContext {
    GitHubContext::new("owner", "repo")
}

/// Parses with shorthand expansion and returns the single paragraph's inlines.
fn inlines(source: &str) -> Vec<Inline> {
    let mut document = parse_github(source, &context());
    assert_eq!(
        document.blocks.len(),
        1,
        "expected one block for {source:?}, got {document:?}"
    );
    match document.blocks.remove(0) {
        Block::Paragraph(children) => children,
        other => panic!("expected a paragraph for {source:?}, got {other:?}"),
    }
}

/// Asserts that expansion changes nothing about the parsed tree.
fn assert_untouched(source: &str) {
    assert_eq!(
        parse_github(source, &context()),
        parse(source),
        "shorthand was expanded in {source:?}"
    );
}

fn mention(login: &str) -> Inline {
    Inline::Link {
        dest: format!("https://github.com/{login}"),
        children: vec![text(&format!("@{login}"))],
    }
}

fn issue(number: u32) -> Inline {
    Inline::Link {
        dest: format!("https://github.com/owner/repo/issues/{number}"),
        children: vec![text(&format!("#{number}"))],
    }
}

fn cross_repo(owner: &str, repo: &str, number: u32) -> Inline {
    Inline::Link {
        dest: format!("https://github.com/{owner}/{repo}/issues/{number}"),
        children: vec![text(&format!("{owner}/{repo}#{number}"))],
    }
}

// --- mentions ---------------------------------------------------------------

#[test]
fn mention_becomes_a_link() {
    assert_eq!(
        inlines("cc @octocat please"),
        vec![text("cc "), mention("octocat"), text(" please")]
    );
}

#[test]
fn mention_at_the_start_of_the_text() {
    assert_eq!(
        inlines("@octocat wrote this"),
        vec![mention("octocat"), text(" wrote this")]
    );
}

#[test]
fn mention_at_the_very_end() {
    assert_eq!(
        inlines("thanks @octocat"),
        vec![text("thanks "), mention("octocat")]
    );
}

#[test]
fn mention_with_hyphens_and_digits() {
    assert_eq!(inlines("@zed-industries"), vec![mention("zed-industries")]);
    assert_eq!(inlines("@user123"), vec![mention("user123")]);
    assert_eq!(inlines("@a1-b2-c3"), vec![mention("a1-b2-c3")]);
}

#[test]
fn single_character_mention() {
    assert_eq!(inlines("@a"), vec![mention("a")]);
}

#[test]
fn mention_of_maximum_length_is_accepted() {
    let login = "a".repeat(39);
    assert_eq!(inlines(&format!("@{login}")), vec![mention(&login)]);
}

#[test]
fn mention_longer_than_the_limit_is_rejected() {
    assert_untouched(&format!("@{}", "a".repeat(40)));
}

#[test]
fn mention_may_not_start_with_a_hyphen() {
    assert_untouched("@-nope");
}

#[test]
fn trailing_hyphens_are_returned_to_the_prose() {
    assert_eq!(inlines("@octocat--"), vec![mention("octocat"), text("--")]);
}

#[test]
fn login_containing_an_underscore_is_rejected() {
    assert_untouched("@octo_cat");
}

#[test]
fn mention_followed_by_punctuation() {
    for (source, tail) in [
        ("@octocat.", "."),
        ("@octocat,", ","),
        ("@octocat!", "!"),
        ("@octocat)", ")"),
        ("@octocat:", ":"),
        ("@octocat/", "/"),
    ] {
        assert_eq!(
            inlines(source),
            vec![mention("octocat"), text(tail)],
            "for {source:?}"
        );
    }
}

#[test]
fn mention_preceded_by_punctuation() {
    assert_eq!(
        inlines("(@octocat)"),
        vec![text("("), mention("octocat"), text(")")]
    );
    assert_eq!(
        inlines("cc:@octocat"),
        vec![text("cc:"), mention("octocat")]
    );
}

#[test]
fn bare_at_signs_are_left_alone() {
    for source in ["@", "@ octocat", "@@octocat", "a @ b"] {
        assert_untouched(source);
    }
}

#[test]
fn several_mentions_in_one_paragraph() {
    assert_eq!(
        inlines("@a and @b-c"),
        vec![mention("a"), text(" and "), mention("b-c")]
    );
}

// --- email addresses --------------------------------------------------------

#[test]
fn email_addresses_are_never_mentions() {
    for source in [
        "foo@bar.com",
        "write to foo@bar.com today",
        "first.last@sub.example.co.uk",
        "user+tag@example.org",
        "1@example.com",
    ] {
        assert_untouched(source);
    }
}

// --- issue references -------------------------------------------------------

#[test]
fn issue_reference_uses_the_context_repository() {
    assert_eq!(inlines("fixes #42"), vec![text("fixes "), issue(42)]);
}

#[test]
fn issue_reference_honours_a_different_context() {
    let context = GitHubContext::new("zed-industries", "zed");
    let document = parse_github("see #7", &context);
    assert_eq!(
        document.blocks,
        vec![Block::Paragraph(vec![
            text("see "),
            Inline::Link {
                dest: "https://github.com/zed-industries/zed/issues/7".into(),
                children: vec![text("#7")],
            },
        ])]
    );
}

#[test]
fn issue_reference_at_the_start_of_a_line() {
    assert_eq!(
        inlines("#1 is the first"),
        vec![issue(1), text(" is the first")]
    );
}

#[test]
fn issue_numbers_may_not_have_a_leading_zero() {
    for source in ["#0", "#01", "#0123"] {
        assert_untouched(source);
    }
}

#[test]
fn absurdly_long_issue_numbers_are_rejected() {
    assert_untouched("#1234567890123");
}

#[test]
fn issue_reference_must_not_run_into_a_word() {
    for source in ["#12abc", "#12_x"] {
        assert_untouched(source);
    }
}

#[test]
fn hash_after_a_word_character_is_not_a_reference() {
    for source in ["abc#12", "v1.2#3", "issue#12", "_#12"] {
        assert_untouched(source);
    }
}

#[test]
fn lone_hash_signs_are_left_alone() {
    for source in ["a # b", "a #", "a ## b", "a #x"] {
        assert_untouched(source);
    }
}

// --- cross-repository references --------------------------------------------

#[test]
fn cross_repository_reference() {
    assert_eq!(
        inlines("see rust-lang/rust#1234 for context"),
        vec![
            text("see "),
            cross_repo("rust-lang", "rust", 1234),
            text(" for context"),
        ]
    );
}

#[test]
fn cross_repository_reference_with_punctuation_in_the_repo_name() {
    assert_eq!(inlines("o/repo.js#7"), vec![cross_repo("o", "repo.js", 7)]);
    assert_eq!(inlines("o/my_repo#7"), vec![cross_repo("o", "my_repo", 7)]);
    assert_eq!(inlines("o/my-repo#7"), vec![cross_repo("o", "my-repo", 7)]);
}

#[test]
fn cross_repository_reference_needs_a_slash_and_a_hash() {
    for source in ["owner#12", "owner/repo", "owner/repo#", "owner//repo#1"] {
        assert_untouched(source);
    }
}

#[test]
fn deeper_paths_are_not_cross_repository_references() {
    for source in ["a/b/c#1", "docs/api/v2#3"] {
        assert_untouched(source);
    }
}

#[test]
fn cross_repository_owner_obeys_login_rules() {
    assert_untouched(&format!("{}/repo#1", "a".repeat(40)));
    assert_untouched("-bad/repo#1");
}

// --- URLs -------------------------------------------------------------------

#[test]
fn bare_urls_with_fragments_are_untouched() {
    for source in [
        "https://github.com/owner/repo#readme",
        "https://github.com/owner/repo/issues/1",
        "see http://example.com/a/b#12 here",
        "git@github.com:owner/repo.git",
    ] {
        assert_untouched(source);
    }
}

#[test]
fn autolink_destinations_are_untouched() {
    assert_untouched("<https://github.com/owner/repo#12>");
}

// --- contexts that must never be rewritten ----------------------------------

#[test]
fn code_spans_are_never_rewritten() {
    assert_eq!(
        inlines("use `@octocat` and `#12` and `o/r#3`"),
        vec![
            text("use "),
            code("@octocat"),
            text(" and "),
            code("#12"),
            text(" and "),
            code("o/r#3"),
        ]
    );
}

#[test]
fn fenced_code_blocks_are_never_rewritten() {
    assert_untouched("```\n@octocat #12 owner/repo#3\n```\n");
    assert_untouched("```rust\n// @octocat see #12\n```\n");
}

#[test]
fn indented_code_blocks_are_never_rewritten() {
    assert_untouched("    @octocat #12\n");
}

#[test]
fn code_blocks_inside_list_items_are_never_rewritten() {
    assert_untouched("- item\n\n  ```\n  @octocat #12\n  ```\n");
}

#[test]
fn code_blocks_inside_block_quotes_are_never_rewritten() {
    assert_untouched("> ```\n> @octocat #12\n> ```\n");
}

#[test]
fn existing_link_children_are_never_rewritten() {
    for source in [
        "[@octocat](https://example.com)",
        "[#12](https://example.com)",
        "[owner/repo#3](https://example.com)",
        "[see #12 and @octocat](https://example.com)",
    ] {
        assert_untouched(source);
    }
}

#[test]
fn nested_markup_inside_a_link_is_still_not_rewritten() {
    assert_untouched("[**@octocat** and *#12*](https://example.com)");
}

#[test]
fn link_destinations_are_never_rewritten() {
    assert_untouched("[text](https://github.com/owner/repo#12)");
}

#[test]
fn image_alt_text_is_never_rewritten() {
    assert_untouched("![@octocat #12](image.png)");
}

#[test]
fn a_document_without_shorthand_is_unchanged() {
    assert_untouched("# Title\n\nBody with *emphasis*.\n\n- item\n\n| a |\n| - |\n| b |\n\n---\n");
}

// --- expansion reaches every text-bearing container --------------------------

#[test]
fn shorthand_inside_emphasis_and_strong() {
    assert_eq!(
        inlines("*@octocat* **#12** ~~o/r#3~~"),
        vec![
            Inline::Emphasis(vec![mention("octocat")]),
            text(" "),
            Inline::Strong(vec![issue(12)]),
            text(" "),
            Inline::Strikethrough(vec![cross_repo("o", "r", 3)]),
        ]
    );
}

#[test]
fn shorthand_inside_a_heading() {
    let document = parse_github("## thanks @octocat", &context());
    assert_eq!(
        document.blocks,
        vec![Block::Heading {
            level: 2,
            children: vec![text("thanks "), mention("octocat")],
        }]
    );
}

#[test]
fn shorthand_inside_list_items() {
    let Some(Block::List { items, .. }) =
        parse_github("- fixes #12\n- [x] cc @octocat", &context())
            .blocks
            .into_iter()
            .next()
    else {
        panic!("expected a list");
    };
    assert_eq!(
        items[0].blocks,
        vec![Block::Paragraph(vec![text("fixes "), issue(12)])]
    );
    assert_eq!(items[1].checked, Some(true));
    assert_eq!(
        items[1].blocks,
        vec![Block::Paragraph(vec![text("cc "), mention("octocat")])]
    );
}

#[test]
fn shorthand_inside_a_block_quote() {
    let document = parse_github("> blocked by owner/repo#9\n", &context());
    assert_eq!(
        document.blocks,
        vec![Block::BlockQuote(vec![Block::Paragraph(vec![
            text("blocked by "),
            cross_repo("owner", "repo", 9),
        ])])]
    );
}

#[test]
fn shorthand_inside_table_cells() {
    let document = parse_github("| @octocat |\n| -------- |\n| #12 |\n", &context());
    assert_eq!(
        document.blocks,
        vec![Block::Table {
            headers: vec![vec![mention("octocat")]],
            rows: vec![vec![vec![issue(12)]]],
        }]
    );
}

#[test]
fn shorthand_survives_soft_breaks() {
    assert_eq!(
        inlines("@octocat\n#12"),
        vec![mention("octocat"), Inline::SoftBreak, issue(12)]
    );
}

// --- unicode ----------------------------------------------------------------

#[test]
fn non_ascii_word_characters_block_a_mention() {
    assert_untouched("日本語@octocat");
    assert_untouched("café@example.com");
}

#[test]
fn non_word_characters_do_not_block_a_mention() {
    assert_eq!(
        inlines("🎉 @octocat 🎉"),
        vec![text("🎉 "), mention("octocat"), text(" 🎉")]
    );
}

#[test]
fn non_ascii_text_around_shorthand_is_preserved() {
    assert_eq!(
        inlines("héllo @octocat — voilà #12"),
        vec![
            text("héllo "),
            mention("octocat"),
            text(" — voilà "),
            issue(12),
        ]
    );
}

#[test]
fn a_mention_may_not_run_into_non_ascii_letters() {
    assert_untouched("@octocaté");
}

// --- combinations -----------------------------------------------------------

#[test]
fn every_form_in_a_single_paragraph() {
    assert_eq!(
        inlines("@octocat fixed #12 which mirrors rust-lang/rust#345."),
        vec![
            mention("octocat"),
            text(" fixed "),
            issue(12),
            text(" which mirrors "),
            cross_repo("rust-lang", "rust", 345),
            text("."),
        ]
    );
}

#[test]
fn shorthand_next_to_untouchable_neighbours() {
    assert_eq!(
        inlines("`#1` then #2 then [#3](u) then #4"),
        vec![
            code("#1"),
            text(" then "),
            issue(2),
            text(" then "),
            Inline::Link {
                dest: "u".into(),
                children: vec![text("#3")],
            },
            text(" then "),
            issue(4),
        ]
    );
}

#[test]
fn mention_followed_by_a_repository_path_only_links_the_login() {
    assert_eq!(
        inlines("@octocat/repo#1"),
        vec![mention("octocat"), text("/repo#1")]
    );
}

#[test]
fn expansion_is_stable_under_repeated_parsing() {
    let source = "cc @octocat about #12 and owner/repo#3";
    assert_eq!(
        parse_github(source, &context()),
        parse_github(source, &context())
    );
}
