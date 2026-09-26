//! Domain types describing repositories and pull requests.

use std::{fmt, num::NonZeroU32, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A repository identifier, always in `owner/name` form.
///
/// Construction is validated, so a `RepoId` in hand is always well-formed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RepoId {
    owner: String,
    name: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseRepoIdError {
    #[error("expected `owner/name`, got `{0}`")]
    Shape(String),
    #[error("`{0}` has an empty owner or name")]
    EmptyComponent(String),
}

impl RepoId {
    /// Build from already-validated parts.
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
        }
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl FromStr for RepoId {
    type Err = ParseRepoIdError;

    /// Accepts `owner/name`, and also tolerates a pasted GitHub URL or a
    /// trailing `.git`, since both are common when adding a repo by hand.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let without_scheme = trimmed
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("github.com/")
            .trim_start_matches("www.github.com/");
        let without_suffix = without_scheme
            .trim_end_matches('/')
            .trim_end_matches(".git");

        let mut parts = without_suffix.split('/');
        let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(ParseRepoIdError::Shape(trimmed.to_string()));
        };

        if owner.is_empty() || name.is_empty() {
            return Err(ParseRepoIdError::EmptyComponent(trimmed.to_string()));
        }

        Ok(Self::new(owner, name))
    }
}

impl fmt::Display for RepoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// A pull request number, unique within its repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PrNumber(pub u32);

impl fmt::Display for PrNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A GitHub GraphQL node identifier: opaque, and stable for the life of the
/// object it names.
///
/// Kept distinct from [`PrNumber`] because the two are not interchangeable. The
/// number is what a human types and what REST paths are built from; the node id
/// is the only handle GraphQL mutations accept. Confusing them is a runtime
/// error GitHub reports late, so the type system separates them here.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for NodeId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub login: String,
    pub avatar_url: Option<String>,
}

impl User {
    pub fn key(&self) -> LoginKey {
        LoginKey::new(&self.login)
    }
}

/// A GitHub login folded to its comparison form.
///
/// GitHub logins are case-insensitive: `RhizoNymph` and `rhizonymph` are the
/// same account, and the API is free to echo either casing back. Every
/// comparison and every persisted selection therefore goes through this type
/// rather than through a bare `String`, so "did the caller remember to
/// lowercase?" is not a question any call site can get wrong.
///
/// The display casing is deliberately *not* carried here. It belongs to the
/// `User` the login came from, which is what the UI renders; a key is only
/// ever an identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct LoginKey(String);

/// Normalising on the way in rather than deriving `Deserialize` is what makes
/// the invariant hold for *every* source, including the hand-edited config
/// file where `"RhizoNymph"` is what a person would naturally type.
impl<'de> Deserialize<'de> for LoginKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::new(&String::deserialize(deserializer)?))
    }
}

impl LoginKey {
    pub fn new(raw: &str) -> Self {
        Self(raw.trim().to_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A login that is all whitespace identifies nobody. Config is
    /// hand-editable, so this is reachable without a bug.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for LoginKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Aggregate review state. `None` on the wire means "no decision yet", which we
/// represent as `Option::None` rather than an extra variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

/// Rolled-up CI state for the head commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckState {
    Expected,
    Error,
    Failure,
    Pending,
    Success,
}

impl CheckState {
    /// Whether this state should block a merge affordance.
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Error | Self::Failure)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Mergeable {
    Mergeable,
    Conflicting,
    /// GitHub is still computing the merge state; not an error.
    #[default]
    Unknown,
}

/// GitHub's `mergeStateStatus`: *why* a pull request can or cannot be merged.
///
/// `mergeable` only distinguishes "the trees combine" from "they do not". This
/// is the field that separates a branch blocked by protection rules from one
/// that is merely behind its base, which is the distinction a reviewer acts on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeStateStatus {
    /// The head is out of date with the base and protection requires it be
    /// current before merging.
    Behind,
    /// Blocked by branch protection: a required review, a required check, or a
    /// code-owner approval is missing.
    Blocked,
    /// Mergeable, with everything passing.
    Clean,
    /// Conflicts with the base.
    Dirty,
    Draft,
    /// Mergeable, but a pre-receive hook will run.
    HasHooks,
    /// Not yet computed, or not visible to this token.
    #[default]
    Unknown,
    /// Mergeable, but the checks are failing or still running.
    Unstable,
}

/// What the UI says about a pull request's mergeability.
///
/// Derived from `mergeable` and `mergeStateStatus` together in one place, so
/// the feed chip, the merge button, and its tooltip cannot disagree about
/// whether a pull request can be merged or why it cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeStatus {
    /// GitHub computes merge state lazily; the first query after a change
    /// returns `UNKNOWN` and kicks off the computation.
    Computing,
    Conflicts,
    Draft,
    Blocked,
    Behind,
    /// Merging is permitted, but CI is not green.
    Unstable,
    Ready,
}

