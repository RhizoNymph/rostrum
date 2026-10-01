//! The Kotlin the Android app is built against generates from the library
//! this crate builds, with the crate's own `uniffi-bindgen` — the exact
//! command the app's build runs (see `docs/features/android_core.md`).

mod support;

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

use support::Scratch;

/// The `librostrum_ffi` cdylib this test run built.
///
/// `cargo test` builds the library as a dependency of the tests and does not
/// copy it up into the profile directory the way `cargo build` does, so the
/// copy beside the binaries may be left over from an older build. The
/// library that was just compiled is the most recently written one under the
/// profile directory: `deps/` in cargo's classic layout, `build/*/out/` in
/// the newer one.
fn built_library(profile: &Path) -> PathBuf {
    let name = format!(
        "{}rostrum_ffi{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    let mut candidates = vec![profile.join(&name), profile.join("deps").join(&name)];
    if let Ok(units) = std::fs::read_dir(profile.join("build").join("rostrum-ffi")) {
        candidates.extend(
            units
                .flatten()
                .map(|unit| unit.path().join("out").join(&name)),
        );
    }
    candidates
        .into_iter()
        .filter_map(|path| {
            let written = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()?;
            Some((written, path))
        })
        .max_by_key(|(written, _): &(SystemTime, PathBuf)| *written)
        .map(|(_, path)| path)
        .unwrap_or_else(|| panic!("no {name} under {}", profile.display()))
}

#[test]
fn kotlin_bindings_generate_from_the_built_library() {
    let bindgen = Path::new(env!("CARGO_BIN_EXE_uniffi-bindgen"));
    let profile = bindgen
        .parent()
        .expect("the binary sits in the profile directory");
    let library = built_library(profile);

    let out = Scratch::new("kotlin");
    let status = Command::new(bindgen)
        .args(["generate", "--library"])
        .arg(&library)
        .args(["--language", "kotlin", "--no-format", "--out-dir"])
        .arg(&out.dir)
        .status()
        .expect("run uniffi-bindgen");
    assert!(status.success(), "uniffi-bindgen failed: {status}");

    let kotlin = std::fs::read_to_string(out.dir.join("uniffi/rostrum_ffi/rostrum_ffi.kt"))
        .expect("generated Kotlin");
    for expected in [
        "package uniffi.rostrum_ffi",
        // The object, its async constructor, and methods from every area.
        "open class RostrumCore",
        "suspend fun `open`(`dataDir`: kotlin.String) : RostrumCore",
        "suspend fun `refreshFeed`(): FeedSnapshot",
        "suspend fun `fileDiff`(`repo`: kotlin.String, `number`: kotlin.UInt, `fileIndex`: kotlin.UInt): FileDiff",
        "suspend fun `addDraft`(",
        "suspend fun `pairWithLink`(",
        "suspend fun `checkNotifications`(): List<NotificationEvent>",
        // Copying the desktop's config, under the names the UI uses.
        "suspend fun `desktopConfig`(): DesktopConfigPreview",
        "suspend fun `copyDesktopConfig`(): Settings",
        "data class DesktopConfigPreview",
        "val `changesAnything`: kotlin.Boolean",
        // Profiles: one registry, a core per profile.
        "open class ProfileRegistry",
        "fun `open`(`rootDir`: kotlin.String): ProfileRegistry",
        "fun `profiles`(): List<ProfileInfo>",
        "fun `activeProfile`(): kotlin.String?",
        "fun `setActiveProfile`(`id`: kotlin.String): ProfileInfo",
        "suspend fun `core`(`id`: kotlin.String): RostrumCore",
        "suspend fun `createTokenProfile`(`label`: kotlin.String): ProfileInfo",
        "suspend fun `pairDesktopWithLink`(`uri`: kotlin.String, `deviceName`: kotlin.String): ProfilePairing",
        "suspend fun `pairDesktopManual`(`host`: kotlin.String, `port`: kotlin.UShort, `fingerprint`: kotlin.String, `code`: kotlin.String, `deviceName`: kotlin.String): ProfilePairing",
        "fun `renameProfile`(`id`: kotlin.String, `label`: kotlin.String): ProfileInfo",
        "fun `setProfileLogin`(`id`: kotlin.String, `login`: kotlin.String?): ProfileInfo",
        "suspend fun `removeProfile`(`id`: kotlin.String)",
        "data class ProfileInfo",
        "data class ProfilePairing",
        "sealed class ProfileKind",
        "val `fingerprintShort`: kotlin.String",
        "val `githubLogin`: kotlin.String?",
        "val `lastUsedMs`: kotlin.Long",
        "class ProfileNotFound(",
        // Errors are one sealed class with a readable description.
        "sealed class RostrumException",
        "fun `describe`(): kotlin.String",
        // Callbacks Kotlin implements.
        "public interface FeedObserver",
        "public interface LogSink",
        "fun `installLogSink`(",
        // `uniffi.toml`'s `android = true` was applied.
        "AndroidSystemCleaner",
    ] {
        assert!(
            kotlin.contains(expected),
            "generated Kotlin lacks {expected:?}"
        );
    }
    // The Pair screen reads links and probes through the registry before any
    // profile (and so any core) exists: both must be on the registry itself,
    // not only on the core.
    let registry = kotlin
        .split("public interface ProfileRegistryInterface")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("the registry's interface");
    for expected in [
        "fun `parsePairingLink`(`uri`: kotlin.String): PairingPreview",
        "suspend fun `probeDesktop`(`host`: kotlin.String, `port`: kotlin.UShort): DesktopProbe",
    ] {
        assert!(
            registry.contains(expected),
            "ProfileRegistry lacks {expected:?}"
        );
    }
    // `close` is reserved for releasing the object; no method may shadow it.
    assert!(!kotlin.contains("suspend fun `close`("));
}
