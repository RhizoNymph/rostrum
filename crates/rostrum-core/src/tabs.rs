//! The feed's two lists — pull requests and issues — and how many of each a
//! filter lets through.

use serde::{Deserialize, Serialize};

use crate::{feed::FeedFilter, state::RepoState};

/// Which list the feed is showing.
///
/// Persisted in the config, so it is a closed set with a stable spelling: a
/// value written by a later build that this one does not know decodes as the
/// default rather than failing the whole file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedTab {
    #[default]
    PullRequests,
    Issues,
}

impl FeedTab {
    /// Left to right, as the tab bar draws them.
    pub const ALL: [Self; 2] = [Self::PullRequests, Self::Issues];

    pub fn label(self) -> &'static str {
        match self {
            Self::PullRequests => "Pull requests",
            Self::Issues => "Issues",
        }
    }

    /// Position in [`FeedTab::ALL`].
    pub fn index(self) -> usize {
        match self {
            Self::PullRequests => 0,
            Self::Issues => 1,
        }
    }

    pub fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or_default()
    }

    /// The tab to the right, staying put at the end. Like `j`/`k`, tab
    /// switching does not wrap: a held key must not cycle the feed.
    pub fn next(self) -> Self {
        Self::from_index((self.index() + 1).min(Self::ALL.len() - 1))
    }

    /// The tab to the left, staying put at the start.
    pub fn previous(self) -> Self {
        Self::from_index(self.index().saturating_sub(1))
    }
}

/// Open items per tab that the filter lets through, across every repository.
///
/// Counted over state rather than over feed rows, for the same reason the
/// filter bar's "N of M shown" is: a collapsed repository hides rows without
/// the filter having rejected anything, and a tab badge that shrank on
/// collapse would misreport what is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabCounts {
    pub pull_requests: usize,
    pub issues: usize,
}

impl TabCounts {
    pub fn get(self, tab: FeedTab) -> usize {
        match tab {
            FeedTab::PullRequests => self.pull_requests,
            FeedTab::Issues => self.issues,
        }
    }
}

