#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

usage() {
  printf '%s\n' 'Usage: scripts/package-macos.sh [release|debug] [--beta-release]'
  printf '%s\n' '  default: build only dist/NeoFlash.app for Apple Silicon'
  printf '%s\n' '  --beta-release: also build ZIP, DMG, corresponding source and SHA256SUMS'
}

profile="release"
profile_seen=0
beta_release=0
for arg in "$@"; do
  case "$arg" in
    release|debug)
      if [ "$profile_seen" -eq 1 ]; then
        usage >&2
        exit 1
      fi
      profile="$arg"
      profile_seen=1
      ;;
    --beta-release)
      beta_release=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 1
      ;;
  esac
done
if [ "$beta_release" -eq 1 ] && [ "$profile" != "release" ]; then
  printf '%s\n' 'A full Beta release must use the release Cargo profile.' >&2
  exit 1
fi

if [ -n "$(git status --porcelain=v1 --untracked-files=all)" ]; then
  printf '%s\n' 'Refusing to package source files that do not exactly match HEAD.' >&2
  exit 1
fi
version="0.1.0-beta.1"
commit="$(git rev-parse --verify HEAD)"
build_number="$(git rev-list --count HEAD)"
zip_path="$project_root/dist/NeoFlash-${version}-macos-arm64.zip"
dmg_path="$project_root/dist/NeoFlash-${version}-macos-arm64.dmg"
source_archive_name="NeoFlash-${version}-media-corresponding-source.tar.xz"
source_archive_path="$project_root/dist/$source_archive_name"
sums_path="$project_root/dist/NeoFlash-${version}-SHA256SUMS.txt"
app_dir="$project_root/dist/NeoFlash.app"
studio_app_dir="$project_root/dist/NeoFlash UI Studio.app"
source_bundle_dir="$project_root/target/package-macos/media-corresponding-source"
source_archive_stage="$project_root/target/package-macos/$source_archive_name"
source_report="$project_root/target/package-macos/media-provenance-report.json"
dmg_stage="$project_root/target/dmg-stage"
dmg_rw="$project_root/target/dmg-stage.rw.dmg"
dmg_mount=""
package_complete=0

cleanup_incomplete_package() {
  if [ "$package_complete" -eq 0 ]; then
    if [ -n "$dmg_mount" ] && mount | grep -Fq " on $dmg_mount "; then
      hdiutil detach "$dmg_mount" >/dev/null 2>&1 \
        || hdiutil detach -force "$dmg_mount" >/dev/null 2>&1 \
        || true
    fi
    rm -rf "$app_dir" "$studio_app_dir" "$source_bundle_dir"
    rm -f "$zip_path" "$dmg_path" "$source_archive_path" "$sums_path" "$source_archive_stage" "$project_root/dist/.DS_Store"
    rm -rf "$dmg_stage" "$dmg_rw"
    if [ -n "$dmg_mount" ]; then
      rmdir "$dmg_mount" 2>/dev/null || true
    fi
  fi
}
trap cleanup_incomplete_package EXIT

# Never leave a previous same-version macOS package looking like the result of a failed release gate.
# Clean ONLY macOS-owned artifacts and dist/.DS_Store — never remove Windows/Linux preview archives or
# *-portable-media-corresponding-source.tar.xz in dist/.
rm -f "$zip_path" "$dmg_path" "$source_archive_path" "$sums_path" "$project_root/dist/.DS_Store"
rm -rf "$studio_app_dir"
rm -rf "$source_bundle_dir"
rm -f "$source_archive_stage" "$source_report"
./scripts/build-web.sh
case "$profile" in
  release) cargo build --locked --release -p neoflash-editor ;;
  debug) cargo build --locked -p neoflash-editor ;;
esac
app_binary="$project_root/target/$profile/neoflash-editor"
app_archs="$(lipo -archs "$app_binary")"
if [ "$app_archs" != "arm64" ]; then
  printf 'Refusing to package a non-Apple-Silicon binary: %s (%s)\n' "$app_binary" "$app_archs" >&2
  exit 1
fi
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
  cp "$icon_src/NeoFlash.icns" "$1/AppIcon.icns"
  if [ -f "$icon_car_dir/Assets.car" ]; then
    cp "$icon_car_dir/Assets.car" "$1/Assets.car"
  fi
}

rm -rf "$app_dir"
mkdir -p \
  "$app_dir/Contents/MacOS" \
  "$app_dir/Contents/Resources/runtime" \
  "$app_dir/Contents/Resources/licenses/neoflash"
