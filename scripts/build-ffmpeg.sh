#!/bin/bash
# Builds Wolfgang's own ffmpeg + ffprobe for videos and audio.
#
#   scripts/build-ffmpeg.sh            # → target/ffmpeg/bin/{ffmpeg,ffprobe}
#
# LGPL only, nothing GPL (no x264/x265): Apple VideoToolbox (H.264) and
# AudioToolbox (AAC), libvpx (VP9), Opus, LAME (MP3), zimg (HDR → SDR).
# Everything is linked statically; the binaries only need macOS itself.
# Versions are pinned with checksums. A finished build is reused as long as this
# script is unchanged (CI caches target/ffmpeg with the script's hash as key).
#
# Needs: Xcode command line tools, nasm, pkg-config, autoconf, automake, libtool
# (brew install nasm pkg-config autoconf automake libtool).

set -euo pipefail
cd "$(dirname "$0")/.."

OUT="$PWD/target/ffmpeg"
STAMP="$OUT/.built-by"
HASH=$(shasum -a 256 "$0" | cut -d' ' -f1)
if [[ -x "$OUT/bin/ffmpeg" && -x "$OUT/bin/ffprobe" && "$(cat "$STAMP" 2>/dev/null)" == "$HASH" ]]; then
  echo "ffmpeg ist schon gebaut ($OUT/bin)"
  exit 0
fi

FFMPEG=9.0.2   ; FFMPEG_SHA=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
VPX=1.17.0     ; VPX_SHA=1020f184046187baa2985dbde38e0691f49c44088bca7a1842b0236c6081dc0a
OPUS=1.6.1     ; OPUS_SHA=6ffcb593207be92584df15b32466ed64bbec99109f007c82205f0194572411a1
LAME=3.100     ; LAME_SHA=ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e
ZIMG=3.0.6     ; ZIMG_SHA=be89390f13a5c9b2388ce0f44a5e89364a20c1c57ce46d382b1fcc3967057577

