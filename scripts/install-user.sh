#!/usr/bin/env bash
# Per-user install of Kronoa (no sudo): binary, icons, .desktop.
# Usage: scripts/install-user.sh [path/to/kronoa-binary]   (default: builds release)
set -euo pipefail
cd "$(dirname "$0")/.."
BIN="${1:-}"
if [ -z "$BIN" ]; then cargo build --release -p kronoa; BIN="${CARGO_TARGET_DIR:-target}/release/kronoa"; fi
[ -x "$BIN" ] || { echo "no binary: $BIN" >&2; exit 1; }
PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 "$BIN" "$PREFIX/bin/kronoa"
for s in 16 24 32 48 64 128 256 512; do
  install -Dm644 "assets/icons/kronoa-$s.png" "$PREFIX/share/icons/hicolor/${s}x${s}/apps/kronoa.png"
done
sed "s|^Exec=.*|Exec=$PREFIX/bin/kronoa|" packaging/kronoa.desktop > /tmp/kronoa.desktop.$$
install -Dm644 /tmp/kronoa.desktop.$$ "$PREFIX/share/applications/kronoa.desktop"; rm -f /tmp/kronoa.desktop.$$
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$PREFIX/share/icons/hicolor" 2>/dev/null || true
command -v update-desktop-database >/dev/null && update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
echo "Kronoa installed in $PREFIX (run: kronoa)"
