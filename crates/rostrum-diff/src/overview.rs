//! Aggregate views over a pull request's diff, for the visual overview.
//!
//! Everything here is pure data-shaping over [`DiffFile`]s: totals, a
//! churn-ranked file order, and the proportional layout for the change map
//! (directories as columns, files as tiles). The UI multiplies the `share`
//! fractions into element sizes; no pixel arithmetic happens here, so all of it
//! is testable without a window.

use std::collections::BTreeMap;

use crate::model::{DiffFile, FileStatus};

/// Whole-diff totals for the overview's summary strip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverviewStats {
    pub files: usize,
    pub additions: u64,
    pub deletions: u64,
    pub added_files: usize,
    pub removed_files: usize,
    pub renamed_files: usize,
    pub modified_files: usize,
}

/// One rectangle of the change map: a file, or the aggregate of the files that
/// did not fit their directory's tile budget.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    /// Index into the `files` slice, or `None` for an aggregate tile.
    pub file: Option<usize>,
    /// File name (last path component), or `+N more` for an aggregate.
    pub label: String,
    pub additions: u64,
    pub deletions: u64,
    /// Fraction of the column this tile occupies. Tiles of a group sum to ~1.
    pub share: f32,
}

/// One column of the change map: a directory, or the aggregate `(other)`
/// column holding the directories that did not fit the column budget.
#[derive(Clone, Debug, PartialEq)]
pub struct DirGroup {
    /// Parent directory path, `(root)` for top-level files, or `(other)`.
    pub label: String,
    pub additions: u64,
    pub deletions: u64,
    pub file_count: usize,
    /// Fraction of the map's width this column occupies. Columns sum to ~1.
    pub share: f32,
    pub tiles: Vec<Tile>,
}

/// Total churn of a file as GitHub counts it.
pub fn churn(file: &DiffFile) -> u64 {
    u64::from(file.additions) + u64::from(file.deletions)
}

/// Layout weight: like [`churn`], but a zero-churn file (a pure rename, a mode
/// change) still occupies a visible sliver instead of vanishing from the map.
fn weight(file: &DiffFile) -> u64 {
    churn(file).max(1)
}

pub fn overview_stats(files: &[DiffFile]) -> OverviewStats {
    let mut stats = OverviewStats {
        files: files.len(),
        ..OverviewStats::default()
    };
    for file in files {
        stats.additions += u64::from(file.additions);
        stats.deletions += u64::from(file.deletions);
        match file.status {
            FileStatus::Added => stats.added_files += 1,
            FileStatus::Removed => stats.removed_files += 1,
            FileStatus::Renamed | FileStatus::Copied => stats.renamed_files += 1,
            FileStatus::Modified | FileStatus::Changed | FileStatus::Unchanged => {
                stats.modified_files += 1
            }
        }
    }
    stats
}

/// File indices ordered by churn, largest first; ties keep patch order.
pub fn ranked_files(files: &[DiffFile]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by_key(|&ix| std::cmp::Reverse(churn(&files[ix])));
    order
}

/// The path's parent directory, or `None` for a top-level file.
fn parent_dir(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(dir, _)| dir)
}

