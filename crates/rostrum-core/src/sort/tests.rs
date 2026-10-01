use chrono::{DateTime, TimeZone, Utc};

use super::*;
use crate::{
    feed::{FeedFilter, FeedRow, PrIx, RepoIx, flatten},
    model::{PrNumber, PullRequest, User},
    repo_meta::{OwnerKind, RepoMeta, RepoOwner},
    state::{LoadState, RepoState},
    test_support::pull,
};

// --- builders -----------------------------------------------------------

fn at(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + secs, 0)
        .single()
        .expect("valid timestamp")
}

/// A pull request whose every sortable field is pinned, so no ordering in
/// these tests depends on when the test happened to run.
fn item(number: u32) -> PullRequest {
    PullRequest {
        created_at: at(0),
        updated_at: at(0),
        pushed_at: Some(at(0)),
        ..pull(number)
    }
}

fn created(number: u32, secs: i64) -> PullRequest {
    PullRequest {
        created_at: at(secs),
        ..item(number)
    }
}

fn updated(number: u32, secs: i64) -> PullRequest {
    PullRequest {
        updated_at: at(secs),
        ..item(number)
    }
}

fn pushed(number: u32, secs: Option<i64>) -> PullRequest {
    PullRequest {
        pushed_at: secs.map(at),
        ..item(number)
    }
}

fn titled(number: u32, title: &str) -> PullRequest {
    PullRequest {
        title: title.into(),
        ..item(number)
    }
}

fn authored(number: u32, login: Option<&str>) -> PullRequest {
    PullRequest {
        author: login.map(|login| User {
            login: login.into(),
            avatar_url: None,
        }),
        ..item(number)
    }
}

fn meta(owner: &str) -> RepoMeta {
    RepoMeta {
        owner: RepoOwner {
            login: owner.into(),
            kind: OwnerKind::Organization,
        },
        pushed_at: Some(at(0)),
        created_at: at(0),
        updated_at: at(0),
        stars: 0,
    }
}

fn repo(name: &str) -> RepoState {
    let id: crate::RepoId = name.parse().expect("valid repo id");
    let owner = id.owner().to_string();
    RepoState {
        prs: Vec::new(),
        load: LoadState::Loaded { at: at(0) },
        collapsed: false,
        meta: Some(meta(&owner)),
        id,
    }
}

fn with_meta(name: &str, edit: impl FnOnce(&mut RepoMeta)) -> RepoState {
    let mut state = repo(name);
    if let Some(meta) = state.meta.as_mut() {
        edit(meta);
    }
    state
}

fn unknown(name: &str) -> RepoState {
    RepoState {
        meta: None,
        ..repo(name)
    }
}

fn repo_order(repos: &[RepoState], sort: Sort<RepoSortKey>) -> Vec<String> {
    order_repos(repos, sort)
        .into_iter()
        .map(|RepoIx(ix)| repos[ix].id.to_string())
        .collect()
}

fn item_order(prs: &[PullRequest], sort: Sort<ItemSortKey>) -> Vec<u32> {
    let mut indices: Vec<PrIx> = (0..prs.len()).map(PrIx).collect();
    order_items(prs, &mut indices, sort);
    indices
        .into_iter()
        .map(|PrIx(ix)| prs[ix].number.0)
        .collect()
}

fn asc<K: SortKey>(key: K) -> Sort<K> {
    Sort::with_direction(key, SortDirection::Ascending)
}

fn desc<K: SortKey>(key: K) -> Sort<K> {
    Sort::with_direction(key, SortDirection::Descending)
}

// --- directions and defaults -------------------------------------------

#[test]
fn the_feed_defaults_to_repos_by_pushed_and_items_by_created_newest_first() {
    let sort = FeedSort::default();
    assert_eq!(sort.repos, desc(RepoSortKey::Pushed));
    assert_eq!(sort.items, desc(ItemSortKey::Created));
    assert_eq!(
        sort.summary(),
        "Sort: pushed \u{2193} \u{00b7} created \u{2193}"
    );
}

#[test]
fn every_repo_key_has_the_requested_default_direction() {
    use RepoSortKey::*;
    use SortDirection::*;
    let expected = [
        (Pushed, Descending),
        (Updated, Descending),
        (Created, Descending),
        (Owner, Ascending),
        (Name, Ascending),
        (Stars, Descending),
    ];
    assert_eq!(RepoSortKey::ALL.len(), expected.len());
    for (key, direction) in expected {
        assert_eq!(key.default_direction(), direction, "{key:?}");
        assert_eq!(Sort::new(key).direction(), direction, "{key:?}");
    }
}

