#!/usr/bin/env bash
# Atomically Developer-ID sign, notarize, staple and replace the complete macOS Beta 1 release set.
#
# Environment configuration:
#   Required owner credentials (production):
#     NEOFLASH_APPLE_DEVELOPER_ID   Developer ID Application signing identity (never "-")
#     NEOFLASH_NOTARY_PROFILE       xcrun notarytool keychain profile name
#
#   Optional hermetic pipeline-testing overrides (transactional harness only; overriding tool
#   binaries validates staging, checksum and rollback mechanics only, never real Apple Developer ID
#   signing or Apple Notary Service acceptance):
#     NEOFLASH_NOTARIZE_PROJECT_ROOT, NEOFLASH_DIST_DIR, NEOFLASH_NOTARIZE_WORK_PARENT,
#     NEOFLASH_NOTARIZE_MOUNT_PARENT, NEOFLASH_VERSION, NEOFLASH_ENTITLEMENTS_PLIST,
#     NEOFLASH_ICON_SRC, NEOFLASH_CODESIGN_BIN, NEOFLASH_XCRUN_BIN, NEOFLASH_HDIUTIL_BIN,
#     NEOFLASH_SPCTL_BIN, NEOFLASH_DITTO_BIN, NEOFLASH_SHASUM_BIN, NEOFLASH_MV_BIN,
#     NEOFLASH_SETFILE_BIN, NEOFLASH_XATTR_BIN, NEOFLASH_PYTHON3_BIN,
#     NEOFLASH_STAGE_RUST_LICENSES_SCRIPT, NEOFLASH_RUST_LICENSE_CACHE,
#     NEOFLASH_BUNDLED_MEDIA_PROVENANCE_SCRIPT, NEOFLASH_MEDIA_SOURCE_CACHE,
#     NEOFLASH_HOMEBREW_CELLAR.
set -euo pipefail

if [ "$#" -gt 0 ]; then
  printf '%s\n' 'Error: scripts/notarize-macos-beta.sh accepts no positional arguments; configure via environment variables.' >&2
  exit 2
fi

scripts_dir="$(cd "$(dirname "$0")" && pwd)"
default_project_root="$(cd "$scripts_dir/.." && pwd)"
project_root="${NEOFLASH_NOTARIZE_PROJECT_ROOT:-$default_project_root}"
cd "$project_root"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

if [ -z "${NEOFLASH_APPLE_DEVELOPER_ID:-}" ] || [ -z "${NEOFLASH_NOTARY_PROFILE:-}" ] \
  || [ -z "${NEOFLASH_APPLE_DEVELOPER_ID//[[:space:]]/}" ] || [ -z "${NEOFLASH_NOTARY_PROFILE//[[:space:]]/}" ]; then
  printf '%s\n' 'Error: NEOFLASH_APPLE_DEVELOPER_ID and NEOFLASH_NOTARY_PROFILE must be set explicitly by the project owner.' >&2
  exit 1
fi
if [ "$NEOFLASH_APPLE_DEVELOPER_ID" = "-" ]; then
  printf '%s\n' 'Error: NEOFLASH_APPLE_DEVELOPER_ID must be a Developer ID Application identity, not ad-hoc (-).' >&2
  exit 1
fi
if [ -n "$(git status --porcelain=v1 --untracked-files=all)" ]; then
  printf '%s\n' 'Refusing to notarize source files that do not exactly match HEAD.' >&2
  exit 1
fi

tool_overrides_active=0
for override_var in \
  "${NEOFLASH_CODESIGN_BIN:-}" \
  "${NEOFLASH_XCRUN_BIN:-}" \
  "${NEOFLASH_HDIUTIL_BIN:-}" \
  "${NEOFLASH_SPCTL_BIN:-}" \
  "${NEOFLASH_DITTO_BIN:-}" \
  "${NEOFLASH_SHASUM_BIN:-}" \
  "${NEOFLASH_MV_BIN:-}" \
  "${NEOFLASH_SETFILE_BIN:-}" \
  "${NEOFLASH_XATTR_BIN:-}" \
  "${NEOFLASH_STAGE_RUST_LICENSES_SCRIPT:-}" \
  "${NEOFLASH_BUNDLED_MEDIA_PROVENANCE_SCRIPT:-}"; do
  if [ -n "$override_var" ]; then
    tool_overrides_active=1
    break
  fi
