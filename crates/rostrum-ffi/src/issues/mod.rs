//! Issues: the Issues tab's rows (built by the feed), the issue screen, and
//! every issue action — comment, close as completed or not planned, reopen,
//! labels, assignees — plus creating one.
//!
//! The model, the requests and the rules are the desktop's: `rostrum-core`'s
//! `Issue`, `rostrum-github`'s `IssueMutation` (one REST call each) and
//! `IssueDraft` (no blank title reaches GitHub), `rostrum-db`'s issue cache.
//!
//! Editing the title and description (with conflict detection) and paging
//! the timeline back ("load earlier") are in [`edit`] and [`paging`].

mod edit;
mod paging;
pub(crate) mod summary;
mod types;

use std::sync::Arc;

use rostrum_core::{IssueDetail as CoreIssueDetail, User};
use rostrum_github::{
    AddLabels, Assignees, CloseAs, CommentBody, DraftError, IssueDraft, IssueMutation,
    IssueStateChange,
};

pub use types::{CloseIssueAs, IssueCloseReason, IssueDetail, IssueStatus, IssueSummary};

use crate::{
    detail::{earlier_of, render_timeline},
    engine::{
        RostrumCore,
        state::{IssueKey, parse_repo},
        writer::Write,
    },
    error::RostrumError,
    types::UserRef,
};
use summary::summarize_issue;

impl From<CloseIssueAs> for CloseAs {
    fn from(reason: CloseIssueAs) -> Self {
        match reason {
            CloseIssueAs::Completed => Self::Completed,
            CloseIssueAs::NotPlanned => Self::NotPlanned,
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

/// The issue screen's record, built off the caller's thread (markdown).
async fn render(
    key: &IssueKey,
    detail: Arc<CoreIssueDetail>,
    viewer: Option<User>,
) -> Result<IssueDetail, RostrumError> {
    let repo = key.repo.clone();
    let (has_earlier, earlier_count) = earlier_of(&detail.conversation);
    Ok(tokio::task::spawn_blocking(move || IssueDetail {
        issue: summarize_issue(&repo, &detail.issue, viewer.map(|user| user.key()).as_ref()),
        timeline: render_timeline(&detail.conversation, &repo),
        has_earlier,
        earlier_count,
    })
    .await?)
}

impl RostrumCore {
    /// The issue detail last fetched (with every earlier page loaded), from
    /// memory or the cache.
    pub(crate) async fn held_issue(
        &self,
        key: &IssueKey,
    ) -> Result<Option<Arc<CoreIssueDetail>>, RostrumError> {
        let lookup = key.clone();
        if let Some(held) = self
            .actor
            .call(move |state| state.issue_details.get(&lookup).cloned())
            .await?
        {
            return Ok(Some(held));
        }
        let Some(stored) = self.db.load_issue_detail(&key.repo, key.number).await? else {
            return Ok(None);
        };
        let stored = Arc::new(stored);
        let store = key.clone();
        let kept = stored.clone();
        self.actor
            .call(move |state| {
                // A fresher copy from the network wins over the cache.
                if state.issue_details.get(&store).is_none() {
                    state.issue_details.insert(store, kept);
                }
            })
            .await?;
        Ok(Some(stored))
    }

    /// Keep an issue detail in memory and the cache.
    pub(crate) async fn keep_issue(
        &self,
        key: &IssueKey,
        detail: Arc<CoreIssueDetail>,
    ) -> Result<(), RostrumError> {
        let store = key.clone();
        self.actor
            .call(move |state| {
                state.writer.send(Write::IssueDetail {
                    repo: store.repo.clone(),
                    detail: Box::new((*detail).clone()),
                });
                state.issue_details.insert(store, detail);
            })
            .await
    }

    /// Fetch the newest page of an issue, keep the earlier pages already
    /// held, and keep the result.
    pub(crate) async fn fetch_issue(
        &self,
        key: &IssueKey,
    ) -> Result<Arc<CoreIssueDetail>, RostrumError> {
        let client = self.actor.try_call(|state| state.github()).await?;
        let fresh = self
            .github(client.issue_detail(&key.repo, key.number).await)
            .await?;
        let detail = Arc::new(match self.held_issue(key).await? {
            Some(held) => CoreIssueDetail {
                conversation: held.conversation.refreshed_by(fresh.conversation),
                issue: fresh.issue,
            },
            None => fresh,
        });
        self.keep_issue(key, detail.clone()).await?;
        Ok(detail)
    }

    async fn viewer_now(&self) -> Result<Option<User>, RostrumError> {
        self.actor
            .call(|state| state.session.viewer().cloned())
            .await
    }

    /// Send one issue mutation, then re-read the repository's issues so the
    /// feed shows GitHub's answer (a close drops the row). The re-read's
    /// failure is logged, not reported: the mutation itself succeeded.
    pub(crate) async fn mutate_issue(
        &self,
        key: IssueKey,
        mutation: IssueMutation,
    ) -> Result<(), RostrumError> {
        let client = self.actor.try_call(|state| state.github()).await?;
        self.github(client.mutate_issue(&key.repo, key.number, &mutation).await)
            .await?;
        tracing::info!(repo = %key.repo, number = key.number.0, action = mutation.progress_label(), "issue changed");
        self.after_issue_change(&key.repo).await;
        Ok(())
    }

    pub(crate) async fn after_issue_change(&self, repo: &rostrum_core::RepoId) {
        if let Err(error) = self.refresh_issues(repo).await {
            tracing::warn!(%repo, %error, "could not refresh issues after a change");
        }
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Fetch an issue with its description, comments and events, and cache it.
    pub async fn issue_detail(
        &self,
        repo: String,
        number: u32,
    ) -> Result<IssueDetail, RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        let detail = self.fetch_issue(&key).await?;
        render(&key, detail, self.viewer_now().await?).await
    }

    /// The last fetched issue screen from memory or the cache, or `None`.
    /// No network.
    pub async fn cached_issue_detail(
        &self,
        repo: String,
        number: u32,
    ) -> Result<Option<IssueDetail>, RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        let Some(detail) = self.held_issue(&key).await? else {
            return Ok(None);
        };
        render(&key, detail, self.viewer_now().await?)
            .await
            .map(Some)
    }

    /// Comment on an issue.
    pub async fn comment_on_issue(
        &self,
        repo: String,
        number: u32,
        body: String,
    ) -> Result<(), RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        let body =
            CommentBody::new(body).map_err(|error| RostrumError::invalid(error.to_string()))?;
        self.mutate_issue(key, IssueMutation::Comment(body)).await
    }

    /// Close an issue as completed or not planned. No confirmation needed:
    /// reopening undoes it.
    pub async fn close_issue(
        &self,
        repo: String,
        number: u32,
        reason: CloseIssueAs,
    ) -> Result<(), RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        self.mutate_issue(
            key,
            IssueMutation::SetState(IssueStateChange::Close(reason.into())),
        )
        .await
    }

