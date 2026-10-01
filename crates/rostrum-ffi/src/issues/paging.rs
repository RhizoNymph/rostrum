//! "Load earlier" on the issue screen: the page of comments and events
//! before the oldest held, merged by `rostrum_core::paging`'s rules (no
//! duplicates, chronological order) and cached with its cursors, so a later
//! launch continues from where the user left off.

use std::sync::Arc;

use super::{IssueDetail, render};
use crate::{
    engine::{RostrumCore, state::IssueKey},
    error::RostrumError,
};

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Fetch the page before the oldest comment and event held, merge it in,
    /// cache the result and return the detail. With nothing earlier, the
    /// held detail comes back without a request. The issue must have been
    /// opened (`issue_detail` or `cached_issue_detail`) first.
    pub async fn load_earlier_issue(
        &self,
        repo: String,
        number: u32,
    ) -> Result<IssueDetail, RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        let held = self.held_issue(&key).await?.ok_or_else(|| {
            RostrumError::invalid(format!("open {repo}#{number} before loading earlier"))
        })?;
        let request = held.conversation.earlier_request();
        if request.is_empty() {
            return render(&key, held, self.viewer_now().await?).await;
        }
        let client = self.actor.try_call(|state| state.github()).await?;
        let (page, update) = self
            .github(client.issue_earlier(&key.repo, key.number, &request).await)
            .await?;
        let mut merged = (*held).clone();
        merged.conversation.merge_earlier(page, &update);
        tracing::debug!(
            repo = %key.repo,
            number,
            items = merged.conversation.items.len(),
            remaining = merged.conversation.earlier_remaining(),
            "earlier issue page merged"
        );
        let merged = Arc::new(merged);
        self.keep_issue(&key, merged.clone()).await?;
        render(&key, merged, self.viewer_now().await?).await
    }
}
