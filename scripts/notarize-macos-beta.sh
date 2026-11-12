#!/usr/bin/env bash
# Atomically Developer-ID sign, notarize, staple and replace the complete macOS Beta 1 release set.
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

if [ -z "${NEOFLASH_APPLE_DEVELOPER_ID:-}" ] || [ -z "${NEOFLASH_NOTARY_PROFILE:-}" ]; then
  printf '%s\n' 'Error: NEOFLASH_APPLE_DEVELOPER_ID and NEOFLASH_NOTARY_PROFILE must be set explicitly by the project owner.' >&2
  exit 1
fi
if [ -n "$(git status --porcelain=v1 --untracked-files=all)" ]; then
  printf '%s\n' 'Refusing to notarize source files that do not exactly match HEAD.' >&2
  exit 1
fi

version="0.1.0-beta.1"
app_dir="$project_root/dist/NeoFlash.app"
entitlements="$project_root/scripts/macos-entitlements.plist"
zip_path="$project_root/dist/NeoFlash-${version}-macos-arm64.zip"
dmg_path="$project_root/dist/NeoFlash-${version}-macos-arm64.dmg"
source_archive_name="NeoFlash-${version}-media-corresponding-source.tar.xz"
source_archive_path="$project_root/dist/$source_archive_name"
sums_path="$project_root/dist/NeoFlash-${version}-SHA256SUMS.txt"
icon_src="$project_root/crates/neoflash-editor/assets/app-icon"

for required in "$app_dir" "$source_archive_path"; do
  if [ ! -e "$required" ]; then
    printf 'Error: %s is missing. Run ./scripts/package-macos.sh release first.\n' "$required" >&2
    exit 1
  fi
done
command -v hdiutil >/dev/null 2>&1 || {
  printf '%s\n' 'Error: hdiutil is required for the notarized macOS release.' >&2
  exit 1
}

work_parent="$project_root/target/notarize-macos-beta"
mkdir -p "$work_parent"
work_root="$(mktemp -d "$work_parent/run.XXXXXX")"
staged_app="$work_root/NeoFlash.app"
staged_zip="$work_root/NeoFlash-${version}-macos-arm64.zip"
staged_dmg="$work_root/NeoFlash-${version}-macos-arm64.dmg"
staged_source="$work_root/$source_archive_name"
staged_sums="$work_root/NeoFlash-${version}-SHA256SUMS.txt"
source_bundle_dir="$work_root/media-corresponding-source"
backup_dir="$work_root/original-release"
dmg_stage="$work_root/dmg-stage"
dmg_rw="$work_root/dmg-stage.rw.dmg"
dmg_mount=""
commit_started=0
commit_complete=0

cleanup() {
  if [ -n "$dmg_mount" ] && mount | grep -Fq " on $dmg_mount "; then
    hdiutil detach "$dmg_mount" >/dev/null 2>&1 \
      || hdiutil detach -force "$dmg_mount" >/dev/null 2>&1 \
      || true
  fi
  if [ "$commit_started" -eq 1 ] && [ "$commit_complete" -eq 0 ]; then
    rm -rf "$app_dir"
    rm -f "$zip_path" "$dmg_path" "$source_archive_path" "$sums_path"
    [ ! -e "$backup_dir/NeoFlash.app" ] || mv "$backup_dir/NeoFlash.app" "$app_dir"
    [ ! -e "$backup_dir/$(basename "$zip_path")" ] || mv "$backup_dir/$(basename "$zip_path")" "$zip_path"
    [ ! -e "$backup_dir/$(basename "$dmg_path")" ] || mv "$backup_dir/$(basename "$dmg_path")" "$dmg_path"
    [ ! -e "$backup_dir/$source_archive_name" ] || mv "$backup_dir/$source_archive_name" "$source_archive_path"
    [ ! -e "$backup_dir/$(basename "$sums_path")" ] || mv "$backup_dir/$(basename "$sums_path")" "$sums_path"
  fi
  rm -rf "$work_root"
}
trap cleanup EXIT

ditto "$app_dir" "$staged_app"

# Rebuild the exact Rust license bundle before any final signing.
rm -rf "$staged_app/Contents/Resources/licenses/rust"
python3 "$project_root/scripts/stage-rust-dependency-licenses.py" \
  --project-root "$project_root" \
  --package neoflash-editor \
  --dest "$staged_app/Contents/Resources/licenses/rust" \
  --license-cache "$project_root/target/rust-license-text-cache" \
  --download-license-texts

# Prove the ad-hoc packaged media before Developer ID signing changes its signature bytes.
python3 "$project_root/scripts/bundled-media-provenance.py" \
  --media-root "$staged_app/Contents/Resources/media" \
  --cache-dir "$project_root/target/bundled-media-source-cache" \
  --report "$work_root/media-provenance-before-signing.json" \
  --download

