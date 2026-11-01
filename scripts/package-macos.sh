#!/usr/bin/env bash
# Build the macOS app and its disk image:
#
#   dist/Pausetape.app                 universal (Apple Silicon + Intel), ffmpeg inside
#   dist/Pausetape-<version>.dmg       drag-to-Applications disk image
#   dist/Pausetape-<version>.dmg.sha256
#
# Not signed with a Developer ID yet (no Apple Developer Program): the app is ad-hoc signed,
# so on first launch macOS asks the user to approve it (see "Read me first.txt" in the DMG).
#
# Usage: scripts/package-macos.sh
set -euo pipefail

# Placeholder name and identifier; change together with APP_NAME in src/main.rs.
APP_NAME="Pausetape"
BUNDLE_ID="io.github.pausetape.app"
MIN_MACOS=12.0

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"
BIN=pausetape

echo "== $APP_NAME $VERSION"

echo "== building (arm64, x86_64)"
export MACOSX_DEPLOYMENT_TARGET=$MIN_MACOS
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked --target "$target" 2>&1 | grep -E "^(warning|error)|Finished" || true
    [[ -x "target/$target/release/$BIN" ]] || { echo "build failed for $target"; exit 1; }
done

echo "== ffmpeg"
scripts/build-ffmpeg-macos.sh

echo "== icon"
iconset="$ROOT/target/AppIcon.iconset"
rm -rf "$iconset" && mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z $size $size assets/icon/icon-1024.png --out "$iconset/icon_${size}x${size}.png" > /dev/null
    sips -z $((size * 2)) $((size * 2)) assets/icon/icon-1024.png --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$iconset" -o "$ROOT/target/AppIcon.icns"

echo "== assembling $APP"
rm -rf "$APP" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/Licenses/FFmpeg"
lipo -create "target/aarch64-apple-darwin/release/$BIN" "target/x86_64-apple-darwin/release/$BIN" -output "$APP/Contents/MacOS/$BIN"
# Next to the main executable, where the app looks for it first.
cp target/ffmpeg-macos/ffmpeg "$APP/Contents/MacOS/ffmpeg"
cp target/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp LICENSE "$APP/Contents/Resources/Licenses/LICENSE.txt"
cp target/ffmpeg-macos/COPYING.LGPLv2.1 target/ffmpeg-macos/COPYING.LAME target/ffmpeg-macos/BUILD-INFO.txt "$APP/Contents/Resources/Licenses/FFmpeg/"
python3 -I scripts/third-party-notices.py > "$APP/Contents/Resources/Licenses/THIRD-PARTY.txt"
sed -e "s/__APP_NAME__/$APP_NAME/g" -e "s/__BUNDLE_ID__/$BUNDLE_ID/g" -e "s/__VERSION__/$VERSION/g" \
    assets/macos/Info.plist > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" > /dev/null

echo "== signing (ad-hoc)"
# Inner executables first, then the bundle. Apple Silicon refuses to run unsigned code at all.
codesign --force --sign - --timestamp=none "$APP/Contents/MacOS/ffmpeg"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --strict --verbose=1 "$APP"

echo "== disk image"
stage="$ROOT/target/dmg-stage"
rm -rf "$stage" && mkdir -p "$stage"
cp -R "$APP" "$stage/"
ln -s /Applications "$stage/Applications"
cat > "$stage/Read me first.txt" <<EOF
$APP_NAME $VERSION

1. Drag $APP_NAME onto the Applications folder.
2. Open it. macOS will say it "cannot verify" the app, because this beta isn't
   signed by a registered Apple developer yet. Click Done (not Move to Bin).
3. Open System Settings › Privacy & Security, scroll down, and click
   "Open Anyway" next to $APP_NAME. Confirm with your password.

You only have to do this once. From then on it opens like any other app.

(Terminal alternative: xattr -dr com.apple.quarantine /Applications/$APP_NAME.app)
EOF
dmg="$DIST/$APP_NAME-$VERSION.dmg"
rm -f "$dmg"
# hdiutil warns it's deprecated on recent macOS but still works, and its replacement
# (diskutil image) doesn't exist on older systems or CI runners.
hdiutil create -volname "$APP_NAME $VERSION" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg" > /dev/null
(cd "$DIST" && shasum -a 256 "$(basename "$dmg")" > "$(basename "$dmg").sha256")

echo "== smoke test"
"$APP/Contents/MacOS/$BIN" --version

echo "== done"
echo "   $(du -sh "$APP" | cut -f1)  $APP  ($(lipo -archs "$APP/Contents/MacOS/$BIN"))"
echo "   $(du -sh "$dmg" | cut -f1)  $dmg"