cp "target/$profile/neoflash-editor" "$app_dir/Contents/MacOS/neoflash-editor"
cp target/web-runtime/neoflash_web.js target/web-runtime/neoflash_web_bg.wasm "$app_dir/Contents/Resources/runtime/"
cp THIRD_PARTY_NOTICES.txt "$app_dir/Contents/Resources/THIRD_PARTY_NOTICES.txt"
cp LICENSE-MIT LICENSE-APACHE "$app_dir/Contents/Resources/licenses/neoflash/"
install_app_icon "$app_dir/Contents/Resources"
python3 "$project_root/scripts/stage-bundled-media.py" --dest "$app_dir/Contents/Resources/media"

# This bundle is generated from the exact Cargo.lock closure. Any dependency without a
# reproducible source identity and license record stops packaging before an archive is emitted.
python3 "$project_root/scripts/stage-rust-dependency-licenses.py" \
  --project-root "$project_root" \
  --package neoflash-editor \
  --dest "$app_dir/Contents/Resources/licenses/rust" \
  --license-cache "$project_root/target/rust-license-text-cache" \
  --download-license-texts

# Normalize permissions and scrub Finder clutter / resource-fork xattrs before nested signing.
find "$app_dir" -name '.DS_Store' -delete
xattr -cr "$app_dir"
chmod -R u+rwX,go+rX "$app_dir"

