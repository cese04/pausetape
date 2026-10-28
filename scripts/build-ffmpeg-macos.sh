#!/usr/bin/env bash
# Build the minimal, LGPL-only ffmpeg that ships inside the macOS app.
#
# It only does what NeatLibrary asks of it: decode the formats the built-in decoder can't
# (Dolby E-AC-3/AC-3/TrueHD, Opus, APE, WavPack, WMA, DTS…) and write a stereo 16-bit WAV.
# No GPL components, no network, no video. Output is a universal (arm64 + x86_64) binary:
#
#   target/ffmpeg-macos/ffmpeg              the program
#   target/ffmpeg-macos/COPYING.LGPLv2.1    its licence (shipped with the app)
#   target/ffmpeg-macos/BUILD-INFO.txt      version, source URL and configure flags
#
# Usage: scripts/build-ffmpeg-macos.sh     (takes a few minutes; skipped if already built)
set -euo pipefail

VERSION=9.0.2
# Recorded from the first download on 2026-10-27. FFmpeg also publishes a GPG signature
# (ffmpeg-$VERSION.tar.xz.asc) that can be checked by hand with the FFmpeg release key.
SHA256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
MIN_MACOS=12.0

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK="$ROOT/target/ffmpeg-build"
OUT="$ROOT/target/ffmpeg-macos"
URL="https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz"

if [[ -x "$OUT/ffmpeg" && "${1:-}" != "--force" ]] && grep -q "ffmpeg $VERSION" "$OUT/BUILD-INFO.txt" 2>/dev/null; then
    echo "ffmpeg $VERSION already built at $OUT (pass --force to rebuild)"
    exit 0
fi

mkdir -p "$WORK" "$OUT"
tarball="$WORK/ffmpeg-$VERSION.tar.xz"
[[ -f "$tarball" ]] || curl -fL "$URL" -o "$tarball"
echo "$SHA256  $tarball" | shasum -a 256 -c -

COMMON=(
    --disable-everything --disable-autodetect --disable-network
    --disable-doc --disable-debug --disable-programs --enable-ffmpeg
    --disable-avdevice --enable-static --disable-shared
    --enable-zlib                                   # compressed MP4 headers
    --enable-protocol=file,pipe
    --enable-demuxer=mov,ogg,matroska,asf,ape,wv,aac,ac3,eac3,flac,mp3,wav,w64,aiff,caf,dts,truehd,mlp
    --enable-parser=aac,ac3,opus,vorbis,flac,mpegaudio,dca,mlp
    --enable-decoder=aac,aac_latm,alac,ac3,eac3,truehd,mlp,dca,opus,vorbis,ape,wavpack,wmav1,wmav2,wmapro,wmalossless,mp3,mp3float,mp2,mp2float,flac,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,pcm_f32le,pcm_f32be,pcm_s8,pcm_u8
    --enable-encoder=pcm_s16le --enable-muxer=wav
    --enable-filter=aresample,aformat,anull,abuffer,abuffersink
)

jobs=$(sysctl -n hw.ncpu)
for arch in arm64 x86_64; do
    src="$WORK/src-$arch"
    rm -rf "$src" && mkdir -p "$src"
    tar -xf "$tarball" -C "$src" --strip-components=1
    flags="-arch $arch -mmacosx-version-min=$MIN_MACOS"
    extra=()
    if [[ "$arch" != "$(uname -m)" ]]; then
        extra+=(--enable-cross-compile --target-os=darwin)
    fi
    if [[ "$arch" == x86_64 ]]; then
        # Without nasm: slower SIMD paths don't matter for converting one track at a time.
        extra+=(--disable-x86asm)
    fi
    echo "== configuring ffmpeg $VERSION for $arch"
    (cd "$src" && ./configure --cc=clang --arch="$arch" --extra-cflags="$flags" --extra-ldflags="$flags" \
        ${extra[@]+"${extra[@]}"} "${COMMON[@]}" > "$WORK/configure-$arch.log")
    echo "== building for $arch"
    make -C "$src" -j"$jobs" ffmpeg > "$WORK/make-$arch.log"
    cp "$src/ffmpeg" "$WORK/ffmpeg-$arch"
done

lipo -create "$WORK/ffmpeg-arm64" "$WORK/ffmpeg-x86_64" -output "$OUT/ffmpeg"
strip -x "$OUT/ffmpeg"
cp "$WORK/src-arm64/COPYING.LGPLv2.1" "$OUT/"
{
    echo "ffmpeg $VERSION, LGPL v2.1 or later (no GPL or non-free components)"
    echo "Source: $URL"
    echo "SHA-256 of source: $SHA256"
    echo "Built for: arm64 + x86_64, macOS $MIN_MACOS or later"
    echo "Configure flags: ${COMMON[*]}"
} > "$OUT/BUILD-INFO.txt"

echo "== done: $(lipo -archs "$OUT/ffmpeg") · $(du -h "$OUT/ffmpeg" | cut -f1)"
"$OUT/ffmpeg" -hide_banner -L 2>/dev/null | head -3 || true
