//! Rendering the bundle: one Markdown file with everything a handler needs.
//!
//! The order is most actionable first. A harness reads top-down and a human
//! skims, and both want the commands and the marker orientation before the
//! commit history and long before the pull request body. Everything is
//! derived from a [`ConflictContext`] rostrum-git already gathered and the
//! pull request metadata the feed already holds, so rendering is a pure
//! function and every case is a literal in a test.

use std::path::Path;

use rostrum_core::{PrNumber, RepoId};
use rostrum_git::{
    CommitList, CommitSummary, ConflictBody, ConflictContext, ConflictedPath, Side,
    StoppedOperation,
};

/// The pull request a conflict belongs to, as much of it as the bundle shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrMeta {
    pub repo: RepoId,
    pub number: PrNumber,
    pub title: String,
    pub url: String,
    pub body: String,
    pub head_ref: String,
    pub base_ref: String,
}

/// Everything [`render_bundle`] reads.
#[derive(Clone, Copy, Debug)]
pub struct Handoff<'a> {
    pub pr: &'a PrMeta,
    pub context: &'a ConflictContext,
    pub worktree: &'a Path,
    /// The opening paragraph under "What to do"; [`DEFAULT_INSTRUCTIONS`]
    /// unless the caller has better ones.
    pub instructions: &'a str,
}

/// The standing instructions for a handler, written for a harness that will
/// follow them literally and a human who will skim them.
pub const DEFAULT_INSTRUCTIONS: &str = "A git rebase or merge in this worktree has stopped on conflicts. \
Resolve only the files listed under \"Conflicted paths\" by editing them in place, keeping the intent of \
both sides as described by the commit messages and pull request below; remove every `<<<<<<<`, `=======`, \
`|||||||` and `>>>>>>>` marker. Mark each file resolved with the command under \"Mark resolved\" (or \
\"Mark removed\" for a file one side deleted), then run the \"Continue\" command; if it stops again on a \
later commit, repeat. Do not run the abort command, `git stash pop`, `git push`, `git reset`, or \
`git checkout --theirs/--ours` wholesale, and do not touch files that are not listed. If a conflict cannot \
be resolved without guessing at intent, or a resolution would need code changes beyond the conflicted \
lines, stop before continuing and report what is unresolved and why.";

/// The pull request body is shown for intent, not in full; past this many
/// bytes it is cut on a character boundary and marked.
pub const MAX_PR_BODY_BYTES: usize = 4096;