# Finalize nested media signatures before recording their distributed whole-file hashes.
for dylib in "$app_dir/Contents/Resources/media/lib"/*.dylib; do
  [ -f "$dylib" ] || continue
  codesign --force --sign - "$dylib"
done
for helper in "$app_dir/Contents/Resources/media/bin"/*; do
  [ -f "$helper" ] || continue
  codesign --force --sign - "$helper"
done

# The full-codec media helper is intentionally retained. Every build refreshes and verifies its
# post-signing payload hashes. The exact source/provenance closure and compressed redistribution
# bundle are release-only work that requires --beta-release.
mkdir -p "$(dirname "$source_archive_stage")"
if [ "$beta_release" -eq 1 ]; then
  python3 "$project_root/scripts/bundled-media-provenance.py" \
    --media-root "$app_dir/Contents/Resources/media" \
    --cache-dir "$project_root/target/bundled-media-source-cache" \
    --output-dir "$source_bundle_dir" \
    --archive "$source_archive_stage" \
    --report "$source_report" \
    --refresh-staged-hashes \
    --download
  mv "$source_archive_stage" "$source_archive_path"
else
  python3 "$project_root/scripts/bundled-media-provenance.py" \
    --media-root "$app_dir/Contents/Resources/media" \
    --report "$source_report" \
    --refresh-staged-hashes \
    --local-payload-only
fi

if [ "$beta_release" -eq 1 ]; then
  cat > "$app_dir/Contents/Resources/LICENSES-AND-SOURCE.txt" <<NOTICE
NeoFlash ${version} — Licenses and bundled-media source
========================================================

NeoFlash is free and open-source software licensed under MIT OR Apache-2.0.
The complete NeoFlash license texts are in Resources/licenses/neoflash/.

Notices for adapted algorithms and other third-party material are in
Resources/THIRD_PARTY_NOTICES.txt. Exact Rust dependency license records, versions,
Cargo.lock checksums and packaged license evidence are in Resources/licenses/rust/.

NeoFlash ships full-codec FFmpeg/FFprobe helper programs under GPL-3.0-or-later.
Their component notices and license texts are in Resources/media/licenses/ and
their exact provenance is in Resources/media/provenance/.

The complete, hash-verified corresponding-source bundle for the shipped media
helpers is distributed beside this application as:

  $source_archive_name

Verify the application archives and that source bundle with:

  NeoFlash-${version}-SHA256SUMS.txt
NOTICE
else
  cat > "$app_dir/Contents/Resources/LICENSES-AND-SOURCE.txt" <<NOTICE
NeoFlash ${version} — Local Apple Silicon application build
===========================================================

NeoFlash is free and open-source software licensed under MIT OR Apache-2.0.
The complete NeoFlash license texts are in Resources/licenses/neoflash/.

Notices for adapted algorithms and other third-party material are in
Resources/THIRD_PARTY_NOTICES.txt. Exact Rust dependency license records, versions,
Cargo.lock checksums and packaged license evidence are in Resources/licenses/rust/.

NeoFlash ships full-codec FFmpeg/FFprobe helper programs under GPL-3.0-or-later.
Their component notices, license texts and exact provenance are in Resources/media/.

This command intentionally built only the local Apple Silicon NeoFlash.app. Before
redistribution, explicitly create and verify the complete Beta release set with:

  ./scripts/package-macos.sh release --beta-release

That mode produces the corresponding-source archive, ZIP, DMG and SHA256SUMS beside
the application. Do not redistribute this app-only development build as a release.
NOTICE
fi
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
<key>NSAppleEventsUsageDescription</key><string>NeoFlash uses Adobe After Effects to save editable compositions when you export an After Effects project.</string>
<key>NSPrincipalClass</key><string>NSApplication</string>
<key>UTExportedTypeDeclarations</key>
<array>
  <dict>
    <key>UTTypeIdentifier</key><string>app.neoflash.project</string>
    <key>UTTypeDescription</key><string>NeoFlash Document</string>
    <key>UTTypeConformsTo</key>
    <array>
      <string>public.data</string>
      <string>public.composite-content</string>
    </array>
    <key>UTTypeIconFile</key><string>AppIcon</string>
    <key>UTTypeTagSpecification</key>
    <dict>
      <key>public.filename-extension</key>
      <array>
        <string>nflash</string>
      </array>
      <key>public.mime-type</key>
      <array>
        <string>application/vnd.neoflash.project+json</string>
      </array>
    </dict>
  </dict>
</array>
<key>UTImportedTypeDeclarations</key>
<array>
  <dict>
    <key>UTTypeIdentifier</key><string>com.adobe.actionscript-source</string>
    <key>UTTypeDescription</key><string>ActionScript Source</string>
    <key>UTTypeConformsTo</key>
    <array>
      <string>public.source-code</string>
      <string>public.plain-text</string>
    </array>
    <key>UTTypeTagSpecification</key>
    <dict>
      <key>public.filename-extension</key>
      <array>
        <string>as</string>
      </array>
      <key>public.mime-type</key>
      <array>
        <string>text/x-actionscript</string>
        <string>application/x-actionscript</string>
      </array>
    </dict>
  </dict>
</array>
<key>CFBundleDocumentTypes</key>
<array>
  <dict>
    <key>CFBundleTypeName</key><string>NeoFlash Document</string>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSHandlerRank</key><string>Owner</string>
    <key>LSItemContentTypes</key>
    <array>
      <string>app.neoflash.project</string>
    </array>
    <key>CFBundleTypeExtensions</key>
    <array>
      <string>nflash</string>
    </array>
    <key>CFBundleTypeIconFile</key><string>AppIcon</string>
  </dict>
  <dict>
    <key>CFBundleTypeName</key><string>ActionScript Source</string>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSHandlerRank</key><string>Alternate</string>
    <key>LSItemContentTypes</key>
    <array>
      <string>com.adobe.actionscript-source</string>
      <string>com.apple.applesingle-archive</string>
      <string>public.source-code</string>
      <string>public.script</string>
      <string>public.plain-text</string>
    </array>
    <key>CFBundleTypeExtensions</key>
    <array>
      <string>as</string>
    </array>
  </dict>
  <dict>
    <key>CFBundleTypeName</key><string>Flash &amp; Design Interchange Documents</string>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>LSHandlerRank</key><string>Alternate</string>
    <key>LSItemContentTypes</key>
    <array>
      <string>com.apple.applesingle-archive</string>
      <string>com.adobe.actionscript-source</string>
      <string>public.source-code</string>
      <string>public.script</string>
      <string>public.plain-text</string>
      <string>public.data</string>
      <string>public.content</string>
      <string>public.item</string>
      <string>public.folder</string>
      <string>public.archive</string>
      <string>com.adobe.pdf</string>
      <string>com.adobe.illustrator.ai-image</string>
      <string>com.adobe.encapsulated-postscript</string>
      <string>com.adobe.photoshop-image</string>
      <string>public.svg-image</string>
    </array>
    <key>CFBundleTypeExtensions</key>
    <array>
      <string>fla</string>
      <string>xfl</string>
      <string>as</string>
      <string>swf</string>
      <string>swc</string>
      <string>fig</string>
      <string>figma</string>
      <string>svg</string>
      <string>psd</string>
      <string>psb</string>
      <string>pdf</string>
      <string>ai</string>
      <string>eps</string>
      <string>fxg</string>
    </array>
  </dict>
</array>
</dict></plist>
PLIST
plutil -lint "$app_dir/Contents/Info.plist" >/dev/null
find "$app_dir" -name '.DS_Store' -delete
xattr -cr "$app_dir"
chmod -R u+rwX,go+rX "$app_dir"
codesign --force --sign - "$app_dir/Contents/MacOS/neoflash-editor"
codesign --force --sign - "$app_dir"
codesign --verify --deep --strict --verbose=2 "$app_dir"
bad_perms="$(find "$app_dir" \( -type d ! -perm -0555 -o -type f ! -perm -0444 \))"
if [ -n "$bad_perms" ]; then
  printf 'Bundle contains non-world-readable paths:\n%s\n' "$bad_perms" >&2
  exit 1
fi
ds_store_hits="$(find "$app_dir" -name '.DS_Store')"
if [ -n "$ds_store_hits" ]; then
  printf 'Bundle contains .DS_Store clutter:\n%s\n' "$ds_store_hits" >&2
  exit 1
fi
# Signing the outer bundle must not change nested media bytes certified above. Local app
# builds verify the staged payload only; complete source-closure auditing remains release-only.
if [ "$beta_release" -eq 1 ]; then
  python3 "$project_root/scripts/bundled-media-provenance.py" \
    --media-root "$app_dir/Contents/Resources/media" \
    --cache-dir "$project_root/target/bundled-media-source-cache" \
    --report "$project_root/target/package-macos/media-provenance-final.json" \
    --download
else
  python3 "$project_root/scripts/bundled-media-provenance.py" \
    --media-root "$app_dir/Contents/Resources/media" \
    --report "$project_root/target/package-macos/media-provenance-final.json" \
    --local-payload-only
fi
printf '%s\n' "$app_dir"

if [ "$beta_release" -eq 0 ]; then
  rm -rf "$source_bundle_dir"
  package_complete=1
  trap - EXIT
  printf '%s\n' 'App-only build complete; ZIP, DMG, corresponding source and SHA256SUMS were not produced.'
  exit 0
fi

ditto -c -k --norsrc --keepParent "$app_dir" "$zip_path"
command -v hdiutil >/dev/null 2>&1 || {
  printf '%s\n' 'hdiutil is required for the macOS release DMG.' >&2
  exit 1
}
# Drag-to-install layout: the app, an Applications link, the licence/source notice
# and the NeoFlash volume icon. Every step is required for a complete release set.
rm -rf "$dmg_stage" "$dmg_rw"
mkdir -p "$dmg_stage"
ditto --norsrc "$app_dir" "$dmg_stage/NeoFlash.app"
cp "$app_dir/Contents/Resources/LICENSES-AND-SOURCE.txt" "$dmg_stage/Licenses and source.txt"
ln -s /Applications "$dmg_stage/Applications"
cp "$icon_src/NeoFlash.icns" "$dmg_stage/.VolumeIcon.icns"
find "$dmg_stage" -name '.DS_Store' -delete
hdiutil create -volname "NeoFlash ${version}" -srcfolder "$dmg_stage" -ov -format UDRW "$dmg_rw" >/dev/null
dmg_mount="$(mktemp -d "${TMPDIR:-/tmp}/neoflash-dmg.XXXXXX")"
hdiutil attach "$dmg_rw" -nobrowse -noautoopen -mountpoint "$dmg_mount" >/dev/null
SetFile -a C "$dmg_mount" 2>/dev/null \
  || xattr -wx com.apple.FinderInfo 0000000000000000040000000000000000000000000000000000000000000000 "$dmg_mount"
find "$dmg_mount" -name '.DS_Store' -delete 2>/dev/null || true
hdiutil detach "$dmg_mount" >/dev/null \
  || hdiutil detach -force "$dmg_mount" >/dev/null
rmdir "$dmg_mount"
dmg_mount=""
hdiutil convert "$dmg_rw" -format UDZO -ov -o "$dmg_path" >/dev/null
rm -rf "$dmg_stage" "$dmg_rw"
codesign --force --sign - "$dmg_path"
codesign --verify --verbose=2 "$dmg_path"
rm -f "$project_root/dist/.DS_Store"
(
  cd "$project_root/dist"
  shasum -a 256 \
    "$(basename "$zip_path")" \
    "$(basename "$dmg_path")" \
    "$(basename "$source_archive_path")" \
    > "$(basename "$sums_path")"
)

if [ "${NEOFLASH_PACKAGE_UI_STUDIO:-0}" = "1" ]; then
  mkdir -p "$studio_app_dir/Contents/MacOS" "$studio_app_dir/Contents/Resources/licenses"
  cp "target/$profile/neoflash-editor" "$studio_app_dir/Contents/MacOS/neoflash-ui-studio"
  cp THIRD_PARTY_NOTICES.txt "$studio_app_dir/Contents/Resources/THIRD_PARTY_NOTICES.txt"
  ditto "$app_dir/Contents/Resources/licenses" "$studio_app_dir/Contents/Resources/licenses"
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
  plutil -lint "$studio_app_dir/Contents/Info.plist" >/dev/null
  find "$studio_app_dir" -name '.DS_Store' -delete
  xattr -cr "$studio_app_dir"
  chmod -R u+rwX,go+rX "$studio_app_dir"
  codesign --force --deep --sign - "$studio_app_dir"
  printf '%s\n' "$studio_app_dir"
fi

# The archive is the durable corresponding-source artifact; the expanded staging tree is
# reproducible and would otherwise duplicate roughly the same payload in target/.
rm -rf "$source_bundle_dir"
package_complete=1
trap - EXIT
