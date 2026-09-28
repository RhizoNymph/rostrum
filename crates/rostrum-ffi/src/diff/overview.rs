//! The Files tab's overview and file list, from `rostrum-diff`'s layout.

use std::collections::HashSet;

use rostrum_core::ReviewThread;
use rostrum_diff::{
    DiffFile, FileStatus as CoreStatus, PatchAvailability, change_map, max_churn,
    overview_stats, ranked_files, tile_heat,
};

use crate::{
    diff::{
        ChangedFile, DiffAvailability, DiffStats, FileStatus, FilesOverview, MapColumn, MapTile,
        RankedFile, TileHeat, load::LoadedFiles,
    },
    feed::count,
    review::book::Draft,
    types::Side,
};

/// Column budget for the change map; the rest fold into `(other)`. The
/// desktop's numbers, so the two maps agree.
const MAX_DIRS: usize = 8;
/// Tile budget per column; the rest fold into `+N more`.
const MAX_TILES: usize = 12;

pub(crate) fn overview(
    loaded: &LoadedFiles,
    threads: &[ReviewThread],
    drafts: &[Draft],
) -> FilesOverview {
    let files = &loaded.files;
    let stats = overview_stats(files);
    let scale = max_churn(files);
    FilesOverview {
        head_sha: loaded.head_sha.clone(),
        stats: DiffStats {
            files: count(stats.files),
            additions: stats.additions,
            deletions: stats.deletions,
            added_files: count(stats.added_files),
            removed_files: count(stats.removed_files),
            renamed_files: count(stats.renamed_files),
            modified_files: count(stats.modified_files),
        },
        change_map: change_map(files, MAX_DIRS, MAX_TILES)
            .into_iter()
            .map(|group| MapColumn {
                label: group.label,
                additions: group.additions,
                deletions: group.deletions,
                file_count: count(group.file_count),
                share: group.share,
                tiles: group
                    .tiles
                    .into_iter()
                    .map(|tile| {
                        let heat = tile_heat(tile.additions, tile.deletions, scale);
                        MapTile {
                            file_index: tile.file.map(|ix| ix as u32),
                            label: tile.label,
                            additions: tile.additions,
                            deletions: tile.deletions,
                            share: tile.share,
                            heat: TileHeat {
                                removed_ratio: heat.removed_ratio,
                                alpha: heat.alpha,
                            },
                        }
                    })
                    .collect(),
            })
            .collect(),
        ranked: ranked_files(files)
            .into_iter()
            .map(|ix| {
                let file = &files[ix];
                RankedFile {
                    file_index: ix as u32,
                    path: file.path.clone(),
                    status: status(file.status),
                    additions: file.additions,
                    deletions: file.deletions,
                    additions_share: file.additions as f32 / scale as f32,
                    deletions_share: file.deletions as f32 / scale as f32,
                }
            })
            .collect(),
        files: files
            .iter()
            .enumerate()
            .map(|(ix, file)| changed_file(ix, file, threads, drafts))
            .collect(),
    }
}

/// One file's entry, with how many threads and drafts sit on its lines.
pub(crate) fn changed_file(
    index: usize,
    file: &DiffFile,
    threads: &[ReviewThread],
    drafts: &[Draft],
) -> ChangedFile {
    let anchors: HashSet<(u32, Side)> = file
        .lines()
        .filter_map(|line| line.anchor(&file.path))
        .map(|anchor| (anchor.line, anchor.side.into()))
        .collect();
    let threads = threads
        .iter()
        .filter(|thread| {
            thread.path == file.path
                && thread
                    .line
                    .is_some_and(|line| anchors.contains(&(line, thread.side.into())))
        })
        .count();
    let drafts = drafts
        .iter()
        .filter(|draft| draft.comment.path == file.path)
        .count();
    ChangedFile {
        index: index as u32,
        path: file.path.clone(),
        previous_path: file.previous_path.clone(),
        status: status(file.status),
        additions: file.additions,
        deletions: file.deletions,
        availability: availability(file),
        threads: count(threads),
        drafts: count(drafts),
    }
}

/// Why a file has, or lacks, lines to show. GitHub sends no patch for binary
/// files, for files too large to diff, and for renames with no line changes;
/// the counts and status tell them apart.
pub(crate) fn availability(file: &DiffFile) -> DiffAvailability {
    match file.availability {
        PatchAvailability::Present if !file.hunks.is_empty() => DiffAvailability::Text,
        PatchAvailability::Present => DiffAvailability::NoTextChanges,
        PatchAvailability::Truncated => DiffAvailability::Unparseable,
        PatchAvailability::Omitted if file.additions + file.deletions > 0 => {
            DiffAvailability::TooLarge
        }
        PatchAvailability::Omitted => match file.status {
            CoreStatus::Renamed | CoreStatus::Copied | CoreStatus::Changed | CoreStatus::Unchanged => {
                DiffAvailability::NoTextChanges
            }
            CoreStatus::Added | CoreStatus::Removed | CoreStatus::Modified => {
                DiffAvailability::Binary
            }
        },
    }
}

