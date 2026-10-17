#!/usr/bin/env bash
# Builds the per-user Windows installer: dist/Listen-Setup-<version>.exe
#
#   packaging/windows/build-installer.sh [--no-build] [--out DIR]
#
# 1. cross-builds listen.exe (x86_64-pc-windows-gnu), unless --no-build
# 2. assembles the folder (listen.exe, licenses); mpv, yt-dlp and Deno come via winget on first start
# 3. compiles packaging/windows/listen.iss with Inno Setup (hash-pinned) under wine
#
# Needs: wine, curl, sha256sum, cargo-about, the mingw toolchain (see .cargo/config.toml).
# Fedora's wine-core 11.0 hangs creating a fresh prefix (missing winmd files), so a missing prefix
# is copied from ~/.wine, with syswow64 filled from /usr/lib64/wine-wow64/wine/i386-windows.
# Cache: $LISTEN_WIN_CACHE (default ~/.cache/listen-windows). Wine prefix: $LISTEN_WINE_PREFIX.
set -euo pipefail

cd "$(dirname "$0")/../.."
ROOT=$PWD

# ---- pins (update together with the version, hashes are SHA-256 of the downloaded file) ----
# Inno Setup: hash from jrsoftware.org's signed .issig for innosetup-6.7.3.exe.
INNO_URL="https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe"
INNO_SHA256="9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732"

build=1 out="$ROOT/dist"
while (($#)); do
  case "$1" in
    --no-build) build=0; shift ;;
    --out) out="$2"; shift 2 ;;
    *) echo "usage: $0 [--no-build] [--out DIR]" >&2; exit 2 ;;
  esac
done

CACHE="${LISTEN_WIN_CACHE:-$HOME/.cache/listen-windows}"
export WINEPREFIX="${LISTEN_WINE_PREFIX:-$HOME/.cache/listen-wine-build}"
export WINEDEBUG="${WINEDEBUG:--all}"
mkdir -p "$CACHE" "$out"

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }
need() { command -v "$1" >/dev/null || { echo "missing tool: $1" >&2; exit 1; }; }
for t in wine curl sha256sum cargo-about; do need "$t"; done

# fetch URL SHA256 -> path in the cache (re-downloads when the hash does not match)
fetch() {
  local url="$1" sha="$2" file="$CACHE/$(basename "$1")"
  if [[ ! -f "$file" ]] || [[ "$(sha256sum "$file" | cut -d' ' -f1)" != "$sha" ]]; then
    echo "downloading $(basename "$file")" >&2
    curl -fsSL --retry 3 -o "$file.part" "$url"
    mv "$file.part" "$file"
  fi
  [[ "$(sha256sum "$file" | cut -d' ' -f1)" == "$sha" ]] || {
    echo "SHA-256 mismatch for $file (expected $sha); refusing to use it" >&2; rm -f "$file"; exit 1; }
  echo "$file"
}

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
[[ -n "$version" ]] || { echo "no version in Cargo.toml" >&2; exit 1; }

if ((build)); then
  step "listen.exe $version"
  cargo build --release --target x86_64-pc-windows-gnu -p listen
fi
exe="$ROOT/target/x86_64-pc-windows-gnu/release/listen.exe"
[[ -f "$exe" ]] || { echo "$exe missing (run without --no-build)" >&2; exit 1; }

inno=$(fetch "$INNO_URL" "$INNO_SHA256")

stage=$(mktemp -d "${TMPDIR:-/tmp}/listen-stage.XXXXXX")
trap 'rm -rf "$stage"' EXIT
dist="$stage/dist"
mkdir -p "$dist"
cp "$exe" "$dist/listen.exe"
cp LICENSE "$dist/LICENSE"
cp NOTICE.md "$dist/NOTICE.md"
cargo about generate packaging/about.hbs -o "$dist/THIRD_PARTY_LICENSES.html"
# never ship the wine-only stub some test setups put next to the exe
rm -f "$dist/wldap32.dll"

step "Inno Setup under wine"
if [[ ! -d "$WINEPREFIX/drive_c/windows/syswow64" || -z "$(ls -A "$WINEPREFIX/drive_c/windows/syswow64" 2>/dev/null)" ]]; then
  # No usable prefix yet: see the note in the header.
  if [[ ! -d "$WINEPREFIX" ]]; then
    [[ -d "$HOME/.wine/drive_c/windows/system32" ]] || { echo "need an initialised ~/.wine to copy (run 'wineboot' once, it can take long)" >&2; exit 1; }
    cp -a "$HOME/.wine" "$WINEPREFIX"
  fi
  wine32=/usr/lib64/wine-wow64/wine/i386-windows
  [[ -d "$wine32" ]] || { echo "no 32-bit wine DLLs at $wine32" >&2; exit 1; }
  cp -n "$wine32"/* "$WINEPREFIX/drive_c/windows/syswow64/"
fi
if [[ ! -f "$WINEPREFIX/drive_c/InnoSetup/ISCC.exe" ]]; then
  wine "$inno" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER '/DIR=C:\InnoSetup'
fi
[[ -f "$WINEPREFIX/drive_c/InnoSetup/ISCC.exe" ]] || { echo "Inno Setup install failed" >&2; exit 1; }

step "Listen-Setup-$version.exe"
winpath() { wine winepath.exe -w "$1" 2>/dev/null | tr -d '\r'; }
wine 'C:\InnoSetup\ISCC.exe' /Qp "/DAppVersion=$version" "/DDistDir=$(winpath "$dist")" \
  "/DOutputDir=$(winpath "$out")" "$(winpath "$ROOT/packaging/windows/listen.iss")"
ls -l "$out/Listen-Setup-$version.exe"
(cd "$out" && sha256sum "Listen-Setup-$version.exe")
