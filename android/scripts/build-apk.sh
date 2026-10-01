#!/usr/bin/env bash
# Builds the signed release APK end to end: librostrum_ffi.so for each ABI
# (cargo-ndk), the Kotlin bindings (uniffi-bindgen), then the app itself.
#
#   android/scripts/build-apk.sh [extra gradle args...]
#
# Output: android/app/build/outputs/apk/release/app-release.apk
# See docs/features/android_build.md.
set -euo pipefail

ANDROID_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKSPACE_DIR="$(dirname "$ANDROID_DIR")"
CARGO_BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
# The rustup proxy, never a PATH lookup: `cargo` on PATH may be a shim that
# builds on a remote host without the NDK.
CARGO="${CARGO:-$CARGO_BIN_DIR/cargo}"
RUSTUP="${RUSTUP:-$CARGO_BIN_DIR/rustup}"
RUST_TARGETS=(aarch64-linux-android x86_64-linux-android)
CARGO_NDK_VERSION=4.1.2
SIGNING_BACKUP="$HOME/.config/rostrum/android/release.env"

die() { printf 'build-apk: error: %s\n' "$*" >&2; exit 1; }
info() { printf 'build-apk: %s\n' "$*" >&2; }

# Value of KEY in local.properties, or empty.
local_prop() {
    [ -f "$ANDROID_DIR/local.properties" ] || return 0
    sed -n "s/^$1[[:space:]]*=[[:space:]]*//p" "$ANDROID_DIR/local.properties" | tail -n 1
}

# --- Rust ---------------------------------------------------------------------
[ -x "$CARGO" ] || die "cargo not found at $CARGO. Install rustup (https://rustup.rs), or set CARGO to a local cargo."
[ -x "$RUSTUP" ] || die "rustup not found at $RUSTUP. Install rustup (https://rustup.rs), or set RUSTUP."
toolchain="$(cd "$WORKSPACE_DIR" && "$RUSTUP" show active-toolchain | cut -d' ' -f1)"
installed="$(cd "$WORKSPACE_DIR" && "$RUSTUP" target list --installed)"
for target in "${RUST_TARGETS[@]}"; do
    grep -qx "$target" <<<"$installed" ||
        die "Rust target $target is not installed for $toolchain. Run: rustup target add --toolchain $toolchain ${RUST_TARGETS[*]}"
done
"$CARGO" ndk --version >/dev/null 2>&1 ||
    die "cargo-ndk is not installed. Run: cargo install cargo-ndk --version $CARGO_NDK_VERSION --locked"

# --- Android SDK and NDK ------------------------------------------------------
if [ ! -f "$ANDROID_DIR/local.properties" ]; then
    sdk_guess="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
    [ -d "$sdk_guess/platforms" ] ||
        die "no Android SDK found (tried $sdk_guess). Set ANDROID_HOME or write sdk.dir to $ANDROID_DIR/local.properties."
    info "writing local.properties (sdk.dir=$sdk_guess)"
    printf 'sdk.dir=%s\n' "$sdk_guess" >"$ANDROID_DIR/local.properties"
fi
sdk_dir="$(local_prop sdk.dir)"
sdk_dir="${sdk_dir:-${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}}"
[ -n "$sdk_dir" ] && [ -d "$sdk_dir" ] || die "Android SDK directory '$sdk_dir' does not exist (sdk.dir in local.properties)."

ndk_version="$(sed -n 's/^ndk[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$ANDROID_DIR/gradle/libs.versions.toml")"
[ -n "$ndk_version" ] || die "no 'ndk' version in gradle/libs.versions.toml"
ndk_dir="${ANDROID_NDK_HOME:-$(local_prop ndk.dir)}"
ndk_dir="${ndk_dir:-$sdk_dir/ndk/$ndk_version}"
[ -d "$ndk_dir/toolchains/llvm/prebuilt" ] ||
    die "Android NDK not found at $ndk_dir. Install it with: sdkmanager --install 'ndk;$ndk_version' (or set ANDROID_NDK_HOME)."

# --- JDK ----------------------------------------------------------------------
# Any Java runs the wrapper; the daemon itself needs a JDK 21, which Gradle
# finds among local installations (gradle/gradle-daemon-jvm.properties).
[ -n "${JAVA_HOME:-}" ] || command -v java >/dev/null 2>&1 ||
    die "no Java found to launch Gradle. Install a JDK 21 or set JAVA_HOME."

# --- Signing ------------------------------------------------------------------
if [ ! -f "$ANDROID_DIR/.env" ] && [ -f "$SIGNING_BACKUP" ]; then
    info "restoring android/.env from $SIGNING_BACKUP"
    (umask 077 && cp "$SIGNING_BACKUP" "$ANDROID_DIR/.env")
fi
[ -f "$ANDROID_DIR/.env" ] || [ -n "${ROSTRUM_KEYSTORE:-}" ] ||
    info "warning: no android/.env and no ROSTRUM_* signing variables; the APK will be signed with the DEBUG key"

# --- Build --------------------------------------------------------------------
info "toolchain=$toolchain ndk=$ndk_dir"
cd "$ANDROID_DIR"
CARGO="$CARGO" ANDROID_NDK_HOME="$ndk_dir" ./gradlew --console=plain :app:assembleRelease "$@"

apk="$ANDROID_DIR/app/build/outputs/apk/release/app-release.apk"
[ -f "$apk" ] || die "Gradle finished but $apk is missing"
info "built $apk ($(stat -c %s "$apk") bytes)"
