//! End-to-end checks of the public API against realistic GitHub patches.
//!
//! These exercise `parse_patch` + `DiffLine::anchor` the way the review UI does:
//! walk every line of a file and ask where a comment on it would land. A
//! regression here means review comments on the wrong lines of a real pull
//! request, so the expectations are written out line by line rather than
//! computed.

use rostrum_diff::{
    CommentAnchor, DiffFile, DiffLine, FileStatus, LineKind, PatchAvailability, Side, parse_patch,
};

const PATH: &str = "crates/rostrum-diff/src/parse.rs";

/// `(side, line)` for each line, with `None` for non-commentable lines.
fn anchors(patch: &str) -> Vec<Option<(Side, u32)>> {
    let hunks = match parse_patch(patch) {
        Ok(hunks) => hunks,
        Err(err) => panic!("fixture failed to parse: {err}"),
    };
    hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .map(|l| {
            let anchor = l.anchor(PATH)?;
            assert_eq!(anchor.path, PATH);
            Some((anchor.side, anchor.line))
        })
        .collect()
}

fn lines(patch: &str) -> Vec<DiffLine> {
    match parse_patch(patch) {
        Ok(hunks) => hunks.into_iter().flat_map(|h| h.lines).collect(),
        Err(err) => panic!("fixture failed to parse: {err}"),
    }
}

/// A realistic three-hunk patch: an edit near the top, a pure insertion in the
/// middle, and a pure deletion at the end. The old and new sides drift apart at
/// each hunk, which is exactly the condition under which a naive implementation
/// anchors on the wrong number.
const THREE_HUNK_PATCH: &str = concat!(
    "@@ -1,5 +1,6 @@\n",
    " //! Module docs.\n",
    "\n",
    "-use std::collections::HashMap;\n",
    "+use std::collections::BTreeMap;\n",
    "+use std::fmt;\n",
    "\n",
    " pub struct Thing {\n",
    "@@ -40,4 +41,7 @@ impl Thing {\n",
    "     pub fn new() -> Self {\n",
    "+        // three new lines\n",
    "+        // with no removals\n",
    "+        // at all\n",
    "         Self::default()\n",
    "     }\n",
    " }\n",
    "@@ -80,7 +84,4 @@ mod tests {\n",
    "     #[test]\n",
    "     fn it_works() {\n",
    "-        let a = 1;\n",
    "-        let b = 2;\n",
    "-        assert_eq!(a + b, 3);\n",
    "     }\n",
    " }\n",
);

#[test]
fn every_line_of_a_three_hunk_patch_anchors_where_github_expects() {
    assert_eq!(
        anchors(THREE_HUNK_PATCH),
        vec![
            // -- hunk 1: old starts at 1, new at 1 ---------------------------
            Some((Side::Right, 1)), // " //! Module docs."   old 1 / new 1
            Some((Side::Right, 2)), // "" (blank context)    old 2 / new 2
            Some((Side::Left, 3)),  // "-use ...HashMap;"    old 3
            Some((Side::Right, 3)), // "+use ...BTreeMap;"   new 3
            Some((Side::Right, 4)), // "+use std::fmt;"      new 4
            Some((Side::Right, 5)), // "" (blank context)    old 4 / new 5
            Some((Side::Right, 6)), // " pub struct Thing {" old 5 / new 6
            // -- hunk 2: old starts at 40, new at 41 -------------------------
            Some((Side::Right, 41)), // " pub fn new..."      old 40 / new 41
            Some((Side::Right, 42)), // "+ // three..."       new 42
            Some((Side::Right, 43)), // "+ // with no..."     new 43
            Some((Side::Right, 44)), // "+ // at all"         new 44
            Some((Side::Right, 45)), // " Self::default()"    old 41 / new 45
            Some((Side::Right, 46)), // " }"                  old 42 / new 46
            Some((Side::Right, 47)), // " }"                  old 43 / new 47
            // -- hunk 3: old starts at 80, new at 84 -------------------------
            Some((Side::Right, 84)), // " #[test]"            old 80 / new 84
            Some((Side::Right, 85)), // " fn it_works() {"    old 81 / new 85
            Some((Side::Left, 82)),  // "- let a = 1;"        old 82
            Some((Side::Left, 83)),  // "- let b = 2;"        old 83
            Some((Side::Left, 84)),  // "- assert_eq!..."     old 84
            Some((Side::Right, 86)), // " }"                  old 85 / new 86
            Some((Side::Right, 87)), // " }"                  old 86 / new 87
        ]
    );
}

#[test]
fn removed_and_context_lines_can_share_a_line_number_across_sides() {
    // In hunk 3 above, old line 84 is a deletion and new line 84 is context.
    // They are different lines of different files; only `side` separates them.
    let all = lines(THREE_HUNK_PATCH);
    let left_84 = all
        .iter()
        .find(|l| l.anchor(PATH) == Some(anchor(84, Side::Left)))
        .expect("a LEFT anchor at 84 exists");
    let right_84 = all
        .iter()
        .find(|l| l.anchor(PATH) == Some(anchor(84, Side::Right)))
        .expect("a RIGHT anchor at 84 exists");

    assert_eq!(left_84.kind, LineKind::Removed);
    assert_eq!(left_84.content, "        assert_eq!(a + b, 3);");
    assert_eq!(right_84.kind, LineKind::Context);
    assert_eq!(right_84.content, "    #[test]");
}

