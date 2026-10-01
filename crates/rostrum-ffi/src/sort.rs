//! The feed's two sorts — repositories, and the items inside each — as the
//! phone's Sort sheet shows and changes them.
//!
//! The model is `rostrum_core::sort`'s: two key enums, so "pull requests by
//! stars" cannot be built; choosing a different key resets the direction to
//! that key's default; directions are named by the key's kind ("Newest
//! first", "A→Z", "Most"). Both sorts persist in the settings file beside the
//! desktop's.

use rostrum_core::{
    FeedSort, KeyKind, Sort, SortKey,
    sort::{ItemSortKey as CoreItemKey, RepoSortKey as CoreRepoKey, SortDirection as CoreDir},
};

use crate::{engine::RostrumCore, error::RostrumError, feed::FeedSnapshot};

/// What orders repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum RepoSortKey {
    /// Last push to any branch.
    Pushed,
    /// Newest activity on an open pull request or issue.
    Updated,
    Created,
    Owner,
    Name,
    Stars,
}

/// What orders pull requests and issues inside a repository. One sort serves
/// both tabs; "pushed" on an issue means its last update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ItemSortKey {
    Pushed,
    Updated,
    Created,
    Author,
    Title,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// One key the repository sort can use, as the sheet lists it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RepoSortOption {
    pub key: RepoSortKey,
    /// `pushed`, `stars`, …
    pub label: String,
    /// What choosing this key starts at.
    pub default_direction: SortDirection,
    /// How each direction reads for this key: "Newest first" / "Oldest
    /// first", "A→Z" / "Z→A", "Most" / "Fewest".
    pub descending_label: String,
    pub ascending_label: String,
}

/// One key the item sort can use.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ItemSortOption {
    pub key: ItemSortKey,
    pub label: String,
    pub default_direction: SortDirection,
    pub descending_label: String,
    pub ascending_label: String,
}

/// Both sorts as they stand, and every valid choice for each.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SortSettings {
    pub repo_key: RepoSortKey,
    pub repo_direction: SortDirection,
    /// "Newest first".
    pub repo_direction_label: String,
    pub item_key: ItemSortKey,
    pub item_direction: SortDirection,
    pub item_direction_label: String,
    /// The button's caption: `pushed ↓ · created ↓`.
    pub summary: String,
    /// Menu order.
    pub repo_options: Vec<RepoSortOption>,
    pub item_options: Vec<ItemSortOption>,
}

// --- conversions -------------------------------------------------------------

impl From<CoreRepoKey> for RepoSortKey {
    fn from(key: CoreRepoKey) -> Self {
        match key {
            CoreRepoKey::Pushed => Self::Pushed,
            CoreRepoKey::Updated => Self::Updated,
            CoreRepoKey::Created => Self::Created,
            CoreRepoKey::Owner => Self::Owner,
            CoreRepoKey::Name => Self::Name,
            CoreRepoKey::Stars => Self::Stars,
        }
    }
}

impl From<RepoSortKey> for CoreRepoKey {
    fn from(key: RepoSortKey) -> Self {
        match key {
            RepoSortKey::Pushed => Self::Pushed,
            RepoSortKey::Updated => Self::Updated,
            RepoSortKey::Created => Self::Created,
            RepoSortKey::Owner => Self::Owner,
            RepoSortKey::Name => Self::Name,
            RepoSortKey::Stars => Self::Stars,
        }
    }
}

impl From<CoreItemKey> for ItemSortKey {
    fn from(key: CoreItemKey) -> Self {
        match key {
            CoreItemKey::Pushed => Self::Pushed,
            CoreItemKey::Updated => Self::Updated,
            CoreItemKey::Created => Self::Created,
            CoreItemKey::Author => Self::Author,
            CoreItemKey::Title => Self::Title,
        }
    }
}

impl From<ItemSortKey> for CoreItemKey {
    fn from(key: ItemSortKey) -> Self {
        match key {
            ItemSortKey::Pushed => Self::Pushed,
            ItemSortKey::Updated => Self::Updated,
            ItemSortKey::Created => Self::Created,
            ItemSortKey::Author => Self::Author,
            ItemSortKey::Title => Self::Title,
        }
    }
}

impl From<CoreDir> for SortDirection {
    fn from(direction: CoreDir) -> Self {
        match direction {
            CoreDir::Ascending => Self::Ascending,
            CoreDir::Descending => Self::Descending,
        }
    }
}

impl From<SortDirection> for CoreDir {
    fn from(direction: SortDirection) -> Self {
        match direction {
            SortDirection::Ascending => Self::Ascending,
            SortDirection::Descending => Self::Descending,
        }
    }
}

fn labels(kind: KeyKind) -> (String, String) {
    (
        kind.direction_label(CoreDir::Descending).to_string(),
        kind.direction_label(CoreDir::Ascending).to_string(),
    )
}

/// The settings view of a [`FeedSort`].
pub(crate) fn settings(sort: FeedSort) -> SortSettings {
    SortSettings {
        repo_key: sort.repos.key().into(),
        repo_direction: sort.repos.direction().into(),
        repo_direction_label: sort.repos.direction_label().to_string(),
        item_key: sort.items.key().into(),
        item_direction: sort.items.direction().into(),
        item_direction_label: sort.items.direction_label().to_string(),
        summary: sort.summary(),
        repo_options: CoreRepoKey::ALL
            .iter()
            .map(|&key| {
                let (descending_label, ascending_label) = labels(key.kind());
                RepoSortOption {
                    key: key.into(),
                    label: key.label().to_string(),
                    default_direction: key.default_direction().into(),
                    descending_label,
                    ascending_label,
                }
            })
            .collect(),
        item_options: CoreItemKey::ALL
            .iter()
            .map(|&key| {
                let (descending_label, ascending_label) = labels(key.kind());
                ItemSortOption {
                    key: key.into(),
                    label: key.label().to_string(),
                    default_direction: key.default_direction().into(),
                    descending_label,
                    ascending_label,
                }
            })
            .collect(),
    }
}

