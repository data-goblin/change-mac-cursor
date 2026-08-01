#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# WORKAROUND: macOS TCC denies launchd and skhd any access under ~/Desktop
# (exit 126), so the installed copy under ~/.local/share wins when present.
default_image() {
    if [ -f "$HOME/.local/share/cmc/cursor.png" ]; then
        printf '%s\n' "$HOME/.local/share/cmc/cursor.png"
    else
        printf '%s\n' "$REPO_ROOT/example-cursors/orc-cursor.png"
    fi
}

CMC="${CMC_BIN:-$HOME/.cargo/bin/cmc}"
IMAGE="${CMC_IMAGE:-$(default_image)}"
SIZE="${CMC_SIZE:-32}"
HOTSPOT_X="${CMC_HOTSPOT_X:-2}"
HOTSPOT_Y="${CMC_HOTSPOT_Y:-2}"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}/cmc/active"

die() { printf '%s\n' "$*" >&2; exit 1; }

[ -x "$CMC" ] || die "cmc not found at $CMC (cargo install --path $REPO_ROOT)"
[ -f "$IMAGE" ] || die "cursor image not found: $IMAGE"

apply() {
    "$CMC" --image "$IMAGE" --size "$SIZE" \
        --hotspot-x "$HOTSPOT_X" --hotspot-y "$HOTSPOT_Y" >/dev/null
    mkdir -p "$(dirname "$STATE")"
    : > "$STATE"
}

# INVARIANT: the marker is cleared even when --restore fails, so the toggle
# cannot latch on after a WindowServer reset has already dropped the cursor.
restore() {
    "$CMC" --restore >/dev/null || true
    rm -f "$STATE"
}

case "${1:-toggle}" in
    on)     apply ;;
    off)    restore ;;
    toggle) if [ -e "$STATE" ]; then restore; else apply; fi ;;
    status) if [ -e "$STATE" ]; then echo custom; else echo default; fi ;;
    *)      die "usage: $(basename "$0") [on|off|toggle|status]" ;;
esac