pub(crate) fn status(status: CoreStatus) -> FileStatus {
    match status {
        CoreStatus::Added => FileStatus::Added,
        CoreStatus::Removed => FileStatus::Removed,
        CoreStatus::Modified => FileStatus::Modified,
        CoreStatus::Renamed => FileStatus::Renamed,
        CoreStatus::Copied => FileStatus::Copied,
        CoreStatus::Changed => FileStatus::Changed,
        CoreStatus::Unchanged => FileStatus::Unchanged,
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{DraftAnchor as CoreAnchor, Side as CoreSide, ThreadId};
    use rostrum_github::DraftComment;

    use super::*;

    fn file(path: &str, status: &str, additions: u32, deletions: u32, patch: Option<&str>) -> DiffFile {
        DiffFile::from_patch(path.into(), None, status, additions, deletions, patch)
    }

    fn loaded() -> LoadedFiles {
        LoadedFiles {
            head_sha: "head".into(),
            files: vec![
                file("src/a.rs", "modified", 3, 1, Some("@@ -1,2 +1,4 @@\n x\n-y\n+y2\n+z\n+w\n")),
                file("docs/b.md", "added", 10, 0, Some("@@ -0,0 +1,1 @@\n+hello\n")),
                file("logo.png", "added", 0, 0, None),
                file("big.json", "modified", 900, 20, None),
                file("moved.rs", "renamed", 0, 0, None),
            ],
        }
    }

    fn thread(path: &str, line: Option<u32>, side: CoreSide) -> ReviewThread {
        ReviewThread {
            id: ThreadId("t".into()),
            path: path.into(),
            line,
            original_line: line,
            side,
            is_resolved: false,
            is_outdated: false,
            comments: vec![],
        }
    }

    #[test]
    fn availability_tells_the_missing_patches_apart() {
        let files = loaded().files;
        let kinds: Vec<DiffAvailability> = files.iter().map(availability).collect();
        assert_eq!(
            kinds,
            vec![
                DiffAvailability::Text,
                DiffAvailability::Text,
                DiffAvailability::Binary,
                DiffAvailability::TooLarge,
                DiffAvailability::NoTextChanges,
            ]
        );
        assert_eq!(
            availability(&file("x.rs", "modified", 1, 0, Some("garbage"))),
            DiffAvailability::Unparseable
        );
        assert_eq!(
            availability(&file("x.rs", "modified", 0, 0, Some(""))),
            DiffAvailability::NoTextChanges
        );
    }

    #[test]
    fn the_overview_ranks_maps_and_lists_every_file() {
        let overview = overview(&loaded(), &[], &[]);
        assert_eq!(overview.head_sha, "head");
        assert_eq!(overview.stats.files, 5);
        assert_eq!(overview.stats.additions, 913);
        assert_eq!(overview.ranked[0].path, "big.json");
        assert!((overview.ranked[0].additions_share + overview.ranked[0].deletions_share - 1.0).abs() < 1e-6);
        assert_eq!(overview.files.len(), 5);
        assert_eq!(overview.files[3].index, 3);
        let shares: f32 = overview.change_map.iter().map(|column| column.share).sum();
        assert!((shares - 1.0).abs() < 1e-4, "{shares}");
        for column in &overview.change_map {
            let tiles: f32 = column.tiles.iter().map(|tile| tile.share).sum();
            assert!((tiles - 1.0).abs() < 1e-4, "{tiles}");
        }
        let rename_tile = overview
            .change_map
            .iter()
            .flat_map(|column| &column.tiles)
            .find(|tile| tile.label == "moved.rs")
            .expect("rename tile");
        assert_eq!(rename_tile.heat.removed_ratio, None);
    }

    #[test]
    fn thread_and_draft_counts_only_count_lines_in_the_diff() {
        let threads = vec![
            thread("src/a.rs", Some(2), CoreSide::Right),
            thread("src/a.rs", Some(2), CoreSide::Left),
            thread("src/a.rs", Some(99), CoreSide::Right),
            thread("src/a.rs", None, CoreSide::Right),
            thread("docs/b.md", Some(1), CoreSide::Right),
        ];
        let draft = Draft {
            id: 1,
            comment: {
                let anchor = CoreAnchor::single("src/a.rs", 3, CoreSide::Right);
                DraftComment::single(anchor.path, anchor.line, anchor.side, "x")
            },
        };
        let overview = overview(&loaded(), &threads, &[draft]);
        assert_eq!(overview.files[0].threads, 2);
        assert_eq!(overview.files[0].drafts, 1);
        assert_eq!(overview.files[1].threads, 1);
        assert_eq!(overview.files[2].threads, 0);
    }
}