fn anchor(line: u32, side: Side) -> CommentAnchor {
    CommentAnchor {
        path: PATH.into(),
        line,
        side,
    }
}

#[test]
fn a_newly_added_file_is_all_right_side_anchors() {
    let patch = concat!(
        "@@ -0,0 +1,5 @@\n",
        "+fn main() {\n",
        "+    println!(\"hello\");\n",
        "+}\n",
        "+\n",
        "+// trailing\n",
    );
    assert_eq!(
        anchors(patch),
        vec![
            Some((Side::Right, 1)),
            Some((Side::Right, 2)),
            Some((Side::Right, 3)),
            Some((Side::Right, 4)),
            Some((Side::Right, 5)),
        ]
    );
}

#[test]
fn a_deleted_file_is_all_left_side_anchors() {
    let patch = "@@ -1,3 +0,0 @@\n-a\n-b\n-c\n";
    assert_eq!(
        anchors(patch),
        vec![
            Some((Side::Left, 1)),
            Some((Side::Left, 2)),
            Some((Side::Left, 3)),
        ]
    );
}

#[test]
fn a_rename_with_no_content_change_has_no_hunks() {
    // GitHub sends `status: renamed` with no `patch` when only the path moved.
    let file = DiffFile {
        path: "crates/b/src/lib.rs".into(),
        previous_path: Some("crates/a/src/lib.rs".into()),
        status: FileStatus::from_api("renamed"),
        additions: 0,
        deletions: 0,
        hunks: match parse_patch("") {
            Ok(hunks) => hunks,
            Err(err) => panic!("empty patch must parse: {err}"),
        },
        availability: PatchAvailability::Omitted,
    };
    assert_eq!(file.status, FileStatus::Renamed);
    assert!(file.hunks.is_empty());
    assert_eq!(file.lines().count(), 0);
}

#[test]
fn a_binary_file_has_no_patch_and_no_lines() {
    let file = DiffFile {
        path: "assets/logo.png".into(),
        previous_path: None,
        status: FileStatus::from_api("modified"),
        additions: 0,
        deletions: 0,
        hunks: Vec::new(),
        availability: PatchAvailability::Omitted,
    };
    assert_eq!(file.availability, PatchAvailability::Omitted);
    assert_eq!(file.lines().count(), 0);
}

#[test]
fn a_single_line_change_with_omitted_counts_anchors_on_both_sides() {
    let patch = "@@ -7 +7 @@ fn f() {\n-    let x = 1;\n+    let x = 2;\n";
    assert_eq!(
        anchors(patch),
        vec![Some((Side::Left, 7)), Some((Side::Right, 7))]
    );
}

#[test]
fn a_no_newline_at_eof_change_anchors_normally() {
    let patch = concat!(
        "@@ -1,3 +1,3 @@\n",
        " first\n",
        " second\n",
        "-third\n",
        "\\ No newline at end of file\n",
        "+third\n",
        "\\ No newline at end of file\n",
    );
    let all = lines(patch);
    assert_eq!(all.len(), 4, "markers must not become lines");
    assert!(all[2].no_newline_at_eof);
    assert!(all[3].no_newline_at_eof);
    assert_eq!(
        anchors(patch),
        vec![
            Some((Side::Right, 1)),
            Some((Side::Right, 2)),
            Some((Side::Left, 3)),
            Some((Side::Right, 3)),
        ]
    );
}

#[test]
fn every_line_is_commentable_in_a_well_formed_patch() {
    // The UI disables commenting on a line whose anchor is None; that should
    // only ever happen for degenerate patches, never for a real one.
    for line in lines(THREE_HUNK_PATCH) {
        assert!(
            line.is_commentable(),
            "line {line:?} should be commentable in a well-formed patch"
        );
    }
}

#[test]
fn anchors_never_use_line_zero() {
    for patch in [
        THREE_HUNK_PATCH,
        "@@ -0,0 +1,2 @@\n+a\n+b\n",
        "@@ -1,2 +0,0 @@\n-a\n-b\n",
    ] {
        for anchor in anchors(patch).into_iter().flatten() {
            assert!(anchor.1 >= 1, "line numbers are 1-based, got {anchor:?}");
        }
    }
}

#[test]
fn the_old_and_new_sides_each_form_a_gapless_run() {
    // Within a hunk the old-side numbers must be 1,2,3,... from old_start with
    // no gaps or repeats, and likewise for the new side. A gap would mean the
    // walk skipped a line and everything after it anchors one off.
    let hunks = match parse_patch(THREE_HUNK_PATCH) {
        Ok(hunks) => hunks,
        Err(err) => panic!("fixture failed to parse: {err}"),
    };
    for hunk in &hunks {
        let old: Vec<u32> = hunk.lines.iter().filter_map(|l| l.old_line).collect();
        let new: Vec<u32> = hunk.lines.iter().filter_map(|l| l.new_line).collect();

        let expected_old: Vec<u32> = (hunk.old_start..hunk.old_start + hunk.old_count).collect();
        let expected_new: Vec<u32> = (hunk.new_start..hunk.new_start + hunk.new_count).collect();

        assert_eq!(old, expected_old, "old side of {}", hunk.header);
        assert_eq!(new, expected_new, "new side of {}", hunk.header);
    }
}
