#!/usr/bin/env bash
# WORKAROUND: everything installs under ~/.local because macOS TCC denies
# launchd and skhd access to ~/Desktop, where this repo lives (exit 126).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CURSOR="${1:-$REPO_ROOT/example-cursors/orc-cursor.png}"

BIN_DIR="$HOME/.local/bin"
SHARE_DIR="$HOME/.local/share/cmc"
AGENT="$HOME/Library/LaunchAgents/com.kurt.cmc.plist"
LABEL="com.kurt.cmc"

[ -f "$CURSOR" ] || { printf 'cursor image not found: %s\n' "$CURSOR" >&2; exit 1; }

mkdir -p "$BIN_DIR" "$SHARE_DIR"

cargo install --path "$REPO_ROOT" --quiet
install -m 0755 "$REPO_ROOT/scripts/cmc-toggle.sh" "$BIN_DIR/cmc-toggle"
install -m 0644 "$CURSOR" "$SHARE_DIR/cursor.png"

cat > "$AGENT" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>$LABEL</string>
    <key>ProgramArguments</key>
    <array>
        <string>$BIN_DIR/cmc-toggle</string>
        <string>on</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
    <key>StandardOutPath</key>
    <string>/tmp/cmc.out.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/cmc.err.log</string>
</dict>
</plist>
PLIST

plutil -lint "$AGENT" >/dev/null
launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$AGENT"

printf 'installed: %s, %s, %s\n' "$BIN_DIR/cmc-toggle" "$SHARE_DIR/cursor.png" "$AGENT"
printf 'bind a hotkey to: %s toggle\n' "$BIN_DIR/cmc-toggle"
