#!/usr/bin/env bash
# Regenerate docs/screenshots/tui.png and tui-light.png without a display
# server.
#
# Pipeline: tmux runs the TUI headlessly, `tmux capture-pane -e` grabs the
# pane with ANSI escape codes, `aha` converts that to HTML, and headless
# chromium renders it to PNG — once per terminal palette (dark then light,
# same ANSI bytes). Requires the repo's `screenshots` dev shell:
#
#   nix develop .#screenshots -c demo/screenshot-tui.sh
#
# Assumes a demo daemon is already running (demo/config.toml, port 18420).
set -euo pipefail
cd "$(dirname "$0")/.."

SESSION=barduck-shot
COLS=170
ROWS=55
OUT_DARK=docs/screenshots/tui.png
OUT_LIGHT=docs/screenshots/tui-light.png

# Both palettes render the same ANSI capture: aha emits named/256 colors,
# so only the page's default fg/bg differs between the two shots.
render() {
    local out="$1" wrap="$2"
    chromium --headless --no-sandbox --disable-gpu --hide-scrollbars \
        --force-device-scale-factor=2 \
        --window-size=1450,700 \
        --screenshot="$out" \
        --virtual-time-budget=3000 \
        "file://$wrap" 2>/dev/null
    echo "wrote $out"
}

tmux kill-session -t "$SESSION" 2>/dev/null || true
tmux new-session -d -x "$COLS" -y "$ROWS" -s "$SESSION" \
    './target/release/barduck --config demo/config.toml tui'
sleep 4   # let the TUI paint once

html="$(mktemp /tmp/barduck-tui-XXXX.html)"
trap 'rm -f "$html" "$html.wrap.html" "$html.light.wrap.html"' EXIT
tmux capture-pane -t "$SESSION" -e -p | aha --no-header > "$html"
tmux kill-session -t "$SESSION" 2>/dev/null || true

body() {
    printf '<!doctype html><html><head><meta charset="utf-8"><style>\n'
    printf 'body { background:%s; margin:0; padding:16px; }\n' "$1"
    printf '#term { font-family:"DejaVu Sans Mono",monospace; font-size:14px; line-height:1.3; color:%s; white-space:pre; }\n' "$2"
    printf '</style></head><body><div id="term">\n'
    cat "$html"
    printf '</div></body></html>\n'
}

body '#1b1b1b' '#cccccc' > "$html.wrap.html"
render "$OUT_DARK" "$html.wrap.html"

body '#f5f5f0' '#1b1b1b' > "$html.light.wrap.html"
render "$OUT_LIGHT" "$html.light.wrap.html"
