//! Finding conflict blocks in a working-tree file.
//!
//! Split from [`crate::context`] only for size: the types it fills are defined
//! there, and it is re-exported from there, so nothing outside the crate can
//! tell.

use super::{Caps, ConflictBody, ConflictRegion};

/// The four marker lines git writes, recognised at the start of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    /// `<<<<<<< <label>`
    Open,
    /// `||||||| <label>` — the merged base, under `diff3` and `zdiff3`.
    Base,
    /// `=======`
    Separator,
    /// `>>>>>>> <label>`
    Close,
}

fn marker(line: &str) -> Option<Marker> {
    let labelled = |prefix: &str| {
        line.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    };
    if labelled("<<<<<<<") {
        Some(Marker::Open)
    } else if labelled("|||||||") {
        Some(Marker::Base)
    } else if line == "=======" {
        Some(Marker::Separator)
    } else if labelled(">>>>>>>") {
        Some(Marker::Close)
    } else {
        None
    }
}

/// Find every conflict block in a file and return it with its context.
///
/// A block runs from a `<<<<<<< ` line to the next `>>>>>>> ` line; a
/// `||||||| ` base section and the `=======` separator are simply inside it.
/// An opening marker with no closing one is not a block — it is prose that
/// happens to start with seven angle brackets — and neither is anything after
/// it.
///
/// Each block is widened by `context_lines` on both sides and overlapping or
/// adjacent windows are merged, so two nearby blocks become one region rather
/// than two that repeat the same lines. Line numbers are 1-based and
/// inclusive, matching an editor.
///
/// `max_bytes` bounds the total text. The cut falls on a line boundary, the
/// region that was being written keeps the lines it got (its `last_line` says
/// so), every later region is dropped, and the flag is set.
pub fn extract_conflict_regions(
    text: &str,
    context_lines: u32,
    max_bytes: usize,
) -> (Vec<ConflictRegion>, bool) {
    let lines = split_lines(text);
    let count = lines.len();

    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut index = 0;
    while index < count {
        if marker(lines[index]) == Some(Marker::Open) {
            let Some(close) = (index + 1..count).find(|&j| marker(lines[j]) == Some(Marker::Close))
            else {
                break;
            };
            blocks.push((index, close));
            index = close + 1;
        } else {
            index += 1;
        }
    }

    let context = context_lines as usize;
    let mut windows: Vec<(usize, usize)> = Vec::new();
    for (open, close) in blocks {
        let first = open.saturating_sub(context);
        let last = (close + context).min(count - 1);
        match windows.last_mut() {
            Some(window) if first <= window.1 + 1 => window.1 = window.1.max(last),
            _ => windows.push((first, last)),
        }
    }

    let mut regions = Vec::new();
    let mut used = 0usize;
    let mut truncated = false;
    for (first, last) in windows {
        let mut text = String::new();
        let mut included = 0usize;
        for line in &lines[first..=last] {
            // Each line costs its bytes plus the newline that separates it.
            let cost = line.len() + 1;
            if used + cost > max_bytes {
                truncated = true;
                break;
            }
            if included > 0 {
                text.push('\n');
            }
            text.push_str(line);
            used += cost;
            included += 1;
        }
        if included > 0 {
            regions.push(ConflictRegion {
                first_line: (first + 1) as u32,
                last_line: (first + included) as u32,
                text,
            });
        }
        if truncated {
            break;
        }
    }

    (regions, truncated)
}

/// Split into lines the way an editor numbers them: a trailing newline does
/// not start an empty last line, and a CR before the LF belongs to the line
/// ending, not the line.
fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// Running totals across the paths of one context, so the per-context caps
/// in [`Caps`] can be applied path by path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegionBudget {
    pub files: usize,
    pub bytes: usize,
}