/// Render the bundle.
pub fn render_bundle(h: &Handoff) -> String {
    let ctx = h.context;
    let mut out = String::new();

    // 1. Title.
    out.push_str(&format!(
        "# Conflict handoff: {}{} — {}\n\n",
        h.pr.repo, h.pr.number, h.pr.title
    ));

    // 2. What to do.
    let commands = ctx.commands();
    out.push_str("## What to do\n\n");
    out.push_str(h.instructions);
    out.push_str("\n\n");
    out.push_str(&format!("- Worktree: `{}`\n", h.worktree.display()));
    out.push_str(&format!("- Operation: {}\n", describe_operation(ctx)));
    out.push_str(&format!("- Mark resolved: `{}`\n", commands.mark_resolved));
    out.push_str(&format!("- Mark removed: `{}`\n", commands.mark_removed));
    out.push_str(&format!("- Continue: `{}`\n", commands.continue_));
    out.push_str(&format!("- Abort (do not run): `{}`\n\n", commands.abort));

    // 3. Marker orientation.
    let sides = ctx.marker_sides();
    out.push_str("## How to read the markers\n\n");
    out.push_str(&format!(
        "`<<<<<<< HEAD` … `=======` is {}; `=======` … `>>>>>>>` is {}. \
         A `|||||||` … `=======` section, if present, is the common ancestor of both.\n\n",
        describe_side(sides.head_is, ctx),
        describe_side(sides.incoming_is, ctx),
    ));
    out.push_str(
        "If git is holding an autostash it will restore it after `--continue`; \
         never run `git stash pop` yourself.\n\n",
    );

    // 4. Conflicted paths.
    out.push_str(&format!("## Conflicted paths ({})\n\n", ctx.paths.len()));
    for path in &ctx.paths {
        out.push_str(&format!("- `{}` — {}\n", path.path, path.kind.describe()));
    }
    out.push('\n');

    // 5. The commit being replayed, when there is one.
    if let StoppedOperation::Rebase {
        applying: Some(commit),
        ..
    } = &ctx.operation
    {
        out.push_str("## Commit being applied\n\n");
        out.push_str(&format!("- sha: `{}`\n", commit.oid.as_str()));
        out.push_str(&format!("- author: {}\n", commit.author));
        out.push_str(&format!("- date: {}\n\n", commit.date));
        push_fenced(&mut out, &commit.message);
    }

    // 6. Conflict regions.
    out.push_str("## Conflict regions\n\n");
    for path in &ctx.paths {
        render_regions(&mut out, path, ctx.caps.max_region_bytes);
    }

    // 7. Commits each side has that the other lacks.
    let branch = ctx.branch.as_str();
    let target = ctx.target.as_str();
    render_commits(&mut out, &ctx.branch_commits, branch, target);
    render_commits(&mut out, &ctx.target_commits, target, branch);

    // 8. Pull request.
    out.push_str("## Pull request\n\n");
    out.push_str(&format!("- number: {}\n", h.pr.number));
    out.push_str(&format!("- url: {}\n", h.pr.url));
    out.push_str(&format!(
        "- branches: `{}` ← `{}`\n\n",
        h.pr.base_ref, h.pr.head_ref
    ));
    let body = h.pr.body.trim();
    if body.is_empty() {
        out.push_str("(no description)\n\n");
    } else {
        let shown = truncate_on_char_boundary(body, MAX_PR_BODY_BYTES);
        out.push_str(shown);
        if shown.len() < body.len() {
            out.push_str("\n\n(truncated)");
        }
        out.push_str("\n\n");
    }

    // 9. git's own account, when it gave one.
    let message = ctx.git_message.trim();
    if !message.is_empty() {
        out.push_str("## git said\n\n");
        push_fenced(&mut out, message);
    }

    out
}

/// "a rebase of `feat-x` onto `refs/remotes/origin/main`, stopped at step 3
/// of 7 while applying `abc12345 subject`" or "a merge of
/// `refs/remotes/origin/main` into `feat-x`".
fn describe_operation(ctx: &ConflictContext) -> String {
    match &ctx.operation {
        StoppedOperation::Rebase { step, applying, .. } => {
            let mut text = format!("a rebase of `{}` onto `{}`", ctx.branch, ctx.target);
            match (step, applying) {
                (Some((n, of)), Some(commit)) => text.push_str(&format!(
                    ", stopped at step {n} of {of} while applying `{} {}`",
                    commit.oid.short(),
                    commit.subject()
                )),
                (Some((n, of)), None) => text.push_str(&format!(", stopped at step {n} of {of}")),
                (None, Some(commit)) => text.push_str(&format!(
                    ", stopped while applying `{} {}`",
                    commit.oid.short(),
                    commit.subject()
                )),
                (None, None) => text.push_str(", stopped on conflicts"),
            }
            text
        }
        StoppedOperation::Merge { merging } => format!(
            "a merge of `{}` (`{}`) into `{}`",
            ctx.target,
            merging.short(),
            ctx.branch
        ),
    }
}