done

codesign_bin="${NEOFLASH_CODESIGN_BIN:-codesign}"
xcrun_bin="${NEOFLASH_XCRUN_BIN:-xcrun}"
hdiutil_bin="${NEOFLASH_HDIUTIL_BIN:-hdiutil}"
spctl_bin="${NEOFLASH_SPCTL_BIN:-spctl}"
ditto_bin="${NEOFLASH_DITTO_BIN:-ditto}"
shasum_bin="${NEOFLASH_SHASUM_BIN:-shasum}"
mv_bin="${NEOFLASH_MV_BIN:-mv}"
setfile_bin="${NEOFLASH_SETFILE_BIN:-SetFile}"
xattr_bin="${NEOFLASH_XATTR_BIN:-xattr}"
python3_bin="${NEOFLASH_PYTHON3_BIN:-python3}"

command -v "$hdiutil_bin" >/dev/null 2>&1 || {
  printf '%s\n' 'Error: hdiutil is required for the notarized macOS release.' >&2
  exit 1
}
for required_tool in \
  "$codesign_bin" \
  "$xcrun_bin" \
  "$spctl_bin" \
  "$ditto_bin" \
  "$shasum_bin" \
  "$mv_bin" \
  "$xattr_bin" \
  "$python3_bin" \
  git; do
  command -v "$required_tool" >/dev/null 2>&1 || {
    printf 'Error: required tool %s is not available.\n' "$required_tool" >&2
    exit 1
  }
done

codesign() { command "$codesign_bin" "$@"; }
xcrun() { command "$xcrun_bin" "$@"; }
hdiutil() { command "$hdiutil_bin" "$@"; }
spctl() { command "$spctl_bin" "$@"; }
ditto() { command "$ditto_bin" "$@"; }
shasum() { command "$shasum_bin" "$@"; }
mv() { command "$mv_bin" "$@"; }
xattr() { command "$xattr_bin" "$@"; }

version="${NEOFLASH_VERSION:-0.1.0-beta.1}"
dist_dir="${NEOFLASH_DIST_DIR:-$project_root/dist}"
app_dir="$dist_dir/NeoFlash.app"
entitlements="${NEOFLASH_ENTITLEMENTS_PLIST:-$project_root/scripts/macos-entitlements.plist}"
zip_path="$dist_dir/NeoFlash-${version}-macos-arm64.zip"
dmg_path="$dist_dir/NeoFlash-${version}-macos-arm64.dmg"
source_archive_name="NeoFlash-${version}-media-corresponding-source.tar.xz"
source_archive_path="$dist_dir/$source_archive_name"
sums_path="$dist_dir/NeoFlash-${version}-SHA256SUMS.txt"
icon_src="${NEOFLASH_ICON_SRC:-$project_root/crates/neoflash-editor/assets/app-icon}"
stage_rust_licenses_script="${NEOFLASH_STAGE_RUST_LICENSES_SCRIPT:-$project_root/scripts/stage-rust-dependency-licenses.py}"
rust_license_cache="${NEOFLASH_RUST_LICENSE_CACHE:-$project_root/target/rust-license-text-cache}"
bundled_media_provenance_script="${NEOFLASH_BUNDLED_MEDIA_PROVENANCE_SCRIPT:-$project_root/scripts/bundled-media-provenance.py}"
media_source_cache="${NEOFLASH_MEDIA_SOURCE_CACHE:-$project_root/target/bundled-media-source-cache}"

if [ ! -d "$app_dir" ]; then
  printf 'Error: %s is missing. Run ./scripts/package-macos.sh release first.\n' "$app_dir" >&2
  exit 1
fi
for required in "$zip_path" "$dmg_path" "$source_archive_path" "$sums_path"; do
  if [ ! -f "$required" ]; then
    printf 'Error: %s is missing. Run ./scripts/package-macos.sh release first.\n' "$required" >&2
    exit 1
  fi
