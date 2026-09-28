//! UniFFI's binding generator, built from this workspace so the generator and
//! the scaffolding compiled into `librostrum_ffi` are always the same release.
//!
//! ```sh
//! cargo build -p rostrum-ffi
//! cargo run -p rostrum-ffi --bin uniffi-bindgen -- generate \
//!     --library target/debug/librostrum_ffi.so --language kotlin --out-dir out/
//! ```
//!
//! The generator is a host tool: uniffi's `cli` feature is enabled only for
//! non-Android targets, so an Android build of this crate compiles the stub
//! below instead of the whole generator.

#[cfg(not(target_os = "android"))]
fn main() {
    uniffi::uniffi_bindgen_main()
}

#[cfg(target_os = "android")]
fn main() {
    eprintln!("uniffi-bindgen runs on the build host; build it without an Android target");
    std::process::exit(2);
}