impl MergeStatus {
    /// Whether the merge affordance must be disabled.
    ///
    /// `Unstable` does not block: GitHub itself allows merging with red checks
    /// unless protection forbids it, and when it does forbid it the state comes
    /// back as `Blocked` instead. `Computing` blocks because acting on a state
    /// GitHub has not finished computing is how you merge a conflict.
    pub fn blocks_merge(self) -> bool {
        matches!(
            self,
            Self::Computing | Self::Conflicts | Self::Draft | Self::Blocked | Self::Behind
        )
    }

    /// Short chip text, or `None` for states not worth a chip of their own.
    ///
    /// `Draft` and `Unstable` are omitted because both surfaces that show this
    /// chip already carry a draft chip and a CI indicator; repeating them would
    /// be noise.
    pub fn chip(self) -> Option<&'static str> {
        match self {
            Self::Conflicts => Some("conflict"),
            Self::Behind => Some("behind"),
            Self::Blocked => Some("blocked"),
            Self::Computing | Self::Draft | Self::Unstable | Self::Ready => None,
        }
    }

    /// One line explaining the state, used as button tooltip and detail text.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::Computing => "GitHub is still computing the merge state",
            Self::Conflicts => "This branch has conflicts that must be resolved",
            Self::Draft => "This pull request is still a draft",
            Self::Blocked => "Blocked by branch protection: a required review or check is missing",
            Self::Behind => "The base branch has moved; this branch must be updated first",
            Self::Unstable => "Mergeable, but the checks are not green",
            Self::Ready => "Ready to merge",
        }
    }
}

/// How far two commits have drifted apart, in commits each carries that the
/// other does not.
///
/// Always both counts. "Behind by 3" alone cannot distinguish a branch that
/// will fast-forward from one that has also moved on, and those two want
/// different buttons — so the pair is the unit of currency, and a caller
/// cannot read one while forgetting the other.
///
/// Direction is fixed by the order of the arguments that produced it: `ahead`
/// counts what the left side has, `behind` what the right side has. For a pull
/// request the left is the head branch and the right is whatever it is being
/// compared against, so `behind` is always "work this branch has not caught up
/// with".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Divergence {
    pub ahead: u32,
    pub behind: u32,
}

/// The verdict a caller acts on, projected from a [`Divergence`].
///
/// `NonZeroU32` is what keeps the projection honest: `Ahead(0)` is not
/// constructible, so a match arm can never be reached with a count that
/// contradicts the variant it arrived in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Identical,
    Ahead(NonZeroU32),
    Behind(NonZeroU32),
    Diverged {
        ahead: NonZeroU32,
        behind: NonZeroU32,
    },
}

impl Divergence {
    pub const IDENTICAL: Self = Self {
        ahead: 0,
        behind: 0,
    };

    pub const fn new(ahead: u32, behind: u32) -> Self {
        Self { ahead, behind }
    }

    pub fn relation(self) -> Relation {
        match (NonZeroU32::new(self.ahead), NonZeroU32::new(self.behind)) {
            (None, None) => Relation::Identical,
            (Some(ahead), None) => Relation::Ahead(ahead),
            (None, Some(behind)) => Relation::Behind(behind),
            (Some(ahead), Some(behind)) => Relation::Diverged { ahead, behind },
        }
    }

    /// Whether catching up is a fast-forward — nothing of this branch's own
    /// would be rewritten, so merge and rebase would produce the same result.
    pub fn fast_forwards(self) -> bool {
        self.behind > 0 && self.ahead == 0
    }

