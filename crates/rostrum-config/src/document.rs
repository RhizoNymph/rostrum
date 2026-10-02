//! `config.json` as a file several writers share.
//!
//! The desktop app and `rostrumd` (on a phone's behalf) both write the file.
//! Neither may lose the other's change, so every write here is:
//!
//! - **a compare-and-swap on the content hash.** A writer remembers the
//!   [`ContentHash`] of what it last read or wrote. If the file still has that
//!   hash, its edit is written as is; if not, someone else wrote in between,
//!   and the edit is re-applied on top of what is there now ([`merge3`]);
//! - **an overlay onto the document**, so keys the writer does not know —
//!   from a newer build, or written by hand — survive ([`crate::shared`]);
//! - **atomic**: a temporary file beside the target, flushed, then renamed
//!   over it, keeping the file's permissions. A reader sees the old file or
//!   the new one, never half of either.
//!
//! The window between reading the hash and the rename is not locked: two
//! processes writing in the same few microseconds can still race. Inside one
//! process writers are serialised by the caller.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{Config, ConfigError, shared::overlay_config};

/// SHA-256 of the file's bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}

/// The file as it is now.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub hash: ContentHash,
    /// The parsed document, with every key it has.
    pub document: serde_json::Value,
    /// The same document read as a [`Config`] (with defaults for anything
    /// absent), or `None` when it is JSON but not a usable config.
    pub config: Option<Config>,
}

/// Read the file. `Ok(None)` when it does not exist.
///
/// A file that is not JSON at all is an error: a writer must not replace a
/// file it cannot read, or a hand-edit with a typo would be erased.
pub fn read(path: &Path) -> Result<Option<Snapshot>, ConfigError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let document: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|source| ConfigError::Malformed {
            path: path.to_path_buf(),
            source,
        })?;
    let config = serde_json::from_value::<Config>(document.clone()).ok();
    Ok(Some(Snapshot {
        hash: ContentHash::of(&bytes),
        document,
        config,
    }))
}

/// The hash of the file as it is now, or `None` when it does not exist or
/// cannot be read. Cheap enough to poll: one read of a small file.
pub fn current_hash(path: &Path) -> Option<ContentHash> {
    fs::read(path).ok().map(|bytes| ContentHash::of(&bytes))
}

/// Write `document` to `path` atomically, pretty-printed, keeping the
/// existing file's permissions. Returns the hash of what was written.
pub fn write_atomic(path: &Path, document: &serde_json::Value) -> Result<ContentHash, ConfigError> {
    let io = |source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    };
    let mut bytes = serde_json::to_vec_pretty(document).map_err(ConfigError::Serialize)?;
    bytes.push(b'\n');
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(io)?;
    let tmp = tmp_sibling(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        if let Ok(existing) = fs::metadata(path) {
            file.set_permissions(existing.permissions())?;
        }
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if let Err(source) = result {
        let _ = fs::remove_file(&tmp);
        return Err(io(source));
    }
    Ok(ContentHash::of(&bytes))
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(format!(".tmp-{}", std::process::id()));
    path.with_file_name(name)
}

/// Re-apply an edit made from `base` (`mine`) on top of `disk`, field by
/// field: a field `mine` changed from `base` takes `mine`'s value; every other
/// field takes `disk`'s. So an edit wins only where it was actually made, and
/// everything someone else changed in the meantime is kept.
///
/// Listing every field (no `..`) is deliberate: a field added to [`Config`]
/// does not compile here until someone decides how it merges.
pub fn merge3(base: &Config, disk: &Config, mine: &Config) -> Config {
    fn pick<T: Clone + PartialEq>(base: &T, disk: &T, mine: &T) -> T {
        if mine != base {
            mine.clone()
        } else {
            disk.clone()
        }
    }
    Config {
        repos: pick(&base.repos, &disk.repos, &mine.repos),
        refresh_secs: pick(&base.refresh_secs, &disk.refresh_secs, &mine.refresh_secs),
        prs_per_repo: pick(&base.prs_per_repo, &disk.prs_per_repo, &mine.prs_per_repo),
        issues_per_repo: pick(
            &base.issues_per_repo,
            &disk.issues_per_repo,
            &mine.issues_per_repo,
        ),
        feed_tab: pick(&base.feed_tab, &disk.feed_tab, &mine.feed_tab),
        notifications: pick(
            &base.notifications,
            &disk.notifications,
            &mine.notifications,
        ),
        notify_review_requests: pick(
            &base.notify_review_requests,
            &disk.notify_review_requests,
            &mine.notify_review_requests,
        ),
        hide_empty_repos: pick(
            &base.hide_empty_repos,
            &disk.hide_empty_repos,
            &mine.hide_empty_repos,
        ),
        clones: pick(&base.clones, &disk.clones, &mine.clones),
        autostash: pick(&base.autostash, &disk.autostash, &mine.autostash),
        conflict_handler: pick(
            &base.conflict_handler,
            &disk.conflict_handler,
            &mine.conflict_handler,
        ),
        hide_drafts: pick(&base.hide_drafts, &disk.hide_drafts, &mine.hide_drafts),
        authors: pick(&base.authors, &disk.authors, &mine.authors),
        include_involved: pick(
            &base.include_involved,
            &disk.include_involved,
            &mine.include_involved,
        ),
        trunks: pick(&base.trunks, &disk.trunks, &mine.trunks),
        repo_sort: pick(&base.repo_sort, &disk.repo_sort, &mine.repo_sort),
        item_sort: pick(&base.item_sort, &disk.item_sort, &mine.item_sort),
    }
}

