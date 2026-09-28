//! Pull-request-level mutations. Each checks its input, makes one GitHub
//! call, and then refreshes the repository so the feed and the header show
//! GitHub's authoritative answer rather than a guess made here.

use rostrum_core::{PullRequest, ThreadId};
use rostrum_github::{
    BranchUpdateMethod as GitHubUpdate, DraftState, GitHubClient, IssueState,
    MergeMethod as GitHubMerge, MergePullRequest,
};

use crate::{
    detail::{BranchUpdateMethod, MergeMethod},
    engine::{RostrumCore, state::PullKey},
    error::RostrumError,
    feed::{Probes, Scope},
};

impl From<MergeMethod> for GitHubMerge {
    fn from(method: MergeMethod) -> Self {
        match method {
            MergeMethod::Merge => Self::Merge,
            MergeMethod::Squash => Self::Squash,
            MergeMethod::Rebase => Self::Rebase,
        }
    }
}

impl From<BranchUpdateMethod> for GitHubUpdate {
    fn from(method: BranchUpdateMethod) -> Self {
        match method {
            BranchUpdateMethod::Merge => Self::Merge,
            BranchUpdateMethod::Rebase => Self::Rebase,
        }
    }
}

/// Text Kotlin passed, trimmed, or `InvalidInput` naming `what` if blank.
fn required(text: &str, what: &str) -> Result<String, RostrumError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(RostrumError::invalid(format!("{what} cannot be blank")));
    }
    Ok(text.to_string())
}

impl RostrumCore {
    /// The client, and the pull request a mutation addresses.
    async fn target(&self, key: &PullKey) -> Result<(GitHubClient, PullRequest), RostrumError> {
        self.ensure_hydrated().await?;
        let lookup = key.clone();
        self.actor
            .try_call(move |state| Ok((state.github()?, state.known(&lookup)?.clone())))
            .await
    }

    /// After a mutation GitHub accepted: re-read the repository. Its failure
    /// is logged, not reported — the mutation itself succeeded.
    pub(crate) async fn after_mutation(&self, key: &PullKey) {
        if let Err(error) = self
            .refresh(Scope::One(key.repo.clone()), Probes::Schedule)
            .await
        {
            tracing::warn!(repo = %key.repo, %error, "could not refresh after a change");
        }
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Apply a label. Applying one already present is not an error.
    pub async fn add_label(
        &self,
        repo: String,
        number: u32,
        label: String,
    ) -> Result<(), RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let label = required(&label, "a label")?;
        let (client, _) = self.target(&key).await?;
        self.github(client.add_labels(&key.repo, key.number, &[label]).await)
            .await?;
        self.after_mutation(&key).await;
        Ok(())
    }

    /// Remove a label. Removing one not present is not an error.
    pub async fn remove_label(
        &self,
        repo: String,
        number: u32,
        label: String,
    ) -> Result<(), RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let label = required(&label, "a label")?;
        let (client, _) = self.target(&key).await?;
        self.github(client.remove_label(&key.repo, key.number, &label).await)
            .await?;
        self.after_mutation(&key).await;
        Ok(())
    }

    /// Post a top-level conversation comment.
    pub async fn add_comment(
        &self,
        repo: String,
        number: u32,
        body: String,
    ) -> Result<(), RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let body = required(&body, "a comment")?;
        let (client, _) = self.target(&key).await?;
        self.github(client.add_comment(&key.repo, key.number, &body).await)
            .await?;
        self.after_mutation(&key).await;
        Ok(())
    }

    /// Reply into an inline thread, by `ReviewThreadView::id`.
    pub async fn reply_to_thread(
        &self,
        repo: String,
        number: u32,
        thread_id: String,
        body: String,
    ) -> Result<(), RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let body = required(&body, "a reply")?;
        let (client, _) = self.target(&key).await?;
        let conversation = self.known_conversation(&key).await?.ok_or_else(|| {
            RostrumError::invalid("load the conversation before replying to a thread")
        })?;
        let thread = conversation
            .thread(&ThreadId(thread_id.clone()))
            .ok_or_else(|| RostrumError::invalid(format!("there is no thread {thread_id}")))?;
        let target = thread.reply_target().ok_or_else(|| {
            RostrumError::invalid("GitHub gave this thread no comment id to reply to")
        })?;
        self.github(
            client
                .reply_to_thread(&key.repo, key.number, target, &body)
                .await,
        )
        .await?;
        self.after_mutation(&key).await;
        Ok(())
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
        let key = PullKey::parse(&repo, number)?;
        let expected = required(&expected_head_sha, "the expected head sha")?;
        let (client, _) = self.target(&key).await?;
        let request = MergePullRequest::new(method.into())
            .expecting_head(expected)
            .with_message(commit_title, commit_message);
        self.github(client.merge(&key.repo, key.number, &request).await)
            .await?;
        tracing::info!(repo = %key.repo, number = key.number.0, ?method, "merged");
        self.after_mutation(&key).await;
        Ok(())
    }

    /// Close without merging. Confirm in the UI first.
    pub async fn close_pull_request(&self, repo: String, number: u32) -> Result<(), RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let (client, _) = self.target(&key).await?;
        self.github(
            client
                .set_state(&key.repo, key.number, IssueState::Closed)
                .await,
        )
        .await?;
        self.after_mutation(&key).await;
        Ok(())
    }

    /// Reopen a closed pull request.
    pub async fn reopen_pull_request(&self, repo: String, number: u32) -> Result<(), RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let (client, _) = self.target(&key).await?;
        self.github(
            client
                .set_state(&key.repo, key.number, IssueState::Open)
                .await,
        )
        .await?;
        self.after_mutation(&key).await;
        Ok(())
    }

    /// Move into (`draft = true`) or out of draft. Pass the header's
    /// `draft_action.to_draft`. No confirmation needed: it is reversible.
    pub async fn set_draft(
        &self,
        repo: String,
        number: u32,
        draft: bool,
    ) -> Result<(), RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let (client, pr) = self.target(&key).await?;
        let target = if draft {
            DraftState::Draft
        } else {
            DraftState::ReadyForReview
        };
        self.github(
            client
                .set_draft(&key.repo, key.number, &pr.node_id, target)
                .await,
        )
        .await?;
        self.after_mutation(&key).await;
        Ok(())
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
        let key = PullKey::parse(&repo, number)?;
        let expected = required(&expected_head_oid, "the expected head sha")?;
        let (client, pr) = self.target(&key).await?;
        self.github(
            client
                .update_branch(&key.repo, key.number, &pr.node_id, &expected, method.into())
                .await,
        )
        .await?;
        self.after_mutation(&key).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_input_is_refused_with_what_was_missing() {
        assert_eq!(required("  x ", "a label"), Ok("x".to_string()));
        assert_eq!(
            required(" \n", "a comment"),
            Err(RostrumError::invalid("a comment cannot be blank"))
        );
    }

    #[test]
    fn methods_map_onto_githubs() {
        assert_eq!(GitHubMerge::from(MergeMethod::Squash), GitHubMerge::Squash);
        assert_eq!(GitHubMerge::from(MergeMethod::Rebase), GitHubMerge::Rebase);
        assert_eq!(GitHubMerge::from(MergeMethod::Merge), GitHubMerge::Merge);
        assert_eq!(
            GitHubUpdate::from(BranchUpdateMethod::Rebase),
            GitHubUpdate::Rebase
        );
    }
}
