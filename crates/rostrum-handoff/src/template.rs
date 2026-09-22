//! Turning the configured command template into a line of shell.
//!
//! The template is **shell-interpreted, not split on whitespace**. The
//! configuration example is
//!
//! ```text
//! claude 'Resolve the conflicts described in {context}'
//! ```
//!
//! and splitting that on spaces would hand the harness five arguments instead
//! of one quoted prompt. So the template is treated as text the user's own
//! shell will read, and the only thing this module does to it is replace the
//! two placeholders with a path each, quoted so the shell reads the path as a
//! single word whatever it contains.
//!
//! Every other `{…}` is left exactly as written: `{a,b}` is legitimate brace
//! expansion, and a template author who typed it meant it.

use std::path::Path;

use crate::error::HandoffError;

/// The placeholder for the bundle path. Its absence is an error, because a
/// command that never reads the bundle cannot do the job.
pub const CONTEXT_PLACEHOLDER: &str = "{context}";

/// The placeholder for the worktree path. Optional: the session already starts
/// in the worktree, so most templates need not mention it.
pub const WORKTREE_PLACEHOLDER: &str = "{worktree}";

/// Substitute `{context}` and `{worktree}` into `template`, shell-quoted.
///
/// Fails with [`HandoffError::TemplateLacksContext`] when the template never
/// mentions `{context}`, and with [`HandoffError::PathNotUtf8`] when a path
/// cannot be spelled as text.
pub fn substitute(template: &str, context: &Path, worktree: &Path) -> Result<String, HandoffError> {
    if !template.contains(CONTEXT_PLACEHOLDER) {
        return Err(HandoffError::TemplateLacksContext {
            template: template.to_string(),
        });
    }
    let context = quoted_path(context)?;
    let worktree = quoted_path(worktree)?;
    Ok(template
        .replace(CONTEXT_PLACEHOLDER, &context)
        .replace(WORKTREE_PLACEHOLDER, &worktree))
}

fn quoted_path(path: &Path) -> Result<String, HandoffError> {
    let text = path.to_str().ok_or_else(|| HandoffError::PathNotUtf8 {
        path: path.to_path_buf(),
    })?;
    Ok(shell_quote(text))
}

/// Quote `text` so a POSIX shell reads it as exactly one word.
///
/// Single quotes are the only quoting in which *nothing* is special, so the
/// whole value goes inside a pair of them, and each embedded `'` is spelled
/// `'\''`: close the quote, a backslash-escaped quote, reopen the quote. An
/// empty string becomes `''`, which is still one (empty) word.
pub fn shell_quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('\'');
    for ch in text.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = "claude 'Resolve the conflicts described in {context}'";

    #[test]
    fn the_example_template_gets_a_quoted_path_inside_its_own_quotes() {
        let line = substitute(
            EXAMPLE,
            Path::new("/home/u/.cache/rostrum/handoff/s.md"),
            Path::new("/home/u/src/repo"),
        )
        .expect("substitutes");
        // The user's own quoting is left alone; ours closes and reopens it,
        // which the shell concatenates back into one word.
        assert_eq!(
            line,
            "claude 'Resolve the conflicts described in '/home/u/.cache/rostrum/handoff/s.md''"
        );
    }

    #[test]
    fn a_path_with_a_space_stays_one_word() {
        let line = substitute(
            "tool {context}",
            Path::new("/home/u/My Cache/s.md"),
            Path::new("/w"),
        )
        .expect("substitutes");
        assert_eq!(line, "tool '/home/u/My Cache/s.md'");
    }

    #[test]
    fn a_path_containing_a_single_quote_is_escaped_the_posix_way() {
        let line = substitute(
            "tool {context}",
            Path::new("/home/o'brien/s.md"),
            Path::new("/w"),
        )
        .expect("substitutes");
        assert_eq!(line, "tool '/home/o'\\''brien/s.md'");
    }

    #[test]
    fn a_template_without_the_context_placeholder_is_refused() {
        let err = substitute("claude 'fix it'", Path::new("/c.md"), Path::new("/w"))
            .expect_err("refused");
        assert!(
            matches!(err, HandoffError::TemplateLacksContext { ref template } if template == "claude 'fix it'"),
            "{err:?}"
        );
    }

    #[test]
    fn both_placeholders_are_replaced_every_time_they_appear() {
        let line = substitute(
            "a {context} {worktree} b {worktree} {context}",
            Path::new("/c.md"),
            Path::new("/w"),
        )
        .expect("substitutes");
        assert_eq!(line, "a '/c.md' '/w' b '/w' '/c.md'");
    }

    /// `{a,b}` is brace expansion and `{foo}` is whatever the author meant;
    /// neither is ours to touch.
    #[test]
    fn other_braces_survive_untouched() {
        let line = substitute(
            "cp {context} {foo} /tmp/{a,b}",
            Path::new("/c.md"),
            Path::new("/w"),
        )
        .expect("substitutes");
        assert_eq!(line, "cp '/c.md' {foo} /tmp/{a,b}");
    }

    #[test]
    fn quoting_the_empty_string_gives_an_empty_word() {
        assert_eq!(shell_quote(""), "''");
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_is_refused() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        let bad = Path::new(OsStr::from_bytes(b"/c/\xff.md"));
        let err = substitute("tool {context}", bad, Path::new("/w")).expect_err("refused");
        assert!(matches!(err, HandoffError::PathNotUtf8 { .. }), "{err:?}");
    }
}
