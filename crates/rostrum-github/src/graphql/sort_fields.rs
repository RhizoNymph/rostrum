//! The feed query's fields that exist only to sort the feed: the
//! repository's own facts, and when each pull request's head was last pushed.
//!
//! Kept apart from the rest of the feed's wire types so the reasoning about
//! *which* GraphQL field stands in for "pushed" lives next to the code that
//! applies it.

use chrono::{DateTime, Utc};
use rostrum_core::{OwnerKind, RepoMeta, RepoOwner};
use serde::Deserialize;

/// The repository-level selections of `OPEN_PULL_REQUESTS`, flattened into
/// [`super::RepositoryNode`].
///
/// Every field is optional on the wire so a response from before they were
/// requested — a replayed fixture — still decodes; [`Self::into_domain`]
/// decides what is enough to call the metadata known.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoMetaNode {
    /// `null` for a repository nothing has ever been pushed to.
    pub pushed_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub stargazer_count: Option<u32>,
    pub owner: Option<OwnerNode>,
}

/// `Repository.owner`, the `RepositoryOwner` interface.
#[derive(Debug, Deserialize)]
pub struct OwnerNode {
    #[serde(rename = "__typename")]
    pub typename: Option<String>,
    pub login: String,
}

impl OwnerNode {
    /// `RepositoryOwner` is implemented by `Organization` and `User` only.
    /// Anything else — a type GitHub adds later — is treated as a user rather
    /// than failing the repository: the kind only labels the owner, and the
    /// sort is by login either way.
    fn into_domain(self) -> RepoOwner {
        let kind = match self.typename.as_deref() {
            Some("Organization") => OwnerKind::Organization,
            _ => OwnerKind::User,
        };
        RepoOwner {
            login: self.login,
            kind,
        }
    }
}

impl RepoMetaNode {
    /// The repository's metadata, or `None` when the response did not carry
    /// it. Only `pushedAt` may be absent from a complete answer.
    pub fn into_domain(self) -> Option<RepoMeta> {
        Some(RepoMeta {
            owner: self.owner?.into_domain(),
            pushed_at: self.pushed_at,
            created_at: self.created_at?,
            updated_at: self.updated_at?,
            stars: self.stargazer_count?,
        })
    }
}

/// One `HeadRefForcePushedEvent` from `timelineItems`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForcePushNode {
    /// Absent when GitHub hands back an event of another type, which the
    /// `itemTypes` filter should prevent but costs nothing to tolerate.
    pub created_at: Option<DateTime<Utc>>,
}