done
for bundle_member in \
  "$app_dir/Contents/Info.plist" \
  "$app_dir/Contents/MacOS/neoflash-editor" \
  "$app_dir/Contents/Resources/LICENSES-AND-SOURCE.txt" \
  "$app_dir/Contents/Resources/media/bin/ffmpeg" \
  "$app_dir/Contents/Resources/media/bin/ffprobe" \
  "$entitlements" \
  "$icon_src/NeoFlash.icns" \
  "$stage_rust_licenses_script" \
  "$bundled_media_provenance_script"; do
  if [ ! -f "$bundle_member" ]; then
    printf 'Error: required release input %s is missing.\n' "$bundle_member" >&2
    exit 1
  fi
done

verify_manifest_exact() {
  local target_dir="$1"
  local manifest_file="$2"
  (
    cd "$target_dir"
    shasum -a 256 -c "$(basename "$manifest_file")" >/dev/null
  )
  "$python3_bin" - "$manifest_file" \
    "$(basename "$zip_path")" \
    "$(basename "$dmg_path")" \
    "$source_archive_name" <<'PY'
import re
import sys
from pathlib import Path

manifest = Path(sys.argv[1])
expected_names = sys.argv[2:]
lines = [line.strip() for line in manifest.read_text(encoding="utf-8").splitlines() if line.strip()]
parsed = []
for line in lines:
    match = re.fullmatch(r"([0-9a-fA-F]{64})\s+\*?(.+)", line)
    if not match:
        sys.stderr.write(f"Error: malformed checksum entry in {manifest}: {line!r}\n")
        sys.exit(1)
    parsed.append((match.group(1).lower(), match.group(2)))
names = [name for _, name in parsed]
if len(names) != len(expected_names) or set(names) != set(expected_names):
    sys.stderr.write(
        f"Error: {manifest} must record exact release archives {expected_names}, got {names}.\n"
    )
    sys.exit(1)
PY
}

# Fail closed before staging if the existing 5-artifact release set in dist/ is inconsistent.
verify_manifest_exact "$dist_dir" "$sums_path"

work_parent="${NEOFLASH_NOTARIZE_WORK_PARENT:-$project_root/target/notarize-macos-beta}"
work_parent_created=0
if [ ! -d "$work_parent" ]; then
  mkdir -p "$work_parent"
  work_parent_created=1
fi
mount_parent="${NEOFLASH_NOTARIZE_MOUNT_PARENT:-${TMPDIR:-/tmp}}"
mount_parent_created=0
if [ ! -d "$mount_parent" ]; then
  mkdir -p "$mount_parent"
  mount_parent_created=1
fi

work_root="$(mktemp -d "$work_parent/run.XXXXXX")"
staged_app="$work_root/NeoFlash.app"
staged_zip="$work_root/NeoFlash-${version}-macos-arm64.zip"
staged_dmg="$work_root/NeoFlash-${version}-macos-arm64.dmg"
staged_source="$work_root/$source_archive_name"
staged_sums="$work_root/NeoFlash-${version}-SHA256SUMS.txt"
source_bundle_dir="$work_root/media-corresponding-source"
tx_dir=""
incoming_dir=""
backup_dir="$work_root/original-release"
dmg_stage="$work_root/dmg-stage"
dmg_rw="$work_root/dmg-stage.rw.dmg"
dmg_mount=""
dmg_attached=0
commit_started=0
commit_complete=0
cleanup_ran=0

