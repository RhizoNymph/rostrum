#!/usr/bin/env bash
# Install the release APK on a running emulator or device, launch it, and fail
# if the app crashes on the way to its first screen.
#
# The JVM tests and the host smoke test never execute the minified build, and
# R8 breaks things only it can break (a route enum renamed out from under
# Navigation, a reflected class removed). This is the check that runs the APK
# that actually ships.
#
#   android/scripts/smoke-release.sh [path/to/app-release.apk]
#
# Needs `adb` on PATH and exactly one device or emulator attached.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
apk="${1:-$here/../app/build/outputs/apk/release/app-release.apk}"
package="io.github.rhizonymph.rostrum"
activity="$package/.MainActivity"
settle_secs="${ROSTRUM_SMOKE_SETTLE_SECS:-8}"

fail() { echo "smoke-release: $*" >&2; exit 1; }

command -v adb >/dev/null || fail "adb is not on PATH"
[[ -f "$apk" ]] || fail "no APK at $apk (run android/scripts/build-apk.sh first)"

devices="$(adb devices | awk 'NR > 1 && $2 == "device" { print $1 }')"
[[ -n "$devices" ]] || fail "no device or emulator attached"
[[ "$(wc -l <<<"$devices")" -eq 1 ]] || fail "more than one device attached; set ANDROID_SERIAL"

echo "smoke-release: installing $(basename "$apk") on $devices"
adb install -r "$apk" >/dev/null
adb shell am force-stop "$package"
adb logcat -c
adb logcat -c -b crash

echo "smoke-release: launching and waiting ${settle_secs}s"
adb shell am start -W -n "$activity" >/dev/null
sleep "$settle_secs"

crash="$(adb logcat -d -b crash | grep -A 30 "Process: $package" || true)"
if [[ -n "$crash" ]]; then
  echo "$crash" >&2
  fail "the app crashed on launch"
fi
adb shell pidof "$package" >/dev/null || fail "the app is not running after launch"

echo "smoke-release: ok — launched and still running"
