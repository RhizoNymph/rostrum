//! Changed files, the overview, and one file's diff at a time.

mod highlight;
pub(crate) mod load;
mod overview;
mod rows;
mod segments;
mod types;

pub(crate) use load::LoadedFiles;

pub use types::{
    ChangedFile, CodeSegment, CommentAnchor, DiffAvailability, DiffLineView, DiffRow, DiffStats,
    FileDiff, FileDiffBody, FileStatus, FilesOverview, LineKind, MapColumn, MapTile, RankedFile,
    TileHeat,
};

use crate::{
    engine::{RostrumCore, state::PullKey},
    error::RostrumError,
};

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// The Files tab's overview. Fetches the changed files once per head
    /// commit; later calls for the same head are served from the cache.
    pub async fn files_overview(
        &self,
        repo: String,
        number: u32,
    ) -> Result<FilesOverview, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let loaded = self.load_files(&key).await?;
        let threads = self.known_threads(&key).await?;
        let drafts = self.draft_list(&key).await?;
        let built = tokio::task::spawn_blocking(move || {
            overview::overview(&loaded, &threads, &drafts)
        })
        .await?;
        Ok(built)
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
        let key = PullKey::parse(&repo, number)?;
        let loaded = self.load_files(&key).await?;
        let index = file_index as usize;
        if index >= loaded.files.len() {
            return Err(RostrumError::invalid(format!(
                "file {file_index} is out of range; this diff has {} files",
                loaded.files.len()
            )));
        }
        let threads = self.known_threads(&key).await?;
        let drafts = self.draft_list(&key).await?;
        let built = tokio::task::spawn_blocking(move || {
            let file = &loaded.files[index];
            let changed = overview::changed_file(index, file, &threads, &drafts);
            let body = if changed.availability == DiffAvailability::Text {
                FileDiffBody::Rows {
                    rows: rows::build_rows(file, &threads, &drafts, &key.repo),
                }
            } else {
                FileDiffBody::Unavailable
            };
            FileDiff {
                file: changed,
                head_sha: loaded.head_sha.clone(),
                body,
            }
        })
        .await?;
        Ok(built)
    }
}
