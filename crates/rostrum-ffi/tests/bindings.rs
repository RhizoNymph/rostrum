//! The Kotlin the Android app is built against generates from the library
//! this crate builds, with the crate's own `uniffi-bindgen` — the exact
//! command the app's build runs (see `docs/features/android_core.md`).

mod support;

use std::{path::Path, process::Command};

use support::Scratch;

#[test]
fn kotlin_bindings_generate_from_the_built_library() {
    let bindgen = Path::new(env!("CARGO_BIN_EXE_uniffi-bindgen"));
    let library = bindgen.with_file_name(format!(
        "{}rostrum_ffi{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    assert!(library.exists(), "no library at {}", library.display());

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
    // `close` is reserved for releasing the object; no method may shadow it.
    assert!(!kotlin.contains("suspend fun `close`("));
}
