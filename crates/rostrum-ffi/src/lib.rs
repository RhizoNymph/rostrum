//! The Rust surface the Android app calls through UniFFI.
//!
//! This is a placeholder that only proves the build pipeline links end to end:
//! Gradle → cargo-ndk → `librostrum_ffi.so` → uniffi-bindgen → Kotlin → APK.
//! The real crate replaces the exports below; the crate name (`rostrum_ffi`,
//! which is also the Kotlin package `uniffi.rostrum_ffi` and the `.so` name)
//! and the `uniffi-bindgen` binary must stay as they are, because the Gradle
//! build refers to both. See `docs/features/android_build.md`.

uniffi::setup_scaffolding!();

/// The version of this crate, as declared in its manifest.
#[uniffi::export]
pub fn ffi_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    use super::ffi_version;

    #[test]
    fn ffi_version_is_the_workspace_version() {
        assert_eq!(ffi_version(), "0.1.0");
    }
}
