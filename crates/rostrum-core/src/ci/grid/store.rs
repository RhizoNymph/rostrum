//! The fetched checks the grid is built from: one [`RepoChecks`] per watched
//! repository, replaced wholesale on every fetch.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::{
    model::{PrNumber, RepoId},
    state::LoadState,
};

use super::super::model::PrChecks;

/// One repository's fetched checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoChecks {
    pub prs: BTreeMap<PrNumber, PrChecks>,
    pub load: LoadState,
}

impl Default for RepoChecks {
    fn default() -> Self {
        Self {
            prs: BTreeMap::new(),
            load: LoadState::Idle,
        }
    }
}

/// Every watched repository's checks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CiChecks {
    repos: BTreeMap<RepoId, RepoChecks>,
}

impl CiChecks {
    pub fn repo(&self, id: &RepoId) -> Option<&RepoChecks> {
        self.repos.get(id)
    }

    pub fn pr(&self, repo: &RepoId, number: PrNumber) -> Option<&PrChecks> {
        self.repos.get(repo)?.prs.get(&number)
    }

    pub fn pr_mut(&mut self, repo: &RepoId, number: PrNumber) -> Option<&mut PrChecks> {
        self.repos.get_mut(repo)?.prs.get_mut(&number)
    }

    /// A fetch is starting. Only a first load shows as loading; a refresh
    /// keeps what is on screen.
    pub fn begin(&mut self, repo: &RepoId) {
        let entry = self.repos.entry(repo.clone()).or_default();
        if entry.prs.is_empty() {
            entry.load = LoadState::Loading;
        }
    }

    /// A fetch answered: the repository's checks are replaced wholesale.
    pub fn loaded(&mut self, repo: &RepoId, checks: Vec<PrChecks>, at: DateTime<Utc>) {
        let entry = self.repos.entry(repo.clone()).or_default();
        entry.prs = checks.into_iter().map(|pr| (pr.number, pr)).collect();
        entry.load = LoadState::Loaded { at };
    }

    /// A fetch failed. Checks already held stay; the failure is recorded.
    pub fn failed(&mut self, repo: &RepoId, message: String, at: DateTime<Utc>) {
        self.repos.entry(repo.clone()).or_default().load = LoadState::Failed { message, at };
    }

    pub fn forget(&mut self, repo: &RepoId) {
        self.repos.remove(repo);
    }

    /// Whether anything anywhere is queued or running — what decides the
    /// grid's faster poll.
    pub fn any_running(&self) -> bool {
        self.repos
            .values()
            .flat_map(|repo| repo.prs.values())
            .any(|pr| pr.latest().values().any(|entry| entry.status.is_running()))
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::ci::model::{CheckEntry, CheckKey, CheckSource, CheckStatus};

    fn id(name: &str) -> RepoId {
        name.parse().expect("valid repo id")
    }

    fn checks(number: u32, status: CheckStatus) -> PrChecks {
        PrChecks {
            number: PrNumber(number),
            head_sha: "deadbeef".into(),
            entries: vec![CheckEntry {
                key: CheckKey::new(Some("CI".into()), "t"),
                status,
                started_at: None,
                completed_at: None,
                details_url: None,
                source: CheckSource::Actions {
                    job_id: u64::from(number),
                    run_id: 7,
                    run_attempt: 1,
                    suite_id: 1,
                },
            }],
            truncated: false,
        }
    }

    #[test]
    fn running_anywhere_is_known() {
        let mut ci = CiChecks::default();
        ci.loaded(
            &id("a/b"),
            vec![
                checks(1, CheckStatus::Failure),
                checks(2, CheckStatus::InProgress),
            ],
            Utc::now(),
        );
        assert!(ci.any_running());
        let mut quiet = CiChecks::default();
        quiet.loaded(
            &id("a/b"),
            vec![checks(1, CheckStatus::Success)],
            Utc::now(),
        );
        assert!(!quiet.any_running());
    }

    #[test]
    fn a_refresh_keeps_checks_on_screen_and_records_failures() {
        let mut ci = CiChecks::default();
        ci.loaded(
            &id("a/b"),
            vec![
                checks(1, CheckStatus::Success),
                checks(2, CheckStatus::Success),
            ],
            Utc::now(),
        );
        ci.begin(&id("a/b"));
        assert!(matches!(
            ci.repo(&id("a/b")).map(|r| &r.load),
            Some(LoadState::Loaded { .. })
        ));
        ci.failed(&id("a/b"), "boom".into(), Utc::now());
        assert_eq!(ci.repo(&id("a/b")).map(|r| r.prs.len()), Some(2));
        ci.begin(&id("new/repo"));
        assert_eq!(
            ci.repo(&id("new/repo")).map(|r| &r.load),
            Some(&LoadState::Loading)
        );
    }
}