/// Apply a choice: with a direction, exactly that; without one, the menu's
/// rule — a different key starts at its default direction, the same key is
/// left as it is.
pub(crate) fn chosen<K: SortKey>(
    current: Sort<K>,
    key: K,
    direction: Option<SortDirection>,
) -> Sort<K> {
    match direction {
        Some(direction) => Sort::with_direction(key, direction.into()),
        None => {
            let mut next = current;
            next.choose(key);
            next
        }
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Both sorts and every choice for each, for the Sort sheet.
    pub async fn sort_settings(&self) -> Result<SortSettings, RostrumError> {
        self.actor
            .call(|state| settings(state.feed.filter.sort))
            .await
    }

    /// Order repositories by `key`. With no `direction`, a new key starts at
    /// its default and the current key keeps its direction. Persisted.
    pub async fn set_repo_sort(
        &self,
        key: RepoSortKey,
        direction: Option<SortDirection>,
    ) -> Result<FeedSnapshot, RostrumError> {
        self.edit_sort(move |sort| sort.repos = chosen(sort.repos, key.into(), direction))
            .await
    }

    /// Order pull requests and issues by `key`, as `set_repo_sort` does.
    pub async fn set_item_sort(
        &self,
        key: ItemSortKey,
        direction: Option<SortDirection>,
    ) -> Result<FeedSnapshot, RostrumError> {
        self.edit_sort(move |sort| sort.items = chosen(sort.items, key.into(), direction))
            .await
    }
}

impl RostrumCore {
    async fn edit_sort(
        &self,
        edit: impl FnOnce(&mut FeedSort) + Send + 'static,
    ) -> Result<FeedSnapshot, RostrumError> {
        self.actor
            .try_call(move |state| {
                let mut filter = state.feed.filter.clone();
                edit(&mut filter.sort);
                state.edit_config(|config| config.absorb_filter(&filter))?;
                tracing::debug!(sort = %filter.sort.summary(), "feed sort changed");
                state.feed.filter = filter;
                Ok(state.publish())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_read_as_the_desktop_shows_them() {
        let settings = settings(FeedSort::default());
        assert_eq!(settings.repo_key, RepoSortKey::Pushed);
        assert_eq!(settings.repo_direction, SortDirection::Descending);
        assert_eq!(settings.repo_direction_label, "Newest first");
        assert_eq!(settings.item_key, ItemSortKey::Created);
        assert_eq!(settings.summary, FeedSort::default().summary());
    }

    #[test]
    fn the_options_are_exactly_the_valid_keys_in_menu_order() {
        let settings = settings(FeedSort::default());
        assert_eq!(
            settings
                .repo_options
                .iter()
                .map(|option| option.key)
                .collect::<Vec<_>>(),
            vec![
                RepoSortKey::Pushed,
                RepoSortKey::Updated,
                RepoSortKey::Created,
                RepoSortKey::Owner,
                RepoSortKey::Name,
                RepoSortKey::Stars,
            ]
        );
        // Stars is a repository key only; author an item key only.
        assert!(
            settings
                .item_options
                .iter()
                .all(|option| option.label != "stars")
        );
        let stars = settings
            .repo_options
            .iter()
            .find(|option| option.key == RepoSortKey::Stars)
            .expect("stars");
        assert_eq!(stars.default_direction, SortDirection::Descending);
        assert_eq!(
            (
                stars.descending_label.as_str(),
                stars.ascending_label.as_str()
            ),
            ("Most", "Fewest")
        );
        let title = settings
            .item_options
            .iter()
            .find(|option| option.key == ItemSortKey::Title)
            .expect("title");
        assert_eq!(title.default_direction, SortDirection::Ascending);
        assert_eq!(title.ascending_label, "A\u{2192}Z");
    }

    #[test]
    fn choosing_a_new_key_resets_the_direction_and_the_same_key_keeps_it() {
        let oldest = Sort::with_direction(CoreItemKey::Created, CoreDir::Ascending);
        let title = chosen(oldest, CoreItemKey::Title, None);
        assert_eq!(title.direction(), CoreDir::Ascending);
        let author_desc = chosen(
            Sort::with_direction(CoreItemKey::Author, CoreDir::Descending),
            CoreItemKey::Author,
            None,
        );
        assert_eq!(author_desc.direction(), CoreDir::Descending);
        let updated = chosen(oldest, CoreItemKey::Updated, None);
        assert_eq!(updated.direction(), CoreDir::Descending);
        let explicit = chosen(oldest, CoreItemKey::Updated, Some(SortDirection::Ascending));
        assert_eq!(explicit.direction(), CoreDir::Ascending);
    }

    #[test]
    fn keys_and_directions_round_trip() {
        for &key in CoreRepoKey::ALL {
            assert_eq!(CoreRepoKey::from(RepoSortKey::from(key)), key);
        }
        for &key in CoreItemKey::ALL {
            assert_eq!(CoreItemKey::from(ItemSortKey::from(key)), key);
        }
        for direction in [CoreDir::Ascending, CoreDir::Descending] {
            assert_eq!(CoreDir::from(SortDirection::from(direction)), direction);
        }
    }
}
