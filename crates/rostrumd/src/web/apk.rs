//! The Android build the page offers: `<apk_dir>/rostrum.apk` and its
//! description `<apk_dir>/rostrum.apk.json`.
//!
//! The JSON is the contract with `android/scripts/publish-apk.sh`:
//!
//! ```json
//! {"version_name": "0.3.0", "version_code": 12,
//!  "built_at": "2026-09-28T12:00:00Z",
//!  "sha256": "<64 hex>", "size": 12345678}
//! ```
//!
//! `built_at` is RFC 3339 (integer Unix seconds are accepted too). An APK
//! whose description is missing, malformed, or describes a file of another
//! size is still offered, marked as unlabelled, so a half-finished publish is
//! visible rather than silently hidden.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use axum::{
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use rostrum_remote::ApiErrorCode;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{daemon::Daemon, http::ApiFailure};

pub const APK_FILE: &str = "rostrum.apk";
pub const META_FILE: &str = "rostrum.apk.json";
pub const PUBLISH_COMMAND: &str = "android/scripts/publish-apk.sh";
pub const APK_MIME: &str = "application/vnd.android.package-archive";

/// What `publish-apk.sh` writes beside the APK.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApkMeta {
    pub version_name: String,
    pub version_code: u32,
    #[serde(deserialize_with = "timestamp")]
    pub built_at: DateTime<Utc>,
    pub sha256: Sha256Hex,
    pub size: u64,
}

/// A SHA-256 digest as 64 lowercase hex characters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256Hex(String);

#[derive(Debug, thiserror::Error)]
#[error("`{0}` is not a SHA-256 digest in hex")]
pub struct Sha256HexError(String);

impl TryFrom<String> for Sha256Hex {
    type Error = Sha256HexError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let trimmed = text.trim();
        if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(Self(trimmed.to_ascii_lowercase()))
        } else {
            Err(Sha256HexError(text))
        }
    }
}

impl From<Sha256Hex> for String {
    fn from(hex: Sha256Hex) -> Self {
        hex.0
    }
}

impl fmt::Display for Sha256Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// RFC 3339, or integer Unix seconds.
fn timestamp<'de, D: Deserializer<'de>>(deserializer: D) -> Result<DateTime<Utc>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Text(String),
        Seconds(i64),
    }
    match Wire::deserialize(deserializer)? {
        Wire::Text(text) => DateTime::parse_from_rfc3339(text.trim())
            .map(|at| at.with_timezone(&Utc))
            .map_err(serde::de::Error::custom),
        Wire::Seconds(secs) => DateTime::from_timestamp(secs, 0)
            .ok_or_else(|| serde::de::Error::custom("timestamp out of range")),
    }
}

/// What is published, as the page describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApkListing {
    /// No `rostrum.apk`.
    Missing,
    Published {
        meta: ApkMeta,
        path: PathBuf,
    },
    /// An APK without a usable description.
    Unlabelled {
        size: u64,
        problem: String,
        path: PathBuf,
    },
}

impl ApkListing {
    /// The file name a browser saves it as.
    pub fn download_name(&self) -> String {
        match self {
            Self::Published { meta, .. } => {
                format!("rostrum-{}.apk", safe_file_part(&meta.version_name))
            }
            Self::Missing | Self::Unlabelled { .. } => APK_FILE.to_string(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Missing => None,
            Self::Published { path, .. } | Self::Unlabelled { path, .. } => Some(path),
        }
    }
}

/// Look at `<apk_dir>` now. Cheap: one stat and one small read.
pub fn inspect(apk_dir: &Path) -> ApkListing {
    let path = apk_dir.join(APK_FILE);
    let size = match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        _ => return ApkListing::Missing,
    };
    let unlabelled = |problem: String| ApkListing::Unlabelled {
        size,
        problem,
        path: path.clone(),
    };
    let bytes = match std::fs::read(apk_dir.join(META_FILE)) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return unlabelled(format!("{META_FILE} is missing"));
        }
        Err(err) => return unlabelled(format!("{META_FILE} could not be read: {err}")),
    };
    match serde_json::from_slice::<ApkMeta>(&bytes) {
        Ok(meta) if meta.size == size => ApkListing::Published { meta, path },
        Ok(meta) => unlabelled(format!(
            "{META_FILE} describes a {}-byte file but {APK_FILE} is {size} bytes",
            meta.size
        )),
        Err(err) => unlabelled(format!("{META_FILE} is not valid: {err}")),
    }
}