cleanup() {
  if [ "${cleanup_ran:-0}" -eq 1 ]; then
    return 0
  fi
  cleanup_ran=1
  set +e
  # Always use OS /bin/mv during rollback so injected forward-swap faults never block recovery.
  unset -f mv 2>/dev/null || true

  if [ -n "$dmg_mount" ]; then
    if [ "$dmg_attached" -eq 1 ] || mount | grep -Fq " on $dmg_mount "; then
      hdiutil detach "$dmg_mount" >/dev/null 2>&1 \
        || hdiutil detach -force "$dmg_mount" >/dev/null 2>&1 \
        || true
      dmg_attached=0
    fi
    rmdir "$dmg_mount" 2>/dev/null || true
    dmg_mount=""
  fi

  local rollback_failed=0
  if [ "$commit_started" -eq 1 ] && [ "$commit_complete" -eq 0 ]; then
    if [ -e "$backup_dir/NeoFlash.app" ] || [ -L "$backup_dir/NeoFlash.app" ]; then
      rm -rf "$app_dir"
      mv "$backup_dir/NeoFlash.app" "$app_dir" || rollback_failed=1
    fi
    if [ -e "$backup_dir/$(basename "$zip_path")" ] || [ -L "$backup_dir/$(basename "$zip_path")" ]; then
      rm -f "$zip_path"
      mv "$backup_dir/$(basename "$zip_path")" "$zip_path" || rollback_failed=1
    fi
    if [ -e "$backup_dir/$(basename "$dmg_path")" ] || [ -L "$backup_dir/$(basename "$dmg_path")" ]; then
      rm -f "$dmg_path"
      mv "$backup_dir/$(basename "$dmg_path")" "$dmg_path" || rollback_failed=1
    fi
    if [ -e "$backup_dir/$source_archive_name" ] || [ -L "$backup_dir/$source_archive_name" ]; then
      rm -f "$source_archive_path"
      mv "$backup_dir/$source_archive_name" "$source_archive_path" || rollback_failed=1
    fi
    if [ -e "$backup_dir/$(basename "$sums_path")" ] || [ -L "$backup_dir/$(basename "$sums_path")" ]; then
      rm -f "$sums_path"
      mv "$backup_dir/$(basename "$sums_path")" "$sums_path" || rollback_failed=1
    fi
  fi

  if [ "$rollback_failed" -eq 0 ] && [ -n "$tx_dir" ] && [ -d "$tx_dir" ]; then
    rm -rf "$tx_dir"
  elif [ "$rollback_failed" -ne 0 ]; then
    printf 'CRITICAL: rollback could not restore all release files; backup preserved at %s\n' "$backup_dir" >&2
  fi
  if [ -n "$work_root" ] && [ -d "$work_root" ]; then
    rm -rf "$work_root"
  fi
  if [ "$work_parent_created" -eq 1 ]; then
    rmdir "$work_parent" 2>/dev/null || true
  fi
  if [ "$mount_parent_created" -eq 1 ]; then
    rmdir "$mount_parent" 2>/dev/null || true
  fi
}

handle_signal() {
  local exit_code="$1"
  trap - EXIT HUP INT TERM
  cleanup
  exit "$exit_code"
}

trap cleanup EXIT
trap 'handle_signal 129' HUP
trap 'handle_signal 130' INT
trap 'handle_signal 143' TERM

submit_and_verify_notarization() {
  local archive_path="$1"
  local label="$2"
  local result_json="$work_root/notary-${label}.json"
  local submit_rc=0
  xcrun notarytool submit "$archive_path" \
    --keychain-profile "$NEOFLASH_NOTARY_PROFILE" \
    --wait \
    --output-format json >"$result_json" || submit_rc=$?

  local parsed=""
  if [ -s "$result_json" ]; then
    parsed="$("$python3_bin" - "$result_json" <<'PY' || true
import json
import sys
from pathlib import Path

try:
    payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    if isinstance(payload, dict):
        status = str(payload.get("status", ""))
        sub_id = str(payload.get("id", ""))
        print(f"{status}\t{sub_id}")
except Exception:
    pass
PY
)"
  fi

  local tab=$'\t'
  local status="${parsed%%"$tab"*}"
  local submission_id=""
  if [[ "$parsed" == *"$tab"* ]]; then
    submission_id="${parsed#*"$tab"}"
  fi

  if [ "$submit_rc" -ne 0 ] || [ "$status" != "Accepted" ]; then
    printf 'Error: notarization failed for %s (exit=%d, status=%s, id=%s).\n' \
      "$label" "$submit_rc" "${status:-unknown}" "${submission_id:-none}" >&2
    if [ -n "$submission_id" ]; then
      xcrun notarytool log "$submission_id" \
        --keychain-profile "$NEOFLASH_NOTARY_PROFILE" >&2 || true
    fi
    return 1
  fi
}

media_provenance_extra_args=()
if [ -n "${NEOFLASH_HOMEBREW_CELLAR:-}" ]; then
  media_provenance_extra_args+=(--cellar "$NEOFLASH_HOMEBREW_CELLAR")
fi

ditto "$app_dir" "$staged_app"