#[test]
fn every_item_key_has_the_requested_default_direction() {
    use ItemSortKey::*;
    use SortDirection::*;
    let expected = [
        (Pushed, Descending),
        (Updated, Descending),
        (Created, Descending),
        (Author, Ascending),
        (Title, Ascending),
    ];
    assert_eq!(ItemSortKey::ALL.len(), expected.len());
    for (key, direction) in expected {
        assert_eq!(key.default_direction(), direction, "{key:?}");
        assert_eq!(Sort::new(key).direction(), direction, "{key:?}");
    }
}

#[test]
fn choosing_a_different_key_resets_to_its_default_direction() {
    let mut sort = Sort::new(RepoSortKey::Pushed);
    sort.reverse();
    assert_eq!(sort.direction(), SortDirection::Ascending);

    sort.choose(RepoSortKey::Name);
    assert_eq!(sort, asc(RepoSortKey::Name));

    sort.reverse();
    sort.choose(RepoSortKey::Stars);
    assert_eq!(sort, desc(RepoSortKey::Stars));

    let mut items = asc(ItemSortKey::Created);
    items.choose(ItemSortKey::Updated);
    assert_eq!(items, desc(ItemSortKey::Updated));
    items.reverse();
    items.choose(ItemSortKey::Author);
    assert_eq!(items, asc(ItemSortKey::Author));
}

#[test]
fn choosing_the_current_key_again_keeps_the_direction() {
    let mut sort = Sort::new(ItemSortKey::Title);
    sort.reverse();
    sort.choose(ItemSortKey::Title);
    assert_eq!(sort, desc(ItemSortKey::Title));
}

#[test]
fn reversing_twice_is_the_identity() {
    let mut sort = Sort::new(RepoSortKey::Created);
    sort.reverse();
    sort.reverse();
    assert_eq!(sort, Sort::new(RepoSortKey::Created));
}

#[test]
fn directions_are_named_for_their_key() {
    assert_eq!(desc(RepoSortKey::Pushed).direction_label(), "Newest first");
    assert_eq!(asc(RepoSortKey::Pushed).direction_label(), "Oldest first");
    assert_eq!(asc(RepoSortKey::Owner).direction_label(), "A\u{2192}Z");
    assert_eq!(desc(ItemSortKey::Title).direction_label(), "Z\u{2192}A");
    assert_eq!(desc(RepoSortKey::Stars).direction_label(), "Most");
    assert_eq!(asc(RepoSortKey::Stars).direction_label(), "Fewest");
    assert_eq!(asc(ItemSortKey::Updated).direction_label(), "Oldest first");
}

#[test]
fn summaries_use_arrows_for_times_and_counts_and_the_alphabet_for_text() {
    assert_eq!(desc(RepoSortKey::Stars).summary(), "stars \u{2193}");
    assert_eq!(asc(RepoSortKey::Created).summary(), "created \u{2191}");
    assert_eq!(asc(ItemSortKey::Title).summary(), "title A\u{2192}Z");
    assert_eq!(desc(RepoSortKey::Owner).summary(), "owner Z\u{2192}A");
}

/// Stars is a repository key only and author an item key only; the two
/// enums are what make the cross-over unrepresentable, so the menus built
/// from `ALL` cannot offer it either.
#[test]
fn each_sort_offers_only_its_own_keys() {
    let repo_labels: Vec<_> = RepoSortKey::ALL.iter().map(|k| k.label()).collect();
    let item_labels: Vec<_> = ItemSortKey::ALL.iter().map(|k| k.label()).collect();
    assert_eq!(
        repo_labels,
        ["pushed", "updated", "created", "owner", "name", "stars"]
    );
    assert_eq!(
        item_labels,
        ["pushed", "updated", "created", "author", "title"]
    );
}

// --- repository keys ----------------------------------------------------

#[test]
fn repos_by_pushed() {
    let repos = [
        with_meta("o/old", |m| m.pushed_at = Some(at(10))),
        with_meta("o/new", |m| m.pushed_at = Some(at(30))),
        with_meta("o/mid", |m| m.pushed_at = Some(at(20))),
    ];
    assert_eq!(
        repo_order(&repos, desc(RepoSortKey::Pushed)),
        ["o/new", "o/mid", "o/old"]
    );
    assert_eq!(
        repo_order(&repos, asc(RepoSortKey::Pushed)),
        ["o/old", "o/mid", "o/new"]
    );
}