    pub async fn reopen_issue(&self, repo: String, number: u32) -> Result<(), RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        self.mutate_issue(key, IssueMutation::SetState(IssueStateChange::Reopen))
            .await
    }

    /// Apply a label (from `repository_labels`). Applying one already present
    /// is not an error.
    pub async fn add_issue_label(
        &self,
        repo: String,
        number: u32,
        label: String,
    ) -> Result<(), RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        let label = required(&label, "a label")?;
        self.mutate_issue(key, IssueMutation::AddLabels(AddLabels::new([label])))
            .await
    }

    /// Remove a label. Removing one not present is not an error.
    pub async fn remove_issue_label(
        &self,
        repo: String,
        number: u32,
        label: String,
    ) -> Result<(), RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        let label = required(&label, "a label")?;
        self.mutate_issue(key, IssueMutation::RemoveLabel(label))
            .await
    }

    /// Everyone who can be assigned issues in the repository, for the
    /// assignee picker. Fetched once per repository and kept.
    pub async fn assignable_users(&self, repo: String) -> Result<Vec<UserRef>, RostrumError> {
        let id = parse_repo(&repo)?;
        let lookup = id.clone();
        let (cached, client) = self
            .actor
            .call(move |state| {
                (
                    state.assignable.get(&lookup).cloned(),
                    state.session.client(),
                )
            })
            .await?;
        let users = match cached {
            Some(users) => users,
            None => {
                let client = client.ok_or(RostrumError::NotSignedIn)?;
                let fetched = Arc::new(self.github(client.assignable_users(&id).await).await?);
                let kept = fetched.clone();
                self.actor
                    .call(move |state| {
                        state.assignable.insert(id, kept);
                    })
                    .await?;
                fetched
            }
        };
        Ok(users.iter().map(UserRef::from).collect())
    }

    pub async fn add_issue_assignee(
        &self,
        repo: String,
        number: u32,
        login: String,
    ) -> Result<(), RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        let login = required(&login, "a login")?;
        self.mutate_issue(key, IssueMutation::AddAssignees(Assignees::new([login])))
            .await
    }

    pub async fn remove_issue_assignee(
        &self,
        repo: String,
        number: u32,
        login: String,
    ) -> Result<(), RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        let login = required(&login, "a login")?;
        self.mutate_issue(key, IssueMutation::RemoveAssignees(Assignees::new([login])))
            .await
    }

    /// Open an issue and return its number. A blank title is refused before
    /// anything is sent; the body is markdown and may be empty. On success
    /// the repository's issues are re-read so the new one is in the feed.
    pub async fn create_issue(
        &self,
        repo: String,
        title: String,
        body: String,
        labels: Vec<String>,
        assignees: Vec<String>,
    ) -> Result<u32, RostrumError> {
        let id = parse_repo(&repo)?;
        let mut draft = IssueDraft::new(Some(id));
        // The draft toggles; a name Kotlin sent twice is still one choice.
        for label in labels
            .iter()
            .map(|label| label.trim())
            .filter(|label| !label.is_empty())
        {
            if !draft.labels().contains(label) {
                draft.toggle_label(label);
            }
        }
        for login in assignees
            .iter()
            .map(|login| login.trim())
            .filter(|login| !login.is_empty())
        {
            if !draft.assignees().contains(login) {
                draft.toggle_assignee(login);
            }
        }
        let (target, request) = draft.request(&title, &body).map_err(|error| match error {
            DraftError::NoRepository | DraftError::EmptyTitle(_) => {
                RostrumError::invalid(error.to_string())
            }
        })?;
        let client = self.actor.try_call(|state| state.github()).await?;
        let number = self
            .github(client.create_issue(&target, &request).await)
            .await?;
        tracing::info!(repo = %target, number = number.0, "issue created");
        self.after_issue_change(&target).await;
        Ok(number.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phone_closes_with_the_two_reasons_a_person_chooses() {
        assert_eq!(CloseAs::from(CloseIssueAs::Completed), CloseAs::Completed);
        assert_eq!(CloseAs::from(CloseIssueAs::NotPlanned), CloseAs::NotPlanned);
    }

    #[test]
    fn blank_input_names_what_is_missing() {
        assert_eq!(required(" bug ", "a label"), Ok("bug".to_string()));
        assert_eq!(
            required("  ", "a login"),
            Err(RostrumError::invalid("a login cannot be blank"))
        );
    }
}