[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || { echo "Nur für Macs mit Apple-Chip"; exit 1; }
for tool in nasm pkg-config autoreconf automake glibtoolize; do
  command -v "$tool" >/dev/null || { echo "$tool fehlt (brew install nasm pkg-config autoconf automake libtool)"; exit 1; }
done

WORK="$OUT/work"
DEPS="$OUT/deps"
rm -rf "$WORK" "$DEPS" "$OUT/bin" "$STAMP"
mkdir -p "$WORK" "$DEPS" "$OUT/bin" "$OUT/downloads"

export MACOSX_DEPLOYMENT_TARGET=13.0
export CFLAGS="-arch arm64 -mmacosx-version-min=13.0 -O2"
export CXXFLAGS="$CFLAGS"
export LDFLAGS="-arch arm64 -mmacosx-version-min=13.0"
export PKG_CONFIG_PATH="$DEPS/lib/pkgconfig"
export PKG_CONFIG_LIBDIR="$DEPS/lib/pkgconfig" # never pick up Homebrew's copies
JOBS=$(sysctl -n hw.ncpu)

step() { echo; echo "▶ $1"; }

# fetch NAME URL SHA256 → unpacked into $WORK/NAME
fetch() {
  local name=$1 url=$2 sha=$3 file="$OUT/downloads/$(basename "$2")"
  [[ -f "$file" ]] || curl -fsSL --retry 3 -o "$file" "$url"
  echo "$sha  $file" | shasum -a 256 -c --quiet - || { echo "Prüfsumme falsch: $file"; rm -f "$file"; exit 1; }
  mkdir -p "$WORK/$name"
  tar xf "$file" -C "$WORK/$name" --strip-components 1
}

step "LAME $LAME (MP3)"
fetch lame "https://downloads.sourceforge.net/project/lame/lame/$LAME/lame-$LAME.tar.gz" $LAME_SHA
(cd "$WORK/lame"
  # 3.100 exports a symbol that no longer exists; Apple's linker refuses that.
  sed -i '' '/lame_init_old/d' include/libmp3lame.sym
  ./configure --prefix="$DEPS" --host=aarch64-apple-darwin --disable-shared --enable-static \
    --disable-frontend --disable-decoder --disable-gtktest >/dev/null
  make -j"$JOBS" >/dev/null && make install >/dev/null)

step "Opus $OPUS"
fetch opus "https://downloads.xiph.org/releases/opus/opus-$OPUS.tar.gz" $OPUS_SHA
(cd "$WORK/opus"
  ./configure --prefix="$DEPS" --disable-shared --enable-static --disable-doc --disable-extra-programs >/dev/null
  make -j"$JOBS" >/dev/null && make install >/dev/null)

step "libvpx $VPX (VP9)"
fetch vpx "https://github.com/webmproject/libvpx/archive/refs/tags/v$VPX.tar.gz" $VPX_SHA
(cd "$WORK/vpx"
  ./configure --prefix="$DEPS" --target=arm64-darwin22-gcc --disable-shared --enable-static \
    --disable-examples --disable-tools --disable-docs --disable-unit-tests \
    --disable-vp8 --disable-vp9-decoder --enable-vp9-encoder >/dev/null
  make -j"$JOBS" >/dev/null && make install >/dev/null)

step "zimg $ZIMG (HDR → SDR)"
fetch zimg "https://github.com/sekrit-twc/zimg/archive/refs/tags/release-$ZIMG.tar.gz" $ZIMG_SHA
(cd "$WORK/zimg"
  ./autogen.sh >/dev/null 2>&1
  ./configure --prefix="$DEPS" --disable-shared --enable-static >/dev/null
  make -j"$JOBS" >/dev/null && make install >/dev/null)

step "ffmpeg $FFMPEG"
fetch ffmpeg "https://ffmpeg.org/releases/ffmpeg-$FFMPEG.tar.xz" $FFMPEG_SHA
(cd "$WORK/ffmpeg"
  # Encoders/muxers: only what Wolfgang writes (plus ffv1/pcm/matroska/wav for test clips).
  # Decoders, demuxers and filters: all of them, so any input file can be read.
  ./configure --prefix="$OUT" --pkg-config-flags=--static \
    --extra-cflags="$CFLAGS -I$DEPS/include" --extra-ldflags="$LDFLAGS -L$DEPS/lib" --extra-libs="-lc++" \
    --disable-shared --enable-static --disable-autodetect --disable-debug --disable-doc \
    --disable-network --disable-ffplay --disable-devices --enable-indev=lavfi \
    --enable-videotoolbox --enable-audiotoolbox --enable-zlib \
    --enable-libvpx --enable-libopus --enable-libmp3lame --enable-libzimg \
    --disable-encoders --enable-encoder=h264_videotoolbox,libvpx_vp9,aac_at,libopus,libmp3lame,png,ffv1,pcm_s16le \
    --disable-muxers --enable-muxer=mp4,ipod,webm,mp3,image2,matroska,wav,null \
    >/dev/null
  make -j"$JOBS" >/dev/null
  cp ffmpeg ffprobe "$OUT/bin/"
  cp COPYING.LGPLv2.1 "$OUT/LICENSE-ffmpeg.txt")
strip "$OUT/bin/ffmpeg" "$OUT/bin/ffprobe"

step "Prüfen"
"$OUT/bin/ffmpeg" -hide_banner -L | grep -q "Lesser General Public" || { echo "Nicht LGPL!"; exit 1; }
"$OUT/bin/ffmpeg" -hide_banner -buildconf | grep -q -- "--enable-gpl" && { echo "GPL-Bauweise!"; exit 1; }
for bin in ffmpeg ffprobe; do
  if otool -L "$OUT/bin/$bin" | tail -n +2 | grep -vE '^\s*(/usr/lib/|/System/Library/)'; then
    echo "$bin hängt an Bibliotheken außerhalb von macOS"; exit 1
  fi
done
"$OUT/bin/ffmpeg" -hide_banner -version | head -1
ls -lh "$OUT/bin"
rm -rf "$WORK"
echo "$HASH" > "$STAMP"
