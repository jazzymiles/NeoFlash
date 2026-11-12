#!/usr/bin/env bash
# Reproducible source build script for the optional LGPL-only FFmpeg/FFprobe helper profile.
# Excludes `--enable-gpl`, `libx264`, and `libx265` for downstream redistributors who wish to
# ship an LGPLv2.1+ helper relying on platform VideoToolbox/Media Foundation hardware encoders
# and BSD/LGPL codecs (`libvpx-vp9`, `libopus`, `libvorbis`, `libmp3lame`, `libdav1d`, `libsvtav1`).
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
FFMPEG_VERSION="${NEOFLASH_FFMPEG_VERSION:-8.0.1}"
FFMPEG_SHA256="${NEOFLASH_FFMPEG_SHA256:-}"
work_dir="${NEOFLASH_FFMPEG_BUILD_DIR:-$project_root/target/ffmpeg-source-build/lgpl}"
prefix_dir="$work_dir/install"
dest_dir="${1:-$project_root/target/bundled-media-lgpl}"

mkdir -p "$work_dir"
tarball="$work_dir/ffmpeg-${FFMPEG_VERSION}.tar.xz"
if [ ! -f "$tarball" ]; then
  curl -fL "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" -o "$tarball"
fi
if [ -n "$FFMPEG_SHA256" ]; then
  echo "${FFMPEG_SHA256}  $tarball" | shasum -a 256 -c -
fi

rm -rf "$work_dir/ffmpeg-${FFMPEG_VERSION}" "$prefix_dir"
tar -xf "$tarball" -C "$work_dir"
cd "$work_dir/ffmpeg-${FFMPEG_VERSION}"

export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:/usr/local/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

./configure \
  --prefix="$prefix_dir" \
  --disable-gpl \
  --disable-nonfree \
  --enable-shared \
  --enable-pthreads \
  --enable-ffmpeg \
  --enable-ffprobe \
  --disable-ffplay \
  --disable-doc \
  --enable-videotoolbox \
  --enable-audiotoolbox \
  --enable-libvpx \
  --enable-libopus \
  --enable-libvorbis \
  --enable-libmp3lame \
  --enable-libdav1d \
  --enable-libsvtav1

make -j"$(sysctl -n hw.ncpu 2>/dev/null || echo 4)"
make install

NEOFLASH_SOURCE_FFMPEG="$prefix_dir/bin/ffmpeg" \
NEOFLASH_SOURCE_FFPROBE="$prefix_dir/bin/ffprobe" \
python3 "$project_root/scripts/stage-bundled-media.py" --dest "$dest_dir"
