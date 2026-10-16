//! The poll as a whole: which lists to fetch for which repositories.

use chrono::{DateTime, Utc};

use super::{Cadence, Trigger, issues_due, paused};
use crate::{budget::Pause, model::RepoId, state::RepoState};

/// What the planner is told about the moment.
#[derive(Clone, Copy, Debug)]
pub struct PollContext<'a> {
    pub now: DateTime<Utc>,
    pub trigger: Trigger,
    pub cadence: Cadence,
    /// The Closed tab is showing, so its lists ride the poll.
    pub closed_shown: bool,
    /// A rate-limit pause, if one was recorded; ignored once it has run out.
    pub pause: Option<&'a Pause>,
}

/// The repositories each list is fetched for, in feed order. CI checks are
/// not here: they are judged after the pull requests land, against the
/// heads those return ([`super::plan_ci`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PollPlan {
    pub feed: Vec<RepoId>,
    pub issues: Vec<RepoId>,
    pub closed: Vec<RepoId>,
}

impl PollPlan {
    pub fn is_empty(&self) -> bool {
        self.feed.is_empty() && self.issues.is_empty() && self.closed.is_empty()
    }
}

/// Either nothing at all — GitHub is refusing — or a plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PollDecision {
    Paused(Pause),
    Run(PollPlan),
}

/// Plan one poll over `repos`.
pub fn plan_poll(repos: &[RepoState], context: &PollContext<'_>) -> PollDecision {
    if let Some(pause) = paused(context.pause, context.now) {
        return PollDecision::Paused(pause);
    }
    let ids = || repos.iter().map(|repo| repo.id.clone());
    PollDecision::Run(PollPlan {
        feed: ids().collect(),
        issues: repos
            .iter()
            .filter(|repo| {
                issues_due(
                    &repo.issues_load,
                    context.now,
                    context.cadence.issues,
                    context.trigger,
                )
            })
            .map(|repo| repo.id.clone())
            .collect(),
        closed: if context.closed_shown {
            ids().collect()
        } else {
            Vec::new()
        },
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        budget::{Jitter, RateLimitHit},
        state::LoadState,
    };

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid timestamp")
    }

    fn id(name: &str) -> RepoId {
        name.parse().expect("valid repo id")
    }

    fn repo(name: &str, issues: LoadState) -> RepoState {
        let mut repo = RepoState::new(id(name));
        repo.issues_load = issues;
        repo
    }

    fn context(now: i64, trigger: Trigger) -> PollContext<'static> {
        PollContext {
            now: at(now),
            trigger,
            cadence: Cadence::default(),
            closed_shown: false,
            pause: None,
        }
    }

    fn plan(repos: &[RepoState], context: &PollContext<'_>) -> PollPlan {
        match plan_poll(repos, context) {
            PollDecision::Run(plan) => plan,
            PollDecision::Paused(pause) => panic!("unexpectedly paused: {pause:?}"),
        }
    }

    #[test]
    fn every_poll_takes_the_feed_and_issues_only_when_due() {
        let repos = [
            repo("a/fresh", LoadState::Loaded { at: at(0) }),
            repo("a/stale", LoadState::Loaded { at: at(-300) }),
            repo("a/never", LoadState::Idle),
        ];
        let plan = plan(&repos, &context(60, Trigger::Timer));
        assert_eq!(plan.feed, vec![id("a/fresh"), id("a/stale"), id("a/never")]);
        assert_eq!(plan.issues, vec![id("a/stale"), id("a/never")]);
        assert!(plan.closed.is_empty());
    }

    #[test]
    fn an_explicit_refresh_takes_every_issue_list() {
        let repos = [repo("a/fresh", LoadState::Loaded { at: at(0) })];
        let plan = plan(&repos, &context(1, Trigger::Explicit));
        assert_eq!(plan.issues, vec![id("a/fresh")]);
    }

    #[test]
    fn closed_lists_ride_only_while_their_tab_shows() {
        let repos = [repo("a/b", LoadState::Idle)];
        let mut shown = context(0, Trigger::Timer);
        shown.closed_shown = true;
        assert_eq!(plan(&repos, &shown).closed, vec![id("a/b")]);
    }

    #[test]
    fn a_pause_in_force_plans_nothing_even_when_asked() {
        let pause = Pause::after(
            RateLimitHit::Primary { reset_at: at(600) },
            at(0),
            Jitter::new(Duration::from_secs(5)),
        );
        let repos = [repo("a/b", LoadState::Idle)];
        let mut asked = context(10, Trigger::Explicit);
        asked.pause = Some(&pause);
        assert_eq!(plan_poll(&repos, &asked), PollDecision::Paused(pause));

        let mut later = context(605, Trigger::Timer);
        later.pause = Some(&pause);
        assert_eq!(
            plan(&repos, &later).feed,
            vec![id("a/b")],
            "runs once it lifts"
        );
    }

    #[test]
    fn no_repositories_is_an_empty_plan() {
        assert!(plan(&[], &context(0, Trigger::Timer)).is_empty());
    }
}