for dylib in "$staged_app/Contents/Resources/media/lib"/*.dylib; do
  [ -f "$dylib" ] || continue
  codesign --force --options runtime --timestamp \
    --sign "$NEOFLASH_APPLE_DEVELOPER_ID" "$dylib"
done
for helper in "$staged_app/Contents/Resources/media/bin"/*; do
  [ -f "$helper" ] || continue
  codesign --force --options runtime --timestamp \
    --entitlements "$entitlements" \
    --sign "$NEOFLASH_APPLE_DEVELOPER_ID" "$helper"
done

# Refresh only the signed whole-file hashes, then reconstruct and re-audit the complete
# corresponding-source archive against those final media bytes.
python3 "$project_root/scripts/bundled-media-provenance.py" \
  --media-root "$staged_app/Contents/Resources/media" \
  --cache-dir "$project_root/target/bundled-media-source-cache" \
  --output-dir "$source_bundle_dir" \
  --archive "$staged_source" \
  --report "$work_root/media-provenance-after-signing.json" \
  --refresh-staged-hashes \
  --download

codesign --force --options runtime --timestamp \
  --entitlements "$entitlements" \
  --sign "$NEOFLASH_APPLE_DEVELOPER_ID" \
  "$staged_app/Contents/MacOS/neoflash-editor"
codesign --force --options runtime --timestamp \
  --entitlements "$entitlements" \
  --sign "$NEOFLASH_APPLE_DEVELOPER_ID" \
  "$staged_app"
codesign --verify --deep --strict --verbose=2 "$staged_app"
python3 "$project_root/scripts/bundled-media-provenance.py" \
  --media-root "$staged_app/Contents/Resources/media" \
  --cache-dir "$project_root/target/bundled-media-source-cache" \
  --report "$work_root/media-provenance-final.json" \
  --download

# Submit a temporary ZIP, staple the app, then build the actual distributable ZIP.
ditto -c -k --sequesterRsrc --keepParent "$staged_app" "$staged_zip"
xcrun notarytool submit "$staged_zip" --keychain-profile "$NEOFLASH_NOTARY_PROFILE" --wait
xcrun stapler staple "$staged_app"
rm -f "$staged_zip"
ditto -c -k --sequesterRsrc --keepParent "$staged_app" "$staged_zip"

# Recreate the same drag-to-install wrapper as package-macos.sh.
mkdir -p "$dmg_stage"
ditto "$staged_app" "$dmg_stage/NeoFlash.app"
cp "$staged_app/Contents/Resources/LICENSES-AND-SOURCE.txt" "$dmg_stage/Licenses and source.txt"
ln -s /Applications "$dmg_stage/Applications"
cp "$icon_src/NeoFlash.icns" "$dmg_stage/.VolumeIcon.icns"
hdiutil create -volname "NeoFlash ${version}" -srcfolder "$dmg_stage" -ov -format UDRW "$dmg_rw" >/dev/null
dmg_mount="$(mktemp -d "${TMPDIR:-/tmp}/neoflash-notary-dmg.XXXXXX")"
hdiutil attach "$dmg_rw" -nobrowse -noautoopen -mountpoint "$dmg_mount" >/dev/null
SetFile -a C "$dmg_mount" 2>/dev/null \
  || xattr -wx com.apple.FinderInfo 0000000000000000040000000000000000000000000000000000000000000000 "$dmg_mount"
hdiutil detach "$dmg_mount" >/dev/null \
  || hdiutil detach -force "$dmg_mount" >/dev/null
rmdir "$dmg_mount"
dmg_mount=""
hdiutil convert "$dmg_rw" -format UDZO -ov -o "$staged_dmg" >/dev/null
codesign --force --timestamp --sign "$NEOFLASH_APPLE_DEVELOPER_ID" "$staged_dmg"
xcrun notarytool submit "$staged_dmg" --keychain-profile "$NEOFLASH_NOTARY_PROFILE" --wait
xcrun stapler staple "$staged_dmg"

spctl --assess --type execute --verbose=4 "$staged_app"
(
  cd "$work_root"
  shasum -a 256 \
    "$(basename "$staged_zip")" \
    "$(basename "$staged_dmg")" \
    "$(basename "$staged_source")" \
    > "$(basename "$staged_sums")"
)

# Replace the five-file release set only after every gate, signature and notarization succeeds.
mkdir -p "$backup_dir"
commit_started=1
[ ! -e "$app_dir" ] || mv "$app_dir" "$backup_dir/NeoFlash.app"
[ ! -e "$zip_path" ] || mv "$zip_path" "$backup_dir/$(basename "$zip_path")"
[ ! -e "$dmg_path" ] || mv "$dmg_path" "$backup_dir/$(basename "$dmg_path")"
[ ! -e "$source_archive_path" ] || mv "$source_archive_path" "$backup_dir/$source_archive_name"
[ ! -e "$sums_path" ] || mv "$sums_path" "$backup_dir/$(basename "$sums_path")"
mv "$staged_app" "$app_dir"
mv "$staged_zip" "$zip_path"
mv "$staged_dmg" "$dmg_path"
mv "$staged_source" "$source_archive_path"
mv "$staged_sums" "$sums_path"
commit_complete=1
rm -rf "$backup_dir"
trap - EXIT
rm -rf "$work_root"
printf 'Notarized and verified complete release set: %s\n' "$app_dir"