#[test]
fn repos_by_updated_use_the_newest_open_item() {
    let mut busy = with_meta("o/busy", |m| m.updated_at = at(1));
    busy.prs = vec![updated(1, 5), updated(2, 50)];
    let mut quiet = with_meta("o/quiet", |m| m.updated_at = at(40));
    quiet.prs = vec![updated(1, 20)];
    let repos = [quiet, busy];

    // busy's newest item (50) beats quiet's (20), even though quiet's own
    // updatedAt (40) is newer than anything of busy's but that item.
    assert_eq!(
        repo_order(&repos, desc(RepoSortKey::Updated)),
        ["o/busy", "o/quiet"]
    );
    assert_eq!(
        repo_order(&repos, asc(RepoSortKey::Updated)),
        ["o/quiet", "o/busy"]
    );
}

#[test]
fn repos_by_updated_fall_back_to_the_repository_without_items() {
    let mut active = repo("o/active");
    active.prs = vec![updated(1, 30)];
    let idle_recent = with_meta("o/idle-recent", |m| m.updated_at = at(40));
    let idle_old = with_meta("o/idle-old", |m| m.updated_at = at(10));
    let repos = [idle_old, active, idle_recent];

    assert_eq!(
        repo_order(&repos, desc(RepoSortKey::Updated)),
        ["o/idle-recent", "o/active", "o/idle-old"]
    );
    assert_eq!(
        repo_order(&repos, asc(RepoSortKey::Updated)),
        ["o/idle-old", "o/active", "o/idle-recent"]
    );
}

/// Items are known before metadata only for a repository restored from an
/// old cache; the items alone still give it an "updated" value.
#[test]
fn repos_by_updated_need_no_metadata_when_they_have_items() {
    let mut cached = unknown("o/cached");
    cached.prs = vec![updated(1, 30)];
    let fetched = with_meta("o/fetched", |m| m.updated_at = at(20));
    assert_eq!(
        repo_order(&[fetched, cached], desc(RepoSortKey::Updated)),
        ["o/cached", "o/fetched"]
    );
}

#[test]
fn repos_by_created() {
    let repos = [
        with_meta("o/b", |m| m.created_at = at(20)),
        with_meta("o/a", |m| m.created_at = at(10)),
        with_meta("o/c", |m| m.created_at = at(30)),
    ];
    assert_eq!(
        repo_order(&repos, desc(RepoSortKey::Created)),
        ["o/c", "o/b", "o/a"]
    );
    assert_eq!(
        repo_order(&repos, asc(RepoSortKey::Created)),
        ["o/a", "o/b", "o/c"]
    );
}

#[test]
fn repos_by_owner_ignore_case_and_break_ties_by_name() {
    let repos = [
        repo("zeta/one"),
        repo("Alpha/two"),
        repo("beta/x"),
        repo("alpha/one"),
    ];
    assert_eq!(
        repo_order(&repos, asc(RepoSortKey::Owner)),
        ["alpha/one", "Alpha/two", "beta/x", "zeta/one"]
    );
    assert_eq!(
        repo_order(&repos, desc(RepoSortKey::Owner)),
        ["zeta/one", "beta/x", "alpha/one", "Alpha/two"]
    );
}

/// The owner is known from the repository's id alone, so the owner sort
/// works before the first refresh.
#[test]
fn repos_by_owner_need_no_metadata() {
    let repos = [unknown("b/x"), unknown("a/y")];
    assert_eq!(repo_order(&repos, asc(RepoSortKey::Owner)), ["a/y", "b/x"]);
}

#[test]
fn repos_by_name_ignore_the_owner_and_case() {
    let repos = [repo("a/zed"), repo("z/Alpha"), repo("m/mid")];
    assert_eq!(
        repo_order(&repos, asc(RepoSortKey::Name)),
        ["z/Alpha", "m/mid", "a/zed"]
    );
    assert_eq!(
        repo_order(&repos, desc(RepoSortKey::Name)),
        ["a/zed", "m/mid", "z/Alpha"]
    );
}

