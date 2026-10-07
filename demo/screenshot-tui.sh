#!/usr/bin/env bash
# Regenerate docs/screenshots/tui.png without a display server.
#
# Pipeline: tmux runs the TUI headlessly, `tmux capture-pane -e` grabs the
# pane with ANSI escape codes, `aha` converts that to HTML, and headless
# chromium renders it to PNG. Requires the repo's `screenshots` dev shell:
#
#   nix develop .#screenshots -c demo/screenshot-tui.sh
#
# Assumes a demo daemon is already running (demo/config.toml, port 18420).
set -euo pipefail
cd "$(dirname "$0")/.."

SESSION=barduck-shot
COLS=170
ROWS=55
OUT=docs/screenshots/tui.png

tmux kill-session -t "$SESSION" 2>/dev/null || true
tmux new-session -d -x "$COLS" -y "$ROWS" -s "$SESSION" \
    './target/release/barduck --config demo/config.toml tui'
sleep 4   # let the TUI paint once

html="$(mktemp /tmp/barduck-tui-XXXX.html)"
wrap="$html.wrap.html"
trap 'rm -f "$html" "$wrap"' EXIT
tmux capture-pane -t "$SESSION" -e -p | aha --no-header > "$html"
tmux kill-session -t "$SESSION" 2>/dev/null || true

cat > "$wrap" <<EOF
<!doctype html><html><head><meta charset="utf-8"><style>
body { background:#1b1b1b; margin:0; padding:16px; }
#term { font-family:"DejaVu Sans Mono",monospace; font-size:14px; line-height:1.3; color:#ccc; white-space:pre; }
</style></head><body><div id="term">
$(cat "$html")
</div></body></html>
EOF

chromium --headless --no-sandbox --disable-gpu --hide-scrollbars \
    --force-device-scale-factor=2 \
    --window-size=1450,700 \
    --screenshot="$OUT" \
    --virtual-time-budget=3000 \
    "file://$wrap"

echo "wrote $OUT"