/// The path's file name.
fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Build the change-map layout: at most `max_dirs` columns of at most
/// `max_tiles` tiles each, with overflow folded into `(other)` / `+N more`
/// aggregates so the totals stay honest.
pub fn change_map(files: &[DiffFile], max_dirs: usize, max_tiles: usize) -> Vec<DirGroup> {
    if files.is_empty() || max_dirs == 0 || max_tiles == 0 {
        return Vec::new();
    }

    // Group file indices by parent directory, deterministically ordered.
    let mut by_dir: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (ix, file) in files.iter().enumerate() {
        by_dir
            .entry(parent_dir(&file.path).unwrap_or(""))
            .or_default()
            .push(ix);
    }

    let dir_weight =
        |members: &[usize]| -> u64 { members.iter().map(|&ix| weight(&files[ix])).sum() };

    let mut dirs: Vec<(&str, Vec<usize>)> = by_dir.into_iter().collect();
    // Heaviest directories first; the BTreeMap ordering breaks ties by name.
    dirs.sort_by_key(|(_, members)| std::cmp::Reverse(dir_weight(members)));

    // Fold the tail into one aggregate column when there are too many.
    if dirs.len() > max_dirs {
        let tail: Vec<usize> = dirs
            .split_off(max_dirs - 1)
            .into_iter()
            .flat_map(|(_, members)| members)
            .collect();
        dirs.push(("(other)", tail));
    }

    let total: u64 = dirs.iter().map(|(_, members)| dir_weight(members)).sum();

    dirs.into_iter()
        .map(|(dir, mut members)| {
            let group_weight = dir_weight(&members);
            let file_count = members.len();
            members.sort_by_key(|&ix| std::cmp::Reverse(weight(&files[ix])));

            // Fold the group's own tail into one aggregate tile.
            let overflow = if members.len() > max_tiles {
                members.split_off(max_tiles - 1)
            } else {
                Vec::new()
            };

            let mut tiles: Vec<Tile> = members
                .into_iter()
                .map(|ix| Tile {
                    file: Some(ix),
                    label: file_name(&files[ix].path).to_owned(),
                    additions: u64::from(files[ix].additions),
                    deletions: u64::from(files[ix].deletions),
                    share: weight(&files[ix]) as f32 / group_weight as f32,
                })
                .collect();
            if !overflow.is_empty() {
                tiles.push(Tile {
                    file: None,
                    label: format!("+{} more", overflow.len()),
                    additions: overflow
                        .iter()
                        .map(|&ix| u64::from(files[ix].additions))
                        .sum(),
                    deletions: overflow
                        .iter()
                        .map(|&ix| u64::from(files[ix].deletions))
                        .sum(),
                    share: overflow.iter().map(|&ix| weight(&files[ix])).sum::<u64>() as f32
                        / group_weight as f32,
                });
            }

            DirGroup {
                label: if dir.is_empty() {
                    "(root)".to_owned()
                } else {
                    dir.to_owned()
                },
                additions: tiles.iter().map(|tile| tile.additions).sum(),
                deletions: tiles.iter().map(|tile| tile.deletions).sum(),
                file_count,
                share: group_weight as f32 / total as f32,
                tiles,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PatchAvailability;

    fn file(path: &str, additions: u32, deletions: u32, status: FileStatus) -> DiffFile {
        DiffFile {
            path: path.into(),
            previous_path: None,
            status,
            additions,
            deletions,
            hunks: Vec::new(),
            availability: PatchAvailability::Present,
        }
    }

    fn modified(path: &str, additions: u32, deletions: u32) -> DiffFile {
        file(path, additions, deletions, FileStatus::Modified)
    }

    #[test]
    fn stats_total_the_whole_diff() {
        let files = [
            file("a.rs", 10, 2, FileStatus::Added),
            file("b/c.rs", 5, 5, FileStatus::Modified),
            file("b/d.rs", 0, 7, FileStatus::Removed),
            file("e.rs", 0, 0, FileStatus::Renamed),
        ];
        let stats = overview_stats(&files);
        assert_eq!(stats.files, 4);
        assert_eq!(stats.additions, 15);
        assert_eq!(stats.deletions, 14);
        assert_eq!(stats.added_files, 1);
        assert_eq!(stats.removed_files, 1);
        assert_eq!(stats.renamed_files, 1);
        assert_eq!(stats.modified_files, 1);
    }

    #[test]
    fn ranking_is_by_churn_descending_with_stable_ties() {
        let files = [
            modified("small.rs", 1, 0),
            modified("big.rs", 100, 50),
            modified("tie-a.rs", 3, 3),
            modified("tie-b.rs", 6, 0),
        ];
        assert_eq!(ranked_files(&files), vec![1, 2, 3, 0]);
    }

    #[test]
    fn ranking_of_no_files_is_empty() {
        assert!(ranked_files(&[]).is_empty());
        assert!(change_map(&[], 8, 8).is_empty());
    }

    #[test]
    fn map_groups_by_parent_directory() {
        let files = [
            modified("src/a.rs", 10, 0),
            modified("src/b.rs", 5, 0),
            modified("tests/t.rs", 1, 0),
            modified("README.md", 2, 0),
        ];
        let map = change_map(&files, 8, 8);
        let labels: Vec<&str> = map.iter().map(|g| g.label.as_str()).collect();
        // src weighs 15, the root README 2, tests 1.
        assert_eq!(labels, vec!["src", "(root)", "tests"]);
    }

    #[test]
    fn map_shares_sum_to_one() {
        let files = [
            modified("src/a.rs", 10, 3),
            modified("src/b.rs", 5, 1),
            modified("tests/t.rs", 1, 0),
            modified("README.md", 2, 2),
        ];
        let map = change_map(&files, 8, 8);
        let total: f32 = map.iter().map(|g| g.share).sum();
        assert!((total - 1.0).abs() < 1e-5, "column shares sum to {total}");
        for group in &map {
            let inner: f32 = group.tiles.iter().map(|t| t.share).sum();
            assert!(
                (inner - 1.0).abs() < 1e-5,
                "tile shares of {} sum to {inner}",
                group.label
            );
        }
    }

    #[test]
    fn heaviest_directory_comes_first() {
        let files = [
            modified("light/a.rs", 1, 0),
            modified("heavy/b.rs", 100, 100),
        ];
        let map = change_map(&files, 8, 8);
        assert_eq!(map[0].label, "heavy");
        assert!(map[0].share > map[1].share);
    }

    #[test]
    fn root_files_group_under_a_root_label() {
        let files = [modified("Cargo.toml", 1, 1)];
        let map = change_map(&files, 8, 8);
        assert_eq!(map[0].label, "(root)");
        assert_eq!(map[0].tiles[0].label, "Cargo.toml");
    }

    #[test]
    fn excess_directories_fold_into_other() {
        let files = [
            modified("a/f.rs", 40, 0),
            modified("b/f.rs", 30, 0),
            modified("c/f.rs", 20, 0),
            modified("d/f.rs", 10, 0),
        ];
        let map = change_map(&files, 3, 8);
        assert_eq!(map.len(), 3);
        assert_eq!(map[0].label, "a");
        assert_eq!(map[1].label, "b");
        let other = &map[2];
        assert_eq!(other.label, "(other)");
        assert_eq!(other.additions, 30);
        assert_eq!(other.file_count, 2);
        // Totals survive the fold.
        let total_adds: u64 = map.iter().map(|g| g.additions).sum();
        assert_eq!(total_adds, 100);
    }

    #[test]
    fn excess_tiles_fold_into_an_aggregate() {
        let files = [
            modified("src/a.rs", 40, 0),
            modified("src/b.rs", 30, 0),
            modified("src/c.rs", 20, 0),
            modified("src/d.rs", 10, 0),
        ];
        let map = change_map(&files, 8, 3);
        let tiles = &map[0].tiles;
        assert_eq!(tiles.len(), 3);
        assert_eq!(tiles[0].file, Some(0));
        assert_eq!(tiles[1].file, Some(1));
        let aggregate = &tiles[2];
        assert_eq!(aggregate.file, None);
        assert_eq!(aggregate.label, "+2 more");
        assert_eq!(aggregate.additions, 30);
        assert_eq!(map[0].file_count, 4);
        let inner: f32 = tiles.iter().map(|t| t.share).sum();
        assert!((inner - 1.0).abs() < 1e-5);
    }

    #[test]
    fn zero_churn_files_still_occupy_a_sliver() {
        let files = [
            modified("src/big.rs", 100, 100),
            file("src/renamed.rs", 0, 0, FileStatus::Renamed),
        ];
        let map = change_map(&files, 8, 8);
        let sliver = map[0]
            .tiles
            .iter()
            .find(|t| t.label == "renamed.rs")
            .expect("renamed file present");
        assert!(sliver.share > 0.0);
    }

    #[test]
    fn tiles_within_a_group_are_ordered_by_churn() {
        let files = [
            modified("src/small.rs", 1, 0),
            modified("src/big.rs", 50, 50),
        ];
        let map = change_map(&files, 8, 8);
        assert_eq!(map[0].tiles[0].label, "big.rs");
        assert_eq!(map[0].tiles[1].label, "small.rs");
    }
}
