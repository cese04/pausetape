#!/usr/bin/env bash
# Build the minimal, LGPL-only ffmpeg that the Windows installer offers (ticked by default).
#
# The same program as the Mac's (scripts/build-ffmpeg-macos.sh), with the same configure
# flags: keep the two lists in step. Cross-compiled on Linux with mingw-w64 and linked
# statically, so ffmpeg.exe needs no DLLs beyond Windows' own. Output:
#
#   target/ffmpeg-windows/ffmpeg.exe          the program (x86_64)
#   target/ffmpeg-windows/COPYING.LGPLv2.1    its licence (installed with it)
#   target/ffmpeg-windows/COPYING.LAME        LAME's licence (LGPL; installed with it)
#   target/ffmpeg-windows/BUILD-INFO.txt      version, source URL and configure flags
#
# Needs, on Debian or Ubuntu: apt install gcc make mingw-w64 libz-mingw-w64-dev curl xz-utils
# (gcc builds the tools ffmpeg uses while it builds itself).
# Usage: scripts/build-ffmpeg-windows.sh     (takes a few minutes; skipped if already built)
set -euo pipefail

VERSION=9.0.2
SHA256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
LAME_VERSION=3.100
LAME_SHA256=ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e
LAME_URL="https://downloads.sourceforge.net/project/lame/lame/$LAME_VERSION/lame-$LAME_VERSION.tar.gz"
HOST=x86_64-w64-mingw32

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK="$ROOT/target/ffmpeg-build-windows"
OUT="$ROOT/target/ffmpeg-windows"
URL="https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz"

# As in build-ffmpeg-macos.sh.
COMMON=(
    --disable-everything --disable-autodetect --disable-network
    --disable-doc --disable-debug --disable-programs --enable-ffmpeg
    --disable-avdevice --enable-static --disable-shared
    --enable-zlib                                   # compressed MP4 headers
    --enable-protocol=file,pipe
    --enable-demuxer=mov,ogg,matroska,asf,ape,wv,aac,ac3,eac3,flac,mp3,wav,w64,aiff,caf,dts,truehd,mlp
    --enable-parser=aac,ac3,opus,vorbis,flac,mpegaudio,dca,mlp
    --enable-decoder=aac,aac_latm,alac,ac3,eac3,truehd,mlp,dca,opus,vorbis,ape,wavpack,wmav1,wmav2,wmapro,wmalossless,mp3,mp3float,mp2,mp2float,flac,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,pcm_f32le,pcm_f32be,pcm_s8,pcm_u8
    --enable-encoder=pcm_s16le,flac,libmp3lame --enable-muxer=wav,flac,mp3 --enable-libmp3lame
    --enable-filter=aresample,aformat,anull,atrim,abuffer,abuffersink
)

if [[ -f "$OUT/ffmpeg.exe" && "${1:-}" != "--force" ]] && grep -q "ffmpeg $VERSION" "$OUT/BUILD-INFO.txt" 2>/dev/null \
    && grep -qF -- "${COMMON[*]}" "$OUT/BUILD-INFO.txt"; then
    echo "ffmpeg $VERSION already built at $OUT (pass --force to rebuild)"
    exit 0
fi

for tool in gcc make "$HOST-gcc"; do
    command -v "$tool" > /dev/null || { echo "needs $tool: apt install gcc make mingw-w64 libz-mingw-w64-dev"; exit 1; }
done

mkdir -p "$WORK" "$OUT"
tarball="$WORK/ffmpeg-$VERSION.tar.xz"
[[ -f "$tarball" ]] || curl -fL "$URL" -o "$tarball"
echo "$SHA256  $tarball" | sha256sum -c -
lame_tarball="$WORK/lame-$LAME_VERSION.tar.gz"
[[ -f "$lame_tarball" ]] || curl -fL "$LAME_URL" -o "$lame_tarball"
echo "$LAME_SHA256  $lame_tarball" | sha256sum -c -

jobs=$(nproc)

# LAME first: a static library, found by ffmpeg's configure below.
lame="$WORK/lame"
lame_src="$WORK/lame-src"
rm -rf "$lame" "$lame_src" && mkdir -p "$lame_src"
tar -xf "$lame_tarball" -C "$lame_src" --strip-components=1
# LAME lists a function it doesn't build in its export list; harmless for a static library,
# but some toolchains trip over it.
sed -i '/lame_init_old/d' "$lame_src/include/libmp3lame.sym"
echo "== building LAME $LAME_VERSION"
(cd "$lame_src" && ./configure --host="$HOST" --prefix="$lame" --disable-shared --enable-static --disable-frontend \
    --disable-decoder CFLAGS="-O2" > "$WORK/lame-configure.log" \
    && make -j"$jobs" install > "$WORK/lame-make.log")

src="$WORK/src"
rm -rf "$src" && mkdir -p "$src"
tar -xf "$tarball" -C "$src" --strip-components=1
echo "== configuring ffmpeg $VERSION for Windows (x86_64)"
# Without nasm: slower SIMD paths don't matter for converting one track at a time. Static
# libgcc and libwinpthread, so nothing but Windows' own DLLs is needed.
(cd "$src" && ./configure --enable-cross-compile --target-os=mingw32 --arch=x86_64 --cross-prefix="$HOST-" \
    --pkg-config=false --disable-x86asm \
    --extra-cflags="-O2 -I$lame/include" --extra-ldflags="-static -static-libgcc -L$lame/lib" \
    "${COMMON[@]}" > "$WORK/configure.log") || { tail -30 "$src/ffbuild/config.log"; exit 1; }
echo "== building"
make -C "$src" -j"$jobs" ffmpeg.exe > "$WORK/make.log"
cp "$src/ffmpeg.exe" "$OUT/ffmpeg.exe"
"$HOST-strip" "$OUT/ffmpeg.exe"

# Only Windows' own DLLs: anything else wouldn't be there on the user's computer.
dlls=$("$HOST-objdump" -p "$OUT/ffmpeg.exe" | sed -n 's/^\s*DLL Name: //p' | tr 'A-Z' 'a-z' | sort -u)
others=$(echo "$dlls" | grep -vxE 'kernel32\.dll|user32\.dll|advapi32\.dll|bcrypt\.dll|shell32\.dll|ole32\.dll|shlwapi\.dll|ws2_32\.dll|msvcrt\.dll|api-ms-win-.*\.dll' || true)
if [[ -n "$others" ]]; then
    echo "ffmpeg.exe needs DLLs Windows doesn't have:"; echo "$others"; exit 1
fi

cp "$src/COPYING.LGPLv2.1" "$OUT/"
cp "$lame_src/COPYING" "$OUT/COPYING.LAME"
{
    echo "ffmpeg $VERSION, LGPL v2.1 or later (no GPL or non-free components)"
    echo "Source: $URL"
    echo "SHA-256 of source: $SHA256"
    echo "With LAME $LAME_VERSION (MP3 encoder, LGPL), linked statically: $LAME_URL (SHA-256 $LAME_SHA256)"
    echo "Built for: Windows x86_64, with $("$HOST-gcc" --version | head -1)"
    echo "Uses: $(echo $dlls)"
    echo "Configure flags: ${COMMON[*]}"
} > "$OUT/BUILD-INFO.txt"

echo "== done: $(du -h "$OUT/ffmpeg.exe" | cut -f1) · uses $(echo $dlls)"
