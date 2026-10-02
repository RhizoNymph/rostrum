//! The comparisons behind [`super::Sort`]: pure functions from state to an
//! order, shared by every client.
//!
//! Three rules hold for every key:
//!
//! - **Unknown sinks.** A value nobody has fetched yet — a repository's push
//!   time before its first refresh, a deleted account's login — sorts after
//!   every known value in *both* directions. An unknown is not "oldest" or
//!   "fewest", and flipping the direction should not drag the not-yet-loaded
//!   to the top.
//! - **Ties are deterministic.** Equal keys fall back to the repository's
//!   name then owner, or the item's number, always ascending, so two
//!   refreshes carrying the same data never reshuffle the rows under the
//!   cursor.
//! - **Text ignores case.** GitHub logins and repository names are
//!   case-insensitive, and titles are read, not byte-compared.

use std::cmp::Ordering;

use chrono::{DateTime, Utc};

use super::{ItemSortKey, KeyKind, RepoSortKey, Sort, SortDirection, SortKey};
use crate::{
    feed::{IssueIx, PrIx, RepoIx},
    issue::Issue,
    model::PullRequest,
    state::RepoState,
};

/// Text folded to the form it is compared in.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextKey(String);

impl TextKey {
    pub fn new(raw: &str) -> Self {
        Self(raw.trim().to_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One key's value for one repository or item.
///
/// A given key always yields the same variant, so values of different
/// variants are never compared against each other in practice; the derived
/// order between variants exists only to make `Ord` total.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SortValue {
    Time(DateTime<Utc>),
    Text(TextKey),
    Count(u64),
}

/// Compare two possibly-unknown values in `direction`, unknowns last.
fn compare_values(
    a: Option<&SortValue>,
    b: Option<&SortValue>,
    direction: SortDirection,
) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => direction.apply(a.cmp(b)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

// --- repositories -------------------------------------------------------

/// `repo`'s value for `key`, or `None` when it is not known yet.
pub fn repo_sort_value(repo: &RepoState, key: RepoSortKey) -> Option<SortValue> {
    let meta = repo.meta.as_ref();
    match key {
        RepoSortKey::Pushed => meta?.pushed_at.map(SortValue::Time),
        RepoSortKey::Updated => repo
            .prs
            .iter()
            .map(|pr| pr.updated_at)
            .chain(repo.issues.iter().map(|issue| issue.updated_at))
            .max()
            .or_else(|| meta.map(|meta| meta.updated_at))
            .map(SortValue::Time),
        RepoSortKey::Created => meta.map(|meta| SortValue::Time(meta.created_at)),
        // The id already names the owner, so this key works before the
        // first refresh; the fetched login only corrects its casing, which
        // the comparison ignores anyway.
        RepoSortKey::Owner => Some(SortValue::Text(TextKey::new(
            meta.map_or(repo.id.owner(), |meta| meta.owner.login.as_str()),
        ))),
        RepoSortKey::Name => Some(SortValue::Text(TextKey::new(repo.id.name()))),
        RepoSortKey::Stars => meta.map(|meta| SortValue::Count(u64::from(meta.stars))),
    }
}

/// How `a` and `b` are ordered under `sort`, ties broken by name then owner.
pub fn compare_repos(a: &RepoState, b: &RepoState, sort: Sort<RepoSortKey>) -> Ordering {
    compare_values(
        repo_sort_value(a, sort.key()).as_ref(),
        repo_sort_value(b, sort.key()).as_ref(),
        sort.direction(),
    )
    .then_with(|| repo_tie_break(a, b))
}

fn repo_tie_break(a: &RepoState, b: &RepoState) -> Ordering {
    TextKey::new(a.id.name())
        .cmp(&TextKey::new(b.id.name()))
        .then_with(|| TextKey::new(a.id.owner()).cmp(&TextKey::new(b.id.owner())))
        // Two ids differing only in case: still a total order.
        .then_with(|| a.id.cmp(&b.id))
}

/// The positions of `repos` in the order they are shown.
pub fn order_repos(repos: &[RepoState], sort: Sort<RepoSortKey>) -> Vec<RepoIx> {
    let mut order: Vec<RepoIx> = (0..repos.len()).map(RepoIx).collect();
    order.sort_by(|a, b| compare_repos(&repos[a.0], &repos[b.0], sort));
    order
}

// --- items --------------------------------------------------------------

/// Something the item sort orders: a pull request or an issue.
///
/// The item comparisons are written once over this trait, so both tabs —
/// and a stack of pull requests filed as one — follow the same rules.
trait SortItem {
    fn sort_value(&self, key: ItemSortKey) -> Option<SortValue>;
    /// The tie-break: the item's number within its repository.
    fn tie_number(&self) -> u32;
}

impl SortItem for PullRequest {
    fn sort_value(&self, key: ItemSortKey) -> Option<SortValue> {
        item_sort_value(self, key)
    }

    fn tie_number(&self) -> u32 {
        self.number.0
    }
}

impl SortItem for Issue {
    fn sort_value(&self, key: ItemSortKey) -> Option<SortValue> {
        issue_sort_value(self, key)
    }

    fn tie_number(&self) -> u32 {
        self.number.0
    }
}

/// `pr`'s value for `key`, or `None` when it is not known.
pub fn item_sort_value(pr: &PullRequest, key: ItemSortKey) -> Option<SortValue> {
    match key {
        ItemSortKey::Pushed => pr.pushed_at.map(SortValue::Time),
        ItemSortKey::Updated => Some(SortValue::Time(pr.updated_at)),
        ItemSortKey::Created => Some(SortValue::Time(pr.created_at)),
        ItemSortKey::Author => pr
            .author
            .as_ref()
            .map(|author| SortValue::Text(TextKey::new(&author.login))),
        ItemSortKey::Title => Some(SortValue::Text(TextKey::new(&pr.title))),
    }
}

/// `issue`'s value for `key`, or `None` when it is not known.
///
/// Issues have no branch, so nothing is ever pushed to one:
/// [`ItemSortKey::Pushed`] falls back to the issue's `updatedAt`, the
/// nearest thing to "last saw work". Falling back rather than reporting
/// unknown keeps a "pushed" sort useful on the Issues tab instead of
/// degrading every issue to the number tie-break.
pub fn issue_sort_value(issue: &Issue, key: ItemSortKey) -> Option<SortValue> {
    match key {
        ItemSortKey::Pushed | ItemSortKey::Updated => Some(SortValue::Time(issue.updated_at)),
        ItemSortKey::Created => Some(SortValue::Time(issue.created_at)),
        ItemSortKey::Author => issue
            .author
            .as_ref()
            .map(|author| SortValue::Text(TextKey::new(&author.login))),
        ItemSortKey::Title => Some(SortValue::Text(TextKey::new(&issue.title))),
    }
}

/// The value a group of pull requests that sorts as one unit — a stack —
/// is filed under.
///
/// `members` runs bottom first: `members[0]` is the pull request the others
/// build on. Text keys use that bottom member, the name the group is known
/// by. Time keys use the member furthest toward the end being looked for —
/// the newest for a descending sort, the oldest for an ascending one — so a
/// stack with fresh activity anywhere in it surfaces with "newest first".
/// Members whose value is unknown are skipped; a group with no known value
/// has none.
///
/// A single pull request is a group of one, and [`compare_items`] goes
/// through here, so lone items and groups can never be ordered by different
/// rules.
pub fn sort_key_for_group(
    members: &[&PullRequest],
    key: ItemSortKey,
    direction: SortDirection,
) -> Option<SortValue> {
    group_value(members, key, direction)
}

fn group_value<T: SortItem>(
    members: &[&T],
    key: ItemSortKey,
    direction: SortDirection,
) -> Option<SortValue> {
    match key.kind() {
        // No item key counts anything today; were one added, it would be
        // filed by the bottom member like text until decided otherwise.
        KeyKind::Text | KeyKind::Count => members.first().and_then(|bottom| bottom.sort_value(key)),
        KeyKind::Time => {
            let known = members.iter().filter_map(|item| item.sort_value(key));
            match direction {
                SortDirection::Descending => known.max(),
                SortDirection::Ascending => known.min(),
            }
        }
    }
}

/// How two groups are ordered under `sort`, ties broken by their bottom
/// members' numbers.
pub fn compare_groups(a: &[&PullRequest], b: &[&PullRequest], sort: Sort<ItemSortKey>) -> Ordering {
    compare_item_groups(a, b, sort)
}

fn compare_item_groups<T: SortItem>(a: &[&T], b: &[&T], sort: Sort<ItemSortKey>) -> Ordering {
    let value = |group| group_value(group, sort.key(), sort.direction());
    compare_values(value(a).as_ref(), value(b).as_ref(), sort.direction()).then_with(|| {
        let bottom = |group: &[&T]| group.first().map(|item| item.tie_number());
        bottom(a).cmp(&bottom(b))
    })
}

/// How two pull requests are ordered under `sort`, ties broken by number.
pub fn compare_items(a: &PullRequest, b: &PullRequest, sort: Sort<ItemSortKey>) -> Ordering {
    compare_item_groups(&[a], &[b], sort)
}

/// How two issues are ordered under `sort`, ties broken by number — the
/// same rules as [`compare_items`], over [`issue_sort_value`].
pub fn compare_issues(a: &Issue, b: &Issue, sort: Sort<ItemSortKey>) -> Ordering {
    compare_item_groups(&[a], &[b], sort)
}

/// Reorder `indices` — positions in `prs`, typically the ones the filter let
/// through — into display order. Only permutes; never adds or drops.
pub fn order_items(prs: &[PullRequest], indices: &mut [PrIx], sort: Sort<ItemSortKey>) {
    indices.sort_by(|a, b| compare_items(&prs[a.0], &prs[b.0], sort));
}

/// [`order_items`] for issues: reorder positions in `issues` into display
/// order under the same item sort.
pub fn order_issues(issues: &[Issue], indices: &mut [IssueIx], sort: Sort<ItemSortKey>) {
    indices.sort_by(|a, b| compare_issues(&issues[a.0], &issues[b.0], sort));
}