#[test]
fn repos_by_stars() {
    let repos = [
        with_meta("o/some", |m| m.stars = 10),
        with_meta("o/none", |m| m.stars = 0),
        with_meta("o/lots", |m| m.stars = 9000),
    ];
    assert_eq!(
        repo_order(&repos, desc(RepoSortKey::Stars)),
        ["o/lots", "o/some", "o/none"]
    );
    assert_eq!(
        repo_order(&repos, asc(RepoSortKey::Stars)),
        ["o/none", "o/some", "o/lots"]
    );
}

/// A repository that has not loaded yet has no push time, no creation time
/// and no star count. It goes to the bottom whichever way the sort runs: an
/// unknown is not "oldest" or "fewest".
#[test]
fn repos_with_unknown_values_go_last_in_both_directions() {
    let never_pushed = with_meta("o/empty", |m| m.pushed_at = None);
    let repos = [
        unknown("o/loading"),
        with_meta("o/early", |m| m.pushed_at = Some(at(1))),
        never_pushed,
        with_meta("o/late", |m| m.pushed_at = Some(at(2))),
    ];
    assert_eq!(
        repo_order(&repos, desc(RepoSortKey::Pushed)),
        ["o/late", "o/early", "o/empty", "o/loading"]
    );
    assert_eq!(
        repo_order(&repos, asc(RepoSortKey::Pushed)),
        ["o/early", "o/late", "o/empty", "o/loading"]
    );

    for key in [
        RepoSortKey::Created,
        RepoSortKey::Stars,
        RepoSortKey::Updated,
    ] {
        for sort in [asc(key), desc(key)] {
            let order = repo_order(&[unknown("o/a"), repo("o/b")], sort);
            assert_eq!(order, ["o/b", "o/a"], "{sort:?}");
        }
    }
}

#[test]
fn repo_ties_fall_back_to_name_then_owner_whatever_the_direction() {
    let repos = [repo("b/same"), repo("a/same"), repo("c/other")];
    for sort in [
        desc(RepoSortKey::Pushed),
        asc(RepoSortKey::Pushed),
        desc(RepoSortKey::Stars),
        asc(RepoSortKey::Created),
        desc(RepoSortKey::Updated),
    ] {
        assert_eq!(
            repo_order(&repos, sort),
            ["c/other", "a/same", "b/same"],
            "{sort:?}"
        );
    }
}

#[test]
fn repo_ordering_is_independent_of_input_order() {
    let a = with_meta("o/a", |m| m.stars = 3);
    let b = with_meta("o/b", |m| m.stars = 3);
    let c = with_meta("o/c", |m| m.stars = 7);
    let forward = repo_order(&[a.clone(), b.clone(), c.clone()], desc(RepoSortKey::Stars));
    let backward = repo_order(&[c, b, a], desc(RepoSortKey::Stars));
    assert_eq!(forward, backward);
    assert_eq!(forward, ["o/c", "o/a", "o/b"]);
}

// --- item keys ----------------------------------------------------------

#[test]
fn items_by_pushed() {
    let prs = [
        pushed(1, Some(20)),
        pushed(2, Some(30)),
        pushed(3, Some(10)),
    ];
    assert_eq!(item_order(&prs, desc(ItemSortKey::Pushed)), [2, 1, 3]);
    assert_eq!(item_order(&prs, asc(ItemSortKey::Pushed)), [3, 1, 2]);
}

#[test]
fn items_by_updated() {
    let prs = [updated(1, 5), updated(2, 1), updated(3, 9)];
    assert_eq!(item_order(&prs, desc(ItemSortKey::Updated)), [3, 1, 2]);
    assert_eq!(item_order(&prs, asc(ItemSortKey::Updated)), [2, 1, 3]);
}

#[test]
fn items_by_created() {
    let prs = [created(1, 5), created(2, 9), created(3, 1)];
    assert_eq!(item_order(&prs, desc(ItemSortKey::Created)), [2, 1, 3]);
    assert_eq!(item_order(&prs, asc(ItemSortKey::Created)), [3, 1, 2]);
}

#[test]
fn items_by_author_ignore_case() {
    let prs = [
        authored(1, Some("carol")),
        authored(2, Some("Alice")),
        authored(3, Some("bob")),
    ];
    assert_eq!(item_order(&prs, asc(ItemSortKey::Author)), [2, 3, 1]);
    assert_eq!(item_order(&prs, desc(ItemSortKey::Author)), [1, 3, 2]);
}

