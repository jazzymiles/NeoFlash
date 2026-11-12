#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
if ! git diff --quiet || ! git diff --cached --quiet; then
  printf '%s\n' 'Refusing to package tracked changes that are not committed.' >&2
  exit 1
fi
version="0.1.0-beta.1"
commit="$(git rev-parse --verify HEAD)"
build_number="$(git rev-list --count HEAD)"
./scripts/build-web.sh
profile="${1:-release}"
case "$profile" in
 release) cargo build --locked --release -p neoflash-editor ;;
 debug) cargo build --locked -p neoflash-editor ;;
 *) printf '%s\n' 'Usage: scripts/package-macos.sh [release|debug]' >&2; exit 1 ;;
esac
app_dir="$project_root/dist/NeoFlash.app"
rm -rf "$app_dir"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources/runtime"
cp "target/$profile/neoflash-editor" "$app_dir/Contents/MacOS/neoflash-editor"
cp target/web-runtime/neoflash_web.js target/web-runtime/neoflash_web_bg.wasm "$app_dir/Contents/Resources/runtime/"
cp THIRD_PARTY_NOTICES.txt "$app_dir/Contents/Resources/THIRD_PARTY_NOTICES.txt"
python3 "$project_root/scripts/stage-bundled-media.py" --dest "$app_dir/Contents/Resources/media"
cat > "$app_dir/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>NeoFlash</string>
<key>CFBundleDisplayName</key><string>NeoFlash</string>
<key>CFBundleIdentifier</key><string>org.neoflash.editor</string>
<key>CFBundleExecutable</key><string>neoflash-editor</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleVersion</key><string>$build_number</string>
<key>NeoFlashGitCommit</key><string>$commit</string>
<key>NeoFlashBuildProfile</key><string>$profile</string>
<key>LSMinimumSystemVersion</key><string>15.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
PLIST
for dylib in "$app_dir/Contents/Resources/media/lib"/*.dylib; do
  [ -f "$dylib" ] || continue
  codesign --force --sign - "$dylib"
done
for helper in "$app_dir/Contents/Resources/media/bin"/*; do
  [ -f "$helper" ] || continue
  codesign --force --sign - "$helper"
done
codesign --force --deep --sign - "$app_dir"
printf '%s\n' "$app_dir"

zip_path="$project_root/dist/NeoFlash-${version}-macos-arm64.zip"
dmg_path="$project_root/dist/NeoFlash-${version}-macos-arm64.dmg"
sums_path="$project_root/dist/NeoFlash-${version}-SHA256SUMS.txt"
rm -f "$zip_path" "$dmg_path" "$sums_path"
ditto -c -k --sequesterRsrc --keepParent "$app_dir" "$zip_path"
if command -v hdiutil >/dev/null 2>&1; then
  hdiutil create -volname "NeoFlash ${version}" -srcfolder "$app_dir" -ov -format UDZO "$dmg_path" >/dev/null 2>&1 || rm -f "$dmg_path"
fi
(
  cd "$project_root/dist"
  if [ -f "$(basename "$dmg_path")" ]; then
    shasum -a 256 "$(basename "$zip_path")" "$(basename "$dmg_path")" > "$(basename "$sums_path")"
  else
    shasum -a 256 "$(basename "$zip_path")" > "$(basename "$sums_path")"
  fi
)

studio_app_dir="$project_root/dist/NeoFlash UI Studio.app"
rm -rf "$studio_app_dir"
if [ "${NEOFLASH_PACKAGE_UI_STUDIO:-0}" = "1" ]; then
  mkdir -p "$studio_app_dir/Contents/MacOS" "$studio_app_dir/Contents/Resources"
  cp "target/$profile/neoflash-editor" "$studio_app_dir/Contents/MacOS/neoflash-ui-studio"
  cp THIRD_PARTY_NOTICES.txt "$studio_app_dir/Contents/Resources/THIRD_PARTY_NOTICES.txt"
  cat > "$studio_app_dir/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>NeoFlash UI Studio</string>
<key>CFBundleDisplayName</key><string>NeoFlash UI Studio</string>
<key>CFBundleIdentifier</key><string>org.neoflash.ui-studio</string>
<key>CFBundleExecutable</key><string>neoflash-ui-studio</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleVersion</key><string>$build_number</string>
<key>NeoFlashGitCommit</key><string>$commit</string>
<key>NeoFlashBuildProfile</key><string>$profile</string>
<key>LSMinimumSystemVersion</key><string>15.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
PLIST
  codesign --force --deep --sign - "$studio_app_dir"
  printf '%s\n' "$studio_app_dir"
fi
