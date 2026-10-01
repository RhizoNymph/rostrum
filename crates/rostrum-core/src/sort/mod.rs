//! Ordering the feed: which repository container comes first, and which item
//! comes first inside each one.
//!
//! The two orders are independent. Each is a key plus a direction
//! ([`Sort`]), and the keys are separate enums ([`RepoSortKey`],
//! [`ItemSortKey`]) so a combination that means nothing — repositories by
//! author, pull requests by stars — cannot be built, persisted or offered.
//!
//! The comparisons themselves live in [`compare`] as pure functions over
//! `RepoState` and `PullRequest`, so both the desktop and the Android core
//! order the feed identically.

pub mod compare;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use compare::{
    SortValue, TextKey, compare_groups, compare_items, compare_repos, item_sort_value, order_items,
    order_repos, repo_sort_value, sort_key_for_group,
};

/// Which way a key runs. What each end is *called* depends on the key's
/// [`KeyKind`] — "newest first" for a time, "A→Z" for text, "most" for a
/// count — see [`KeyKind::direction_label`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    /// Smallest first: oldest, A→Z, fewest.
    Ascending,
    /// Largest first: newest, Z→A, most.
    Descending,
}

impl SortDirection {
    pub fn reversed(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }

    /// Apply this direction to an ascending comparison.
    pub fn apply(self, ascending: std::cmp::Ordering) -> std::cmp::Ordering {
        match self {
            Self::Ascending => ascending,
            Self::Descending => ascending.reverse(),
        }
    }
}

/// What sort of value a key compares, which decides its default direction and
/// how its directions are named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Time,
    Text,
    Count,
}

impl KeyKind {
    /// Newest, A→Z and most first: the end someone choosing the key is
    /// almost always looking for.
    pub fn default_direction(self) -> SortDirection {
        match self {
            Self::Time | Self::Count => SortDirection::Descending,
            Self::Text => SortDirection::Ascending,
        }
    }

    /// The direction's name in a menu.
    pub fn direction_label(self, direction: SortDirection) -> &'static str {
        match (self, direction) {
            (Self::Time, SortDirection::Descending) => "Newest first",
            (Self::Time, SortDirection::Ascending) => "Oldest first",
            (Self::Text, SortDirection::Ascending) => "A\u{2192}Z",
            (Self::Text, SortDirection::Descending) => "Z\u{2192}A",
            (Self::Count, SortDirection::Descending) => "Most",
            (Self::Count, SortDirection::Ascending) => "Fewest",
        }
    }

    /// The direction squeezed into a button label: an arrow for times and
    /// counts, the alphabet's direction for text, where an arrow alone would
    /// not say which end is which.
    pub fn direction_short(self, direction: SortDirection) -> &'static str {
        match (self, direction) {
            (Self::Text, SortDirection::Ascending) => "A\u{2192}Z",
            (Self::Text, SortDirection::Descending) => "Z\u{2192}A",
            (_, SortDirection::Descending) => "\u{2193}",
            (_, SortDirection::Ascending) => "\u{2191}",
        }
    }
}

/// A key one of the feed's two sorts can use.
pub trait SortKey: Copy + Eq + std::fmt::Debug + 'static {
    /// Every key, in the order a menu offers them.
    const ALL: &'static [Self];

    fn kind(self) -> KeyKind;

    /// Lower-case name, for menus and the button summary.
    fn label(self) -> &'static str;

    fn default_direction(self) -> SortDirection {
        self.kind().default_direction()
    }
}

/// How repository containers are ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoSortKey {
    /// The repository's `pushedAt`: a push to any branch.
    Pushed,
    /// The newest `updatedAt` among the repository's open items, falling back
    /// to the repository's own `updatedAt` when it has none.
    Updated,
    Created,
    /// The owning organization's or user's login.
    Owner,
    Name,
    Stars,
}

impl SortKey for RepoSortKey {
    const ALL: &'static [Self] = &[
        Self::Pushed,
        Self::Updated,
        Self::Created,
        Self::Owner,
        Self::Name,
        Self::Stars,
    ];

    fn kind(self) -> KeyKind {
        match self {
            Self::Pushed | Self::Updated | Self::Created => KeyKind::Time,
            Self::Owner | Self::Name => KeyKind::Text,
            Self::Stars => KeyKind::Count,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Pushed => "pushed",
            Self::Updated => "updated",
            Self::Created => "created",
            Self::Owner => "owner",
            Self::Name => "name",
            Self::Stars => "stars",
        }
    }
}

