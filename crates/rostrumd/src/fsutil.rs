//! Private files, written atomically.
//!
//! Everything `rostrumd` persists — the TLS key, the paired devices, the
//! handoff record — is written the same way: to a sibling temporary file with
//! mode `0600`, flushed to disk, then renamed over the target. A reader (or a
//! crash) sees the old file or the new one, never half of either.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// Owner read/write only.
pub const PRIVATE_FILE: u32 = 0o600;
/// Owner only.
pub const PRIVATE_DIR: u32 = 0o700;

/// Write `bytes` to `path` atomically with mode `0600`, creating the parent
/// directory (mode `0700`) if it does not exist.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let tmp = tmp_sibling(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(PRIVATE_FILE)
            .open(&tmp)?;
        // `mode` applies only when the file is created; a stale temporary left
        // by a crash keeps whatever it had, so set it explicitly.
        file.set_permissions(fs::Permissions::from_mode(PRIVATE_FILE))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Create `dir` (and its parents) if missing, and make `dir` itself `0700`.
/// Parents that already exist are left alone.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(PRIVATE_DIR))
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// A scratch directory removed on drop, for tests in this crate and its
/// integration tests. Matches the pattern `rostrum-config` and
/// `rostrum-local` use rather than adding a dependency for it.
#[derive(Debug)]
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    pub fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rostrumd-{tag}-{}-{n}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or_default()
        ));
        let _ = fs::remove_dir_all(&path);
        // A scratch directory that cannot be created is a broken test
        // environment, not a condition to handle.
        fs::create_dir_all(&path).expect("scratch directory");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    #[test]
    fn a_private_write_is_0600_in_a_0700_directory_with_no_leftovers() {
        let scratch = ScratchDir::new("fsutil");
        let target = scratch.join("state/devices.json");
        write_private(&target, b"{}").expect("write");
        assert_eq!(fs::read(&target).expect("read"), b"{}");
        assert_eq!(mode(&target), 0o600);
        assert_eq!(mode(&scratch.join("state")), 0o700);
        let names: Vec<_> = fs::read_dir(scratch.join("state"))
            .expect("list")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("devices.json")]);
    }

    #[test]
    fn rewriting_replaces_the_content_and_keeps_the_mode() {
        let scratch = ScratchDir::new("fsutil-rewrite");
        let target = scratch.join("f.json");
        write_private(&target, b"one").expect("write");
        write_private(&target, b"two, longer").expect("rewrite");
        assert_eq!(fs::read(&target).expect("read"), b"two, longer");
        assert_eq!(mode(&target), 0o600);
    }

    #[test]
    fn a_stale_temporary_with_loose_permissions_is_tightened() {
        let scratch = ScratchDir::new("fsutil-stale");
        let target = scratch.join("f.json");
        let stale = scratch.join("f.json.tmp");
        fs::write(&stale, b"junk").expect("stale");
        fs::set_permissions(&stale, fs::Permissions::from_mode(0o644)).expect("chmod");
        write_private(&target, b"fresh").expect("write");
        assert_eq!(mode(&target), 0o600);
        assert!(!stale.exists());
    }
}
