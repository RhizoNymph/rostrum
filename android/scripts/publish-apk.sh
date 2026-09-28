#!/usr/bin/env bash
# Publishes the release APK where rostrumd serves it to phones:
#
#   ~/.local/share/rostrum/server/apk/rostrum.apk
#   ~/.local/share/rostrum/server/apk/rostrum.apk.json
#
#   android/scripts/publish-apk.sh [path/to/app-release.apk]
#
# The JSON is a contract with rostrumd; its keys and types must not change:
#   {"version_name": "0.1.0", "version_code": 1, "built_at": "<RFC 3339 UTC>",
#    "sha256": "<lowercase hex>", "size": <bytes>}
# Both files are replaced atomically (temp file in the same directory, then
# rename), the APK first, so the JSON never describes a file that is not there.
set -euo pipefail

ANDROID_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APK="${1:-$ANDROID_DIR/app/build/outputs/apk/release/app-release.apk}"
DEST="$HOME/.local/share/rostrum/server/apk"

die() { printf 'publish-apk: error: %s\n' "$*" >&2; exit 1; }
info() { printf 'publish-apk: %s\n' "$*" >&2; }

[ -f "$APK" ] || die "no APK at $APK. Build it first: android/scripts/build-apk.sh"

# --- Android build tools (aapt2, apksigner) from the newest build-tools -------
sdk_dir=""
if [ -f "$ANDROID_DIR/local.properties" ]; then
    sdk_dir="$(sed -n 's/^sdk\.dir[[:space:]]*=[[:space:]]*//p' "$ANDROID_DIR/local.properties" | tail -n 1)"
fi
sdk_dir="${sdk_dir:-${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}}"
build_tools="$(find "$sdk_dir/build-tools" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -n 1)"
[ -n "$build_tools" ] && [ -x "$build_tools/aapt2" ] || die "no Android build-tools with aapt2 under $sdk_dir/build-tools"
AAPT2="$build_tools/aapt2"
APKSIGNER="$build_tools/apksigner"

# --- Refuse anything a phone could not install as an update -------------------
certs="$("$APKSIGNER" verify --print-certs "$APK" 2>&1)" || die "apksigner rejects $APK: $certs"
if grep -q 'certificate DN: CN=Android Debug' <<<"$certs"; then
    die "$APK is signed with the debug key (android/.env missing at build time?); refusing to publish it"
fi

# --- Metadata -----------------------------------------------------------------
# Capture the whole dump first: piping it into a `sed` that quits early would
# SIGPIPE aapt2 and, under pipefail, abort the script.
badging="$("$AAPT2" dump badging "$APK")" || die "aapt2 cannot read $APK"
package_line="$(sed -n '/^package: /{p;q}' <<<"$badging")"
version_code="$(sed -n "s/.* versionCode='\([^']*\)'.*/\1/p" <<<"$package_line")"
version_name="$(sed -n "s/.* versionName='\([^']*\)'.*/\1/p" <<<"$package_line")"
[[ "$version_code" =~ ^[0-9]+$ ]] || die "unexpected versionCode '$version_code' from aapt2"
# Restricting the characters also means version_name never needs JSON escaping.
[[ "$version_name" =~ ^[0-9A-Za-z._+-]+$ ]] || die "unexpected versionName '$version_name' from aapt2"
built_at="$(date -u -r "$APK" +%Y-%m-%dT%H:%M:%SZ)"

# --- Publish ------------------------------------------------------------------
mkdir -p "$DEST"
tmp_apk="$(mktemp "$DEST/.rostrum.apk.XXXXXX")"
tmp_json="$(mktemp "$DEST/.rostrum.apk.json.XXXXXX")"
trap 'rm -f "$tmp_apk" "$tmp_json"' EXIT

cp "$APK" "$tmp_apk"
# Digest and size of the copy: exactly the bytes that will be served.
sha256="$(sha256sum "$tmp_apk" | cut -d' ' -f1)"
size="$(stat -c %s "$tmp_apk")"
printf '{"version_name": "%s", "version_code": %s, "built_at": "%s", "sha256": "%s", "size": %s}\n' \
    "$version_name" "$version_code" "$built_at" "$sha256" "$size" >"$tmp_json"
chmod 644 "$tmp_apk" "$tmp_json"

mv -f "$tmp_apk" "$DEST/rostrum.apk"
mv -f "$tmp_json" "$DEST/rostrum.apk.json"
trap - EXIT

info "published $DEST/rostrum.apk"
cat "$DEST/rostrum.apk.json"