pub fn tab_counts(repos: &[RepoState], filter: &FeedFilter) -> TabCounts {
    TabCounts {
        pull_requests: repos
            .iter()
            .flat_map(|repo| &repo.prs)
            .filter(|pr| filter.accepts(pr))
            .count(),
        issues: repos
            .iter()
            .flat_map(|repo| &repo.issues)
            .filter(|issue| filter.accepts_issue(issue))
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::Utc;

    use super::*;
    use crate::{
        feed::{Chrome, FeedRow, IssueIx, PrIx, RepoIx, flatten, flatten_tab},
        issue::Issue,
        model::{Label, LoginKey, User},
        state::LoadState,
        test_support::{issue, pull},
    };

    fn user(login: &str) -> User {
        User {
            login: login.into(),
            avatar_url: None,
        }
    }

    fn loaded() -> LoadState {
        LoadState::Loaded { at: Utc::now() }
    }

    /// A repository whose pull requests and issues have both loaded.
    fn repo(name: &str, prs: &[u32], issues: &[u32]) -> RepoState {
        let mut state = RepoState::new(name.parse().expect("valid repo id"));
        state.prs = prs.iter().copied().map(pull).collect();
        state.load = loaded();
        state.issues = issues.iter().copied().map(issue).collect();
        state.issues_load = loaded();
        state
    }

    fn issues_feed(repos: &[RepoState], filter: &FeedFilter) -> Vec<FeedRow> {
        flatten_tab(repos, filter, FeedTab::Issues).rows().to_vec()
    }

    // --- the tab itself -----------------------------------------------------

    #[test]
    fn tabs_step_without_wrapping() {
        assert_eq!(FeedTab::PullRequests.next(), FeedTab::Issues);
        assert_eq!(FeedTab::Issues.next(), FeedTab::Issues);
        assert_eq!(FeedTab::Issues.previous(), FeedTab::PullRequests);
        assert_eq!(FeedTab::PullRequests.previous(), FeedTab::PullRequests);
    }

    #[test]
    fn tab_indices_round_trip_and_default_to_pull_requests() {
        for tab in FeedTab::ALL {
            assert_eq!(FeedTab::from_index(tab.index()), tab);
        }
        assert_eq!(FeedTab::from_index(99), FeedTab::PullRequests);
        assert_eq!(FeedTab::default(), FeedTab::PullRequests);
    }

    #[test]
    fn the_tab_is_persisted_in_snake_case() {
        assert_eq!(
            serde_json::to_string(&FeedTab::PullRequests).expect("encodes"),
            r#""pull_requests""#
        );
        let back: FeedTab = serde_json::from_str(r#""issues""#).expect("decodes");
        assert_eq!(back, FeedTab::Issues);
    }

    // --- flattening issues --------------------------------------------------

    #[test]
    fn the_issues_tab_lists_issue_rows_inside_the_same_containers() {
        let rows = issues_feed(&[repo("a/b", &[1], &[7, 8])], &FeedFilter::default());
        assert_eq!(
            rows,
            vec![
                FeedRow::RepoHeader { repo: RepoIx(0) },
                FeedRow::IssueRow {
                    repo: RepoIx(0),
                    issue: IssueIx(0)
                },
                FeedRow::IssueRow {
                    repo: RepoIx(0),
                    issue: IssueIx(1)
                },
                FeedRow::Spacer { repo: RepoIx(0) },
            ]
        );
    }

    /// A tab's stream never mixes kinds: the pull request feed is unchanged by
    /// the repository also having issues.
    #[test]
    fn each_tab_holds_only_its_own_kind() {
        let repos = [repo("a/b", &[1, 2], &[3])];
        let filter = FeedFilter::default();
        let prs = flatten(&repos, &filter);
        assert!(
            !prs.rows()
                .iter()
                .any(|row| matches!(row, FeedRow::IssueRow { .. }))
        );
        assert_eq!(prs.tab(), FeedTab::PullRequests);
        assert_eq!(
            prs.row(1),
            Some(FeedRow::PrRow {
                repo: RepoIx(0),
                pr: PrIx(0),
                stack: None
            })
        );
        let issues = flatten_tab(&repos, &filter, FeedTab::Issues);
        assert_eq!(issues.tab(), FeedTab::Issues);
        assert!(
            !issues
                .rows()
                .iter()
                .any(|row| matches!(row, FeedRow::PrRow { .. }))
        );
    }

    /// Two feeds of identical notice rows built for different tabs must not
    /// compare equal, or switching tabs would skip the rebuild.
    #[test]
    fn feeds_for_different_tabs_are_never_equal() {
        let repos = [repo("a/b", &[], &[])];
        let filter = FeedFilter {
            hide_empty_repos: false,
            ..Default::default()
        };
        let prs = flatten_tab(&repos, &filter, FeedTab::PullRequests);
        let issues = flatten_tab(&repos, &filter, FeedTab::Issues);
        assert_eq!(prs.rows(), issues.rows());
        assert_ne!(prs, issues);
    }

    #[test]
    fn issue_chrome_wraps_each_run() {
        let feed = flatten_tab(
            &[repo("a/b", &[], &[1, 2]), repo("c/d", &[], &[3])],
            &FeedFilter::default(),
            FeedTab::Issues,
        );
        assert_eq!(feed.chrome(0), Chrome::Top);
        assert_eq!(feed.chrome(1), Chrome::Middle);
        assert_eq!(feed.chrome(2), Chrome::Bottom);
        assert_eq!(feed.chrome(3), Chrome::None);
        assert_eq!(feed.chrome(4), Chrome::Top);
        assert_eq!(feed.chrome(5), Chrome::Bottom);
    }

    /// Issues have their own load state: a repository whose pull requests
    /// loaded and whose issues are still in flight is loading on the Issues
    /// tab, and one whose issues failed shows the error there alone.
    #[test]
    fn the_issues_tab_follows_the_issues_load_state() {
        let filter = FeedFilter::default();
        let mut state = repo("a/b", &[1], &[]);

        state.issues_load = LoadState::Loading;
        assert_eq!(
            issues_feed(std::slice::from_ref(&state), &filter)[1],
            FeedRow::RepoLoading { repo: RepoIx(0) }
        );

        state.issues_load = LoadState::Idle;
        assert_eq!(
            issues_feed(std::slice::from_ref(&state), &filter)[1],
            FeedRow::RepoLoading { repo: RepoIx(0) }
        );

        state.issues_load = LoadState::Failed {
            message: "boom".into(),
            at: Utc::now(),
        };
        assert_eq!(
            issues_feed(std::slice::from_ref(&state), &filter)[1],
            FeedRow::RepoError { repo: RepoIx(0) }
        );
        // The pull request tab is untouched by the issues failing.
        assert_eq!(
            flatten(std::slice::from_ref(&state), &filter).row(1),
            Some(FeedRow::PrRow {
                repo: RepoIx(0),
                pr: PrIx(0),
                stack: None
            })
        );
    }

    /// A failed issue refresh with issues still held shows them rather than
    /// replacing the card with the error.
    #[test]
    fn failed_issues_with_stale_data_still_list() {
        let mut state = repo("a/b", &[], &[5]);
        state.issues_load = LoadState::Failed {
            message: "rate limited".into(),
            at: Utc::now(),
        };
        assert_eq!(
            issues_feed(&[state], &FeedFilter::default())[1],
            FeedRow::IssueRow {
                repo: RepoIx(0),
                issue: IssueIx(0)
            }
        );
    }

    #[test]
    fn repos_without_open_issues_are_hidden_on_the_issues_tab_only() {
        let repos = [repo("a/b", &[1], &[]), repo("c/d", &[], &[2])];
        let filter = FeedFilter::default();

        let issues = flatten_tab(&repos, &filter, FeedTab::Issues);
        assert_eq!(issues.hidden_repos(), 1);
        assert!(!issues.rows().iter().any(|row| row.repo() == RepoIx(0)));

        let prs = flatten(&repos, &filter);
        assert_eq!(prs.hidden_repos(), 1);
        assert!(!prs.rows().iter().any(|row| row.repo() == RepoIx(1)));
    }

    #[test]
    fn showing_empty_repos_restores_them_on_the_issues_tab() {
        let rows = issues_feed(
            &[repo("a/b", &[1], &[])],
            &FeedFilter {
                hide_empty_repos: false,
                ..Default::default()
            },
        );
        assert_eq!(rows[1], FeedRow::RepoEmpty { repo: RepoIx(0) });
    }

    #[test]
    fn a_collapsed_repo_contributes_header_and_spacer_on_the_issues_tab() {
        let mut state = repo("a/b", &[], &[1, 2, 3]);
        state.collapsed = true;
        assert_eq!(
            issues_feed(&[state], &FeedFilter::default()),
            vec![
                FeedRow::RepoHeader { repo: RepoIx(0) },
                FeedRow::Spacer { repo: RepoIx(0) },
            ]
        );
    }

    #[test]
    fn the_search_query_narrows_issues() {
        let mut state = repo("a/b", &[], &[1, 2]);
        state.issues[1].title = "Panic in the renderer".into();
        let rows = issues_feed(
            &[state],
            &FeedFilter {
                query: "renderer".into(),
                ..Default::default()
            },
        );
        assert_eq!(
            rows[1..rows.len() - 1],
            [FeedRow::IssueRow {
                repo: RepoIx(0),
                issue: IssueIx(1)
            }]
        );
    }

    /// Issues are addressed by their index in the unfiltered list, so a row
    /// resolves to its issue whatever the filter hid around it.
    #[test]
    fn issue_indices_address_the_unfiltered_vector() {
        let mut state = repo("a/b", &[], &[1, 2, 3]);
        state.issues[2].labels = vec![Label {
            name: "needle".into(),
            color: "000000".into(),
        }];
        let rows = issues_feed(
            &[state],
            &FeedFilter {
                query: "needle".into(),
                ..Default::default()
            },
        );
        assert_eq!(
            rows[1],
            FeedRow::IssueRow {
                repo: RepoIx(0),
                issue: IssueIx(2)
            }
        );
    }

    fn authored(number: u32, login: &str) -> Issue {
        Issue {
            author: Some(user(login)),
            ..issue(number)
        }
    }

    #[test]
    fn the_author_filter_applies_to_issue_authors() {
        let mut state = repo("a/b", &[], &[]);
        state.issues = vec![authored(1, "alice"), authored(2, "bob")];
        let filter = FeedFilter {
            authors: BTreeSet::from([LoginKey::new("Alice")]),
            ..Default::default()
        };
        assert!(filter.accepts_issue(&state.issues[0]));
        assert!(!filter.accepts_issue(&state.issues[1]));
        let rows = issues_feed(&[state], &filter);
        assert_eq!(
            rows[1..rows.len() - 1],
            [FeedRow::IssueRow {
                repo: RepoIx(0),
                issue: IssueIx(0)
            }]
        );
    }

    /// "Involved" for an issue is its assignees: checking the box reveals
    /// issues assigned to the selected person and never hides one.
    #[test]
    fn include_involved_widens_issues_to_assignees() {
        let assigned = Issue {
            assignees: vec![user("me")],
            ..authored(1, "alice")
        };
        let narrow = FeedFilter {
            authors: BTreeSet::from([LoginKey::new("me")]),
            ..Default::default()
        };
        assert!(!narrow.accepts_issue(&assigned));
        assert!(narrow.accepts_issue(&authored(2, "me")));

        let wide = FeedFilter {
            include_involved: true,
            ..narrow
        };
        assert!(wide.accepts_issue(&assigned));
        assert!(wide.accepts_issue(&authored(2, "me")));
        assert!(!wide.accepts_issue(&authored(3, "alice")));
    }

    /// Drafts are a pull request notion; hiding them must not touch issues.
    #[test]
    fn hiding_drafts_does_not_hide_issues() {
        let filter = FeedFilter {
            hide_drafts: true,
            ..Default::default()
        };
        assert!(filter.accepts_issue(&issue(1)));
    }

    #[test]
    fn filtering_every_issue_out_hides_the_repo() {
        let feed = flatten_tab(
            &[repo("a/b", &[1], &[1, 2])],
            &FeedFilter {
                authors: BTreeSet::from([LoginKey::new("nobody")]),
                ..Default::default()
            },
            FeedTab::Issues,
        );
        assert!(feed.is_empty());
        assert_eq!(feed.hidden_repos(), 1);
    }

    // --- tab counts ---------------------------------------------------------

    #[test]
    fn tab_counts_report_open_items_after_filters() {
        let mut first = repo("a/b", &[1, 2], &[1, 2, 3]);
        first.prs[1].is_draft = true;
        first.issues[0].author = Some(user("alice"));
        let second = repo("c/d", &[3], &[4]);
        let repos = [first, second];

        assert_eq!(
            tab_counts(&repos, &FeedFilter::default()),
            TabCounts {
                pull_requests: 3,
                issues: 4
            }
        );

        let counts = tab_counts(
            &repos,
            &FeedFilter {
                hide_drafts: true,
                ..Default::default()
            },
        );
        assert_eq!(counts.get(FeedTab::PullRequests), 2);
        assert_eq!(counts.get(FeedTab::Issues), 4);

        let counts = tab_counts(
            &repos,
            &FeedFilter {
                authors: BTreeSet::from([LoginKey::new("alice")]),
                ..Default::default()
            },
        );
        assert_eq!(
            counts,
            TabCounts {
                pull_requests: 0,
                issues: 1
            }
        );
    }

    /// Collapsing hides rows, not open items, so the badge must not shrink.
    #[test]
    fn tab_counts_ignore_collapse() {
        let mut state = repo("a/b", &[1], &[1, 2]);
        state.collapsed = true;
        assert_eq!(
            tab_counts(&[state], &FeedFilter::default()),
            TabCounts {
                pull_requests: 1,
                issues: 2
            }
        );
    }
}
