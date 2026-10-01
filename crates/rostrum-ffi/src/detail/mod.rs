//! One pull request: its header, conversation, checks, labels, and the
//! pull-request-level actions.

mod actions;
mod header;
mod timeline;
mod types;

use std::sync::Arc;

use rostrum_core::{Conversation, User};

pub(crate) use header::header;
pub(crate) use timeline::{thread_view, timeline as render_timeline};
pub use types::{
    BranchUpdateMethod, CheckRunView, DraftAction, MergeMethod, MergeVerdict, PullDetail,
    PullHeader, ReviewThreadView, ThreadCommentView, TimelineEntry, TimelineEvent, TimelineKind,
};

use crate::{
    engine::{
        RostrumCore,
        state::{PullKey, parse_repo},
        writer::Write,
    },
    error::RostrumError,
    feed::count,
    types::LabelView,
};

/// Whether earlier pages remain, and how many entries they hold, as both
/// detail records report it.
pub(crate) fn earlier_of(conversation: &Conversation) -> (bool, Option<u32>) {
    if conversation.has_earlier() {
        (true, Some(conversation.earlier_remaining()))
    } else {
        (false, None)
    }
}

impl RostrumCore {
    /// Keep a conversation for `key` in memory and the cache.
    async fn keep_conversation(
        &self,
        key: &PullKey,
        conversation: Arc<Conversation>,
    ) -> Result<(), RostrumError> {
        let store = key.clone();
        self.actor
            .call(move |state| {
                state.writer.send(Write::Conversation {
                    repo: store.repo.clone(),
                    number: store.number,
                    conversation: (*conversation).clone(),
                });
                state.conversations.insert(store, conversation);
            })
            .await
    }

    /// Assemble the detail from a conversation, the feed's pull request and
    /// the pending review. Markdown is flattened off the caller's thread.
    async fn detail_from(
        &self,
        key: &PullKey,
        conversation: Arc<Conversation>,
    ) -> Result<PullDetail, RostrumError> {
        let pending_review = self.pending(key).await?;
        let lookup = key.clone();
        let (pr, viewer) = self
            .actor
            .try_call(move |state| {
                Ok((
                    state.known(&lookup)?.clone(),
                    state.session.viewer().map(User::key),
                ))
            })
            .await?;
        let repo = key.repo.clone();
        let (has_earlier, earlier_count) = earlier_of(&conversation);
        let detail = tokio::task::spawn_blocking(move || PullDetail {
            header: header(&repo, &pr, viewer.as_ref(), conversation.state),
            timeline: timeline::timeline(&conversation, &repo),
            threads: conversation
                .threads
                .iter()
                .map(|thread| thread_view(thread, &repo))
                .collect(),
            checks: conversation
                .checks
                .iter()
                .map(timeline::check_view)
                .collect(),
            unresolved_threads: count(conversation.unresolved_thread_count()),
            pending_review,
            has_earlier,
            earlier_count,
        })
        .await?;
        Ok(detail)
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Fetch the conversation, threads and checks from GitHub and cache them.
    pub async fn pull_detail(&self, repo: String, number: u32) -> Result<PullDetail, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        self.ensure_hydrated().await?;
        let lookup = key.clone();
        let client = self
            .actor
            .try_call(move |state| {
                state.known(&lookup)?;
                state.github()
            })
            .await?;
        let fetched = self
            .github(client.conversation(&key.repo, key.number).await)
            .await?;
        // The fetch is the newest page; earlier pages already loaded stay.
        let conversation = Arc::new(match self.known_conversation(&key).await? {
            Some(held) => held.refreshed_by(fetched),
            None => fetched,
        });
        self.keep_conversation(&key, conversation.clone()).await?;
        self.detail_from(&key, conversation).await
    }

    /// Fetch the page before the oldest held of every conversation part that
    /// has one, merge it in, cache the result and return the detail. With
    /// nothing earlier, the held detail comes back without a request. The
    /// pull request must have been opened (`pull_detail`) first.
    pub async fn load_earlier_pull(
        &self,
        repo: String,
        number: u32,
    ) -> Result<PullDetail, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        self.ensure_hydrated().await?;
        let held = self.known_conversation(&key).await?.ok_or_else(|| {
            RostrumError::invalid(format!("open {repo}#{number} before loading earlier"))
        })?;
        let request = held.earlier_request();
        if request.is_empty() {
            return self.detail_from(&key, held).await;
        }
        let client = self.actor.try_call(|state| state.github()).await?;
        let (page, update) = self
            .github(
                client
                    .conversation_earlier(&key.repo, key.number, &request)
                    .await,
            )
            .await?;
        let mut merged = (*held).clone();
        merged.merge_earlier(page, &update);
        tracing::debug!(repo = %key.repo, number, items = merged.items.len(), remaining = merged.earlier_remaining(), "earlier pull request page merged");
        let merged = Arc::new(merged);
        self.keep_conversation(&key, merged.clone()).await?;
        self.detail_from(&key, merged).await
    }

    /// The last fetched detail from the cache, or `None`. No network.
    pub async fn cached_pull_detail(
        &self,
        repo: String,
        number: u32,
    ) -> Result<Option<PullDetail>, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        self.ensure_hydrated().await?;
        let Some(conversation) = self.known_conversation(&key).await? else {
            return Ok(None);
        };
        self.detail_from(&key, conversation).await.map(Some)
    }

    /// Just the header, from the feed's data. No network.
    pub async fn pull_header(&self, repo: String, number: u32) -> Result<PullHeader, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        self.ensure_hydrated().await?;
        let state = self
            .known_conversation(&key)
            .await?
            .and_then(|conversation| conversation.state);
        self.actor
            .try_call(move |core| {
                let pr = core.known(&key)?;
                let viewer = core.session.viewer().map(User::key);
                Ok(header(&key.repo, pr, viewer.as_ref(), state))
            })
            .await
    }

    /// Every label defined on the repository, for the label picker. Fetched
    /// once per repository and kept.
    pub async fn repository_labels(&self, repo: String) -> Result<Vec<LabelView>, RostrumError> {
        let id = parse_repo(&repo)?;
        let lookup = id.clone();
        let (cached, client) = self
            .actor
            .call(move |state| (state.labels.get(&lookup).cloned(), state.session.client()))
            .await?;
        let labels = match cached {
            Some(labels) => labels,
            None => {
                let client = client.ok_or(RostrumError::NotSignedIn)?;
                let fetched = Arc::new(self.github(client.repository_labels(&id).await).await?);
                let kept = fetched.clone();
                self.actor
                    .call(move |state| {
                        state.labels.insert(id, kept);
                    })
                    .await?;
                fetched
            }
        };
        Ok(labels.iter().map(LabelView::from).collect())
    }
}