/// Keep a version string safe inside a quoted header parameter and a file
/// name: `[A-Za-z0-9._-]`, anything else `_`.
pub fn safe_file_part(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

/// `GET /rostrum.apk`: streamed, with its size and a versioned file name.
pub async fn download(State(daemon): State<Daemon>) -> Response {
    let listing = inspect(&daemon.apk_dir);
    let Some(path) = listing.path() else {
        return no_build();
    };
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "could not open the published APK");
            return no_build();
        }
    };
    let length = match file.metadata().await {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return ApiFailure::internal("could not read the published APK", &error)
                .into_response();
        }
    };
    let disposition = format!("attachment; filename=\"{}\"", listing.download_name());
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(file));
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, APK_MIME.to_string()),
            (header::CONTENT_LENGTH, length.to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        body,
    )
        .into_response()
}

fn no_build() -> Response {
    ApiFailure::new(
        ApiErrorCode::NotFound,
        format!("no build published yet; publish one with {PUBLISH_COMMAND}"),
    )
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsutil::ScratchDir;

    const SHA: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn publish(dir: &Path, apk: &[u8], meta: &str) {
        std::fs::write(dir.join(APK_FILE), apk).expect("apk");
        std::fs::write(dir.join(META_FILE), meta).expect("meta");
    }

    fn meta_json(size: usize) -> String {
        format!(
            r#"{{"version_name":"0.3.0","version_code":12,"built_at":"2026-09-28T12:00:00Z","sha256":"{SHA}","size":{size}}}"#
        )
    }

    #[test]
    fn nothing_published_is_missing() {
        let scratch = ScratchDir::new("apk-missing");
        assert_eq!(inspect(scratch.path()), ApkListing::Missing);
        assert_eq!(inspect(&scratch.join("absent")), ApkListing::Missing);
    }

    #[test]
    fn a_described_apk_is_published() {
        let scratch = ScratchDir::new("apk-published");
        publish(scratch.path(), b"PK-apk", &meta_json(6));
        let ApkListing::Published { meta, .. } = inspect(scratch.path()) else {
            panic!("published");
        };
        assert_eq!(meta.version_name, "0.3.0");
        assert_eq!(meta.version_code, 12);
        assert_eq!(meta.sha256.to_string(), SHA);
        assert_eq!(meta.size, 6);
        assert_eq!(
            meta.built_at,
            DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z").expect("time")
        );
    }

    #[test]
    fn unix_seconds_are_accepted_for_built_at() {
        let scratch = ScratchDir::new("apk-epoch");
        publish(
            scratch.path(),
            b"PK",
            &format!(
                r#"{{"version_name":"1","version_code":1,"built_at":1759060800,"sha256":"{SHA}","size":2}}"#
            ),
        );
        assert!(matches!(
            inspect(scratch.path()),
            ApkListing::Published { .. }
        ));
    }

    #[test]
    fn a_missing_mismatched_or_malformed_description_is_unlabelled() {
        let scratch = ScratchDir::new("apk-unlabelled");
        std::fs::write(scratch.join(APK_FILE), b"PK-apk").expect("apk");
        let problem = |listing: ApkListing| match listing {
            ApkListing::Unlabelled { problem, size, .. } => {
                assert_eq!(size, 6);
                problem
            }
            other => panic!("expected unlabelled, got {other:?}"),
        };
        assert!(problem(inspect(scratch.path())).contains("missing"));

        std::fs::write(scratch.join(META_FILE), meta_json(999)).expect("meta");
        assert!(problem(inspect(scratch.path())).contains("999-byte"));

        std::fs::write(
            scratch.join(META_FILE),
            meta_json(6).replace(SHA, "not-a-digest"),
        )
        .expect("meta");
        assert!(problem(inspect(scratch.path())).contains("not valid"));
    }

    #[test]
    fn the_download_name_carries_a_sanitised_version() {
        let scratch = ScratchDir::new("apk-name");
        publish(
            scratch.path(),
            b"PK",
            &meta_json(2).replace("0.3.0", r#"0.3.0 \"beta\"/x"#),
        );
        assert_eq!(
            inspect(scratch.path()).download_name(),
            "rostrum-0.3.0__beta__x.apk"
        );
        assert_eq!(ApkListing::Missing.download_name(), "rostrum.apk");
    }

    #[test]
    fn digests_are_validated_and_lowercased() {
        assert!(Sha256Hex::try_from(SHA.to_ascii_uppercase()).is_ok());
        assert_eq!(
            Sha256Hex::try_from(SHA.to_ascii_uppercase())
                .expect("valid")
                .to_string(),
            SHA
        );
        assert!(Sha256Hex::try_from("abc".to_string()).is_err());
        assert!(Sha256Hex::try_from("g".repeat(64)).is_err());
    }
}
