//! Desktop notifications for pull requests that appear between refreshes.
//!
//! The diff itself is pure and lives in `rostrum-core`: [`Baseline`] remembers
//! the pull request numbers each repository held on the previous observation
//! and reports the ones that were not there before. The phone's background
//! check uses the same one. Posting is deliberately fire-and-forget on a
//! background thread — headless machines and CI have no notification daemon,
//! and a missing D-Bus service must never stall or crash the UI.

use gpui::{App, Context, Entity, Subscription};
use rostrum_core::{ArrivalKind, Baseline, PrNumber, RepoId, RepoState};

use crate::sync::Store;

/// A pull request that was not present on the previous observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPr {
    pub repo: RepoId,
    pub number: PrNumber,
    pub title: String,
}

impl NewPr {
    fn summary(&self) -> String {
        format!("New PR in {}", self.repo)
    }

    fn body(&self) -> String {
        format!("#{} {}", self.number.0, self.title)
    }
}

/// Watches the store and posts a desktop notification per newly arrived pull
/// request. Held by the workspace; dropping it stops the notifications.
/// The pull requests that newly arrived since the previous observation. The
/// desktop announces arrivals only; review requests are the phone's concern.
fn arrivals(baseline: &mut Baseline, repos: &[RepoState]) -> Vec<NewPr> {
    baseline
        .observe(repos, None)
        .into_iter()
        .filter(|arrival| arrival.kind == ArrivalKind::Opened)
        .map(|arrival| NewPr {
            repo: arrival.repo,
            number: arrival.number,
            title: arrival.title,
        })
        .collect()
}

pub struct Notifier {
    baseline: Baseline,
    enabled: bool,
    _subscription: Subscription,
}

impl Notifier {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let enabled = store.read(cx).config.notifications;
        let mut baseline = Baseline::new();
        // Seed from whatever has already loaded so a notifier created mid-flight
        // does not announce the existing feed.
        arrivals(&mut baseline, &store.read(cx).state.repos);

        let subscription = cx.observe(&store, |this: &mut Self, store, cx| {
            let arrivals = arrivals(&mut this.baseline, &store.read(cx).state.repos);
            if this.enabled {
                for arrival in arrivals {
                    post(arrival, cx);
                }
            }
        });

        tracing::debug!(enabled, "notifier started");

        Self {
            baseline,
            enabled,
            _subscription: subscription,
        }
    }
}

/// Post one notification off the UI thread.
///
/// `notify-rust` talks to the platform notification service synchronously, so
/// this runs on the background executor and swallows every failure into a
/// `warn`: no daemon is a normal state, not an error worth surfacing.
fn post(new: NewPr, cx: &mut App) {
    let summary = new.summary();
    let body = new.body();

    cx.background_executor()
        .spawn(async move {
            let result = notify_rust::Notification::new()
                .appname("rostrum")
                .summary(&summary)
                .body(&body)
                .show();

            match result {
                Ok(handle) => drop(handle),
                Err(err) => tracing::warn!(error = %err, "could not post desktop notification"),
            }
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rostrum_core::{LoadState, MergeStateStatus, Mergeable, NodeId, PullRequest};

    fn pr(number: u32) -> PullRequest {
        PullRequest {
            number: PrNumber(number),
            node_id: NodeId(format!("PR_{}", number)),
            title: format!("PR {number}"),
            url: String::new(),
            is_draft: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: None,
            head_ref: "feature".into(),
            head_sha: "deadbeef".into(),
            base_ref: "main".into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: Mergeable::Unknown,
            merge_state: MergeStateStatus::Unknown,
            review_decision: None,
            assignees: Vec::new(),
            review_requests: Vec::new(),
            labels: Vec::new(),
            comment_count: 0,
            checks: None,
            base_divergence: None,
        }
    }

    fn loaded(name: &str, values: &[u32]) -> RepoState {
        RepoState {
            id: name.parse().expect("valid repo id"),
            prs: values.iter().copied().map(pr).collect(),
            load: LoadState::Loaded { at: Utc::now() },
            issues: Vec::new(),
            issues_load: LoadState::Idle,
            collapsed: false,
        }
    }

    // The diffing rules themselves are tested in `rostrum_core::arrivals`.

    #[test]
    fn baseline_stays_silent_on_first_load_then_reports_arrivals() {
        let mut baseline = Baseline::new();

        assert!(arrivals(&mut baseline, &[loaded("a/b", &[1, 2])]).is_empty());
        assert!(arrivals(&mut baseline, &[loaded("a/b", &[1, 2])]).is_empty());

        let new = arrivals(&mut baseline, &[loaded("a/b", &[1, 2, 5])]);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].number, PrNumber(5));
        assert_eq!(new[0].repo.to_string(), "a/b");
        assert_eq!(new[0].title, "PR 5");
    }

    #[test]
    fn notification_text_names_the_repo_and_the_pr() {
        let new = NewPr {
            repo: "owner/repo".parse().expect("valid repo id"),
            number: PrNumber(123),
            title: "Title".into(),
        };
        assert_eq!(new.summary(), "New PR in owner/repo");
        assert_eq!(new.body(), "#123 Title");
    }
}
