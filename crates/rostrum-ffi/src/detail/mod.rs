//! One pull request: its header, conversation, checks, labels, and the
//! pull-request-level actions.

mod types;

pub use types::{
    BranchUpdateMethod, CheckRunView, DraftAction, MergeMethod, MergeVerdict, PullDetail,
    PullHeader, ReviewThreadView, ThreadCommentView, TimelineEntry, TimelineEvent, TimelineKind,
};

use crate::{engine::RostrumCore, error::RostrumError, types::LabelView};

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Fetch the conversation, threads and checks from GitHub and cache them.
    pub async fn pull_detail(&self, repo: String, number: u32) -> Result<PullDetail, RostrumError> {
        let _ = (repo, number);
        Err(RostrumError::unimplemented("pull_detail"))
    }

    /// The last fetched detail from the cache, or `None`. No network.
    pub async fn cached_pull_detail(
        &self,
        repo: String,
        number: u32,
    ) -> Result<Option<PullDetail>, RostrumError> {
        let _ = (repo, number);
        Err(RostrumError::unimplemented("cached_pull_detail"))
    }

    /// Just the header, from the feed's data. No network.
    pub async fn pull_header(&self, repo: String, number: u32) -> Result<PullHeader, RostrumError> {
        let _ = (repo, number);
        Err(RostrumError::unimplemented("pull_header"))
    }

    /// Every label defined on the repository, for the label picker. Fetched
    /// once per repository and kept.
    pub async fn repository_labels(&self, repo: String) -> Result<Vec<LabelView>, RostrumError> {
        let _ = repo;
        Err(RostrumError::unimplemented("repository_labels"))
    }

    /// Apply a label. Applying one already present is not an error.
    pub async fn add_label(
        &self,
        repo: String,
        number: u32,
        label: String,
    ) -> Result<(), RostrumError> {
        let _ = (repo, number, label);
        Err(RostrumError::unimplemented("add_label"))
    }

    /// Remove a label. Removing one not present is not an error.
    pub async fn remove_label(
        &self,
        repo: String,
        number: u32,
        label: String,
    ) -> Result<(), RostrumError> {
        let _ = (repo, number, label);
        Err(RostrumError::unimplemented("remove_label"))
    }

    /// Post a top-level conversation comment.
    pub async fn add_comment(
        &self,
        repo: String,
        number: u32,
        body: String,
    ) -> Result<(), RostrumError> {
        let _ = (repo, number, body);
        Err(RostrumError::unimplemented("add_comment"))
    }

    /// Reply into an inline thread, by `ReviewThreadView::id`.
    pub async fn reply_to_thread(
        &self,
        repo: String,
        number: u32,
        thread_id: String,
        body: String,
    ) -> Result<(), RostrumError> {
        let _ = (repo, number, thread_id, body);
        Err(RostrumError::unimplemented("reply_to_thread"))
    }

    /// Merge. Confirm in the UI first. `expected_head_sha` is the header's
    /// `head_sha`; GitHub refuses the merge if the branch has moved since.
    /// Blank title or message means GitHub's default.
    pub async fn merge(
        &self,
        repo: String,
        number: u32,
        method: MergeMethod,
        commit_title: Option<String>,
        commit_message: Option<String>,
        expected_head_sha: String,
    ) -> Result<(), RostrumError> {
        let _ = (
            repo,
            number,
            method,
            commit_title,
            commit_message,
            expected_head_sha,
        );
        Err(RostrumError::unimplemented("merge"))
    }

    /// Close without merging. Confirm in the UI first.
    pub async fn close_pull_request(&self, repo: String, number: u32) -> Result<(), RostrumError> {
        let _ = (repo, number);
        Err(RostrumError::unimplemented("close_pull_request"))
    }

    /// Reopen a closed pull request.
    pub async fn reopen_pull_request(&self, repo: String, number: u32) -> Result<(), RostrumError> {
        let _ = (repo, number);
        Err(RostrumError::unimplemented("reopen_pull_request"))
    }

    /// Move into (`draft = true`) or out of draft. Pass the header's
    /// `draft_action.to_draft`. No confirmation needed: it is reversible.
    pub async fn set_draft(&self, repo: String, number: u32, draft: bool) -> Result<(), RostrumError> {
        let _ = (repo, number, draft);
        Err(RostrumError::unimplemented("set_draft"))
    }

    /// Bring the branch up to date with its base on GitHub. GitHub refuses if
    /// the branch is no longer at `expected_head_oid` (the header's
    /// `head_sha`).
    pub async fn update_branch(
        &self,
        repo: String,
        number: u32,
        method: BranchUpdateMethod,
        expected_head_oid: String,
    ) -> Result<(), RostrumError> {
        let _ = (repo, number, method, expected_head_oid);
        Err(RostrumError::unimplemented("update_branch"))
    }
}
