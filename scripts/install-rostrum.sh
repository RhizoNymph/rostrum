#!/usr/bin/env bash
# Build the rostrum desktop app, install it to ~/.local/bin, and register it
# as the handler for rostrum:// links (rostrum://pr?…, rostrum://issue?…), so
# another app's "Open in Rostrum" opens the pull request or issue here.
#
#   bash scripts/install-rostrum.sh              # build, install, register
#   bash scripts/install-rostrum.sh --entry-only # register an installed binary
#
# What it writes:
#   ~/.local/bin/rostrum
#   ${XDG_DATA_HOME:-~/.local/share}/applications/rostrum.desktop
#   the x-scheme-handler/rostrum default in ~/.config/mimeapps.list (xdg-mime)
#
# CARGO overrides the cargo binary (default: ~/.cargo/bin/cargo, else cargo
# on PATH). CARGO_TARGET_DIR is honoured.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin_dir="$HOME/.local/bin"
apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
entry="rostrum.desktop"

build=1
case "${1:-}" in
    "") ;;
    --entry-only) build=0 ;;
    *)
        echo "usage: $0 [--entry-only]" >&2
        exit 2
        ;;
esac

if [[ "$build" == 1 ]]; then
    if [[ -n "${CARGO:-}" ]]; then
        cargo="$CARGO"
    elif [[ -x "$HOME/.cargo/bin/cargo" ]]; then
        cargo="$HOME/.cargo/bin/cargo"
    else
        cargo="cargo"
    fi
    echo "==> building rostrum (release)"
    "$cargo" build --release --locked -p rostrum --manifest-path "$repo/Cargo.toml"

    built="${CARGO_TARGET_DIR:-$repo/target}/release/rostrum"
    if [[ ! -x "$built" ]]; then
        echo "error: the build succeeded but $built is missing" >&2
        exit 1
    fi
    # Rename into place: a running rostrum keeps its old inode.
    echo "==> installing $bin_dir/rostrum"
    mkdir -p "$bin_dir"
    install -m 0755 "$built" "$bin_dir/.rostrum.new"
    mv -f "$bin_dir/.rostrum.new" "$bin_dir/rostrum"
elif [[ ! -x "$bin_dir/rostrum" ]]; then
    echo "error: --entry-only needs an installed $bin_dir/rostrum" >&2
    exit 1
fi

# Launchers do not always have ~/.local/bin on PATH, so the entry names the
# binary by its absolute path (quoted, per the desktop entry spec).
echo "==> installing $apps_dir/$entry"
mkdir -p "$apps_dir"
exec_line="Exec=\"$bin_dir/rostrum\" %u"
while IFS= read -r line; do
    if [[ "$line" == Exec=* ]]; then
        printf '%s\n' "$exec_line"
    else
        printf '%s\n' "$line"
    fi
done <"$repo/packaging/desktop/$entry" >"$apps_dir/.$entry.new"
mv -f "$apps_dir/.$entry.new" "$apps_dir/$entry"

if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$apps_dir/$entry" || echo "warning: desktop-file-validate complained (above)" >&2
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$apps_dir" || true
fi

if command -v xdg-mime >/dev/null 2>&1; then
    echo "==> registering $entry for x-scheme-handler/rostrum"
    xdg-mime default "$entry" x-scheme-handler/rostrum
    echo "    handler is now: $(xdg-mime query default x-scheme-handler/rostrum)"
else
    echo "warning: xdg-mime is not installed; rostrum:// links will not open rostrum" >&2
    echo "         (install xdg-utils, then rerun with --entry-only)" >&2
fi

echo
echo "Try it:  xdg-open 'rostrum://pr?repo=RhizoNymph%2Frostrum&number=1'"
