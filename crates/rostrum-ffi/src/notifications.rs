//! Background notifications: what is new since the last check.

use rostrum_core::{Arrival, ArrivalKind, Baseline, LoginKey, User};

use crate::{
    engine::{
        RostrumCore,
        writer::{Write, settled},
    },
    error::RostrumError,
    feed::{Probes, Scope},
};

/// Something worth a system notification.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NotificationEvent {
    pub kind: NotificationKind,
    /// `owner/name`.
    pub repo: String,
    pub number: u32,
    pub title: String,
    pub author: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NotificationKind {
    /// A pull request appeared in a watched repository.
    NewPullRequest,
    /// Your review was requested on a pull request.
    ReviewRequested,
}

/// Which arrivals become notifications: filtered by the two settings, never
/// the viewer's own new pull requests, and at most one per pull request — a
/// review request when it is both, since that is the one asking for action.
pub(crate) fn events(
    arrivals: Vec<Arrival>,
    viewer: Option<&LoginKey>,
    new_pull_requests: bool,
    review_requests: bool,
) -> Vec<NotificationEvent> {
    let wanted: Vec<Arrival> = arrivals
        .into_iter()
        .filter(|arrival| match arrival.kind {
            ArrivalKind::Opened => {
                new_pull_requests
                    && !viewer.is_some_and(|viewer| {
                        arrival
                            .author
                            .as_ref()
                            .is_some_and(|author| &author.key() == viewer)
                    })
            }
            ArrivalKind::ReviewRequested => review_requests,
        })
        .collect();

    let mut out: Vec<NotificationEvent> = Vec::new();
    for arrival in wanted {
        let repo = arrival.repo.to_string();
        let kind = match arrival.kind {
            ArrivalKind::Opened => NotificationKind::NewPullRequest,
            ArrivalKind::ReviewRequested => NotificationKind::ReviewRequested,
        };
        if let Some(existing) = out
            .iter_mut()
            .find(|event| event.repo == repo && event.number == arrival.number.0)
        {
            if kind == NotificationKind::ReviewRequested {
                existing.kind = kind;
            }
            continue;
        }
        out.push(NotificationEvent {
            kind,
            repo,
            number: arrival.number.0,
            title: arrival.title,
            author: arrival.author.map(|author| author.login),
            url: arrival.url,
        });
    }
    out
}

impl RostrumCore {
    /// Read the seen set from SQLite into memory, once. A missing or corrupt
    /// one starts fresh: the next observation is then a baseline.
    async fn ensure_baseline(&self) -> Result<(), RostrumError> {
        if self.actor.call(|state| state.baseline.is_some()).await? {
            return Ok(());
        }
        let stored = match self.db.load_baseline().await {
            Ok(stored) => stored,
            Err(error) => {
                tracing::warn!(%error, "could not read the notification baseline; starting fresh");
                None
            }
        };
        self.actor
            .call(move |state| {
                if state.baseline.is_none() {
                    state.baseline = Some(stored.unwrap_or_default());
                }
            })
            .await
    }

