#!/usr/bin/env bash
# Build the Windows installer:
#
#   dist/windows/Pausetape/                 what it installs: the app, ffmpeg, the licences
#   dist/Pausetape-<version>-setup.exe      per-user installer; FFmpeg optional, ticked by default
#   dist/Pausetape-<version>-setup.exe.sha256
#
# Not signed yet (docs/DISTRIBUTION.md §4), so SmartScreen warns on first run: "More info", then
# "Run anyway".
#
# Runs on Windows, in Git Bash. Needs Rust (MSVC), NSIS (makensis, or MAKENSIS=<path>), Python 3
# (or PYTHON=<path>),
# and ffmpeg in target/ffmpeg-windows from scripts/build-ffmpeg-windows.sh, which runs on Linux
# (CI builds it there and brings it over).
#
# Usage: scripts/package-windows.sh
set -euo pipefail

# Placeholder name; change together with APP_NAME in src/main.rs and the macOS script.
APP_NAME="Pausetape"

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
DIST="$ROOT/dist"
STAGE="$DIST/windows/$APP_NAME"
FFMPEG="$ROOT/target/ffmpeg-windows"

echo "== $APP_NAME $VERSION"
[[ -f "$FFMPEG/ffmpeg.exe" ]] || { echo "no $FFMPEG/ffmpeg.exe: run scripts/build-ffmpeg-windows.sh on Linux first"; exit 1; }
makensis=${MAKENSIS:-$(command -v makensis || echo "/c/Program Files (x86)/NSIS/makensis.exe")}
[[ -x "$makensis" ]] || { echo "needs NSIS (makensis): winget install NSIS.NSIS, or set MAKENSIS"; exit 1; }
# `python` and `python3` can be the Microsoft Store's stand-ins, which run nothing: the first
# that does (PYTHON=<path> to choose).
python=""
for p in ${PYTHON:+"$PYTHON"} python python3; do
    if "$p" -c "" 2> /dev/null; then python=$p; break; fi
done
[[ -n "$python" ]] || { echo "needs Python 3 (or PYTHON=<path>)"; exit 1; }

echo "== building"
# Cargo says where build.rs put the icon it made for the .exe (for the installer and uninstaller
# too): where that folder sits inside `target` changes between Cargo versions.
out_dir=$(cargo build --release --locked --message-format=json-render-diagnostics | "$python" -c '
import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "build-script-executed" and "path+file:" in m["package_id"]:
        print(m["out_dir"].replace("\\", "/"))
')
[[ -f target/release/pausetape.exe ]] || { echo "build failed"; exit 1; }
icon="$out_dir/icon.ico"
[[ -f "$icon" ]] || { echo "build.rs made no icon (looked in \"$out_dir\")"; exit 1; }

echo "== assembling $STAGE"
rm -rf "$STAGE" && mkdir -p "$STAGE/Licenses/FFmpeg" "$STAGE/Licenses/Fonts"
cp target/release/pausetape.exe "$STAGE/"
# Next to the app's .exe, where it looks for it first.
cp "$FFMPEG/ffmpeg.exe" "$STAGE/"
cp LICENSE "$STAGE/Licenses/LICENSE.txt"
cp "$FFMPEG/COPYING.LGPLv2.1" "$FFMPEG/COPYING.LAME" "$FFMPEG/BUILD-INFO.txt" "$STAGE/Licenses/FFmpeg/"
cp assets/fonts/*.txt "$STAGE/Licenses/Fonts/"
"$python" -I scripts/third-party-notices.py x86_64-pc-windows-msvc > "$STAGE/Licenses/THIRD-PARTY.txt"

echo "== installer"
setup="$DIST/$APP_NAME-$VERSION-setup.exe"
win() { if command -v cygpath > /dev/null; then cygpath -w "$1"; else echo "$1"; fi; }
"$makensis" -V2 -DAPP_NAME="$APP_NAME" -DVERSION="$VERSION" -DSTAGE="$(win "$STAGE")" -DICON="$(win "$icon")" \
    -DOUTFILE="$(win "$setup")" "$(win "$ROOT/assets/windows/installer.nsi")"
(cd "$DIST" && sha256sum "$(basename "$setup")" > "$(basename "$setup").sha256")

echo "== done"
echo "   $(du -sh "$STAGE" | cut -f1)  $STAGE"
echo "   $(du -sh "$setup" | cut -f1)  $setup"
