//! Where the bundle lives on disk.
//!
//! The bundle is a file rather than an argument because it is long — commit
//! lists, conflict regions, the pull request body — and because a file the
//! harness reads when *it* is ready is the only hand-off that survives the
//! harness taking a moment to start.
//!
//! Bundles are never deleted by this crate. They are small, live under the
//! cache directory where the OS is free to clear them, and a stale one is
//! overwritten by the next hand-off for the same pull request.

use std::path::{Path, PathBuf};

use crate::error::HandoffError;

/// `<cache>/rostrum/handoff/<session_name>.md`.
///
/// Keyed by the tmux session name so the two share one identity.
pub fn context_path(session_name: &str) -> Result<PathBuf, HandoffError> {
    let cache = dirs::cache_dir().ok_or(HandoffError::NoCacheDir)?;
    Ok(cache
        .join("rostrum")
        .join("handoff")
        .join(format!("{session_name}.md")))
}

/// Write `text` to `path`, atomically.
///
/// The parent directory is created, the text goes to `<path>.tmp`, and the
/// temporary is renamed over the target. A rename is atomic on every
/// filesystem that matters, so a harness opening the file mid-write sees
/// either the previous bundle in full or the new one in full, never a prefix.
/// An existing bundle is always overwritten.
pub fn write_context(path: &Path, text: &str) -> Result<(), HandoffError> {
    let failed = |source| HandoffError::ContextWrite {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    let tmp = temp_path(path);
    std::fs::write(&tmp, text).map_err(failed)?;
    std::fs::rename(&tmp, path).map_err(failed)?;
    tracing::debug!(path = %path.display(), bytes = text.len(), "wrote handoff bundle");
    Ok(())
}

/// `<path>.tmp`, beside the target so the rename stays on one filesystem.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock is after the epoch")
                .as_nanos();
            Self {
                path: std::env::temp_dir().join(format!("rostrum-handoff-{tag}-{unique}")),
            }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn the_context_path_is_the_session_name_under_the_cache_directory() {
        let path = context_path("rostrum-o-n-1").expect("a cache dir exists in tests");
        assert!(
            path.ends_with("rostrum/handoff/rostrum-o-n-1.md"),
            "{}",
            path.display()
        );
    }

    #[test]
    fn writing_creates_missing_parents_and_leaves_no_temporary_behind() {
        let dir = TempDir::new("write");
        let path = dir.path().join("nested/deeper/bundle.md");

        write_context(&path, "# hello\n").expect("writes");

        assert_eq!(std::fs::read_to_string(&path).expect("reads"), "# hello\n");
        assert!(!temp_path(&path).exists(), "temporary was renamed away");
    }

    #[test]
    fn writing_again_overwrites_the_previous_bundle() {
        let dir = TempDir::new("overwrite");
        let path = dir.path().join("bundle.md");

        write_context(&path, "first").expect("writes");
        write_context(&path, "second").expect("writes again");

        assert_eq!(std::fs::read_to_string(&path).expect("reads"), "second");
    }

    #[test]
    fn a_write_that_cannot_happen_names_the_target_path() {
        let dir = TempDir::new("blocked");
        // A *file* where the parent directory should be.
        std::fs::create_dir_all(dir.path()).expect("mkdir");
        let blocker = dir.path().join("not-a-dir");
        std::fs::write(&blocker, "").expect("touch");
        let path = blocker.join("bundle.md");

        let err = write_context(&path, "x").expect_err("refused");
        assert!(
            matches!(err, HandoffError::ContextWrite { path: ref p, .. } if *p == path),
            "{err:?}"
        );
    }
}