/// What a compare-and-swap save wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    /// The config now on disk: `mine`, plus anything merged in from a write
    /// that happened since `base` was read.
    pub config: Config,
    pub hash: ContentHash,
    /// Whether the file had changed since `base`, so the result is a merge
    /// and not just `mine`.
    pub merged: bool,
}

/// Save `mine`, an edit of `base` (read or written when the file had hash
/// `base_hash`), without losing anyone else's write.
///
/// - The file still has `base_hash` (or does not exist): `mine` is written.
/// - It changed: it is read again and `mine`'s edits are re-applied on top
///   ([`merge3`]); the merge is written.
/// - It changed into JSON that is not a usable config: `mine` is written,
///   overlaid on the document so its other keys survive.
///
/// Every write overlays onto the existing document, keeping keys this build
/// does not know, and is atomic.
pub fn save_merged(
    path: &Path,
    base: &Config,
    base_hash: Option<ContentHash>,
    mine: &Config,
) -> Result<Saved, ConfigError> {
    let snapshot = read(path)?;
    let (mut document, config, merged) = match snapshot {
        None => (serde_json::Value::Null, mine.clone(), false),
        Some(snapshot) if Some(snapshot.hash) == base_hash => {
            (snapshot.document, mine.clone(), false)
        }
        Some(snapshot) => {
            let merged = match &snapshot.config {
                Some(disk) => merge3(base, disk, mine),
                None => mine.clone(),
            };
            (snapshot.document, merged, true)
        }
    };
    overlay_config(&mut document, &config);
    let hash = write_atomic(path, &document)?;
    Ok(Saved {
        config,
        hash,
        merged,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use rostrum_core::{ItemSortKey, LoginKey, Sort};
    use serde_json::json;

    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(tag: &str) -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "rostrum-config-doc-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("dir");
            Self(path)
        }

        fn file(&self) -> PathBuf {
            self.0.join("config.json")
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // --- merge3 ---------------------------------------------------------------

    #[test]
    fn an_edit_wins_only_where_it_was_made() {
        let base = Config::default();
        let mut disk = base.clone();
        disk.repos = vec!["phone/pushed".into()];
        disk.prs_per_repo = 50;
        let mut mine = base.clone();
        mine.hide_drafts = true;
        mine.refresh_secs = 30;

        let merged = merge3(&base, &disk, &mine);
        assert_eq!(merged.repos, vec!["phone/pushed".to_string()]);
        assert_eq!(merged.prs_per_repo, 50);
        assert!(merged.hide_drafts);
        assert_eq!(merged.refresh_secs, 30);
    }

    #[test]
    fn when_both_changed_a_field_the_edit_wins() {
        let base = Config::default();
        let mut disk = base.clone();
        disk.prs_per_repo = 50;
        let mut mine = base.clone();
        mine.prs_per_repo = 10;
        assert_eq!(merge3(&base, &disk, &mine).prs_per_repo, 10);
    }

    /// Every field: unchanged edits take the disk entirely, and a full edit
    /// over an unchanged disk is the edit entirely.
    #[test]
    fn merge3_covers_every_field() {
        let base = Config::default();
        let mut changed = base.clone();
        changed.repos = vec!["x/y".into()];
        changed.refresh_secs = 11;
        changed.prs_per_repo = 12;
        changed.issues_per_repo = 13;
        changed.feed_tab = rostrum_core::FeedTab::Issues;
        changed.notifications = true;
        changed.notify_review_requests = true;
        changed.hide_empty_repos = !base.hide_empty_repos;
        changed.clones.insert("x/y".into(), PathBuf::from("/src/y"));
        changed.autostash = true;
        changed.conflict_handler = Some(crate::ConflictHandler {
            command: "h {context}".into(),
        });
        changed.hide_drafts = true;
        changed.authors = BTreeSet::from([LoginKey::new("ada")]);
        changed.include_involved = true;
        changed.trunks.insert("x/y".into(), vec!["main".into()]);
        changed.repo_sort.reverse();
        changed.item_sort = Sort::new(ItemSortKey::Title);

        assert_eq!(merge3(&base, &changed, &base), changed, "disk kept whole");
        assert_eq!(merge3(&base, &base, &changed), changed, "edit kept whole");
        // Serialised, every key differs — so the two assertions above cover
        // every key the file has.
        let a = serde_json::to_value(&base).expect("json");
        let b = serde_json::to_value(&changed).expect("json");
        for (key, value) in a.as_object().expect("object") {
            assert_ne!(Some(value), b.get(key), "{key} not exercised");
        }
    }

    // --- writing --------------------------------------------------------------

    #[test]
    fn an_atomic_write_keeps_permissions_and_leaves_no_temporary() {
        let dir = Dir::new("atomic");
        let path = dir.file();
        fs::write(&path, "{}").expect("seed");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).expect("chmod");
        let hash = write_atomic(&path, &json!({"repos": ["a/b"]})).expect("writes");
        assert_eq!(current_hash(&path), Some(hash));
        assert_eq!(
            fs::metadata(&path).expect("meta").permissions().mode() & 0o777,
            0o640
        );
        let names: Vec<_> = fs::read_dir(&dir.0)
            .expect("list")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("config.json")]);
        let read = read(&path).expect("reads").expect("exists");
        assert_eq!(read.document, json!({"repos": ["a/b"]}));
    }

    #[test]
    fn a_file_that_is_not_json_is_an_error_and_is_left_alone() {
        let dir = Dir::new("malformed");
        let path = dir.file();
        fs::write(&path, "{ not json").expect("seed");
        assert!(matches!(read(&path), Err(ConfigError::Malformed { .. })));
        assert!(save_merged(&path, &Config::default(), None, &Config::default()).is_err());
        assert_eq!(fs::read_to_string(&path).expect("read"), "{ not json");
    }

    #[test]
    fn a_missing_file_reads_as_none() {
        let dir = Dir::new("missing");
        assert_eq!(read(&dir.file()).expect("ok"), None);
        assert_eq!(current_hash(&dir.file()), None);
    }

    // --- compare-and-swap saves ---------------------------------------------

    #[test]
    fn an_unchanged_file_is_simply_replaced_by_the_edit() {
        let dir = Dir::new("cas-clean");
        let path = dir.file();
        let base = Config::default();
        let first = save_merged(&path, &base, None, &base).expect("first save");
        let mut mine = base.clone();
        mine.hide_drafts = true;
        let saved = save_merged(&path, &base, Some(first.hash), &mine).expect("save");
        assert!(!saved.merged);
        assert_eq!(saved.config, mine);
        assert_eq!(current_hash(&path), Some(saved.hash));
    }

    #[test]
    fn an_external_write_is_kept_and_the_edit_reapplied_on_top() {
        let dir = Dir::new("cas-merge");
        let path = dir.file();
        let base = Config::default();
        let first = save_merged(&path, &base, None, &base).expect("first save");

        // Someone else (rostrumd, for a phone) writes new repositories, plus
        // a key this build has never heard of.
        let mut document = read(&path).expect("read").expect("exists").document;
        document["repos"] = json!(["phone/one", "phone/two"]);
        document["from_the_future"] = json!({"x": 1});
        write_atomic(&path, &document).expect("external write");

        // Meanwhile the desktop's user ticked "hide drafts", from `base`.
        let mut mine = base.clone();
        mine.hide_drafts = true;
        let saved = save_merged(&path, &base, Some(first.hash), &mine).expect("save");

        assert!(saved.merged);
        assert!(saved.config.hide_drafts, "the edit is kept");
        assert_eq!(
            saved.config.repos,
            vec!["phone/one".to_string(), "phone/two".to_string()],
            "the external write is kept"
        );
        let on_disk = read(&path).expect("read").expect("exists");
        assert_eq!(on_disk.config.as_ref(), Some(&saved.config));
        assert_eq!(on_disk.document["from_the_future"], json!({"x": 1}));
        assert_eq!(on_disk.hash, saved.hash);
    }

    #[test]
    fn unknown_keys_survive_a_plain_save_too() {
        let dir = Dir::new("cas-unknown");
        let path = dir.file();
        fs::write(
            &path,
            r#"{"repos": ["a/b"], "hand_written_note": "keep me"}"#,
        )
        .expect("seed");
        let snapshot = read(&path).expect("read").expect("exists");
        let base = snapshot.config.clone().expect("config");
        let mut mine = base.clone();
        mine.autostash = true;
        save_merged(&path, &base, Some(snapshot.hash), &mine).expect("save");
        let after = read(&path).expect("read").expect("exists");
        assert_eq!(after.document["hand_written_note"], "keep me");
        assert_eq!(after.document["autostash"], true);
    }
}