    /// Fold the feed into the seen set, persist it, and return what arrived.
    async fn observe_arrivals(&self) -> Result<Vec<NotificationEvent>, RostrumError> {
        self.ensure_baseline().await?;
        let (events, ack) = self
            .actor
            .call(|state| {
                let viewer = state.session.viewer().map(User::key);
                let baseline = state.baseline.get_or_insert_with(Baseline::new);
                let arrivals = baseline.observe(&state.feed.repos, viewer.as_ref());
                let ack = state.writer.send_acked(Write::Baseline(baseline.clone()));
                let events = events(
                    arrivals,
                    viewer.as_ref(),
                    state.config.notifications,
                    state.config.notify_review_requests,
                );
                (events, ack)
            })
            .await?;
        settled(ack).await?;
        Ok(events)
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// For the WorkManager job: refresh the feed and report pull requests
    /// that appeared, and review requests that arrived, since the last check.
    /// The seen set is persisted, so the first check (and a newly watched
    /// repository's first) establishes a baseline and reports nothing. Events
    /// are filtered by the notification settings; your own pull requests are
    /// never reported, and a pull request is reported at most once per
    /// check, as a review request when it is both.
    pub async fn check_notifications(&self) -> Result<Vec<NotificationEvent>, RostrumError> {
        self.refresh(Scope::Background, Probes::Skip).await?;
        let events = self.observe_arrivals().await?;
        tracing::info!(events = events.len(), "notification check");
        Ok(events)
    }

    /// Fold the feed as it stands into the seen set without reporting
    /// anything — call when the user has looked at the feed, so the next
    /// check does not announce what they already saw.
    pub async fn mark_notifications_seen(&self) -> Result<(), RostrumError> {
        self.observe_arrivals().await.map(drop)
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{PrNumber, RepoId};

    use super::*;

    fn arrival(number: u32, kind: ArrivalKind, author: &str) -> Arrival {
        Arrival {
            repo: RepoId::new("a", "b"),
            number: PrNumber(number),
            title: format!("PR {number}"),
            url: format!("https://github.com/a/b/pull/{number}"),
            author: Some(User {
                login: author.into(),
                avatar_url: None,
            }),
            kind,
        }
    }

    fn summary(events: &[NotificationEvent]) -> Vec<(u32, NotificationKind)> {
        events
            .iter()
            .map(|event| (event.number, event.kind))
            .collect()
    }

    #[test]
    fn both_kinds_are_reported_when_both_are_on() {
        let events = events(
            vec![
                arrival(1, ArrivalKind::Opened, "bob"),
                arrival(2, ArrivalKind::ReviewRequested, "carol"),
            ],
            None,
            true,
            true,
        );
        assert_eq!(
            summary(&events),
            vec![
                (1, NotificationKind::NewPullRequest),
                (2, NotificationKind::ReviewRequested)
            ]
        );
        assert_eq!(events[0].author.as_deref(), Some("bob"));
        assert_eq!(events[0].repo, "a/b");
        assert_eq!(events[0].url, "https://github.com/a/b/pull/1");
    }

    #[test]
    fn each_setting_switches_its_kind_off() {
        let arrivals = || {
            vec![
                arrival(1, ArrivalKind::Opened, "bob"),
                arrival(2, ArrivalKind::ReviewRequested, "carol"),
            ]
        };
        assert_eq!(
            summary(&events(arrivals(), None, false, true)),
            vec![(2, NotificationKind::ReviewRequested)]
        );
        assert_eq!(
            summary(&events(arrivals(), None, true, false)),
            vec![(1, NotificationKind::NewPullRequest)]
        );
        assert!(events(arrivals(), None, false, false).is_empty());
    }

    #[test]
    fn your_own_new_pull_request_is_not_news() {
        let me = LoginKey::new("Me");
        let events = events(
            vec![
                arrival(1, ArrivalKind::Opened, "me"),
                arrival(2, ArrivalKind::Opened, "bob"),
            ],
            Some(&me),
            true,
            true,
        );
        assert_eq!(
            summary(&events),
            vec![(2, NotificationKind::NewPullRequest)]
        );
    }

    #[test]
    fn a_new_pull_request_asking_for_your_review_is_one_review_request() {
        let events = events(
            vec![
                arrival(3, ArrivalKind::Opened, "bob"),
                arrival(3, ArrivalKind::ReviewRequested, "bob"),
            ],
            None,
            true,
            true,
        );
        assert_eq!(
            summary(&events),
            vec![(3, NotificationKind::ReviewRequested)]
        );

        // With review requests off, the same pull request is still new.
        let quiet = super::events(
            vec![
                arrival(3, ArrivalKind::Opened, "bob"),
                arrival(3, ArrivalKind::ReviewRequested, "bob"),
            ],
            None,
            true,
            false,
        );
        assert_eq!(summary(&quiet), vec![(3, NotificationKind::NewPullRequest)]);
    }
}