/// Which real ref a marker side is, with the role it plays in this
/// operation. During a rebase HEAD is the *target* — the surprising half,
/// and the reason this section exists.
fn describe_side(side: Side, ctx: &ConflictContext) -> String {
    match (ctx.is_rebase(), side) {
        (true, Side::Target) => format!("`{}` (the base you are rebasing onto)", ctx.target),
        (true, Side::Branch) => format!("the commit from `{}` being replayed", ctx.branch),
        (false, Side::Branch) => format!("`{}` (your branch, checked out)", ctx.branch),
        (false, Side::Target) => format!("`{}` (the branch being merged in)", ctx.target),
    }
}

fn render_regions(out: &mut String, path: &ConflictedPath, max_region_bytes: usize) {
    out.push_str(&format!("### {}\n\n", path.path));
    match &path.body {
        ConflictBody::Regions { regions, truncated } => {
            for region in regions {
                let mut numbered = String::new();
                for (offset, line) in region.text.lines().enumerate() {
                    let number = region.first_line + offset as u32;
                    numbered.push_str(&format!("L{number}  {line}\n"));
                }
                push_fenced(out, &numbered);
            }
            if *truncated {
                out.push_str(&format!(
                    "(truncated at {max_region_bytes} bytes; open the file for the rest)\n\n"
                ));
            }
        }
        ConflictBody::Binary => out.push_str("Binary file; open it in the worktree.\n\n"),
        ConflictBody::Absent { reason } => out.push_str(&format!("Not shown: {reason}\n\n")),
        ConflictBody::Omitted => out.push_str(
            "Not shown: the bundle's size cap was reached before this file; open it in the worktree.\n\n",
        ),
    }
}

fn render_commits(out: &mut String, list: &CommitList, on: &str, not_on: &str) {
    out.push_str(&format!(
        "## Commits on `{on}` not on `{not_on}` ({} of {} shown)\n\n",
        list.commits.len(),
        list.total
    ));
    if list.commits.is_empty() {
        out.push_str("(none)\n");
    }
    for commit in &list.commits {
        out.push_str(&format_commit_line(commit));
    }
    if list.truncated() {
        let more = list.total.saturating_sub(list.commits.len() as u32);
        out.push_str(&format!("({more} more; run `git log {not_on}..{on}`)\n"));
    }
    out.push('\n');
}

fn format_commit_line(commit: &CommitSummary) -> String {
    format!(
        "- {} {} — {}, {}\n",
        commit.oid.short(),
        commit.subject(),
        commit.author,
        commit.date
    )
}

/// Append `text` inside a code fence longer than any run of backticks in it,
/// so a conflict region that itself contains a Markdown fence cannot end
/// ours early.
fn push_fenced(out: &mut String, text: &str) {
    let fence = fence_for(text);
    out.push_str(&fence);
    out.push('\n');
    out.push_str(text);
    if !text.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&fence);
    out.push_str("\n\n");
}

/// Three backticks, or one more than the longest run inside `text`.
fn fence_for(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    "`".repeat((longest + 1).max(3))
}

