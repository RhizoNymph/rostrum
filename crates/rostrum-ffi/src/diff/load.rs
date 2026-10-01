//! Getting a pull request's changed files: memory, then SQLite, then GitHub.
//!
//! A diff is a pure function of its head commit, so every layer is keyed on
//! the head sha the feed reports. While the head has not moved the diff
//! cannot have changed, and once fetched it is never fetched again.

use std::sync::Arc;

use rostrum_core::{Conversation, ReviewThread};
use rostrum_diff::DiffFile;
use rostrum_github::PullRequestFile;

use crate::{
    engine::{RostrumCore, state::PullKey, writer::Write},
    error::RostrumError,
};

/// A pull request's files, parsed, as of one head commit.
pub(crate) struct LoadedFiles {
    pub head_sha: String,
    pub files: Vec<DiffFile>,
}

fn parse(files: Vec<PullRequestFile>) -> Vec<DiffFile> {
    files
        .into_iter()
        .map(|file| {
            DiffFile::from_patch(
                file.filename,
                file.previous_filename,
                &file.status,
                file.additions,
                file.deletions,
                file.patch.as_deref(),
            )
        })
        .collect()
}

impl RostrumCore {
    /// The files of `key` at its current head.
    pub(crate) async fn load_files(&self, key: &PullKey) -> Result<Arc<LoadedFiles>, RostrumError> {
        self.ensure_hydrated().await?;
        let lookup = key.clone();
        let (head, cached, client) = self
            .actor
            .try_call(move |state| {
                let head = state.known(&lookup)?.head_sha.clone();
                let cached = state
                    .files
                    .get(&lookup)
                    .filter(|loaded| loaded.head_sha == head)
                    .cloned();
                Ok((head, cached, state.session.client()))
            })
            .await?;
        if let Some(loaded) = cached {
            return Ok(loaded);
        }

        let stored = match self
            .db
            .load_pull_request_files(&key.repo, key.number, &head)
            .await
        {
            Ok(stored) => stored,
            Err(error) => {
                tracing::warn!(repo = %key.repo, number = key.number.0, %error, "could not read cached files");
                None
            }
        };
        let raw = match stored {
            Some(raw) => raw,
            None => {
                let client = client.ok_or(RostrumError::NotSignedIn)?;
                let fetched = self
                    .github(client.files(&key.repo, key.number).await)
                    .await?;
                let write = Write::Files {
                    repo: key.repo.clone(),
                    number: key.number,
                    head_sha: head.clone(),
                    files: fetched.clone(),
                };
                self.actor
                    .call(move |state| state.writer.send(write))
                    .await?;
                fetched
            }
        };

        let loaded = tokio::task::spawn_blocking(move || {
            Arc::new(LoadedFiles {
                head_sha: head,
                files: parse(raw),
            })
        })
        .await?;
        let store = key.clone();
        let kept = loaded.clone();
        self.actor
            .call(move |state| state.files.insert(store, kept))
            .await?;
        Ok(loaded)
    }

    /// The conversation last fetched for `key`, from memory or SQLite. Never
    /// fetches: the diff shows the threads it has, and `pull_detail` is what
    /// refreshes them.
    pub(crate) async fn known_conversation(
        &self,
        key: &PullKey,
    ) -> Result<Option<Arc<Conversation>>, RostrumError> {
        let lookup = key.clone();
        if let Some(found) = self
            .actor
            .call(move |state| state.conversations.get(&lookup).cloned())
            .await?
        {
            return Ok(Some(found));
        }
        let stored = match self.db.load_conversation(&key.repo, key.number).await {
            Ok(stored) => stored,
            Err(error) => {
                tracing::warn!(repo = %key.repo, number = key.number.0, %error, "could not read the cached conversation");
                None
            }
        };
        let Some(conversation) = stored else {
            return Ok(None);
        };
        let conversation = Arc::new(conversation);
        let store = key.clone();
        let kept = conversation.clone();
        self.actor
            .call(move |state| {
                // A fresher copy from the network wins over the cache.
                if state.conversations.get(&store).is_none() {
                    state.conversations.insert(store, kept);
                }
            })
            .await?;
        Ok(Some(conversation))
    }

    /// The threads `known_conversation` holds, or none.
    pub(crate) async fn known_threads(
        &self,
        key: &PullKey,
    ) -> Result<Vec<ReviewThread>, RostrumError> {
        Ok(self
            .known_conversation(key)
            .await?
            .map(|conversation| conversation.threads.clone())
            .unwrap_or_default())
    }
}