/// How items — pull requests now, issues later — are ordered within a
/// repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemSortKey {
    /// When the head branch last received commits; see
    /// [`crate::PullRequest::pushed_at`].
    Pushed,
    /// GitHub's `updatedAt`, which pushes, comments and reviews all bump.
    Updated,
    Created,
    Author,
    Title,
}

impl SortKey for ItemSortKey {
    const ALL: &'static [Self] = &[
        Self::Pushed,
        Self::Updated,
        Self::Created,
        Self::Author,
        Self::Title,
    ];

    fn kind(self) -> KeyKind {
        match self {
            Self::Pushed | Self::Updated | Self::Created => KeyKind::Time,
            Self::Author | Self::Title => KeyKind::Text,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Pushed => "pushed",
            Self::Updated => "updated",
            Self::Created => "created",
            Self::Author => "author",
            Self::Title => "title",
        }
    }
}

/// One sort: a key and which way it runs.
///
/// Every key/direction pair is meaningful, so both are plain values; what is
/// encoded is the *transition* rule — choosing a different key resets the
/// direction to that key's default, because "oldest first" carried over from
/// a time key to a name key would read as "Z→A", which nobody chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Sort<K> {
    key: K,
    direction: SortDirection,
}

impl<K: SortKey> Sort<K> {
    /// `key` in its default direction.
    pub fn new(key: K) -> Self {
        Self {
            key,
            direction: key.default_direction(),
        }
    }

    pub fn with_direction(key: K, direction: SortDirection) -> Self {
        Self { key, direction }
    }

    pub fn key(&self) -> K {
        self.key
    }

    pub fn direction(&self) -> SortDirection {
        self.direction
    }

    /// Switch to `key`. A different key arrives in its default direction;
    /// choosing the current key again leaves the direction alone, so a stray
    /// second click on the selected entry is harmless.
    pub fn choose(&mut self, key: K) {
        if key != self.key {
            *self = Self::new(key);
        }
    }

    pub fn reverse(&mut self) {
        self.direction = self.direction.reversed();
    }

    /// The direction's name for this key: "Newest first", "A→Z", "Most".
    pub fn direction_label(&self) -> &'static str {
        self.key.kind().direction_label(self.direction)
    }

    /// `pushed ↓`, `title A→Z`.
    pub fn summary(&self) -> String {
        format!(
            "{} {}",
            self.key.label(),
            self.key.kind().direction_short(self.direction)
        )
    }
}

/// Written by hand so a hand-edited config may give just a key: a missing
/// `direction` takes the key's default, the same rule [`Sort::choose`]
/// applies in the UI.
impl<'de, K> Deserialize<'de> for Sort<K>
where
    K: SortKey + Deserialize<'de>,
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Repr<K> {
            key: K,
            direction: Option<SortDirection>,
        }
        let Repr { key, direction } = Repr::<K>::deserialize(deserializer)?;
        Ok(match direction {
            Some(direction) => Self::with_direction(key, direction),
            None => Self::new(key),
        })
    }
}

/// Both of the feed's orders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FeedSort {
    pub repos: Sort<RepoSortKey>,
    pub items: Sort<ItemSortKey>,
}

impl Default for FeedSort {
    /// Repositories most recently pushed to first — where work is happening —
    /// and within each, the newest pull request first.
    fn default() -> Self {
        Self {
            repos: Sort::new(RepoSortKey::Pushed),
            items: Sort::new(ItemSortKey::Created),
        }
    }
}

impl FeedSort {
    /// The sort button's label: `Sort: pushed ↓ · created ↓`.
    pub fn summary(&self) -> String {
        format!(
            "Sort: {} \u{00b7} {}",
            self.repos.summary(),
            self.items.summary()
        )
    }
}

/// Which order [`crate::feed::flatten_in`] lays the feed out in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FeedOrder {
    /// Repositories in the order they are listed in state — the user's own
    /// order where a client lets them arrange it — and items in the order
    /// they were fetched.
    AsListed,
    Sorted(FeedSort),
}