# Rebuild the exact Rust license bundle before any final signing.
rm -rf "$staged_app/Contents/Resources/licenses/rust"
"$python3_bin" "$stage_rust_licenses_script" \
  --project-root "$project_root" \
  --package neoflash-editor \
  --dest "$staged_app/Contents/Resources/licenses/rust" \
  --license-cache "$rust_license_cache" \
  --download-license-texts

# Prove the ad-hoc packaged media before Developer ID signing changes its signature bytes.
"$python3_bin" "$bundled_media_provenance_script" \
  --media-root "$staged_app/Contents/Resources/media" \
  --cache-dir "$media_source_cache" \
  --report "$work_root/media-provenance-before-signing.json" \
  --download \
  ${media_provenance_extra_args[@]+"${media_provenance_extra_args[@]}"}

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
"$python3_bin" "$bundled_media_provenance_script" \
  --media-root "$staged_app/Contents/Resources/media" \
  --cache-dir "$media_source_cache" \
  --output-dir "$source_bundle_dir" \
  --archive "$staged_source" \
  --report "$work_root/media-provenance-after-signing.json" \
  --refresh-staged-hashes \
  --download \
  ${media_provenance_extra_args[@]+"${media_provenance_extra_args[@]}"}

find "$staged_app" -name '.DS_Store' -delete
xattr -cr "$staged_app"
chmod -R u+rwX,go+rX "$staged_app"

codesign --force --options runtime --timestamp \
  --entitlements "$entitlements" \
  --sign "$NEOFLASH_APPLE_DEVELOPER_ID" \
  "$staged_app/Contents/MacOS/neoflash-editor"
codesign --force --options runtime --timestamp \
  --entitlements "$entitlements" \
  --sign "$NEOFLASH_APPLE_DEVELOPER_ID" \
  "$staged_app"
codesign --verify --deep --strict --verbose=2 "$staged_app"

bad_perms="$(find "$staged_app" \( -type d ! -perm -0555 -o -type f ! -perm -0444 \))"
if [ -n "$bad_perms" ]; then
  printf 'Bundle contains non-world-readable paths:\n%s\n' "$bad_perms" >&2
  exit 1
fi
ds_store_hits="$(find "$staged_app" -name '.DS_Store')"
if [ -n "$ds_store_hits" ]; then
  printf 'Bundle contains .DS_Store clutter:\n%s\n' "$ds_store_hits" >&2
  exit 1
fi

"$python3_bin" "$bundled_media_provenance_script" \
  --media-root "$staged_app/Contents/Resources/media" \
  --cache-dir "$media_source_cache" \
  --report "$work_root/media-provenance-final.json" \
  --download \
  ${media_provenance_extra_args[@]+"${media_provenance_extra_args[@]}"}

# Submit a temporary ZIP, staple and validate the app, then build the final distributable ZIP.
ditto -c -k --norsrc --keepParent "$staged_app" "$staged_zip"
submit_and_verify_notarization "$staged_zip" "app-zip"
xcrun stapler staple "$staged_app"
xcrun stapler validate "$staged_app"
rm -f "$staged_zip"
ditto -c -k --norsrc --keepParent "$staged_app" "$staged_zip"

# Recreate the same drag-to-install wrapper as package-macos.sh.
mkdir -p "$dmg_stage"
ditto --norsrc "$staged_app" "$dmg_stage/NeoFlash.app"
cp "$staged_app/Contents/Resources/LICENSES-AND-SOURCE.txt" "$dmg_stage/Licenses and source.txt"
ln -s /Applications "$dmg_stage/Applications"
cp "$icon_src/NeoFlash.icns" "$dmg_stage/.VolumeIcon.icns"
find "$dmg_stage" -name '.DS_Store' -delete
hdiutil create -volname "NeoFlash ${version}" -srcfolder "$dmg_stage" -ov -format UDRW "$dmg_rw" >/dev/null
dmg_mount_raw="$(mktemp -d "$mount_parent/neoflash-notary-dmg.XXXXXX")"
dmg_mount="$(cd "$dmg_mount_raw" && pwd -P)"
dmg_attached=1
hdiutil attach "$dmg_rw" -nobrowse -noautoopen -mountpoint "$dmg_mount" >/dev/null
"$setfile_bin" -a C "$dmg_mount" 2>/dev/null \
  || xattr -wx com.apple.FinderInfo 0000000000000000040000000000000000000000000000000000000000000000 "$dmg_mount"