/// Decide what to show for a working-tree file that git wrote markers into.
///
/// The order of the checks is the order of the guarantees: a file past the
/// count or byte cap is [`ConflictBody::Omitted`] before it is inspected at
/// all; a NUL in the first 8 KiB is [`ConflictBody::Binary`], which is the
/// heuristic git itself uses; everything else is decoded lossily and searched
/// for markers with whatever budget remains.
pub fn body_from_file(bytes: &[u8], caps: &Caps, budget: &mut RegionBudget) -> ConflictBody {
    if budget.files >= caps.max_files_with_regions || budget.bytes >= caps.max_total_region_bytes {
        return ConflictBody::Omitted;
    }
    let probe = &bytes[..bytes.len().min(8 * 1024)];
    if probe.contains(&0) {
        return ConflictBody::Binary;
    }
    let text = String::from_utf8_lossy(bytes);
    let remaining = caps.max_total_region_bytes - budget.bytes;
    let per_file = caps.max_region_bytes.min(remaining);
    let (regions, truncated) = extract_conflict_regions(&text, caps.context_lines, per_file);
    budget.files += 1;
    budget.bytes += regions
        .iter()
        .map(|region| region.text.len() + 1)
        .sum::<usize>();
    // A cut made by the *total* cap means the next file could only get the
    // odd line that did not fit here, which is worse than naming it.
    if truncated && per_file == remaining {
        budget.bytes = caps.max_total_region_bytes;
    }
    ConflictBody::Regions { regions, truncated }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_BLOCK: &str = "\
line 1
line 2
line 3
line 4
<<<<<<< HEAD
ours
=======
theirs
>>>>>>> 1234567 (their commit)
line 10
line 11
line 12
line 13
";

    #[test]
    fn one_block_is_widened_by_its_context_on_both_sides() {
        let (regions, truncated) = extract_conflict_regions(ONE_BLOCK, 3, usize::MAX);
        assert!(!truncated);
        assert_eq!(regions.len(), 1);
        assert_eq!((regions[0].first_line, regions[0].last_line), (2, 12));
        assert_eq!(
            regions[0].text,
            "line 2\nline 3\nline 4\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> 1234567 (their commit)\nline 10\nline 11\nline 12"
        );
    }

    /// Two blocks whose context windows touch would otherwise repeat the same
    /// lines twice, once as trailing and once as leading context.
    #[test]
    fn nearby_blocks_merge_into_one_region() {
        let text = "\
a
<<<<<<< HEAD
x
=======
y
>>>>>>> c1
b
c
<<<<<<< HEAD
p
=======
q
>>>>>>> c2
d
";
        let (regions, _) = extract_conflict_regions(text, 3, usize::MAX);
        assert_eq!(regions.len(), 1);
        assert_eq!((regions[0].first_line, regions[0].last_line), (1, 14));

        // With less context than the gap, they stay apart.
        let (regions, _) = extract_conflict_regions(text, 0, usize::MAX);
        assert_eq!(regions.len(), 2);
        assert_eq!((regions[0].first_line, regions[0].last_line), (2, 6));
        assert_eq!((regions[1].first_line, regions[1].last_line), (9, 13));
    }

    /// `merge.conflictStyle=diff3` adds a base section; it is inside the
    /// block, not the end of one.
    #[test]
    fn a_diff3_base_marker_stays_inside_the_block() {
        let text = "\
<<<<<<< HEAD
ours
||||||| merged common ancestors
base
=======
theirs
>>>>>>> theirs
";
        let (regions, _) = extract_conflict_regions(text, 0, usize::MAX);
        assert_eq!(regions.len(), 1);
        assert_eq!((regions[0].first_line, regions[0].last_line), (1, 7));
        assert!(regions[0].text.contains("||||||| merged common ancestors"));
    }

    /// Seven angle brackets at the start of a line is also how some people
    /// write prose. Without a closing marker it is not a conflict.
    #[test]
    fn an_unclosed_opening_marker_is_not_a_region() {
        let text = "<<<<<<< HEAD\nours\n=======\ntheirs\n";
        let (regions, truncated) = extract_conflict_regions(text, 3, usize::MAX);
        assert!(regions.is_empty());
        assert!(!truncated);
        assert_eq!(extract_conflict_regions("", 3, usize::MAX), (vec![], false));
        assert_eq!(
            extract_conflict_regions("no markers here\n", 3, usize::MAX),
            (vec![], false)
        );
    }

    /// The cut lands between lines, the region's `last_line` says how far it
    /// got, and the caller is told.
    #[test]
    fn truncation_cuts_on_a_line_boundary_and_says_so() {
        // "line 2\n" + "line 3\n" is 14 bytes; the third line would need 21.
        let (regions, truncated) = extract_conflict_regions(ONE_BLOCK, 3, 15);
        assert!(truncated);
        assert_eq!(regions.len(), 1);
        assert_eq!((regions[0].first_line, regions[0].last_line), (2, 3));
        assert_eq!(regions[0].text, "line 2\nline 3");

        // No budget at all: no partial region is invented.
        let (regions, truncated) = extract_conflict_regions(ONE_BLOCK, 3, 0);
        assert!(truncated);
        assert!(regions.is_empty());
    }

    /// A file saved by a Windows editor still has a conflict in it.
    #[test]
    fn crlf_input_is_recognised_and_normalised() {
        let text = "a\r\n<<<<<<< HEAD\r\nours\r\n=======\r\ntheirs\r\n>>>>>>> x\r\nb\r\n";
        let (regions, _) = extract_conflict_regions(text, 1, usize::MAX);
        assert_eq!(regions.len(), 1);
        assert_eq!((regions[0].first_line, regions[0].last_line), (1, 7));
        assert_eq!(
            regions[0].text,
            "a\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> x\nb"
        );
    }

    /// Context cannot run off either end of the file, and a missing trailing
    /// newline does not lose the last line.
    #[test]
    fn a_block_at_the_start_or_end_of_the_file_is_clamped() {
        let at_start = "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> x\nafter\n";
        let (regions, _) = extract_conflict_regions(at_start, 3, usize::MAX);
        assert_eq!((regions[0].first_line, regions[0].last_line), (1, 6));

        let at_end = "before\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> x";
        let (regions, _) = extract_conflict_regions(at_end, 3, usize::MAX);
        assert_eq!((regions[0].first_line, regions[0].last_line), (1, 6));
        assert!(regions[0].text.ends_with(">>>>>>> x"));
    }

    #[test]
    fn a_binary_file_is_not_searched_for_markers() {
        let caps = Caps::default();
        let mut budget = RegionBudget::default();
        let body = body_from_file(b"PNG\0\x01\x02", &caps, &mut budget);
        assert_eq!(body, ConflictBody::Binary);
        assert_eq!(
            budget,
            RegionBudget::default(),
            "a binary file spends nothing"
        );
    }

    /// The count and byte caps are enforced across files, and a file past
    /// either is named but not read.
    #[test]
    fn files_past_the_caps_are_omitted() {
        let caps = Caps {
            max_files_with_regions: 1,
            ..Caps::default()
        };
        let mut budget = RegionBudget::default();
        assert!(matches!(
            body_from_file(ONE_BLOCK.as_bytes(), &caps, &mut budget),
            ConflictBody::Regions { .. }
        ));
        assert_eq!(budget.files, 1);
        assert_eq!(
            body_from_file(ONE_BLOCK.as_bytes(), &caps, &mut budget),
            ConflictBody::Omitted
        );

        let caps = Caps {
            max_total_region_bytes: 20,
            ..Caps::default()
        };
        let mut budget = RegionBudget::default();
        let ConflictBody::Regions { truncated, .. } =
            body_from_file(ONE_BLOCK.as_bytes(), &caps, &mut budget)
        else {
            panic!("expected regions");
        };
        assert!(truncated, "the first file gets what is left of the total");
        assert_eq!(
            body_from_file(ONE_BLOCK.as_bytes(), &caps, &mut budget),
            ConflictBody::Omitted
        );
    }
}
