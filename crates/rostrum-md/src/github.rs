//! Expansion of GitHub shorthand into links.
//!
//! Runs as a post-pass over an already-parsed [`Document`], rewriting only
//! [`Inline::Text`] nodes. Three forms are recognised:
//!
//! | shorthand         | target                                             |
//! |-------------------|----------------------------------------------------|
//! | `@login`          | `https://github.com/login`                          |
//! | `#123`            | `https://github.com/{owner}/{repo}/issues/123`      |
//! | `owner/repo#123`  | `https://github.com/owner/repo/issues/123`          |
//!
//! The pass deliberately never descends into code spans, code blocks, image
//! alt text, or the children of an existing link, so it can neither corrupt
//! quoted source nor nest a link inside a link.

use crate::ast::{Block, Document, GitHubContext, Inline};

/// Longest permitted GitHub login.
const MAX_LOGIN_LEN: usize = 39;
/// Longest permitted repository name.
const MAX_REPO_LEN: usize = 100;
/// Longest permitted issue number, in digits.
const MAX_NUMBER_LEN: usize = 12;

/// Rewrites GitHub shorthand in `document` in place.
pub(crate) fn expand(document: &mut Document, context: &GitHubContext) {
    for block in &mut document.blocks {
        expand_block(block, context);
    }
}

fn expand_block(block: &mut Block, context: &GitHubContext) {
    match block {
        Block::Paragraph(children) | Block::Heading { children, .. } => {
            expand_inlines(children, context);
        }
        Block::BlockQuote(blocks) => {
            for block in blocks {
                expand_block(block, context);
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for block in &mut item.blocks {
                    expand_block(block, context);
                }
            }
        }
        Block::Table { headers, rows } => {
            for cell in headers {
                expand_inlines(cell, context);
            }
            for row in rows {
                for cell in row {
                    expand_inlines(cell, context);
                }
            }
        }
        // Verbatim by construction.
        Block::CodeBlock { .. } | Block::Rule => {}
    }
}

fn expand_inlines(children: &mut Vec<Inline>, context: &GitHubContext) {
    let taken = std::mem::take(children);
    let mut out = Vec::with_capacity(taken.len());
    for inline in taken {
        match inline {
            Inline::Text(text) => expand_text(&text, context, &mut out),
            Inline::Emphasis(mut nested) => {
                expand_inlines(&mut nested, context);
                out.push(Inline::Emphasis(nested));
            }
            Inline::Strong(mut nested) => {
                expand_inlines(&mut nested, context);
                out.push(Inline::Strong(nested));
            }
            Inline::Strikethrough(mut nested) => {
                expand_inlines(&mut nested, context);
                out.push(Inline::Strikethrough(nested));
            }
            // Code spans, existing links, images and breaks are left alone.
            other => out.push(other),
        }
    }
    *children = out;
}

/// Splits `text` into literal runs and expanded links.
fn expand_text(text: &str, context: &GitHubContext, out: &mut Vec<Inline>) {
    let mut plain_start = 0usize;
    let mut cursor = 0usize;
    let mut previous: Option<char> = None;
    let mut matched = false;

    while cursor < text.len() {
        let rest = &text[cursor..];
        let Some(current) = rest.chars().next() else {
            break;
        };

        if starts_at_boundary(previous)
            && let Some((len, link)) = try_match(rest, context)
        {
            if plain_start < cursor {
                push_text(out, &text[plain_start..cursor]);
            }
            out.push(link);
            matched = true;
            previous = text[cursor..cursor + len].chars().next_back();
            cursor += len;
            plain_start = cursor;
            continue;
        }

        previous = Some(current);
        cursor += current.len_utf8();
    }

    if !matched {
        push_text(out, text);
        return;
    }
    if plain_start < text.len() {
        push_text(out, &text[plain_start..]);
    }
}

fn push_text(out: &mut Vec<Inline>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text(previous)) = out.last_mut() {
        previous.push_str(text);
    } else {
        out.push(Inline::Text(text.to_owned()));
    }
}

