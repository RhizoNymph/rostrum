//! JSON state files under `<state_dir>`: read when present, written
//! privately and atomically.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::fsutil::write_private;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{} is not a valid state file: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not write {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Read a JSON state file; `None` when it does not exist.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, StoreError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StoreError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|source| StoreError::Parse {
            path: path.to_path_buf(),
            source,
        })
}

/// Write a JSON state file privately and atomically.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    // Serialising plain records cannot fail.
    let text = serde_json::to_vec_pretty(value).unwrap_or_default();
    write_private(path, &text).map_err(|source| StoreError::Write {
        path: path.to_path_buf(),
        source,
    })
}