#[test]
fn items_by_title_ignore_case() {
    let prs = [
        titled(1, "fix the thing"),
        titled(2, "Add a feature"),
        titled(3, "bump deps"),
    ];
    assert_eq!(item_order(&prs, asc(ItemSortKey::Title)), [2, 3, 1]);
    assert_eq!(item_order(&prs, desc(ItemSortKey::Title)), [1, 3, 2]);
}

/// A deleted account has no login, and a pull request cached before
/// `pushed_at` existed has no push time. Both sink, in both directions.
#[test]
fn items_with_unknown_values_go_last_in_both_directions() {
    let prs = [
        authored(1, None),
        authored(2, Some("zed")),
        authored(3, Some("amy")),
    ];
    assert_eq!(item_order(&prs, asc(ItemSortKey::Author)), [3, 2, 1]);
    assert_eq!(item_order(&prs, desc(ItemSortKey::Author)), [2, 3, 1]);

    let prs = [pushed(1, None), pushed(2, Some(1)), pushed(3, Some(2))];
    assert_eq!(item_order(&prs, desc(ItemSortKey::Pushed)), [3, 2, 1]);
    assert_eq!(item_order(&prs, asc(ItemSortKey::Pushed)), [2, 3, 1]);
}

#[test]
fn item_ties_fall_back_to_the_number_whatever_the_direction() {
    let prs = [item(30), item(4), item(17)];
    for sort in ItemSortKey::ALL
        .iter()
        .flat_map(|&key| [asc(key), desc(key)])
    {
        let mut prs = prs.clone();
        for pr in &mut prs {
            pr.title = "same".into();
        }
        assert_eq!(item_order(&prs, sort), [4, 17, 30], "{sort:?}");
    }
}

#[test]
fn ordering_items_only_permutes_the_indices_it_is_given() {
    let prs = [created(1, 1), created(2, 2), created(3, 3), created(4, 4)];
    // Only 1 and 3 survived the filter; 2 and 4 must not reappear.
    let mut visible = vec![PrIx(0), PrIx(2)];
    order_items(&prs, &mut visible, desc(ItemSortKey::Created));
    assert_eq!(visible, [PrIx(2), PrIx(0)]);
}

// --- groups -------------------------------------------------------------

/// A stack is a group of pull requests that sorts as one unit. Text keys
/// use the bottom member — the one the rest build on — so a stack is filed
/// under the name it is known by.
#[test]
fn a_group_sorts_by_its_bottom_member_on_text_keys() {
    let bottom = PullRequest {
        title: "m: base of the stack".into(),
        ..authored(10, Some("mallory"))
    };
    let top = PullRequest {
        title: "a: top of the stack".into(),
        ..authored(11, Some("alice"))
    };
    let group = [&bottom, &top];

    for direction in [SortDirection::Ascending, SortDirection::Descending] {
        assert_eq!(
            sort_key_for_group(&group, ItemSortKey::Title, direction),
            item_sort_value(&bottom, ItemSortKey::Title),
        );
        assert_eq!(
            sort_key_for_group(&group, ItemSortKey::Author, direction),
            item_sort_value(&bottom, ItemSortKey::Author),
        );
    }
}

/// Time keys take whichever member puts the group furthest toward the end
/// being looked for: newest-first sees the group's newest activity,
/// oldest-first its oldest.
#[test]
fn a_group_sorts_by_its_extreme_member_on_time_keys() {
    let bottom = PullRequest {
        created_at: at(10),
        updated_at: at(50),
        pushed_at: Some(at(5)),
        ..item(1)
    };
    let middle = PullRequest {
        created_at: at(20),
        updated_at: at(40),
        pushed_at: None,
        ..item(2)
    };
    let top = PullRequest {
        created_at: at(30),
        updated_at: at(45),
        pushed_at: Some(at(60)),
        ..item(3)
    };
    let group = [&bottom, &middle, &top];
    let time = |secs| Some(SortValue::Time(at(secs)));

    use ItemSortKey::*;
    use SortDirection::*;
    assert_eq!(sort_key_for_group(&group, Created, Descending), time(30));
    assert_eq!(sort_key_for_group(&group, Created, Ascending), time(10));
    assert_eq!(sort_key_for_group(&group, Updated, Descending), time(50));
    assert_eq!(sort_key_for_group(&group, Updated, Ascending), time(40));
    // The middle member's unknown push time is skipped, not treated as 0.
    assert_eq!(sort_key_for_group(&group, Pushed, Descending), time(60));
    assert_eq!(sort_key_for_group(&group, Pushed, Ascending), time(5));
}

