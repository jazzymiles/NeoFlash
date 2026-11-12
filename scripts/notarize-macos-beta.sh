#!/usr/bin/env bash
# Developer ID Application signing, Apple notarization, stapling, and Gatekeeper verification
# for the NeoFlash macOS Beta 1 (`0.1.0-beta.1`) release candidate.
#
# Refuses to run without the project owner's explicit Apple credentials:
#   NEOFLASH_APPLE_DEVELOPER_ID="Developer ID Application: <Team Name> (<TEAMID>)"
#   NEOFLASH_NOTARY_PROFILE="<keychain-notarytool-profile>"
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"

if [ -z "${NEOFLASH_APPLE_DEVELOPER_ID:-}" ] || [ -z "${NEOFLASH_NOTARY_PROFILE:-}" ]; then
  printf '%s\n' 'Error: NEOFLASH_APPLE_DEVELOPER_ID and NEOFLASH_NOTARY_PROFILE must be set explicitly by the project owner.' >&2
  printf '%s\n' 'Example:' >&2
  printf '%s\n' '  NEOFLASH_APPLE_DEVELOPER_ID="Developer ID Application: Example (TEAMID1234)" \' >&2
  printf '%s\n' '  NEOFLASH_NOTARY_PROFILE="neoflash-notary" \' >&2
  printf '%s\n' '  ./scripts/notarize-macos-beta.sh' >&2
  exit 1
fi

version="0.1.0-beta.1"
app_dir="$project_root/dist/NeoFlash.app"
entitlements="$project_root/scripts/macos-entitlements.plist"
zip_path="$project_root/dist/NeoFlash-${version}-macos-arm64.zip"
dmg_path="$project_root/dist/NeoFlash-${version}-macos-arm64.dmg"
sums_path="$project_root/dist/NeoFlash-${version}-SHA256SUMS.txt"

if [ ! -d "$app_dir" ]; then
  printf '%s\n' "Error: $app_dir does not exist. Run ./scripts/package-macos.sh release first." >&2
  exit 1
fi

# 1. Inside-out Developer ID signing with Hardened Runtime and secure timestamp:
#    a) Bundled media shared libraries (.dylib)
if [ -d "$app_dir/Contents/Resources/media/lib" ]; then
  for dylib in "$app_dir/Contents/Resources/media/lib"/*.dylib; do
    [ -f "$dylib" ] || continue
    codesign --force --options runtime --timestamp \
      --sign "$NEOFLASH_APPLE_DEVELOPER_ID" "$dylib"
  done
fi

#    b) Bundled media helper executables (ffmpeg, ffprobe)
if [ -d "$app_dir/Contents/Resources/media/bin" ]; then
  for helper in "$app_dir/Contents/Resources/media/bin"/*; do
    [ -f "$helper" ] || continue
    codesign --force --options runtime --timestamp \
      --entitlements "$entitlements" \
      --sign "$NEOFLASH_APPLE_DEVELOPER_ID" "$helper"
  done
fi

#    c) Main application executable and bundle
codesign --force --options runtime --timestamp \
  --entitlements "$entitlements" \
  --sign "$NEOFLASH_APPLE_DEVELOPER_ID" \
  "$app_dir/Contents/MacOS/neoflash-editor"

codesign --force --options runtime --timestamp \
  --entitlements "$entitlements" \
  --sign "$NEOFLASH_APPLE_DEVELOPER_ID" \
  "$app_dir"

codesign --verify --deep --strict --verbose=2 "$app_dir"

# 2. Create submission ZIP, submit to Apple Notary Service, and staple ticket to .app
rm -f "$zip_path"
ditto -c -k --sequesterRsrc --keepParent "$app_dir" "$zip_path"
xcrun notarytool submit "$zip_path" --keychain-profile "$NEOFLASH_NOTARY_PROFILE" --wait
xcrun stapler staple "$app_dir"

# 3. Rebuild distributable ZIP and DMG with the stapled .app bundle
rm -f "$zip_path"
ditto -c -k --sequesterRsrc --keepParent "$app_dir" "$zip_path"

if command -v hdiutil >/dev/null 2>&1; then
  rm -f "$dmg_path"
  hdiutil create -volname "NeoFlash ${version}" -srcfolder "$app_dir" -ov -format UDZO "$dmg_path"
  codesign --force --timestamp --sign "$NEOFLASH_APPLE_DEVELOPER_ID" "$dmg_path"
  xcrun notarytool submit "$dmg_path" --keychain-profile "$NEOFLASH_NOTARY_PROFILE" --wait
  xcrun stapler staple "$dmg_path"
fi

# 4. Gatekeeper assessment
spctl --assess --type execute --verbose=4 "$app_dir"

# 5. Refresh SHA-256 sums
(
  cd "$project_root/dist"
  if [ -f "$(basename "$dmg_path")" ]; then
    shasum -a 256 "$(basename "$zip_path")" "$(basename "$dmg_path")" > "$(basename "$sums_path")"
  else
    shasum -a 256 "$(basename "$zip_path")" > "$(basename "$sums_path")"
  fi
)
printf 'Notarized and verified: %s\n' "$app_dir"
