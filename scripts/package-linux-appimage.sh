#!/usr/bin/env bash
# Stage a self-contained Linux x64 AppDir from the verified portable stage and
# optionally package NeoFlash-<version>-linux-x86_64.AppImage via appimagetool.
set -euo pipefail

stage_only=0
if [ "${1:-}" = "--stage-only" ]; then
  stage_only=1
  shift
fi

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
  printf '%s\n' "Usage: scripts/package-linux-appimage.sh [--stage-only] FINALIZED_STAGE OUTPUT_DIRECTORY [CHECKSUM_FILE]" >&2
  exit 2
fi

if [ ! -d "$1" ]; then
  printf 'Missing finalized stage directory: %s\n' "$1" >&2
  exit 1
fi
if [ ! -d "$2" ]; then
  printf 'Missing output directory: %s\n' "$2" >&2
  exit 1
fi

stage="$(CDPATH='' cd -- "$1" && pwd)"
output="$(CDPATH='' cd -- "$2" && pwd)"
checksum_file="${3:-}"
project_root="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"

version="$(python3 - "$stage" <<'PY'
import hashlib
import json
import re
import sys
from pathlib import Path

stage = Path(sys.argv[1])
sums_path = stage / "PACKAGE-SHA256SUMS.txt"
if not sums_path.is_file():
    sys.exit(f"Missing {sums_path}")

expected: dict[str, str] = {}
for line_no, raw in enumerate(sums_path.read_text(encoding="utf-8").splitlines(), 1):
    if not raw.strip():
        continue
    match = re.fullmatch(r"([0-9a-fA-F]{64})  (.+)", raw)
    if not match:
        sys.exit(f"Invalid PACKAGE-SHA256SUMS.txt line {line_no}")
    digest, rel = match.groups()
    target = stage / rel
    if not target.is_file():
        sys.exit(f"Missing staged payload file: {rel}")
    actual = hashlib.sha256(target.read_bytes()).hexdigest()
    if actual != digest.lower():
        sys.exit(f"Staged payload hash mismatch for {rel}: {actual} != {digest.lower()}")
    expected[rel] = actual

if not expected:
    sys.exit("PACKAGE-SHA256SUMS.txt is empty")

actual_files = {
    path.relative_to(stage).as_posix()
    for path in stage.rglob("*")
    if path.is_file() and path.name != "PACKAGE-SHA256SUMS.txt"
}
if actual_files != set(expected):
    sys.exit("Staged directory contains unrecorded or missing files")

metadata = json.loads((stage / "BUILD_METADATA.json").read_text(encoding="utf-8"))
version = metadata.get("version", "")
if metadata.get("target") != "x86_64-unknown-linux-gnu" or not re.fullmatch(
    r"[0-9]+(?:\.[0-9]+)+(?:-(?:alpha|beta|rc)\.[0-9]+)?", version
):
    sys.exit("Expected a finalized Linux x64 stage with a release or prerelease version")
if type(metadata.get("build_number")) is not int or metadata["build_number"] < 1:
    sys.exit("A numbered source build is required")
print(version)
PY
)"

tmp=""
cleanup() {
  if [ -n "$tmp" ] && [ -d "$tmp" ]; then
    rm -rf "$tmp"
  fi
}
trap cleanup EXIT

if [ "$stage_only" -eq 1 ]; then
  appdir="$output/NeoFlash.AppDir"
  rm -rf "$appdir"
  mkdir -p "$appdir"
else
  tmp="$(mktemp -d "$output/.neoflash-appimage.XXXXXXXX")"
  appdir="$tmp/NeoFlash.AppDir"
  mkdir -p "$appdir"
fi

mkdir -p \
  "$appdir/usr/lib/neoflash" \
  "$appdir/usr/bin" \
  "$appdir/usr/share/applications" \
  "$appdir/usr/share/mime/packages" \
  "$appdir/usr/share/doc/neoflash"

cp -R "$stage/." "$appdir/usr/lib/neoflash/"
ln -s ../lib/neoflash/neoflash-editor "$appdir/usr/bin/neoflash"

