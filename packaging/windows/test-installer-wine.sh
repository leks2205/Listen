#!/usr/bin/env bash
# Smoke test of dist/Listen-Setup-<version>.exe under wine, in a throw-away copy of the build prefix:
# silent per-user install, listen.exe --version, --demo --screenshot, silent uninstall.
#
#   packaging/windows/test-installer-wine.sh [path/to/Listen-Setup-x.y.z.exe]
#
# The installer carries no helper programs (mpv, yt-dlp, Deno): Listen installs them with winget.
set -euo pipefail
cd "$(dirname "$0")/../.."

setup="${1:-dist/Listen-Setup-$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2).exe}"
[[ -f "$setup" ]] || { echo "no installer at $setup" >&2; exit 1; }
base="${LISTEN_WINE_PREFIX:-$HOME/.cache/listen-wine-build}"
export WINEPREFIX="${base}-test" WINEDEBUG="${WINEDEBUG:--all}"
rm -rf "$WINEPREFIX"
cp -a "$base" "$WINEPREFIX"
trap 'wineserver -k 2>/dev/null || true; rm -rf "$WINEPREFIX"' EXIT

winpath() { wine winepath.exe "$@" 2>/dev/null | tr -d '\r'; }
ok() { printf '\033[1;32mOK\033[0m   %s\n' "$*"; }
fail() { printf '\033[1;31mFAIL\033[0m %s\n' "$*" >&2; exit 1; }

wine "$(winpath -w "$setup")" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG='C:\setup.log'
appdir=$(winpath -u "$(wine cmd /c 'echo %LOCALAPPDATA%' 2>/dev/null | tr -d '\r')")/Programs/Listen
for f in listen.exe LICENSE NOTICE.md THIRD_PARTY_LICENSES.html unins000.exe; do
  [[ -f "$appdir/$f" ]] || fail "$f missing in $appdir"
done
for f in mpv.exe yt-dlp.exe deno.exe; do
  [[ ! -e "$appdir/$f" ]] || fail "$f must not be bundled"
done
ok "installed into $appdir"
start_menu="$(winpath -u "$(wine cmd /c 'echo %APPDATA%' 2>/dev/null | tr -d '\r')")/Microsoft/Windows/Start Menu/Programs"
[[ -e "$start_menu/Listen.lnk" ]] && ok "Start menu shortcut" || echo "note: no Start menu shortcut in wine ($start_menu); wine's shell-link support is limited"

v=$(wine "$appdir/listen.exe" --version 2>/dev/null | tr -d '\r')
[[ "$v" == Listen* || "$v" == listen* ]] && ok "--version: $v" || fail "--version printed: $v"

shot="$(mktemp -d)/shot.png"
wine "$appdir/listen.exe" --demo --ephemeral --screenshot "$(winpath -w "$shot")" >/dev/null 2>&1 || true
[[ -s "$shot" ]] && ok "--demo --screenshot rendered $(stat -c %s "$shot") bytes: $shot" || fail "no screenshot"

# Upgrade as the in-app updater runs it: same installer, silent, /RELAUNCH; Listen must be started again.
wine "$(winpath -w "$setup")" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /RELAUNCH
sleep 5
wine tasklist.exe 2>/dev/null | grep -qi "listen.exe" && ok "/RELAUNCH started Listen again" || fail "Listen was not relaunched"
wineserver -k 2>/dev/null || true
sleep 2

wine "$appdir/unins000.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
sleep 3
[[ ! -e "$appdir/listen.exe" ]] && ok "uninstalled" || fail "listen.exe still there after uninstall"
