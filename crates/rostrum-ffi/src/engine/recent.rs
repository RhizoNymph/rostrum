//! A map that keeps only its most recently inserted entries.
//!
//! The detail and diff caches hold whole conversations and parsed diffs; a
//! session that opens a hundred pull requests should not keep a hundred of
//! each in memory. SQLite keeps everything; this keeps what is likely to be
//! looked at again.

use std::{collections::HashMap, collections::VecDeque, hash::Hash};

pub(crate) struct Recent<K, V> {
    capacity: usize,
    order: VecDeque<K>,
    entries: HashMap<K, V>,
}

impl<K: Clone + Eq + Hash, V> Recent<K, V> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            entries: HashMap::new(),
        }
    }

    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key)
    }

    /// Insert or replace, evicting the oldest entry when full. Replacing an
    /// entry makes it the newest.
    pub(crate) fn insert(&mut self, key: K, value: V) {
        if self.entries.insert(key.clone(), value).is_some() {
            self.order.retain(|existing| existing != &key);
        }
        self.order.push_back(key);
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    /// Drop every entry `keep` rejects.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        self.order.retain(|key| keep(key));
        self.entries.retain(|key, _| keep(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oldest_entry_is_evicted_first() {
        let mut recent = Recent::new(2);
        recent.insert("a", 1);
        recent.insert("b", 2);
        recent.insert("c", 3);
        assert_eq!(recent.get(&"a"), None);
        assert_eq!(recent.get(&"b"), Some(&2));
        assert_eq!(recent.get(&"c"), Some(&3));
    }

    #[test]
    fn replacing_an_entry_makes_it_the_newest() {
        let mut recent = Recent::new(2);
        recent.insert("a", 1);
        recent.insert("b", 2);
        recent.insert("a", 10);
        recent.insert("c", 3);
        assert_eq!(recent.get(&"a"), Some(&10));
        assert_eq!(recent.get(&"b"), None);
    }

    #[test]
    fn retain_drops_rejected_entries() {
        let mut recent = Recent::new(4);
        recent.insert(1, "one");
        recent.insert(2, "two");
        recent.retain(|key| *key != 1);
        assert_eq!(recent.get(&1), None);
        assert_eq!(recent.get(&2), Some(&"two"));
        recent.insert(3, "three");
        recent.insert(4, "four");
        recent.insert(5, "five");
        assert_eq!(recent.get(&2), Some(&"two"));
    }
}