#[test]
fn a_group_with_nothing_known_has_no_key() {
    let a = pushed(1, None);
    let b = pushed(2, None);
    assert_eq!(
        sort_key_for_group(&[&a, &b], ItemSortKey::Pushed, SortDirection::Descending),
        None
    );
    assert_eq!(
        sort_key_for_group(&[], ItemSortKey::Title, SortDirection::Ascending),
        None
    );
}

/// A single pull request is a group of one: the two paths cannot disagree.
#[test]
fn a_group_of_one_is_the_item_itself() {
    let pr = PullRequest {
        title: "solo".into(),
        ..authored(7, Some("sam"))
    };
    for &key in ItemSortKey::ALL {
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            assert_eq!(
                sort_key_for_group(&[&pr], key, direction),
                item_sort_value(&pr, key),
                "{key:?} {direction:?}"
            );
        }
    }
}

#[test]
fn groups_compare_against_each_other_and_against_lone_items() {
    // A stack whose top was created last, against a lone pull request
    // created between the stack's bottom and top.
    let bottom = created(1, 10);
    let top = created(2, 30);
    let lone = created(3, 20);
    let stack = [&bottom, &top];

    use std::cmp::Ordering::*;
    // Newest first: the stack's newest member (30) beats the lone 20.
    assert_eq!(
        compare_groups(&stack, &[&lone], desc(ItemSortKey::Created)),
        Less
    );
    // Oldest first: the stack's oldest member (10) beats the lone 20.
    assert_eq!(
        compare_groups(&stack, &[&lone], asc(ItemSortKey::Created)),
        Less
    );
    // Tied groups fall back to their bottom members' numbers.
    let other_bottom = created(5, 10);
    let other_top = created(6, 30);
    assert_eq!(
        compare_groups(
            &[&other_bottom, &other_top],
            &stack,
            desc(ItemSortKey::Created)
        ),
        Greater
    );
}

// --- flatten ------------------------------------------------------------