    pub fn is_identical(self) -> bool {
        self.ahead == 0 && self.behind == 0
    }

    /// Whether there is anything to catch up with. The buttons that act on a
    /// divergence are pointless without it.
    pub fn is_behind(self) -> bool {
        self.behind > 0
    }
}

/// Which side of a diff a line or comment belongs to.
///
/// GitHub's review-comment API anchors by `path` + `line` + `side`, where
/// `RIGHT` is the new file and `LEFT` the old one. Getting this wrong puts
/// comments on the wrong lines of someone else's pull request, so it is a
/// first-class type rather than a bool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
    /// Six-digit hex, no leading `#`, as GitHub returns it.
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: PrNumber,
    /// Handle for the GraphQL mutations that have no REST equivalent, draft
    /// conversion among them. Fetched with the feed query rather than looked up
    /// per mutation, so toggling draft state is one round trip.
    pub node_id: NodeId,
    pub title: String,
    pub url: String,
    pub is_draft: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub author: Option<User>,
    pub head_ref: String,
    /// Commit the pull request currently points at. Draft review comments are
    /// tagged with this so a force-push can invalidate them rather than
    /// silently anchoring to lines that have moved.
    pub head_sha: String,
    pub base_ref: String,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub mergeable: Mergeable,
    /// Defaulted so pull requests cached before this field existed still
    /// decode; a stale `Unknown` costs one refresh, a failed decode costs the
    /// whole cold-start cache.
    #[serde(default)]
    pub merge_state: MergeStateStatus,
    pub review_decision: Option<ReviewDecision>,
    /// Who the pull request is assigned to.
    ///
    /// Defaulted for the same reason `merge_state` is: a pull request cached
    /// before involvement was fetched must still decode. An empty list then
    /// costs one refresh, where a failed decode would cost the whole cold-start
    /// cache.
    #[serde(default)]
    pub assignees: Vec<User>,
    /// Who has an outstanding review request on the pull request.
    ///
    /// Team review requests are dropped on the way in: a team has a name, not
    /// a login, and the author filter matches people.
    #[serde(default)]
    pub review_requests: Vec<User>,
    pub labels: Vec<Label>,
    pub comment_count: u32,
    pub checks: Option<CheckState>,
    /// How far this branch has drifted from its base, from a follow-up query
    /// issued after the feed refresh. Defaulted for the same reason as
    /// `merge_state`: rows cached before it existed must still decode.
    #[serde(default)]
    pub base_divergence: Option<Divergence>,
}

impl PullRequest {
    /// Collapse `mergeable` and `merge_state` into the single verdict the UI
    /// renders.
    ///
    /// Order matters. Conflicts win over everything, because that is the one
    /// state the author must act on regardless of protection rules. A draft is
    /// reported as such even while the merge state is still being computed —
    /// that is both certain and more useful than "computing".
    pub fn merge_status(&self) -> MergeStatus {
        match (self.mergeable, self.merge_state) {
            (Mergeable::Conflicting, _) | (_, MergeStateStatus::Dirty) => MergeStatus::Conflicts,
            _ if self.is_draft => MergeStatus::Draft,
            (_, MergeStateStatus::Draft) => MergeStatus::Draft,
            (Mergeable::Unknown, _) => MergeStatus::Computing,
            (_, MergeStateStatus::Blocked) => MergeStatus::Blocked,
            (_, MergeStateStatus::Behind) => MergeStatus::Behind,
            (_, MergeStateStatus::Unstable) => MergeStatus::Unstable,
            // CLEAN, HAS_HOOKS, and a mergeable branch whose merge state this
            // token cannot see. `mergeStateStatus` is documented as needing
            // push access; when it is withheld the branch is still reported
            // mergeable, and calling that "computing" forever would be worse
            // than calling it ready.
            (Mergeable::Mergeable, _) => MergeStatus::Ready,
        }
    }

    /// Whether `login` opened this pull request.
    pub fn is_authored_by(&self, login: &LoginKey) -> bool {
        self.author
            .as_ref()
            .is_some_and(|author| &author.key() == login)
    }

