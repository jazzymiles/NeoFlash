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
# App icon (crates/neoflash-editor/assets/app-icon, regenerate with scripts/build-app-icon.py): NeoFlash.icns
# carries every size for macOS 15; with Xcode's actool the Icon Composer document is compiled into Assets.car so
# macOS 26+ shows its light, dark and tinted appearances.
icon_src="$project_root/crates/neoflash-editor/assets/app-icon"
icon_car_dir="$project_root/target/app-icon/actool"
icon_plist_keys='<key>CFBundleIconFile</key><string>NeoFlash</string>'
rm -rf "$icon_car_dir"
if xcrun --find actool >/dev/null 2>&1; then
  mkdir -p "$icon_car_dir"
  if xcrun actool "$icon_src/NeoFlash.icon" --compile "$icon_car_dir" --app-icon NeoFlash --platform macosx \
      --minimum-deployment-target 15.0 --output-partial-info-plist "$icon_car_dir/partial.plist" >/dev/null 2>&1 \
      && [ -f "$icon_car_dir/Assets.car" ]; then
    icon_plist_keys="$icon_plist_keys<key>CFBundleIconName</key><string>NeoFlash</string>"
  else
    printf '%s\n' 'warning: actool could not compile NeoFlash.icon; packaging NeoFlash.icns only.' >&2
    rm -rf "$icon_car_dir"
  fi
fi
install_app_icon() {
  cp "$icon_src/NeoFlash.icns" "$1/NeoFlash.icns"
  if [ -f "$icon_car_dir/Assets.car" ]; then
    cp "$icon_car_dir/Assets.car" "$1/Assets.car"
  fi
}

app_dir="$project_root/dist/NeoFlash.app"
rm -rf "$app_dir"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources/runtime"
cp "target/$profile/neoflash-editor" "$app_dir/Contents/MacOS/neoflash-editor"
cp target/web-runtime/neoflash_web.js target/web-runtime/neoflash_web_bg.wasm "$app_dir/Contents/Resources/runtime/"
cp THIRD_PARTY_NOTICES.txt "$app_dir/Contents/Resources/THIRD_PARTY_NOTICES.txt"
install_app_icon "$app_dir/Contents/Resources"
python3 "$project_root/scripts/stage-bundled-media.py" --dest "$app_dir/Contents/Resources/media"
cat > "$app_dir/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>NeoFlash</string>
<key>CFBundleDisplayName</key><string>NeoFlash</string>
<key>CFBundleIdentifier</key><string>org.neoflash.editor</string>
<key>CFBundleExecutable</key><string>neoflash-editor</string>
$icon_plist_keys
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
  # Drag-to-install layout: the app, an Applications link and the NeoFlash volume icon.
  dmg_stage="$project_root/target/dmg-stage"
  dmg_rw="$project_root/target/dmg-stage.rw.dmg"
  rm -rf "$dmg_stage" "$dmg_rw"
  mkdir -p "$dmg_stage"
  ditto "$app_dir" "$dmg_stage/NeoFlash.app"
  ln -s /Applications "$dmg_stage/Applications"
  cp "$icon_src/NeoFlash.icns" "$dmg_stage/.VolumeIcon.icns"
  if hdiutil create -volname "NeoFlash ${version}" -srcfolder "$dmg_stage" -ov -format UDRW "$dmg_rw" >/dev/null 2>&1; then
    dmg_mount="$(mktemp -d "${TMPDIR:-/tmp}/neoflash-dmg.XXXXXX")"
    if hdiutil attach "$dmg_rw" -nobrowse -noautoopen -mountpoint "$dmg_mount" >/dev/null 2>&1; then
      SetFile -a C "$dmg_mount" 2>/dev/null \
        || xattr -wx com.apple.FinderInfo 0000000000000000040000000000000000000000000000000000000000000000 "$dmg_mount" \
        || true
      hdiutil detach "$dmg_mount" >/dev/null 2>&1 || hdiutil detach -force "$dmg_mount" >/dev/null 2>&1 || true
    fi
    rmdir "$dmg_mount" 2>/dev/null || true
    hdiutil convert "$dmg_rw" -format UDZO -ov -o "$dmg_path" >/dev/null 2>&1 || rm -f "$dmg_path"
  fi
  rm -rf "$dmg_stage" "$dmg_rw"
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
  install_app_icon "$studio_app_dir/Contents/Resources"
  cat > "$studio_app_dir/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>NeoFlash UI Studio</string>
<key>CFBundleDisplayName</key><string>NeoFlash UI Studio</string>
<key>CFBundleIdentifier</key><string>org.neoflash.ui-studio</string>
<key>CFBundleExecutable</key><string>neoflash-ui-studio</string>
$icon_plist_keys
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
