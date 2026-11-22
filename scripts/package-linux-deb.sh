#!/usr/bin/env bash
# Install the same verified payload as the portable Ubuntu x64 archive.
set -euo pipefail
if [ "$#" -ne 2 ]; then
  echo "Usage: scripts/package-linux-deb.sh FINALIZED_STAGE OUTPUT_DIRECTORY" >&2
  exit 2
fi
stage="$(realpath "$1")"
output="$(realpath -e "$2")"
project_root="$(cd "$(dirname "$0")/.." && pwd)"
(cd "$stage" && sha256sum --strict --quiet -c PACKAGE-SHA256SUMS.txt)

version="$(python3 - "$stage/BUILD_METADATA.json" <<'PY'
import json
from pathlib import Path
import re
import sys

metadata = json.loads(Path(sys.argv[1]).read_text())
version = metadata["version"]
if metadata["target"] != "x86_64-unknown-linux-gnu" or not re.fullmatch(
    r"[0-9]+(?:\.[0-9]+)+(?:-(?:alpha|beta|rc)\.[0-9]+)?", version
):
    sys.exit("Expected a finalized Linux x64 stage with a release or prerelease version")
if type(metadata["build_number"]) is not int or metadata["build_number"] < 1:
    sys.exit("A numbered source build is required")
print(f"{version.replace('-', '~')}+build{metadata['build_number']}")
PY
)"
tmp="$(mktemp -d "$output/.neoflash-deb.XXXXXXXX")"
trap 'rm -rf "$tmp"' EXIT
root="$tmp/root"
install -d "$root/usr/lib/neoflash" "$root/usr/bin" "$root/DEBIAN"
cp -a "$stage/." "$root/usr/lib/neoflash/"
ln -s ../lib/neoflash/neoflash-editor "$root/usr/bin/neoflash"
install -Dm644 "$project_root/scripts/linux/neoflash.desktop" "$root/usr/share/applications/neoflash.desktop"
install -Dm644 "$project_root/scripts/linux/neoflash-project.xml" "$root/usr/share/mime/packages/neoflash-project.xml"
for size in 16 32 64 128 256 512; do
  install -Dm644 "$project_root/crates/neoflash-editor/assets/app-icon/light/icon_${size}.png" \
    "$root/usr/share/icons/hicolor/${size}x${size}/apps/neoflash.png"
done
install -d "$root/usr/share/doc/neoflash"
cat > "$root/usr/share/doc/neoflash/copyright" <<'NOTICE'
NeoFlash is licensed under MIT OR Apache-2.0. Both license texts are installed
in /usr/lib/neoflash/licenses/neoflash/. Dependency and bundled media license
texts are in /usr/lib/neoflash/licenses/ and /usr/lib/neoflash/media/licenses/.
Source and provenance information is in /usr/lib/neoflash/LICENSES-AND-SOURCE.txt.

NOTICE
cat "$stage/THIRD_PARTY_NOTICES.txt" >> "$root/usr/share/doc/neoflash/copyright"

# Resolve the exact ABI requirements from the editor ELF binary on the Ubuntu
# build host (bundled media/bin/{ffmpeg,ffprobe} carry their own media/lib/*.so
# via $ORIGIN/../lib; passing them to dpkg-shlibdeps on usr-merged Ubuntu 24.04
# injects an artificial libc6 (<< 2.40) upper bound that blocks newer Ubuntu/Debian).
mkdir "$tmp/debian"
cat > "$tmp/debian/control" <<'CONTROL'
Source: neoflash
Section: graphics
Priority: optional
Maintainer: NeoFlash contributors <developer@redinger.cc>

Package: neoflash
Architecture: amd64
Description: Vector animation and interactive HTML5 authoring
CONTROL
linked="$(cd "$tmp" && dpkg-architecture -aamd64 -c dpkg-shlibdeps -O \
  -e"$stage/neoflash-editor")"
linked="${linked#shlibs:Depends=}"
linked="$(printf '%s' "$linked" | sed -E 's/, *libc6 *\((<<|<)[^)]*\)//g; s/libc6 *\((<<|<)[^)]*\), *//g')"
size_kb="$(du --apparent-size -sk "$root/usr" | cut -f1)"
cat > "$root/DEBIAN/control" <<CONTROL
Package: neoflash
Version: $version
Architecture: amd64
Maintainer: NeoFlash contributors <developer@redinger.cc>
Section: graphics
Priority: optional
Installed-Size: $size_kb
Depends: $linked, libx11-6, libx11-xcb1, libxcb-render0, libxcb-shape0, libxcb-xfixes0, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libegl1, libgl1, libvulkan1, libxcursor1, libxi6, libxrandr2, shared-mime-info
Recommends: libgl1-mesa-dri, mesa-vulkan-drivers, xdg-desktop-portal
Homepage: https://github.com/jazzymiles/NeoFlash
Description: Vector animation and interactive HTML5 authoring
 Edit vector artwork, nested timelines and scripts, and publish animations
 to HTML5. The editor includes its offline web runtime and media helpers.
CONTROL
(cd "$root" && find usr -type f -print0 | LC_ALL=C sort -z | xargs -0 md5sum > DEBIAN/md5sums)

# Set filesystem metadata explicitly so umask and source-copy times cannot
# change the package and so every installed user can read the resources.
find "$root" -type d -exec chmod 0755 {} +
find "$root" -type f -exec chmod 0644 {} +
chmod 0755 "$root/usr/lib/neoflash/"{neoflash-editor,media/bin/ffmpeg,media/bin/ffprobe}
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}"
find "$root" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
name="neoflash_${version}_amd64.deb"
dpkg-deb --root-owner-group --threads-max=2 -Zzstd -z10 --build "$root" "$tmp/$name" >&2
mv "$tmp/$name" "$output/$name"
printf '%s\n' "$output/$name"
