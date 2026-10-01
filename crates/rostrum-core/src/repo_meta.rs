//! Facts about a repository itself, as opposed to its pull requests.
//!
//! Everything here rides along on the feed query and exists so the feed can
//! order repositories by something other than the config file's order. None
//! of it is known until a repository's first refresh (or its cached copy)
//! lands, which is why [`crate::RepoState::meta`] is an `Option`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Whether a repository belongs to an organization or to a person.
///
/// The two are sorted together under "owner" — the user asked for "author,
/// or org for repos that belong to one", and on a repository both are just
/// the owner's login — but the distinction is kept so a renderer can say
/// which it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerKind {
    Organization,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoOwner {
    /// The login with GitHub's casing, which may differ from the casing the
    /// repository was configured with.
    pub login: String,
    pub kind: OwnerKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoMeta {
    pub owner: RepoOwner,
    /// When any branch last received a push. `None` for a repository that
    /// has never been pushed to, which GitHub reports as `null`.
    pub pushed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// GitHub's own `updatedAt` for the repository object: settings, the
    /// description, stars. Only the fallback for the "updated" sort, which
    /// prefers the newest open item; see [`crate::sort`].
    pub updated_at: DateTime<Utc>,
    pub stars: u32,
}
