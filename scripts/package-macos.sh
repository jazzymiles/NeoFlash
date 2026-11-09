#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"
export PATH="$HOME/.cargo/bin:$PATH"
if ! git diff --quiet || ! git diff --cached --quiet; then
  printf '%s\n' 'Refusing to package tracked changes that are not committed.' >&2
  exit 1
fi
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
cat > "$app_dir/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>NeoFlash</string>
<key>CFBundleDisplayName</key><string>NeoFlash</string>
<key>CFBundleIdentifier</key><string>org.neoflash.editor</string>
<key>CFBundleExecutable</key><string>neoflash-editor</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>$build_number</string>
<key>NeoFlashGitCommit</key><string>$commit</string>
<key>NeoFlashBuildProfile</key><string>$profile</string>
<key>LSMinimumSystemVersion</key><string>15.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
PLIST
codesign --force --deep --sign - "$app_dir"
printf '%s\n' "$app_dir"

studio_app_dir="$project_root/dist/NeoFlash UI Studio.app"
rm -rf "$studio_app_dir"
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
<key>CFBundleShortVersionString</key><string>0.1.0</string>
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