# AppRun configures only NeoFlash-private discovery variables (when not already
# overridden by the caller) and never touches LD_LIBRARY_PATH or PATH so child
# processes such as xdg-open and browsers run in a clean host environment.
cat > "$appdir/AppRun" <<'APPRUN'
#!/bin/sh
set -eu
HERE="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"
export NEOFLASH_WEB_RUNTIME_DIR="${NEOFLASH_WEB_RUNTIME_DIR:-$HERE/usr/lib/neoflash/runtime}"
export NEOFLASH_FFMPEG="${NEOFLASH_FFMPEG:-$HERE/usr/lib/neoflash/media/bin/ffmpeg}"
export NEOFLASH_FFPROBE="${NEOFLASH_FFPROBE:-$HERE/usr/lib/neoflash/media/bin/ffprobe}"
exec "$HERE/usr/lib/neoflash/neoflash-editor" "$@"
APPRUN

{
  cat "$project_root/scripts/linux/neoflash.desktop"
  printf 'X-AppImage-Version=%s\n' "$version"
} > "$appdir/neoflash.desktop"
cp "$appdir/neoflash.desktop" "$appdir/usr/share/applications/neoflash.desktop"

cp "$project_root/scripts/linux/neoflash-project.xml" "$appdir/usr/share/mime/packages/neoflash.xml"

for size in 16 32 64 128 256 512; do
  icon_dir="$appdir/usr/share/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$icon_dir"
  cp "$project_root/crates/neoflash-editor/assets/app-icon/light/icon_${size}.png" \
    "$icon_dir/neoflash.png"
done
cp "$project_root/crates/neoflash-editor/assets/app-icon/light/icon_512.png" "$appdir/neoflash.png"
cp "$project_root/crates/neoflash-editor/assets/app-icon/light/icon_512.png" "$appdir/.DirIcon"

cat > "$appdir/usr/share/doc/neoflash/copyright" <<'NOTICE'
NeoFlash is licensed under MIT OR Apache-2.0. Both license texts are bundled
in usr/lib/neoflash/licenses/neoflash/. Dependency and bundled media license
texts are in usr/lib/neoflash/licenses/ and usr/lib/neoflash/media/licenses/.
Source and provenance information is in usr/lib/neoflash/LICENSES-AND-SOURCE.txt.

NOTICE
cat "$stage/THIRD_PARTY_NOTICES.txt" >> "$appdir/usr/share/doc/neoflash/copyright"

find "$appdir" -type d -exec chmod 0755 {} +
find "$appdir" -type f -exec chmod 0644 {} +
chmod 0755 \
  "$appdir/AppRun" \
  "$appdir/usr/lib/neoflash/neoflash-editor" \
  "$appdir/usr/lib/neoflash/media/bin/ffmpeg" \
  "$appdir/usr/lib/neoflash/media/bin/ffprobe"

if [ "$stage_only" -eq 1 ]; then
  printf '%s\n' "$appdir"
  exit 0
fi

appimagetool_bin="${NEOFLASH_APPIMAGETOOL:-}"
if [ -z "$appimagetool_bin" ]; then
  if command -v appimagetool >/dev/null 2>&1; then
    appimagetool_bin="$(command -v appimagetool)"
  else
    printf '%s\n' "appimagetool not found (set NEOFLASH_APPIMAGETOOL or pass --stage-only)." >&2
    exit 1
  fi
fi

appimage_name="NeoFlash-${version}-linux-x86_64.AppImage"
staged_appimage="$tmp/$appimage_name"
final_appimage="$output/$appimage_name"

ARCH=x86_64 SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-315532800}" \
  "$appimagetool_bin" --no-appstream "$appdir" "$staged_appimage" >&2
chmod 0755 "$staged_appimage"
mv -f "$staged_appimage" "$final_appimage"

if [ -n "$checksum_file" ]; then
  python3 - "$final_appimage" "$checksum_file" <<'PY'
import hashlib
import sys
from pathlib import Path

artifact = Path(sys.argv[1])
checksum_path = Path(sys.argv[2])
digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
entry = f"{digest}  {artifact.name}"
existing = (
    [line for line in checksum_path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if checksum_path.is_file()
    else []
)
filtered = [line for line in existing if not line.endswith(f"  {artifact.name}")]
filtered.append(entry)
checksum_path.write_text("\n".join(filtered) + "\n", encoding="utf-8", newline="\n")
PY
fi

printf '%s\n' "$final_appimage"