fn pr_numbers(feed: &crate::Feed, repos: &[RepoState]) -> Vec<(String, u32)> {
    feed.rows()
        .iter()
        .filter_map(|row| match *row {
            FeedRow::PrRow { repo, pr } => {
                let state = &repos[repo.0];
                Some((state.id.to_string(), state.prs[pr.0].number.0))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn flatten_orders_repositories_and_items_by_the_filter_sort() {
    let mut quiet = with_meta("o/quiet", |m| m.pushed_at = Some(at(1)));
    quiet.prs = vec![created(1, 1), created(2, 2)];
    let mut busy = with_meta("o/busy", |m| m.pushed_at = Some(at(9)));
    busy.prs = vec![created(7, 5), created(8, 3), created(9, 4)];
    let repos = [quiet, busy];

    let feed = flatten(&repos, &FeedFilter::default());
    assert_eq!(
        pr_numbers(&feed, &repos),
        [
            ("o/busy".to_string(), 7),
            ("o/busy".to_string(), 9),
            ("o/busy".to_string(), 8),
            ("o/quiet".to_string(), 2),
            ("o/quiet".to_string(), 1),
        ]
    );
    // Indices still address `repos` and `prs` positionally.
    assert_eq!(feed.row(0), Some(FeedRow::RepoHeader { repo: RepoIx(1) }));

    let filter = FeedFilter {
        sort: FeedSort {
            repos: asc(RepoSortKey::Pushed),
            items: asc(ItemSortKey::Created),
        },
        ..FeedFilter::default()
    };
    let feed = flatten(&repos, &filter);
    assert_eq!(
        pr_numbers(&feed, &repos),
        [
            ("o/quiet".to_string(), 1),
            ("o/quiet".to_string(), 2),
            ("o/busy".to_string(), 8),
            ("o/busy".to_string(), 9),
            ("o/busy".to_string(), 7),
        ]
    );
}

/// Sorting must not break the run structure container chrome depends on.
#[test]
fn sorted_repositories_still_form_contiguous_runs() {
    let mut repos: Vec<RepoState> = ["o/c", "o/a", "o/b"]
        .into_iter()
        .zip([3, 1, 2])
        .map(|(name, stars)| {
            let mut state = with_meta(name, |m| m.stars = stars);
            state.prs = vec![created(1, 1), created(2, 2)];
            state
        })
        .collect();
    repos[1].collapsed = true;

    let filter = FeedFilter {
        sort: FeedSort {
            repos: desc(RepoSortKey::Stars),
            ..FeedSort::default()
        },
        ..FeedFilter::default()
    };
    let feed = flatten(&repos, &filter);
    let order: Vec<RepoIx> = feed
        .rows()
        .iter()
        .filter_map(|row| match row {
            FeedRow::RepoHeader { repo } => Some(*repo),
            _ => None,
        })
        .collect();
    assert_eq!(order, [RepoIx(0), RepoIx(2), RepoIx(1)]);

    for ix in 0..repos.len() {
        let positions: Vec<usize> = feed
            .rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| row.repo() == RepoIx(ix))
            .map(|(pos, _)| pos)
            .collect();
        let span = positions[positions.len() - 1] - positions[0] + 1;
        assert_eq!(span, positions.len(), "repo {ix} is not contiguous");
        assert!(matches!(
            feed.row(positions[0]),
            Some(FeedRow::RepoHeader { .. })
        ));
        assert!(matches!(
            feed.row(positions[positions.len() - 1]),
            Some(FeedRow::Spacer { .. })
        ));
    }
}

#[test]
fn sorting_composes_with_filtering() {
    let mut state = repo("o/r");
    state.prs = vec![
        PullRequest {
            is_draft: true,
            ..created(1, 9)
        },
        created(2, 1),
        created(3, 5),
    ];
    let filter = FeedFilter {
        hide_drafts: true,
        ..FeedFilter::default()
    };
    let repos = [state];
    let feed = flatten(&repos, &filter);
    assert_eq!(
        pr_numbers(&feed, &repos),
        [("o/r".to_string(), 3), ("o/r".to_string(), 2)]
    );
}

// --- persistence --------------------------------------------------------

#[test]
fn a_sort_round_trips_through_json() {
    let sort = asc(RepoSortKey::Stars);
    let json = serde_json::to_string(&sort).expect("serialise");
    assert_eq!(json, r#"{"key":"stars","direction":"ascending"}"#);
    let back: Sort<RepoSortKey> = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(back, sort);
}

#[test]
fn a_sort_without_a_direction_takes_the_key_default() {
    let sort: Sort<ItemSortKey> = serde_json::from_str(r#"{"key":"title"}"#).expect("deserialise");
    assert_eq!(sort, asc(ItemSortKey::Title));
}

/// The type system is what keeps stars off the item sort; a hand-edited
/// config cannot sneak it in either.
#[test]
fn a_key_from_the_other_sort_does_not_decode() {
    assert!(serde_json::from_str::<Sort<ItemSortKey>>(r#"{"key":"stars"}"#).is_err());
    assert!(serde_json::from_str::<Sort<RepoSortKey>>(r#"{"key":"author"}"#).is_err());
}

#[test]
fn numbers_are_used_for_ties_not_positions() {
    // Same timestamps, input order reversed: still ascending by number.
    let prs = [item(9), item(2)];
    assert_eq!(item_order(&prs, desc(ItemSortKey::Created)), [2, 9]);
    assert_eq!(prs[1].number, PrNumber(2));
}

/// A client that orders the feed itself gets the listed order back,
/// whatever the filter's sort says.
#[test]
fn flattening_as_listed_keeps_the_listed_order() {
    let mut quiet = with_meta("o/quiet", |m| m.pushed_at = Some(at(1)));
    quiet.prs = vec![created(1, 1), created(2, 2)];
    let mut busy = with_meta("o/busy", |m| m.pushed_at = Some(at(9)));
    busy.prs = vec![created(8, 3), created(7, 5)];
    let repos = [quiet, busy];

    let feed = crate::flatten_in(&repos, &FeedFilter::default(), FeedOrder::AsListed);
    assert_eq!(
        pr_numbers(&feed, &repos),
        [
            ("o/quiet".to_string(), 1),
            ("o/quiet".to_string(), 2),
            ("o/busy".to_string(), 8),
            ("o/busy".to_string(), 7),
        ]
    );

    // And `flatten` is exactly the sorted case.
    assert_eq!(
        flatten(&repos, &FeedFilter::default()),
        crate::flatten_in(
            &repos,
            &FeedFilter::default(),
            FeedOrder::Sorted(FeedSort::default())
        )
    );
}
