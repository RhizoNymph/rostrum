//! Sorting a stack as one unit: the aggregation hook.
//!
//! A sort key is a property of one pull request. A [`FeedUnit`] can hold
//! several, so the feed needs one value to sort the unit by. The rule:
//!
//! - **Text keys** (title, author, branch — anything alphabetical) take the
//!   **bottom** pull request's value. The bottom names the stack: it is the
//!   one that lands first and the one the rest are built on.
//! - **Time keys** (created, updated, pushed) take the **maximum** across the
//!   visible members when sorting newest first, and the **minimum** when
//!   sorting oldest first. A stack is as recent as its most recent member
//!   when you are looking for recent work, and as old as its oldest when you
//!   are looking for stale work.
//!
//! The feed's sort (feat/feed-sort) defines its keys; it calls
//! [`unit_key`] with the key's [`KeyKind`] and its direction, and compares
//! units by the result. Until then the feed uses [`default_order`].

use std::cmp::Ordering;

use crate::{feed::PrIx, model::PullRequest};

use super::group::FeedUnit;

/// How a key aggregates across a stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// Alphabetical and other identity-like keys: the bottom member's value.
    Text,
    /// Timestamps: the extreme in the direction of the sort.
    Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// The value a unit sorts by, or `None` if none of its members resolve in
/// `prs` (a stale index — sort it last rather than panic).
pub fn unit_key<K: Ord>(
    unit: &FeedUnit,
    prs: &[PullRequest],
    kind: KeyKind,
    direction: SortDirection,
    key: impl Fn(&PullRequest) -> K,
) -> Option<K> {
    let mut members = unit.members().iter().filter_map(|PrIx(ix)| prs.get(*ix));
    match kind {
        KeyKind::Text => members.next().map(key),
        KeyKind::Time => {
            let values = members.map(key);
            match direction {
                SortDirection::Descending => values.max(),
                SortDirection::Ascending => values.min(),
            }
        }
    }
}

/// Compare two units by a key, in the given direction, with units whose key
/// cannot be resolved last either way.
pub fn compare_units<K: Ord>(
    a: &FeedUnit,
    b: &FeedUnit,
    prs: &[PullRequest],
    kind: KeyKind,
    direction: SortDirection,
    key: impl Fn(&PullRequest) -> K,
) -> Ordering {
    let ka = unit_key(a, prs, kind, direction, &key);
    let kb = unit_key(b, prs, kind, direction, &key);
    match (ka, kb) {
        (Some(ka), Some(kb)) => match direction {
            SortDirection::Ascending => ka.cmp(&kb),
            SortDirection::Descending => kb.cmp(&ka),
        },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// The order [`super::group::units`] already produced: the position of each
/// unit's first visible member in the repository's list. A no-op today,
/// named so the feed's sort has one obvious place to replace.
pub fn default_order(_units: &mut [FeedUnit]) {}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::*;
    use crate::test_support::pull;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).expect("valid")
    }

    fn pr(number: u32, title: &str, created: i64) -> PullRequest {
        let mut pr = pull(number);
        pr.title = title.into();
        pr.created_at = at(created);
        pr
    }

    /// Bottom "zeta" created at 100, top "alpha" created at 300; a lone
    /// "middle" created at 200.
    fn fixture() -> (Vec<PullRequest>, FeedUnit, FeedUnit) {
        let prs = vec![
            pr(1, "zeta", 100),
            pr(2, "alpha", 300),
            pr(3, "middle", 200),
        ];
        let stack = FeedUnit::Stack {
            group: 0,
            visible: vec![PrIx(0), PrIx(1)],
        };
        (prs, stack, FeedUnit::Single(PrIx(2)))
    }

    #[test]
    fn text_keys_use_the_bottom_member() {
        let (prs, stack, _) = fixture();
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            assert_eq!(
                unit_key(&stack, &prs, KeyKind::Text, direction, |p| p.title.clone()),
                Some("zeta".to_string())
            );
        }
    }

    #[test]
    fn time_keys_use_the_newest_member_newest_first_and_the_oldest_oldest_first() {
        let (prs, stack, _) = fixture();
        assert_eq!(
            unit_key(
                &stack,
                &prs,
                KeyKind::Time,
                SortDirection::Descending,
                |p| p.created_at
            ),
            Some(at(300))
        );
        assert_eq!(
            unit_key(&stack, &prs, KeyKind::Time, SortDirection::Ascending, |p| p
                .created_at),
            Some(at(100))
        );
    }

    #[test]
    fn a_stack_sorts_as_one_unit_in_both_directions() {
        let (prs, stack, lone) = fixture();
        let mut units = vec![lone.clone(), stack.clone()];
        let created = |p: &PullRequest| p.created_at;

        // Newest first: the stack's newest (300) beats the lone 200.
        units.sort_by(|a, b| {
            compare_units(
                a,
                b,
                &prs,
                KeyKind::Time,
                SortDirection::Descending,
                created,
            )
        });
        assert_eq!(units, vec![stack.clone(), lone.clone()]);

        // Oldest first: the stack's oldest (100) beats the lone 200.
        units.sort_by(|a, b| {
            compare_units(a, b, &prs, KeyKind::Time, SortDirection::Ascending, created)
        });
        assert_eq!(units, vec![stack.clone(), lone.clone()]);

        // Alphabetical: "middle" < "zeta" (the bottom), even though the top
        // member's "alpha" would sort first.
        units.sort_by(|a, b| {
            compare_units(a, b, &prs, KeyKind::Text, SortDirection::Ascending, |p| {
                p.title.clone()
            })
        });
        assert_eq!(units, vec![lone, stack]);
    }

    #[test]
    fn a_single_pull_request_is_its_own_aggregate() {
        let (prs, _, lone) = fixture();
        for kind in [KeyKind::Text, KeyKind::Time] {
            assert_eq!(
                unit_key(&lone, &prs, kind, SortDirection::Ascending, |p| p.number),
                Some(prs[2].number)
            );
        }
    }

    #[test]
    fn unresolvable_units_sort_last_in_either_direction() {
        let (prs, stack, _) = fixture();
        let stale = FeedUnit::Single(PrIx(99));
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            assert_eq!(
                compare_units(&stale, &stack, &prs, KeyKind::Time, direction, |p| p
                    .created_at),
                Ordering::Greater
            );
            assert_eq!(
                compare_units(&stack, &stale, &prs, KeyKind::Time, direction, |p| p
                    .created_at),
                Ordering::Less
            );
        }
    }

    #[test]
    fn hidden_members_do_not_count_toward_the_aggregate() {
        let (prs, _, _) = fixture();
        // Only the bottom is visible: its time is the aggregate both ways.
        let partial = FeedUnit::Stack {
            group: 0,
            visible: vec![PrIx(0)],
        };
        assert_eq!(
            unit_key(
                &partial,
                &prs,
                KeyKind::Time,
                SortDirection::Descending,
                |p| p.created_at
            ),
            Some(at(100))
        );
    }
}
