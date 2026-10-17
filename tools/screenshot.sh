#!/usr/bin/env bash
# Headless screenshot of Listen at an exact size, on a virtual X screen (KWin ignores requested sizes).
# Usage: tools/screenshot.sh OUT.png WxH [listen args…]   e.g. --demo --route albums
set -euo pipefail
out=$1 size=$2; shift 2
bin=${LISTEN_BIN:-target/release/listen}
env -u WAYLAND_DISPLAY xvfb-run -a -s "-screen 0 3840x2160x24" \
  "$bin" --screenshot "$out" --size "$size" "$@"