find "$dmg_mount" -name '.DS_Store' -delete 2>/dev/null || true
hdiutil detach "$dmg_mount" >/dev/null \
  || hdiutil detach -force "$dmg_mount" >/dev/null
dmg_attached=0
rmdir "$dmg_mount"
dmg_mount=""
hdiutil convert "$dmg_rw" -format UDZO -ov -o "$staged_dmg" >/dev/null
rm -rf "$dmg_stage" "$dmg_rw"
codesign --force --timestamp --sign "$NEOFLASH_APPLE_DEVELOPER_ID" "$staged_dmg"
codesign --verify --verbose=2 "$staged_dmg"
submit_and_verify_notarization "$staged_dmg" "dmg"
xcrun stapler staple "$staged_dmg"
xcrun stapler validate "$staged_dmg"
hdiutil verify "$staged_dmg" >/dev/null

spctl --assess --type execute --verbose=4 "$staged_app"
spctl --assess --type open --context context:primary-signature --verbose=4 "$staged_dmg"
(
  cd "$work_root"
  shasum -a 256 \
    "$(basename "$staged_zip")" \
    "$(basename "$staged_dmg")" \
    "$(basename "$staged_source")" \
    > "$(basename "$staged_sums")"
)
verify_manifest_exact "$work_root" "$staged_sums"

# Stage incoming artifacts and backups inside dist_dir so every commit/rollback move is an
# atomic same-filesystem rename, never touching live files until incoming checksums pass.
tx_dir="$(mktemp -d "$dist_dir/.notarize-tx.XXXXXX")"
incoming_dir="$tx_dir/incoming"
backup_dir="$tx_dir/backup"
mkdir -p "$incoming_dir" "$backup_dir"
mv "$staged_app" "$incoming_dir/NeoFlash.app"
mv "$staged_zip" "$incoming_dir/$(basename "$zip_path")"
mv "$staged_dmg" "$incoming_dir/$(basename "$dmg_path")"
mv "$staged_source" "$incoming_dir/$source_archive_name"
mv "$staged_sums" "$incoming_dir/$(basename "$sums_path")"
verify_manifest_exact "$incoming_dir" "$incoming_dir/$(basename "$sums_path")"

# Replace the five-file release set only after every gate, signature and notarization succeeds.
commit_started=1
[ ! -e "$app_dir" ] || mv "$app_dir" "$backup_dir/NeoFlash.app"
[ ! -e "$zip_path" ] || mv "$zip_path" "$backup_dir/$(basename "$zip_path")"
[ ! -e "$dmg_path" ] || mv "$dmg_path" "$backup_dir/$(basename "$dmg_path")"
[ ! -e "$source_archive_path" ] || mv "$source_archive_path" "$backup_dir/$source_archive_name"
[ ! -e "$sums_path" ] || mv "$sums_path" "$backup_dir/$(basename "$sums_path")"
mv "$incoming_dir/NeoFlash.app" "$app_dir"
mv "$incoming_dir/$(basename "$zip_path")" "$zip_path"
mv "$incoming_dir/$(basename "$dmg_path")" "$dmg_path"
mv "$incoming_dir/$source_archive_name" "$source_archive_path"
mv "$incoming_dir/$(basename "$sums_path")" "$sums_path"
verify_manifest_exact "$dist_dir" "$sums_path"
commit_complete=1

rm -rf "$tx_dir"
tx_dir=""
trap - EXIT HUP INT TERM
rm -rf "$work_root"
if [ "$work_parent_created" -eq 1 ]; then
  rmdir "$work_parent" 2>/dev/null || true
fi
if [ "$mount_parent_created" -eq 1 ]; then
  rmdir "$mount_parent" 2>/dev/null || true
fi

if [ "$tool_overrides_active" -eq 1 ]; then
  printf 'Simulated notarization pipeline complete (tool overrides active; not real Apple Notary acceptance): %s\n' "$app_dir"
else
  printf 'Notarized and verified complete release set: %s\n' "$app_dir"
fi