/// Whether a shorthand may begin after `previous`.
///
/// Rejecting word characters keeps `foo@bar.com` from becoming a mention;
/// rejecting `/` and `.` keeps `https://github.com/o/r#1` and `v1.2#3` intact.
fn starts_at_boundary(previous: Option<char>) -> bool {
    match previous {
        None => true,
        Some(character) => {
            !character.is_alphanumeric() && !matches!(character, '_' | '-' | '/' | '.' | '@' | '#')
        }
    }
}

/// Attempts one shorthand at the start of `rest`.
///
/// Returns the number of bytes consumed and the link replacing them.
fn try_match(rest: &str, context: &GitHubContext) -> Option<(usize, Inline)> {
    let first = rest.as_bytes().first()?;
    match first {
        b'@' => {
            let login = scan_login(rest.get(1..)?)?;
            let len = 1 + login.len();
            check_terminator(rest.get(len..)?)?;
            Some((
                len,
                link(rest.get(..len)?, format!("https://github.com/{login}")),
            ))
        }
        b'#' => {
            let number = scan_number(rest.get(1..)?)?;
            let len = 1 + number.len();
            check_terminator(rest.get(len..)?)?;
            Some((
                len,
                link(
                    rest.get(..len)?,
                    issue_url(&context.owner, &context.repo, number),
                ),
            ))
        }
        byte if byte.is_ascii_alphanumeric() => {
            let owner = scan_login(rest)?;
            let after_owner = rest.get(owner.len()..)?.strip_prefix('/')?;
            let repo = scan_repo(after_owner)?;
            let after_repo = after_owner.get(repo.len()..)?.strip_prefix('#')?;
            let number = scan_number(after_repo)?;
            let len = owner.len() + 1 + repo.len() + 1 + number.len();
            check_terminator(rest.get(len..)?)?;
            Some((len, link(rest.get(..len)?, issue_url(owner, repo, number))))
        }
        _ => None,
    }
}

fn link(label: &str, dest: String) -> Inline {
    Inline::Link {
        dest,
        children: vec![Inline::Text(label.to_owned())],
    }
}

fn issue_url(owner: &str, repo: &str, number: &str) -> String {
    format!("https://github.com/{owner}/{repo}/issues/{number}")
}

/// A shorthand must not run straight into a word character.
fn check_terminator(after: &str) -> Option<()> {
    match after.chars().next() {
        None => Some(()),
        Some(character) if character.is_alphanumeric() || character == '_' => None,
        Some(_) => Some(()),
    }
}

/// Longest valid GitHub login at the start of `text`.
///
/// Trailing hyphens are handed back to the surrounding prose so that `@octo-`
/// still yields the `@octo` mention.
fn scan_login(text: &str) -> Option<&str> {
    let end = text
        .bytes()
        .position(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-'))
        .unwrap_or(text.len());
    let candidate = text.get(..end)?.trim_end_matches('-');
    if candidate.is_empty() || candidate.len() > MAX_LOGIN_LEN || candidate.starts_with('-') {
        return None;
    }
    Some(candidate)
}

/// Longest valid repository name at the start of `text`.
fn scan_repo(text: &str) -> Option<&str> {
    let end = text
        .bytes()
        .position(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
        .unwrap_or(text.len());
    let candidate = text.get(..end)?;
    if candidate.is_empty()
        || candidate.len() > MAX_REPO_LEN
        || candidate.bytes().all(|byte| byte == b'.')
    {
        return None;
    }
    Some(candidate)
}

/// Longest valid issue number at the start of `text`.
fn scan_number(text: &str) -> Option<&str> {
    let end = text
        .bytes()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(text.len());
    let candidate = text.get(..end)?;
    if candidate.is_empty() || candidate.len() > MAX_NUMBER_LEN || candidate.starts_with('0') {
        return None;
    }
    Some(candidate)
}