/// The longest prefix of `text` that is at most `max` bytes and ends on a
/// character boundary.
fn truncate_on_char_boundary(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use rostrum_git::{
        BranchName, Caps, CommitList, CommitSummary, ConflictBody, ConflictContext, ConflictKind,
        ConflictRegion, ConflictedPath, Oid, StoppedOperation,
    };

    use super::*;

    fn oid(seed: char) -> Oid {
        Oid::parse(std::iter::repeat_n(seed, 40).collect::<String>()).expect("40 hex digits")
    }

    fn commit(seed: char, subject: &str) -> CommitSummary {
        CommitSummary {
            oid: oid(seed),
            author: "Ada".to_string(),
            date: "2026-09-21".to_string(),
            message: format!("{subject}\n\nBody of the message.\n"),
        }
    }

    fn region_path(path: &str) -> ConflictedPath {
        ConflictedPath {
            path: path.to_string(),
            kind: ConflictKind::BothModified,
            body: ConflictBody::Regions {
                regions: vec![ConflictRegion {
                    first_line: 10,
                    last_line: 12,
                    text: "<<<<<<< HEAD\nold\n=======\nnew\n>>>>>>> theirs\n".to_string(),
                }],
                truncated: false,
            },
        }
    }

    fn context(operation: StoppedOperation) -> ConflictContext {
        ConflictContext {
            operation,
            branch: BranchName::new("feat-x").expect("valid"),
            target: "refs/remotes/origin/main".to_string(),
            head: oid('c'),
            paths: vec![region_path("src/a.rs")],
            branch_commits: CommitList {
                commits: vec![commit('b', "feat: x")],
                total: 1,
            },
            target_commits: CommitList {
                commits: vec![commit('e', "refactor: split")],
                total: 1,
            },
            git_message: "CONFLICT (content): Merge conflict in src/a.rs".to_string(),
            caps: Caps::default(),
        }
    }

    fn rebase() -> ConflictContext {
        context(StoppedOperation::Rebase {
            step: Some((3, 7)),
            applying: Some(commit('b', "feat: x")),
            onto: Some(oid('a')),
        })
    }

    fn merge() -> ConflictContext {
        context(StoppedOperation::Merge { merging: oid('a') })
    }

    fn pr() -> PrMeta {
        PrMeta {
            repo: RepoId::new("zed-industries", "zed"),
            number: PrNumber(1234),
            title: "Add x".to_string(),
            url: "https://github.com/zed-industries/zed/pull/1234".to_string(),
            body: "Why x matters.".to_string(),
            head_ref: "feat-x".to_string(),
            base_ref: "main".to_string(),
        }
    }

    fn render(ctx: &ConflictContext) -> String {
        render_with(&pr(), ctx)
    }

    fn render_with(pr: &PrMeta, ctx: &ConflictContext) -> String {
        render_bundle(&Handoff {
            pr,
            context: ctx,
            worktree: Path::new("/home/u/src/zed"),
            instructions: DEFAULT_INSTRUCTIONS,
        })
    }

    /// The `## ` headings, in order of appearance.
    fn sections(text: &str) -> Vec<&str> {
        text.lines()
            .filter(|line| line.starts_with("## "))
            .collect()
    }

    fn offset(text: &str, needle: &str) -> usize {
        text.find(needle)
            .unwrap_or_else(|| panic!("`{needle}` missing from:\n{text}"))
    }

    #[test]
    fn a_rebase_bundle_has_every_section_in_actionable_order() {
        let text = render(&rebase());
        assert!(text.starts_with("# Conflict handoff: zed-industries/zed#1234 — Add x\n"));
        assert_eq!(
            sections(&text),
            vec![
                "## What to do",
                "## How to read the markers",
                "## Conflicted paths (1)",
                "## Commit being applied",
                "## Conflict regions",
                "## Commits on `feat-x` not on `refs/remotes/origin/main` (1 of 1 shown)",
                "## Commits on `refs/remotes/origin/main` not on `feat-x` (1 of 1 shown)",
                "## Pull request",
                "## git said",
            ]
        );
        // Instructions and commands come before anything else.
        assert!(offset(&text, DEFAULT_INSTRUCTIONS) < offset(&text, "- Worktree:"));
        assert!(offset(&text, "- Mark resolved: `") < offset(&text, "## How to read"));
        assert!(text.contains("- Abort (do not run): `"));
        assert!(text.contains("- Worktree: `/home/u/src/zed`"));
    }

    #[test]
    fn a_merge_bundle_omits_the_commit_being_applied() {
        let text = render(&merge());
        assert_eq!(
            sections(&text),
            vec![
                "## What to do",
                "## How to read the markers",
                "## Conflicted paths (1)",
                "## Conflict regions",
                "## Commits on `feat-x` not on `refs/remotes/origin/main` (1 of 1 shown)",
                "## Commits on `refs/remotes/origin/main` not on `feat-x` (1 of 1 shown)",
                "## Pull request",
                "## git said",
            ]
        );
        assert!(text.contains(
            "- Operation: a merge of `refs/remotes/origin/main` (`aaaaaaa`) into `feat-x`"
        ));
    }

    #[test]
    fn the_rebase_operation_sentence_names_step_and_commit() {
        let text = render(&rebase());
        assert!(text.contains(
            "- Operation: a rebase of `feat-x` onto `refs/remotes/origin/main`, \
             stopped at step 3 of 7 while applying `bbbbbbb feat: x`"
        ));
        let text = render(&context(StoppedOperation::Rebase {
            step: None,
            applying: None,
            onto: None,
        }));
        assert!(text.contains(
            "- Operation: a rebase of `feat-x` onto `refs/remotes/origin/main`, stopped on conflicts"
        ));
    }

    /// The whole point of the section: during a rebase HEAD is the *target*.
    #[test]
    fn marker_orientation_names_the_real_refs_and_flips_between_rebase_and_merge() {
        let rebased = render(&rebase());
        assert!(rebased.contains(
            "`<<<<<<< HEAD` … `=======` is `refs/remotes/origin/main` (the base you are rebasing onto); \
             `=======` … `>>>>>>>` is the commit from `feat-x` being replayed."
        ));
        let merged = render(&merge());
        assert!(merged.contains(
            "`<<<<<<< HEAD` … `=======` is `feat-x` (your branch, checked out); \
             `=======` … `>>>>>>>` is `refs/remotes/origin/main` (the branch being merged in)."
        ));
        for text in [&rebased, &merged] {
            assert!(text.contains("never run `git stash pop` yourself"));
        }
    }

    #[test]
    fn conflicted_paths_list_each_kind_and_regions_are_line_numbered_and_fenced() {
        let text = render(&rebase());
        assert!(text.contains("- `src/a.rs` — both modified\n"));
        let regions = &text[offset(&text, "## Conflict regions")..];
        assert!(regions.contains("### src/a.rs\n\n```\nL10  <<<<<<< HEAD\nL11  old\nL12  =======\nL13  new\nL14  >>>>>>> theirs\n```\n"));
    }

    #[test]
    fn the_commit_being_applied_carries_the_full_message() {
        let text = render(&rebase());
        let section =
            &text[offset(&text, "## Commit being applied")..offset(&text, "## Conflict regions")];
        assert!(section.contains(&format!("- sha: `{}`", "b".repeat(40))));
        assert!(section.contains("- author: Ada\n"));
        assert!(section.contains("```\nfeat: x\n\nBody of the message.\n```\n"));
    }

    #[test]
    fn a_truncated_region_says_so_with_the_cap() {
        let mut ctx = rebase();
        let ConflictBody::Regions { truncated, .. } = &mut ctx.paths[0].body else {
            panic!("fixture has regions");
        };
        *truncated = true;
        let text = render(&ctx);
        assert!(text.contains(&format!(
            "(truncated at {} bytes; open the file for the rest)",
            Caps::default().max_region_bytes
        )));
    }

    #[test]
    fn truncated_commit_lists_say_how_many_more_and_which_log_to_run() {
        let mut ctx = rebase();
        ctx.branch_commits.total = 5;
        ctx.target_commits.total = 3;
        let text = render(&ctx);
        assert!(text.contains(
            "## Commits on `feat-x` not on `refs/remotes/origin/main` (1 of 5 shown)\n\n\
             - bbbbbbb feat: x — Ada, 2026-09-21\n\
             (4 more; run `git log refs/remotes/origin/main..feat-x`)\n"
        ));
        assert!(text.contains(
            "## Commits on `refs/remotes/origin/main` not on `feat-x` (1 of 3 shown)\n\n\
             - eeeeeee refactor: split — Ada, 2026-09-21\n\
             (2 more; run `git log feat-x..refs/remotes/origin/main`)\n"
        ));
    }

    #[test]
    fn an_empty_commit_list_renders_none() {
        let mut ctx = rebase();
        ctx.target_commits = CommitList {
            commits: vec![],
            total: 0,
        };
        let text = render(&ctx);
        assert!(text.contains("(0 of 0 shown)\n\n(none)\n"));
    }

    #[test]
    fn a_long_pull_request_body_is_cut_on_a_char_boundary_and_marked() {
        let mut pr = pr();
        // One ASCII byte then two-byte characters, so the cap falls
        // mid-character.
        pr.body = format!("a{}", "é".repeat(MAX_PR_BODY_BYTES));
        let text = render_with(&pr, &rebase());
        let section = &text[offset(&text, "## Pull request")..offset(&text, "## git said")];
        assert!(section.trim_end().ends_with("(truncated)"), "{section}");
        let shown = section
            .split("\n\n")
            .find(|part| part.starts_with("aé"))
            .expect("body present");
        assert!(shown.len() <= MAX_PR_BODY_BYTES);
        assert_eq!(shown.len(), MAX_PR_BODY_BYTES - 1, "4096 is mid-character");

        let short = render_with(&super::tests::pr(), &rebase());
        assert!(!short.contains("(truncated)"));
        assert!(short.contains("- branches: `main` ← `feat-x`\n\nWhy x matters.\n"));
    }

    #[test]
    fn binary_absent_and_omitted_bodies_are_one_liners_without_a_fence() {
        let mut ctx = rebase();
        ctx.paths = vec![
            ConflictedPath {
                path: "logo.png".to_string(),
                kind: ConflictKind::BothAdded,
                body: ConflictBody::Binary,
            },
            ConflictedPath {
                path: "gone.rs".to_string(),
                kind: ConflictKind::DeletedByThem,
                body: ConflictBody::Absent {
                    reason: "deleted on one side".to_string(),
                },
            },
            ConflictedPath {
                path: "big.rs".to_string(),
                kind: ConflictKind::BothModified,
                body: ConflictBody::Omitted,
            },
        ];
        let text = render(&ctx);
        let regions = &text[offset(&text, "## Conflict regions")..offset(&text, "## Commits on")];
        assert!(!regions.contains("```"), "no fence: {regions}");
        assert!(regions.contains("### logo.png\n\nBinary file; open it in the worktree.\n"));
        assert!(regions.contains("### gone.rs\n\nNot shown: deleted on one side\n"));
        assert!(regions.contains("### big.rs\n\nNot shown: the bundle's size cap was reached"));
        assert!(text.contains("## Conflicted paths (3)"));
        assert!(text.contains("- `gone.rs` — deleted by them\n"));
    }

    #[test]
    fn an_empty_git_message_omits_the_last_section() {
        let mut ctx = rebase();
        ctx.git_message = "  \n".to_string();
        let text = render(&ctx);
        assert!(!text.contains("## git said"));
        assert!(text.trim_end().ends_with("Why x matters."));
    }

    #[test]
    fn an_empty_pull_request_body_says_so() {
        let mut pr = pr();
        pr.body = String::new();
        let text = render_with(&pr, &rebase());
        assert!(text.contains("(no description)"));
    }

    /// A region that contains a Markdown fence must not close ours.
    #[test]
    fn the_fence_outgrows_any_backticks_in_the_text() {
        assert_eq!(fence_for("plain"), "```");
        assert_eq!(fence_for("has ``` inside"), "````");
        assert_eq!(fence_for("has ````` inside"), "``````");
    }

    #[test]
    fn truncation_never_splits_a_character() {
        assert_eq!(truncate_on_char_boundary("abc", 5), "abc");
        assert_eq!(truncate_on_char_boundary("abcdef", 3), "abc");
        assert_eq!(truncate_on_char_boundary("aé", 2), "a");
        assert_eq!(truncate_on_char_boundary("aé", 3), "aé");
    }
}
