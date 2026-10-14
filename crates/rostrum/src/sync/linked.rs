//! Looking up a pull request a link names when the feed does not hold it.
//!
//! The feed lists open pull requests of watched repositories, up to
//! `prs_per_repo` each. A link from another app can name anything else; the
//! detail pane still needs the pull request's header, so it is fetched by
//! itself (`GitHubClient::pull_request`) and kept in [`Store::linked`]. The
//! poll does not refresh it; opening the link again retries a lookup that
//! failed.

use gpui::Context;
use gpui_tokio::Tokio;
use rostrum_core::{PrNumber, RepoId};

use super::{AuthStatus, Store};
use crate::link::linked::{Lookup, failure_message};

impl Store {
    /// Whether the feed already holds this pull request.
    pub fn feed_has_pull(&self, repo: &RepoId, number: PrNumber) -> bool {
        self.state
            .repo(repo)
            .is_some_and(|state| state.prs.iter().any(|pr| pr.number == number))
    }

    /// Look the pull request up on GitHub unless the feed holds it, or a
    /// lookup is already under way or has found it.
    pub fn look_up_pull(&mut self, repo: RepoId, number: PrNumber, cx: &mut Context<Self>) {
        if self.feed_has_pull(&repo, number) || !self.linked.begin(&repo, number) {
            return;
        }
        let Some(client) = self.client.clone() else {
            let reason = match &self.auth {
                AuthStatus::Failed { message } => message.clone(),
                AuthStatus::Resolving | AuthStatus::Ready { .. } => {
                    "not signed in to GitHub".to_string()
                }
            };
            self.linked.finish(
                &repo,
                number,
                Lookup::Failed(failure_message(&repo, number, &reason)),
            );
            cx.notify();
            return;
        };
        tracing::info!(%repo, number = number.0, "looking up a linked pull request");
        cx.notify();
        let fetch_repo = repo.clone();
        cx.spawn(async move |this, cx| {
            let outcome = Tokio::spawn(&*cx, async move {
                client.pull_request(&fetch_repo, number).await
            })
            .await;
            this.update(cx, |this, cx| {
                let lookup = match outcome {
                    Ok(Ok(pull)) => Lookup::Found(Box::new(pull)),
                    Ok(Err(error)) => {
                        tracing::warn!(%repo, number = number.0, %error, "linked pull request lookup failed");
                        Lookup::Failed(failure_message(&repo, number, &error.to_string()))
                    }
                    Err(error) => {
                        Lookup::Failed(failure_message(&repo, number, &error.to_string()))
                    }
                };
                this.linked.finish(&repo, number, lookup);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