/// When a pull request's head branch last received commits.
///
/// GitHub's GraphQL API has no "pushed at" on a pull request, and
/// `Commit.pushedDate` has been removed. The two signals that remain each
/// miss a case the other covers:
///
/// - the head commit's `committedDate` moves with every new commit, amend
///   and rebase — but it is the committer's clock, so pushing a commit made
///   days ago reports days ago, and resetting a branch to an older commit
///   reports that commit's age;
/// - a `HeadRefForcePushedEvent`'s `createdAt` is GitHub's own clock at
///   push time — but there is one only for force pushes.
///
/// The later of the two is the best available answer. It is exact for a
/// force push and for an ordinary push of fresh commits; it can still lag
/// for a plain (non-force) push of commits authored well before the push.
pub fn head_pushed_at(
    head_committed: Option<DateTime<Utc>>,
    force_pushes: impl IntoIterator<Item = ForcePushNode>,
) -> Option<DateTime<Utc>> {
    force_pushes
        .into_iter()
        .filter_map(|event| event.created_at)
        .chain(head_committed)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::{GraphQlResponse, OPEN_PULL_REQUESTS, PrNode, RepoQueryData};
    use rostrum_core::{PrNumber, PullRequest};

    /// `cli/cli`, captured live with the current `OPEN_PULL_REQUESTS` and
    /// trimmed to three pull requests: one force-pushed after its last
    /// commit, one never force-pushed, and one whose force push predates its
    /// newest commit.
    const ORG_REPO: &str = include_str!("../fixtures/feed_org_repo.json");

    /// `RhizoNymph/rostrum`, captured live: a user-owned repository with no
    /// stars and no open pull requests.
    const USER_REPO: &str = include_str!("../fixtures/feed_user_repo.json");

    fn time(raw: &str) -> DateTime<Utc> {
        raw.parse().expect("valid timestamp")
    }

    fn decode(body: &str) -> (Option<RepoMeta>, Vec<PullRequest>) {
        let response: GraphQlResponse<RepoQueryData> =
            serde_json::from_str(body).expect("fixture should decode");
        assert!(response.errors.is_empty(), "{:?}", response.errors);
        let repository = response
            .data
            .expect("data present")
            .repository
            .expect("repository present");
        let meta = repository.meta.into_domain();
        let prs = repository
            .pull_requests
            .into_vec()
            .into_iter()
            .map(PrNode::into_domain)
            .collect();
        (meta, prs)
    }

    #[test]
    fn decodes_an_organization_repository() {
        let (meta, prs) = decode(ORG_REPO);
        let meta = meta.expect("metadata present");
        assert_eq!(
            meta.owner,
            RepoOwner {
                login: "cli".into(),
                kind: OwnerKind::Organization,
            }
        );
        assert_eq!(meta.pushed_at, Some(time("2026-09-30T02:40:01Z")));
        assert_eq!(meta.created_at, time("2019-10-03T15:24:53Z"));
        assert_eq!(meta.updated_at, time("2026-10-01T15:16:28Z"));
        assert_eq!(meta.stars, 46491);
        assert_eq!(prs.len(), 3);
    }

    #[test]
    fn decodes_a_user_repository_with_nothing_open() {
        let (meta, prs) = decode(USER_REPO);
        let meta = meta.expect("metadata present");
        assert_eq!(meta.owner.kind, OwnerKind::User);
        assert_eq!(meta.owner.login, "RhizoNymph");
        assert_eq!(meta.stars, 0);
        assert!(prs.is_empty());
    }

    fn pushed(prs: &[PullRequest], number: u32) -> Option<DateTime<Utc>> {
        prs.iter()
            .find(|pr| pr.number == PrNumber(number))
            .expect("pull request present")
            .pushed_at
    }

    #[test]
    fn a_force_push_after_the_last_commit_is_the_push_time() {
        let (_, prs) = decode(ORG_REPO);
        // Committed 09:22:39, force-pushed 09:22:59.
        assert_eq!(pushed(&prs, 13894), Some(time("2026-07-31T09:22:59Z")));
    }

    #[test]
    fn without_a_force_push_the_head_commit_date_is_the_push_time() {
        let (_, prs) = decode(ORG_REPO);
        assert_eq!(pushed(&prs, 14044), Some(time("2026-08-03T02:20:21Z")));
    }

    #[test]
    fn a_commit_newer_than_the_last_force_push_wins() {
        let (_, prs) = decode(ORG_REPO);
        // Force-pushed in May, committed again in September.
        assert_eq!(pushed(&prs, 13340), Some(time("2026-09-07T14:53:46Z")));
    }

    #[test]
    fn the_created_and_updated_times_decode() {
        let (_, prs) = decode(ORG_REPO);
        let pr = prs
            .iter()
            .find(|pr| pr.number == PrNumber(13340))
            .expect("present");
        assert_eq!(pr.created_at, time("2026-05-04T05:10:01Z"));
        assert_eq!(pr.updated_at, time("2026-09-07T15:10:48Z"));
    }

    /// A response from before these fields were requested still decodes, with
    /// the metadata unknown rather than invented.
    #[test]
    fn a_response_without_the_sort_fields_decodes_as_unknown() {
        let body = r#"{
          "data": {
            "repository": {
              "pullRequests": { "nodes": [{
                "id": "PR_1", "number": 1, "title": "t", "url": "u",
                "isDraft": false,
                "createdAt": "2026-07-30T10:00:00Z",
                "updatedAt": "2026-07-31T11:00:00Z",
                "author": null, "headRefName": "h", "baseRefName": "main",
                "additions": 0, "deletions": 0, "changedFiles": 0,
                "reviewDecision": null,
                "commits": { "nodes": [{ "commit": { "statusCheckRollup": null } }] }
              }] }
            }
          }
        }"#;
        let (meta, prs) = decode(body);
        assert!(meta.is_none());
        assert_eq!(prs[0].pushed_at, None);
    }

    #[test]
    fn a_never_pushed_repository_still_has_metadata() {
        let node: RepoMetaNode = serde_json::from_str(
            r#"{
              "pushedAt": null,
              "createdAt": "2026-01-01T00:00:00Z",
              "updatedAt": "2026-01-02T00:00:00Z",
              "stargazerCount": 3,
              "owner": { "__typename": "User", "login": "me" }
            }"#,
        )
        .expect("decodes");
        let meta = node.into_domain().expect("known");
        assert_eq!(meta.pushed_at, None);
        assert_eq!(meta.stars, 3);
    }

    #[test]
    fn an_unrecognised_owner_type_is_treated_as_a_user() {
        let owner = OwnerNode {
            typename: Some("Enterprise".into()),
            login: "x".into(),
        };
        assert_eq!(owner.into_domain().kind, OwnerKind::User);
    }

    #[test]
    fn the_push_time_is_the_latest_signal() {
        let event = |raw: &str| ForcePushNode {
            created_at: Some(time(raw)),
        };
        assert_eq!(head_pushed_at(None, []), None);
        assert_eq!(
            head_pushed_at(Some(time("2026-01-02T00:00:00Z")), []),
            Some(time("2026-01-02T00:00:00Z"))
        );
        assert_eq!(
            head_pushed_at(
                Some(time("2026-01-02T00:00:00Z")),
                [event("2026-01-03T00:00:00Z")]
            ),
            Some(time("2026-01-03T00:00:00Z"))
        );
        assert_eq!(
            head_pushed_at(None, [ForcePushNode { created_at: None }]),
            None
        );
    }

    #[test]
    fn the_feed_query_asks_for_every_sort_field() {
        for field in [
            "pushedAt",
            "stargazerCount",
            "owner { __typename login }",
            "committedDate",
            "HEAD_REF_FORCE_PUSHED_EVENT",
            "createdAt",
            "updatedAt",
        ] {
            assert!(OPEN_PULL_REQUESTS.contains(field), "missing {field}");
        }
    }
}
