//! UniFFI's bindings generator, built from the same `uniffi` version as the
//! library so the generated Kotlin always matches the scaffolding it calls.
//!
//! Only meaningful on the build host: `uniffi`'s `cli` feature is enabled for
//! non-Android targets only (see `Cargo.toml`), so an Android build of this
//! binary is a stub that refuses to run.

#[cfg(not(target_os = "android"))]
fn main() {
    uniffi::uniffi_bindgen_main()
}

#[cfg(target_os = "android")]
fn main() {
    eprintln!("uniffi-bindgen runs on the build host, not on Android");
    std::process::exit(2);
}
