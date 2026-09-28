//! Changed files, the overview, and one file's diff at a time.

mod types;

pub use types::{
    ChangedFile, CodeSegment, CommentAnchor, DiffAvailability, DiffLineView, DiffRow, DiffStats,
    FileDiff, FileDiffBody, FileStatus, FilesOverview, LineKind, MapColumn, MapTile, RankedFile,
    TileHeat,
};

use crate::{engine::RostrumCore, error::RostrumError};

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// The Files tab's overview. Fetches the changed files once per head
    /// commit; later calls for the same head are served from the cache.
    pub async fn files_overview(
        &self,
        repo: String,
        number: u32,
    ) -> Result<FilesOverview, RostrumError> {
        let _ = (repo, number);
        Err(RostrumError::unimplemented("files_overview"))
    }

    /// One file's diff as rows: syntax-highlighted, word-level changes
    /// emphasised, threads and drafts placed at their lines, and a comment
    /// anchor on every line that can take one. `file_index` is
    /// `ChangedFile::index`.
    pub async fn file_diff(
        &self,
        repo: String,
        number: u32,
        file_index: u32,
    ) -> Result<FileDiff, RostrumError> {
        let _ = (repo, number, file_index);
        Err(RostrumError::unimplemented("file_diff"))
    }
}