    /// Whether `login` is on the hook for this pull request: they opened it,
    /// it is assigned to them, or their review has been requested.
    ///
    /// Deliberately *not* GitHub's full `involves:` qualifier, which also
    /// counts commenting and being mentioned. Those are traces of having looked
    /// at a pull request; these three are the ones that mean it is waiting on
    /// you, which is what the filter is for.
    pub fn involves(&self, login: &LoginKey) -> bool {
        self.is_authored_by(login)
            || self
                .assignees
                .iter()
                .chain(&self.review_requests)
                .any(|user| &user.key() == login)
    }

    /// Text used for feed filtering. Kept here so filtering and display can
    /// never drift apart.
    pub fn matches_query(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_lowercase();
        self.title.to_lowercase().contains(&needle)
            || self.number.0.to_string().contains(&needle)
            || self
                .author
                .as_ref()
                .is_some_and(|a| a.login.to_lowercase().contains(&needle))
            || self
                .labels
                .iter()
                .any(|l| l.name.to_lowercase().contains(&needle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_owner_name() {
        let id: RepoId = "zed-industries/zed".parse().expect("should parse");
        assert_eq!(id.owner(), "zed-industries");
        assert_eq!(id.name(), "zed");
        assert_eq!(id.to_string(), "zed-industries/zed");
    }

    #[test]
    fn parses_pasted_url_forms() {
        for input in [
            "https://github.com/zed-industries/zed",
            "https://github.com/zed-industries/zed.git",
            "https://github.com/zed-industries/zed/",
            "github.com/zed-industries/zed",
            "  zed-industries/zed  ",
        ] {
            let id: RepoId = input.parse().unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(id.to_string(), "zed-industries/zed", "input: {input}");
        }
    }

    #[test]
    fn rejects_malformed() {
        assert!("zed".parse::<RepoId>().is_err());
        assert!("a/b/c".parse::<RepoId>().is_err());
        assert!("/zed".parse::<RepoId>().is_err());
        assert!("zed/".parse::<RepoId>().is_err());
        assert!("".parse::<RepoId>().is_err());
    }

    // --- divergence ---------------------------------------------------------

    /// The full cross product, stated as a table so a change to the projection
    /// has to be a deliberate edit here.
    #[test]
    fn projects_counts_onto_a_relation() {
        let nz = |n: u32| NonZeroU32::new(n).expect("non-zero");
        let cases = [
            (0, 0, Relation::Identical),
            (3, 0, Relation::Ahead(nz(3))),
            (0, 7, Relation::Behind(nz(7))),
            (
                2,
                5,
                Relation::Diverged {
                    ahead: nz(2),
                    behind: nz(5),
                },
            ),
        ];
        for (ahead, behind, expected) in cases {
            assert_eq!(
                Divergence::new(ahead, behind).relation(),
                expected,
                "ahead {ahead}, behind {behind}"
            );
        }
    }

    /// Only a branch with nothing of its own fast-forwards. A branch that is
    /// both ahead and behind must not be offered a plain merge as if it were
    /// free.
    #[test]
    fn only_a_purely_behind_branch_fast_forwards() {
        assert!(Divergence::new(0, 4).fast_forwards());
        assert!(!Divergence::new(1, 4).fast_forwards());
        assert!(!Divergence::new(0, 0).fast_forwards());
        assert!(!Divergence::new(3, 0).fast_forwards());
    }

    #[test]
    fn identical_is_neither_ahead_nor_behind() {
        assert!(Divergence::IDENTICAL.is_identical());
        assert!(!Divergence::IDENTICAL.is_behind());
        assert_eq!(Divergence::default(), Divergence::IDENTICAL);
        assert!(!Divergence::new(0, 1).is_identical());
        assert!(Divergence::new(0, 1).is_behind());
        assert!(!Divergence::new(1, 0).is_behind());
    }

    fn pr(title: &str, author: &str) -> PullRequest {
        PullRequest {
            number: PrNumber(1),
            node_id: NodeId("PR_kwDOAbc".into()),
            title: title.into(),
            url: String::new(),
            is_draft: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: Some(User {
                login: author.into(),
                avatar_url: None,
            }),
            head_ref: "feature".into(),
            head_sha: "abc123".into(),
            base_ref: "main".into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: Mergeable::Unknown,
            merge_state: MergeStateStatus::Unknown,
            review_decision: None,
            assignees: Vec::new(),
            review_requests: Vec::new(),
            labels: vec![Label {
                name: "bug".into(),
                color: "d73a4a".into(),
            }],
            comment_count: 0,
            checks: None,
            base_divergence: None,
        }
    }

    // --- involvement --------------------------------------------------------

    /// GitHub logins are case-insensitive and the API echoes back whatever
    /// casing it stored, so a filter that only matched exact spelling would
    /// silently show nothing.
    #[test]
    fn authorship_ignores_the_casing_of_the_login() {
        let pr = pr("t", "RhizoNymph");
        assert!(pr.is_authored_by(&LoginKey::new("rhizonymph")));
        assert!(pr.is_authored_by(&LoginKey::new("RHIZONYMPH")));
        assert!(!pr.is_authored_by(&LoginKey::new("someone-else")));
    }

    #[test]
    fn a_pull_request_with_no_author_is_authored_by_nobody() {
        let pr = PullRequest {
            author: None,
            ..pr("t", "a")
        };
        assert!(!pr.is_authored_by(&LoginKey::new("a")));
        assert!(!pr.involves(&LoginKey::new("a")));
    }

    #[test]
    fn involvement_covers_author_assignee_and_requested_reviewer() {
        let pr = PullRequest {
            assignees: vec![User {
                login: "Assignee".into(),
                avatar_url: None,
            }],
            review_requests: vec![User {
                login: "Reviewer".into(),
                avatar_url: None,
            }],
            ..pr("t", "Author")
        };

        for login in ["author", "assignee", "reviewer"] {
            assert!(pr.involves(&LoginKey::new(login)), "{login}");
        }
        assert!(!pr.involves(&LoginKey::new("bystander")));
    }

    /// Involvement is strictly wider than authorship: everything authorship
    /// accepts, it must accept too.
    #[test]
    fn involvement_never_rejects_what_authorship_accepts() {
        let pr = pr("t", "author");
        let key = LoginKey::new("author");
        assert!(pr.is_authored_by(&key));
        assert!(pr.involves(&key));
    }

    /// Being assigned is not authoring. The narrow filter has to stay narrow.
    #[test]
    fn an_assignee_is_not_an_author() {
        let pr = PullRequest {
            assignees: vec![User {
                login: "assignee".into(),
                avatar_url: None,
            }],
            ..pr("t", "author")
        };
        assert!(!pr.is_authored_by(&LoginKey::new("assignee")));
        assert!(pr.involves(&LoginKey::new("assignee")));
    }

    /// Config is hand-editable, so a blank entry is reachable without a bug.
    /// It must identify nobody rather than matching the first empty login.
    #[test]
    fn a_blank_login_is_recognised_as_empty() {
        assert!(LoginKey::new("   ").is_empty());
        assert!(!LoginKey::new("a").is_empty());
        assert_eq!(LoginKey::new("  Alice  ").as_str(), "alice");
    }

    /// The config file is hand-edited, so a login typed in its display casing
    /// must normalise on the way in rather than becoming a key that matches
    /// nobody.
    #[test]
    fn a_login_normalises_however_it_is_deserialised() {
        let key: LoginKey = serde_json::from_str(r#""RhizoNymph""#).expect("should decode");
        assert_eq!(key, LoginKey::new("rhizonymph"));
        assert_eq!(
            serde_json::to_string(&key).expect("should encode"),
            r#""rhizonymph""#
        );
    }

    #[test]
    fn query_matches_title_author_and_label_case_insensitively() {
        let pr = pr("Fix the Thing", "RhizoNymph");
        assert!(pr.matches_query(""));
        assert!(pr.matches_query("thing"));
        assert!(pr.matches_query("rhizo"));
        assert!(pr.matches_query("BUG"));
        assert!(pr.matches_query("1"));
        assert!(!pr.matches_query("absent"));
    }

    fn with_merge(
        mergeable: Mergeable,
        merge_state: MergeStateStatus,
        is_draft: bool,
    ) -> PullRequest {
        PullRequest {
            is_draft,
            mergeable,
            merge_state,
            ..pr("t", "a")
        }
    }

    /// The full cross product that matters, stated as a table so a change to
    /// the derivation has to be a deliberate edit here.
    #[test]
    fn derives_merge_status_from_both_fields() {
        use MergeStateStatus as S;
        use MergeStatus as M;

        let cases = [
            // Conflicts win regardless of which field reports them.
            (Mergeable::Conflicting, S::Dirty, false, M::Conflicts),
            (Mergeable::Conflicting, S::Unknown, false, M::Conflicts),
            (Mergeable::Mergeable, S::Dirty, false, M::Conflicts),
            // ...including over a draft, which the author must still fix.
            (Mergeable::Conflicting, S::Draft, true, M::Conflicts),
            // Draft is certain even while the merge state is not.
            (Mergeable::Unknown, S::Unknown, true, M::Draft),
            (Mergeable::Mergeable, S::Draft, false, M::Draft),
            // Nothing known yet.
            (Mergeable::Unknown, S::Unknown, false, M::Computing),
            (Mergeable::Unknown, S::Blocked, false, M::Computing),
            // The states that say why merging is refused.
            (Mergeable::Mergeable, S::Blocked, false, M::Blocked),
            (Mergeable::Mergeable, S::Behind, false, M::Behind),
            (Mergeable::Mergeable, S::Unstable, false, M::Unstable),
            // Green, and the two ways of being green without saying so.
            (Mergeable::Mergeable, S::Clean, false, M::Ready),
            (Mergeable::Mergeable, S::HasHooks, false, M::Ready),
            (Mergeable::Mergeable, S::Unknown, false, M::Ready),
        ];

        for (mergeable, state, draft, want) in cases {
            let got = with_merge(mergeable, state, draft).merge_status();
            assert_eq!(got, want, "{mergeable:?} + {state:?} (draft: {draft})");
        }
    }

    /// Only CI-red and green may be merged from the app; everything else is a
    /// state GitHub itself would refuse or that we cannot yet vouch for.
    #[test]
    fn only_unstable_and_ready_permit_merging() {
        for status in [
            MergeStatus::Computing,
            MergeStatus::Conflicts,
            MergeStatus::Draft,
            MergeStatus::Blocked,
            MergeStatus::Behind,
        ] {
            assert!(status.blocks_merge(), "{status:?} should block");
        }
        assert!(!MergeStatus::Unstable.blocks_merge());
        assert!(!MergeStatus::Ready.blocks_merge());
    }

    /// A cached pull request written before `merge_state` existed must still
    /// decode — otherwise upgrading throws the whole cold-start cache away.
    #[test]
    fn decodes_a_payload_without_merge_state() {
        let mut value = serde_json::to_value(pr("t", "a")).expect("encodes");
        value
            .as_object_mut()
            .expect("object")
            .remove("merge_state")
            .expect("field was present");

        let decoded: PullRequest =
            serde_json::from_value(value).expect("decodes without the field");
        assert_eq!(decoded.merge_state, MergeStateStatus::Unknown);
    }

    /// `base_divergence` arrived later still, and comes from a separate query
    /// rather than the feed, so a cached row without it must decode to "not
    /// yet known" rather than fail.
    #[test]
    fn decodes_a_payload_without_base_divergence() {
        let mut value = serde_json::to_value(pr("t", "a")).expect("encodes");
        value
            .as_object_mut()
            .expect("object")
            .remove("base_divergence")
            .expect("field was present");

        let decoded: PullRequest =
            serde_json::from_value(value).expect("decodes without the field");
        assert_eq!(decoded.base_divergence, None);
    }

    /// The wire spelling has to survive the round trip, since these values are
    /// both parsed from GraphQL and written to the cache.
    #[test]
    fn merge_state_uses_the_api_spelling() {
        let json = serde_json::to_string(&MergeStateStatus::HasHooks).expect("encodes");
        assert_eq!(json, "\"HAS_HOOKS\"");
        let back: MergeStateStatus = serde_json::from_str("\"BEHIND\"").expect("decodes");
        assert_eq!(back, MergeStateStatus::Behind);
    }
}
