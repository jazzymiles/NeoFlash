#!/usr/bin/env bash
# Fail-closed Ubuntu 24.04-compatible Linux x64 Technical Preview packaging (T-289).
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"

profile="${1:-release}"
case "$profile" in
  release|debug) ;;
  *) printf '%s\n' 'Usage: scripts/package-linux.sh [release|debug]' >&2; exit 2 ;;
esac

if [ -n "$(git status --porcelain=v1 --untracked-files=all)" ]; then
  printf '%s\n' 'Refusing to package source files that do not exactly match HEAD.' >&2
  exit 1
fi
if [ "$(git rev-parse --is-shallow-repository)" = "true" ]; then
  printf '%s\n' 'Refusing to package a shallow checkout: the real build number is unavailable.' >&2
  exit 1
fi

version="0.1.0-beta.1"
package_name="NeoFlash-${version}-linux-x64-preview"
commit="$(git rev-parse --verify HEAD)"
build_number="$(git rev-list --count HEAD)"
work_dir="$project_root/target/package-linux"
stage_dir="$work_dir/$package_name"
rust_licenses="$work_dir/rust-licenses"
archive_path="$project_root/dist/$package_name.tar.gz"
source_output="$project_root/dist/NeoFlash-${version}-portable-media-corresponding-source.tar.xz"
checksum_file="$project_root/dist/NeoFlash-${version}-linux-x64-SHA256SUMS.txt"
media_root="${NEOFLASH_LINUX_MEDIA_ROOT:-$project_root/target/bundled-media/linux-x64}"
media_source_archive="${NEOFLASH_LINUX_MEDIA_SOURCE_ARCHIVE:-$project_root/target/bundled-media/NeoFlash-${version}-portable-media-corresponding-source.tar.xz}"
package_complete=0

cleanup_incomplete() {
  if [ "$package_complete" -eq 0 ]; then
    rm -f "$archive_path" "$source_output" "$checksum_file"
  fi
}
trap cleanup_incomplete EXIT

mkdir -p "$project_root/dist" "$work_dir"
rm -f "$archive_path" "$source_output" "$checksum_file"
rm -rf "$stage_dir" "$rust_licenses"

./scripts/build-web.sh
if [ "$profile" = release ]; then
  cargo build --locked --release -p neoflash-editor
else
  cargo build --locked -p neoflash-editor
fi
executable="$project_root/target/$profile/neoflash-editor"
[ -x "$executable" ] || { printf 'Expected Linux executable not found: %s\n' "$executable" >&2; exit 1; }

python3 "$project_root/scripts/stage-rust-dependency-licenses.py" \
  --project-root "$project_root" \
  --package neoflash-editor \
  --dest "$rust_licenses" \
  --license-cache "$project_root/target/rust-license-text-cache" \
  --download-license-texts

python3 "$project_root/scripts/package-portable.py" all \
  --platform linux-x64 \
  --project-root "$project_root" \
  --version "$version" \
  --profile "$profile" \
  --commit "$commit" \
  --build-number "$build_number" \
  --executable "$executable" \
  --runtime-dir "$project_root/target/web-runtime" \
  --media-root "$media_root" \
  --media-source-archive "$media_source_archive" \
  --rust-licenses "$rust_licenses" \
  --stage-dir "$stage_dir" \
  --package-name "$package_name" \
  --archive "$archive_path" \
  --source-output "$source_output" \
  --checksum-file "$checksum_file"

package_complete=1
trap - EXIT
printf '%s\n' "$archive_path"
