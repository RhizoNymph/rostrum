#!/usr/bin/env bash
# Build rostrumd, install it to ~/.local/bin and its systemd user unit to
# ~/.config/systemd/user, then enable and start it (or restart it if it is
# already running).
#
#   bash scripts/install-rostrumd.sh
#
# CARGO overrides the cargo binary (default: ~/.cargo/bin/cargo, else cargo
# on PATH). CARGO_TARGET_DIR is honoured.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -n "${CARGO:-}" ]]; then
    cargo="$CARGO"
elif [[ -x "$HOME/.cargo/bin/cargo" ]]; then
    cargo="$HOME/.cargo/bin/cargo"
else
    cargo="cargo"
fi
bin_dir="$HOME/.local/bin"
unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
unit="rostrumd.service"

echo "==> building rostrumd (release)"
"$cargo" build --release --locked -p rostrumd --manifest-path "$repo/Cargo.toml"

built="${CARGO_TARGET_DIR:-$repo/target}/release/rostrumd"
if [[ ! -x "$built" ]]; then
    echo "error: the build succeeded but $built is missing" >&2
    exit 1
fi

# Rename into place: atomic on one filesystem, and a running rostrumd keeps
# executing the old inode until it is restarted below.
echo "==> installing $bin_dir/rostrumd"
mkdir -p "$bin_dir"
install -m 0755 "$built" "$bin_dir/.rostrumd.new"
mv -f "$bin_dir/.rostrumd.new" "$bin_dir/rostrumd"

echo "==> installing $unit_dir/$unit"
mkdir -p "$unit_dir"
install -m 0644 "$repo/packaging/systemd/$unit" "$unit_dir/.$unit.new"
mv -f "$unit_dir/.$unit.new" "$unit_dir/$unit"

systemctl --user daemon-reload
systemctl --user enable "$unit" >/dev/null
if systemctl --user is-active --quiet "$unit"; then
    echo "==> restarting $unit"
    systemctl --user restart "$unit"
else
    echo "==> starting $unit"
    systemctl --user start "$unit"
fi

port="$(python3 -c 'import json,os
p=os.path.expanduser("~/.config/rostrum/rostrumd.json")
try: print(json.load(open(p)).get("http_port", 8484))
except Exception: print(8484)' 2>/dev/null || echo 8484)"

# Give it a moment to bind, then report.
for _ in $(seq 1 20); do
    curl -fsS -o /dev/null --max-time 1 "http://127.0.0.1:$port/" 2>/dev/null && break
    sleep 0.5
done
systemctl --user --no-pager --lines=0 status "$unit" || true
echo
echo "Pairing page: http://localhost:$port/"
echo "Logs:         journalctl --user -u rostrumd -f"
