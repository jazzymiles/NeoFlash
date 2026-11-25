#!/usr/bin/env bash
# Build, stage, sign, and Rosetta-2-verify the standalone macOS Intel (x86_64-apple-darwin)
# NeoFlash.app bundle under target/artifacts/macos-x86_64 without touching top-level dist/.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/verify-macos-intel.sh [release|debug] [options]

Options:
  release|debug       Cargo profile to build and stage (default: release)
  --keep-deps         Retain target/x86_64-apple-darwin/<profile>/deps after verification
  --skip-gui-smoke    Skip windowed NEOFLASH_SCREENSHOT GUI capture (headless CI fallback)
  --allow-no-rosetta  Allow build+Mach-O verification to pass when Rosetta 2 is unavailable
  -h, --help          Show this help text and exit
EOF
}

profile="release"
profile_seen=0
keep_deps=0
gui_smoke=1
allow_no_rosetta=0

for arg in "$@"; do
  case "$arg" in
    release|debug)
      if [ "$profile_seen" -eq 1 ]; then
        printf '%s\n' "Error: duplicate profile argument '$arg'." >&2
        usage >&2
        exit 2
      fi
      profile="$arg"
      profile_seen=1
      ;;
    --keep-deps)
      keep_deps=1
      ;;
    --skip-gui-smoke)
      gui_smoke=0
      ;;
    --allow-no-rosetta)
      allow_no_rosetta=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf '%s\n' "Error: unknown argument '$arg'." >&2
      usage >&2
      exit 2
      ;;
  esac
done

scripts_dir="$(cd "$(dirname "$0")" && pwd -P)"
default_project_root="$(cd "$scripts_dir/.." && pwd -P)"
project_root="${NEOFLASH_INTEL_PROJECT_ROOT:-$default_project_root}"
cd "$project_root"

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
deployment_target="${NEOFLASH_INTEL_DEPLOYMENT_TARGET:-12.0}"
export MACOSX_DEPLOYMENT_TARGET="$deployment_target"
target_triple="x86_64-apple-darwin"

arch_bin="${NEOFLASH_ARCH_BIN:-/usr/bin/arch}"
lipo_bin="${NEOFLASH_LIPO_BIN:-/usr/bin/lipo}"
file_bin="${NEOFLASH_FILE_BIN:-/usr/bin/file}"
otool_bin="${NEOFLASH_OTOOL_BIN:-/usr/bin/otool}"
codesign_bin="${NEOFLASH_CODESIGN_BIN:-/usr/bin/codesign}"
plutil_bin="${NEOFLASH_PLUTIL_BIN:-/usr/bin/plutil}"
xattr_bin="${NEOFLASH_XATTR_BIN:-/usr/bin/xattr}"
python3_bin="${NEOFLASH_PYTHON3_BIN:-}"
if [ -z "$python3_bin" ]; then
  for candidate_py in /opt/homebrew/bin/python3 /usr/local/bin/python3 /usr/bin/python3; do
    if [ -x "$candidate_py" ]; then
      python3_bin="$candidate_py"
      break
    fi
  done
fi
if [ -z "$python3_bin" ]; then
  printf '%s\n' 'Error: python3 is required for Mach-O, PNG, and JSON verification.' >&2
  exit 1
fi

raw_artifacts_dir="${NEOFLASH_INTEL_ARTIFACTS_DIR:-$project_root/target/artifacts/macos-x86_64}"
artifacts_dir="$("$python3_bin" -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$raw_artifacts_dir")"
dist_real="$("$python3_bin" -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$project_root/dist")"
case "$artifacts_dir" in
  "$dist_real"|"$dist_real"/*)
    printf 'Error: refusing to stage Intel verification artifacts inside top-level dist/ (%s).\n' "$artifacts_dir" >&2
    exit 2
    ;;
esac

mkdir -p "$artifacts_dir"
artifacts_dir="$(cd "$artifacts_dir" && pwd -P)"

app_dir="$artifacts_dir/NeoFlash.app"
bundle_bin="$app_dir/Contents/MacOS/neoflash-editor"
screenshot_png="$artifacts_dir/intel-dark.png"
report_json="$artifacts_dir/verification-report.json"
smoke_work=""
verify_complete=0

cleanup() {
  set +e
  if [ -n "$smoke_work" ] && [ -d "$smoke_work" ]; then
    rm -rf "$smoke_work"
  fi
  if [ "$verify_complete" -eq 0 ]; then
    rm -rf "$app_dir"
    rm -f "$screenshot_png" "${screenshot_png%.png}.state.ron" "$report_json"
  fi
}
trap cleanup EXIT

version="$("$python3_bin" - "$project_root/Cargo.toml" <<'PY'
import re
import sys
from pathlib import Path

cargo_toml = Path(sys.argv[1])
if cargo_toml.is_file():
    text = cargo_toml.read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if match:
        print(match.group(1))
        raise SystemExit(0)
print("0.1.0-beta.1")
PY
)"
commit="$(git rev-parse --verify HEAD 2>/dev/null || printf 'dev')"
build_number="$(git rev-list --count HEAD 2>/dev/null || printf '1')"

if [ -n "${NEOFLASH_INTEL_PREBUILT_BINARY:-}" ]; then
  built_binary="$NEOFLASH_INTEL_PREBUILT_BINARY"
  if [ ! -f "$built_binary" ]; then
    printf 'Error: NEOFLASH_INTEL_PREBUILT_BINARY does not exist: %s\n' "$built_binary" >&2
    exit 1
  fi
else
  if ! rustup target list --installed | grep -Fxq "$target_triple"; then
    rustup target add "$target_triple"
  fi
  if [ ! -f "$project_root/target/web-runtime/neoflash_web.js" ] \
    || [ ! -f "$project_root/target/web-runtime/neoflash_web_bg.wasm" ]; then
    "$project_root/scripts/build-web.sh"
  fi
  case "$profile" in
    release)
      cargo build --locked --release --target "$target_triple" --config "env.MACOSX_DEPLOYMENT_TARGET=\"$deployment_target\"" -p neoflash-editor
      ;;
    debug)
      cargo build --locked --target "$target_triple" --config "env.MACOSX_DEPLOYMENT_TARGET=\"$deployment_target\"" -p neoflash-editor
      ;;
  esac
  built_binary="$project_root/target/$target_triple/$profile/neoflash-editor"
fi

if [ ! -x "$built_binary" ]; then
  printf 'Error: compiled Intel binary is missing or not executable: %s\n' "$built_binary" >&2
  exit 1
fi

icon_src="$project_root/crates/neoflash-editor/assets/app-icon"
icon_car_dir="$project_root/target/app-icon/actool-x86_64"
icon_plist_keys='<key>CFBundleIconFile</key><string>NeoFlash</string>'
rm -rf "$icon_car_dir"
if [ "${NEOFLASH_INTEL_SKIP_ACTOOL:-0}" != "1" ] && xcrun --find actool >/dev/null 2>&1 \
  && [ -d "$icon_src/NeoFlash.icon" ]; then
  mkdir -p "$icon_car_dir"
  if xcrun actool "$icon_src/NeoFlash.icon" --compile "$icon_car_dir" --app-icon NeoFlash \
      --platform macosx --minimum-deployment-target "$deployment_target" \
      --output-partial-info-plist "$icon_car_dir/partial.plist" >/dev/null 2>&1 \
      && [ -f "$icon_car_dir/Assets.car" ]; then
    icon_plist_keys="$icon_plist_keys<key>CFBundleIconName</key><string>NeoFlash</string>"
  else
    rm -rf "$icon_car_dir"
  fi
fi

rm -rf "$app_dir"
rm -f "$screenshot_png" "${screenshot_png%.png}.state.ron" "$report_json"
mkdir -p \
  "$app_dir/Contents/MacOS" \
  "$app_dir/Contents/Resources/runtime" \
  "$app_dir/Contents/Resources/licenses/neoflash"

cp "$built_binary" "$bundle_bin"
chmod 0755 "$bundle_bin"

web_runtime_dir="${NEOFLASH_INTEL_WEB_RUNTIME_DIR:-$project_root/target/web-runtime}"
for runtime_asset in neoflash_web.js neoflash_web_bg.wasm; do
  if [ ! -f "$web_runtime_dir/$runtime_asset" ]; then
    printf 'Error: missing web runtime asset %s/%s.\n' "$web_runtime_dir" "$runtime_asset" >&2
    exit 1
  fi
  cp "$web_runtime_dir/$runtime_asset" "$app_dir/Contents/Resources/runtime/$runtime_asset"
done

cp "$project_root/THIRD_PARTY_NOTICES.txt" "$app_dir/Contents/Resources/THIRD_PARTY_NOTICES.txt"
cp "$project_root/LICENSE-MIT" "$project_root/LICENSE-APACHE" "$app_dir/Contents/Resources/licenses/neoflash/"

if [ -f "$icon_src/NeoFlash.icns" ]; then
  cp "$icon_src/NeoFlash.icns" "$app_dir/Contents/Resources/NeoFlash.icns"
  cp "$icon_src/NeoFlash.icns" "$app_dir/Contents/Resources/AppIcon.icns"
fi
if [ -f "$icon_car_dir/Assets.car" ]; then
  cp "$icon_car_dir/Assets.car" "$app_dir/Contents/Resources/Assets.car"
fi

if [ "${NEOFLASH_INTEL_SKIP_RUST_LICENSES:-0}" != "1" ]; then
  "$python3_bin" "$project_root/scripts/stage-rust-dependency-licenses.py" \
    --project-root "$project_root" \
    --package neoflash-editor \
    --dest "$app_dir/Contents/Resources/licenses/rust" \
    --license-cache "$project_root/target/rust-license-text-cache" \
    --download-license-texts \
    --replace
fi

cat > "$app_dir/Contents/Resources/LICENSES-AND-SOURCE.txt" <<NOTICE
NeoFlash ${version} — Local macOS Intel (x86_64) verification build
===================================================================

NeoFlash is free and open-source software licensed under MIT OR Apache-2.0.
The complete NeoFlash license texts are in Resources/licenses/neoflash/.
Third-party notices are in Resources/THIRD_PARTY_NOTICES.txt and exact Rust
dependency license evidence is in Resources/licenses/rust/.

This self-contained x86_64 verification bundle is staged under:
  target/artifacts/macos-x86_64/NeoFlash.app
NOTICE

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
<key>NeoFlashTargetTriple</key><string>$target_triple</string>
<key>LSMinimumSystemVersion</key><string>$deployment_target</string>
<key>LSArchitecturePriority</key>
<array>
  <string>x86_64</string>
</array>
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

"$plutil_bin" -lint "$app_dir/Contents/Info.plist" >/dev/null
find "$app_dir" -name '.DS_Store' -delete
"$xattr_bin" -cr "$app_dir"
chmod -R u+rwX,go+rX "$app_dir"

"$codesign_bin" --force --sign - "$bundle_bin"
"$codesign_bin" --force --sign - "$app_dir"

# 1. Strict Mach-O architecture, deployment target, LC_RPATH, and linkage verification.
bundle_archs="$("$lipo_bin" -archs "$bundle_bin" | tr -d '\r')"
if [ "$bundle_archs" != "x86_64" ]; then
  printf 'Error: expected lipo -archs strictly equal to x86_64, got: %s\n' "$bundle_archs" >&2
  exit 1
fi

bundle_file_desc="$("$file_bin" -b "$bundle_bin" | tr -d '\r')"
case "$bundle_file_desc" in
  *"Mach-O 64-bit executable x86_64"*) ;;
  *)
    printf 'Error: expected file(1) to report Mach-O 64-bit executable x86_64, got: %s\n' "$bundle_file_desc" >&2
    exit 1
    ;;
esac

macho_audit_json="$("$python3_bin" - "$bundle_bin" "$otool_bin" <<'PY'
import hashlib
import json
import subprocess
import sys
from pathlib import Path

binary = Path(sys.argv[1])
otool_bin = sys.argv[2]

otool_l = subprocess.run(
    [otool_bin, "-l", str(binary)],
    check=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
).stdout
otool_deps_raw = subprocess.run(
    [otool_bin, "-L", str(binary)],
    check=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
).stdout

rpaths = []
minos = None
in_rpath = False
in_build_ver = False
for raw in otool_l.splitlines():
    line = raw.strip()
    if line.startswith("Load command "):
        in_rpath = False
        in_build_ver = False
        continue
    if line.startswith("cmd "):
        in_rpath = line == "cmd LC_RPATH"
        in_build_ver = line == "cmd LC_BUILD_VERSION"
        continue
    if in_rpath and line.startswith("path "):
        val = line[len("path ") :].strip()
        if " (offset " in val and val.endswith(")"):
            val = val.rsplit(" (offset ", 1)[0]
        rpaths.append(val)
        in_rpath = False
    elif in_build_ver and line.startswith("minos "):
        minos = line[len("minos ") :].strip()

def is_internal_rpath(p: str) -> bool:
    if not (
        p == "@executable_path"
        or p.startswith("@executable_path/")
        or p == "@loader_path"
        or p.startswith("@loader_path/")
    ):
        return False
    return not any(bad in p for bad in ("/opt/homebrew", "/usr/local", "/Users/"))

external_rpaths = [p for p in rpaths if not is_internal_rpath(p)]
if external_rpaths:
    sys.stderr.write(f"Error: disallowed external LC_RPATH entries in {binary}: {external_rpaths}\n")
    raise SystemExit(1)

non_system_dylibs = []
linked_dylibs = []
for raw in otool_deps_raw.splitlines()[1:]:
    stripped = raw.strip()
    if not stripped:
        continue
    dep = stripped.split(" (compatibility version", 1)[0].strip()
    if not dep:
        continue
    linked_dylibs.append(dep)
    if not (dep.startswith("/System/Library/") or dep.startswith("/usr/lib/")):
        non_system_dylibs.append(dep)

if non_system_dylibs:
    sys.stderr.write(f"Error: non-system dynamic libraries linked by {binary}: {non_system_dylibs}\n")
    raise SystemExit(1)

h = hashlib.sha256()
with binary.open("rb") as handle:
    for chunk in iter(lambda: handle.read(1024 * 1024), b""):
        h.update(chunk)

print(
    json.dumps(
        {
            "sha256": h.hexdigest(),
            "size_bytes": binary.stat().st_size,
            "minos": minos,
            "lc_rpaths": rpaths,
            "lc_rpath_count": len(rpaths),
            "external_lc_rpath_count": len(external_rpaths),
            "linked_dylib_count": len(linked_dylibs),
            "non_system_dylibs": non_system_dylibs,
        }
    )
)
PY
)"

"$codesign_bin" --verify --deep --strict --verbose=2 "$app_dir" >/dev/null 2>&1

bad_perms="$(find "$app_dir" \( -type d ! -perm -0555 -o -type f ! -perm -0444 \))"
if [ -n "$bad_perms" ]; then
  printf 'Error: bundle contains non-world-readable paths:\n%s\n' "$bad_perms" >&2
  exit 1
fi
ds_store_hits="$(find "$app_dir" -name '.DS_Store')"
if [ -n "$ds_store_hits" ]; then
  printf 'Error: bundle contains .DS_Store clutter:\n%s\n' "$ds_store_hits" >&2
  exit 1
fi

# 2. Rosetta 2 / native Intel execution smoke with system-only PATH.
host_arch="${NEOFLASH_HOST_ARCH_OVERRIDE:-$(uname -m)}"
can_exec_x86_64=0
execution_mode="unavailable"
if [ "$host_arch" = "x86_64" ]; then
  if "$arch_bin" -x86_64 /usr/bin/true >/dev/null 2>&1; then
    can_exec_x86_64=1
    execution_mode="native-x86_64"
  fi
elif "$arch_bin" -x86_64 /usr/bin/true >/dev/null 2>&1; then
  can_exec_x86_64=1
  execution_mode="rosetta2"
fi

smoke_work="$(mktemp -d "${TMPDIR:-/tmp}/neoflash-intel-smoke.XXXXXX")"
smoke_home="$smoke_work/home"
smoke_tmp="$smoke_work/tmp"
mkdir -p "$smoke_home" "$smoke_tmp"

run_intel_bundle() {
  (
    cd "$smoke_work"
    env -i \
      HOME="$smoke_home" \
      TMPDIR="$smoke_tmp" \
      USER="${USER:-neoflash}" \
      LOGNAME="${LOGNAME:-neoflash}" \
      PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
      "$arch_bin" -x86_64 "$bundle_bin" "$@"
  )
}

cli_summary_json='{"executed":false}'
gui_summary_json='{"status":"skipped"}'

if [ "$can_exec_x86_64" -eq 0 ]; then
  if [ "$allow_no_rosetta" -eq 1 ]; then
    printf 'Warning: Rosetta 2 is unavailable on %s; skipped x86_64 runtime execution (--allow-no-rosetta).\n' "$host_arch" >&2
  else
    printf 'Error: cannot execute x86_64 binaries on host %s (install Rosetta 2 or pass --allow-no-rosetta).\n' "$host_arch" >&2
    exit 1
  fi
else
  version_out="$(run_intel_bundle --version)"
  capabilities_out="$(run_intel_bundle --headless capabilities)"

  smoke_project="$smoke_work/intel-smoke.nflash"
  new_out="$(run_intel_bundle --headless new --language as3 --output "$smoke_project")"

  batch_out="$(run_intel_bundle --headless batch --project "$smoke_project" --in-place --stdin <<'JSON'
{
  "ops": [
    {
      "op": "add_element",
      "kind": "rectangle",
      "name": "intel_stage_rect",
      "x": 48.0,
      "y": 64.0,
      "width": 200.0,
      "height": 120.0,
      "fill": "#FF5500"
    },
    {
      "op": "set_script",
      "frame": 0,
      "language": "as3",
      "source": "trace(\"intel-rosetta-as3-ok\");"
    }
  ]
}
JSON
)"

  validate_out="$(run_intel_bundle --headless validate --strict --project "$smoke_project")"
  compile_out="$(run_intel_bundle --headless compile --strict --project "$smoke_project")"
  eval_out="$(run_intel_bundle --headless eval --scripts --project "$smoke_project")"

  html5_dir="$smoke_work/html5-bundle"
  export_html5_out="$(run_intel_bundle --headless export --target html5 --project "$smoke_project" --out "$html5_dir")"

  swf_file="$smoke_work/intel-smoke.swf"
  export_swf_out="$(run_intel_bundle --headless export --target swf --project "$smoke_project" --out "$swf_file")"

  printf '%s\n' "$version_out" > "$smoke_work/version.txt"
  printf '%s\n' "$capabilities_out" > "$smoke_work/capabilities.json"
  printf '%s\n' "$new_out" > "$smoke_work/new.json"
  printf '%s\n' "$batch_out" > "$smoke_work/batch.json"
  printf '%s\n' "$validate_out" > "$smoke_work/validate.json"
  printf '%s\n' "$compile_out" > "$smoke_work/compile.json"
  printf '%s\n' "$eval_out" > "$smoke_work/eval.json"
  printf '%s\n' "$export_html5_out" > "$smoke_work/export-html5.json"
  printf '%s\n' "$export_swf_out" > "$smoke_work/export-swf.json"

  cli_summary_json="$("$python3_bin" - "$smoke_work" "$version" <<'PY'
import json
import sys
from pathlib import Path

work = Path(sys.argv[1])
expected_version = sys.argv[2]

version_line = (work / "version.txt").read_text(encoding="utf-8").strip()
if expected_version not in version_line:
    raise SystemExit(f"Unexpected --version output: {version_line!r}")

def load_ok(name: str, expected_cmd: str | None = None) -> dict:
    data = json.loads((work / name).read_text(encoding="utf-8"))
    if data.get("ok") is not True:
        raise SystemExit(f"CLI check {name} failed: {data}")
    if expected_cmd and data.get("command") != expected_cmd:
        raise SystemExit(f"CLI check {name} returned unexpected command: {data.get('command')}")
    return data

caps = load_ok("capabilities.json", "capabilities")
new_res = load_ok("new.json", "new")
load_ok("batch.json", "batch")
val_res = load_ok("validate.json", "validate")
if not val_res.get("valid") or val_res.get("validation_issue_count") != 0 or val_res.get("script_error_count") != 0 or val_res.get("script_warning_count") != 0:
    raise SystemExit(f"Strict validation reported issues: {val_res}")

cmp_res = load_ok("compile.json", "compile")
if cmp_res.get("error_count") != 0 or cmp_res.get("warning_count") != 0 or cmp_res.get("compiled_scripts_count", 0) < 1:
    raise SystemExit(f"Strict compilation check failed: {cmp_res}")

eval_res = load_ok("eval.json", "eval")
traces = eval_res.get("trace_lines", [])
if "intel-rosetta-as3-ok" not in traces:
    raise SystemExit(f"Expected AS3 trace output in eval --scripts, got: {traces}")

h5_res = load_ok("export-html5.json", "export")
html5_dir = work / "html5-bundle"
for member in ("index.html", "bundle.js", "neoflash_web.js", "neoflash-export.json"):
    p = html5_dir / member
    if not p.is_file() or p.stat().st_size == 0:
        raise SystemExit(f"Published HTML5 bundle missing or empty {member}")

load_ok("export-swf.json", "export")
swf_path = work / "intel-smoke.swf"
swf_bytes = swf_path.read_bytes()
if len(swf_bytes) <= 32 or swf_bytes[:3] not in (b"FWS", b"CWS", b"ZWS"):
    raise SystemExit(f"Published SWF has invalid header or size ({len(swf_bytes)} bytes)")

print(
    json.dumps(
        {
            "executed": True,
            "system_only_path": "/usr/bin:/bin:/usr/sbin:/sbin",
            "version_output": version_line,
            "capabilities_subcommand_count": len(caps.get("subcommands", {})),
            "new_project_format_version": new_res.get("format_version"),
            "validate_strict_valid": True,
            "compile_strict_compiled_scripts": cmp_res.get("compiled_scripts_count"),
            "eval_scripts_traces": traces,
            "export_html5_bundle_bytes": (html5_dir / "bundle.js").stat().st_size,
            "export_html5_runtime_dir": h5_res.get("runtime_dir"),
            "export_swf_bytes": len(swf_bytes),
            "export_swf_signature": swf_bytes[:3].decode("ascii"),
        }
    )
)
PY
)"

  if [ "$gui_smoke" -eq 1 ]; then
    env -i \
      HOME="$smoke_home" \
      TMPDIR="$smoke_tmp" \
      USER="${USER:-neoflash}" \
      LOGNAME="${LOGNAME:-neoflash}" \
      PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
      NEOFLASH_WINDOW="1280x800" \
      NEOFLASH_SCREENSHOT="$screenshot_png" \
      NEOFLASH_SCREENSHOT_THEME="dark" \
      "$arch_bin" -x86_64 "$bundle_bin"
    rm -f "${screenshot_png%.png}.state.ron"

    gui_summary_json="$("$python3_bin" - "$screenshot_png" <<'PY'
import json
import struct
import sys
from pathlib import Path

png_path = Path(sys.argv[1])
if not png_path.is_file():
    raise SystemExit(f"GUI screenshot was not written: {png_path}")
data = png_path.read_bytes()
if len(data) < 64 or data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
    raise SystemExit(f"Invalid PNG header in {png_path} ({len(data)} bytes)")
width, height = struct.unpack(">II", data[16:24])
if width < 1280 or height < 800:
    raise SystemExit(f"Unexpected PNG dimensions {width}x{height} (expected >= 1280x800)")

print(
    json.dumps(
        {
            "status": "passed",
            "path": str(png_path),
            "size_bytes": len(data),
            "width": width,
            "height": height,
        }
    )
)
PY
)"
  fi
fi

cleaned_intermediate=0
if [ "$keep_deps" -eq 0 ] && [ -z "${NEOFLASH_INTEL_PREBUILT_BINARY:-}" ]; then
  rm -rf \
    "$project_root/target/$target_triple/$profile/deps" \
    "$project_root/target/$target_triple/$profile/incremental"
  cleaned_intermediate=1
fi

"$python3_bin" - \
  "$report_json" \
  "$version" \
  "$commit" \
  "$build_number" \
  "$profile" \
  "$target_triple" \
  "$deployment_target" \
  "$host_arch" \
  "$execution_mode" \
  "$app_dir" \
  "$bundle_archs" \
  "$bundle_file_desc" \
  "$macho_audit_json" \
  "$cli_summary_json" \
  "$gui_summary_json" \
  "$cleaned_intermediate" <<'PY'
import json
import sys
from pathlib import Path

(
    report_path,
    version,
    commit,
    build_number,
    profile,
    target_triple,
    deployment_target,
    host_arch,
    execution_mode,
    app_dir,
    lipo_archs,
    file_desc,
    macho_raw,
    cli_raw,
    gui_raw,
    cleaned_flag,
) = sys.argv[1:]

macho = json.loads(macho_raw)
cli_checks = json.loads(cli_raw)
gui_checks = json.loads(gui_raw)

receipt = {
    "schema_version": 1,
    "status": "passed",
    "version": version,
    "git_commit": commit,
    "build_number": int(build_number) if build_number.isdigit() else build_number,
    "profile": profile,
    "target_triple": target_triple,
    "deployment_target": deployment_target,
    "host_arch": host_arch,
    "execution_mode": execution_mode,
    "app_bundle_path": app_dir,
    "mach_o": {
        "lipo_archs": lipo_archs,
        "file_description": file_desc,
        "codesign_verified": True,
        **macho,
    },
    "rosetta_cli_checks": cli_checks,
    "gui_screenshot_smoke": gui_checks,
    "cleaned_intermediate_deps": cleaned_flag == "1",
}

Path(report_path).write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

verify_complete=1
trap - EXIT
if [ -n "$smoke_work" ] && [ -d "$smoke_work" ]; then
  rm -rf "$smoke_work"
fi

printf 'Verified macOS Intel (%s) bundle (%s): %s\nReceipt: %s\n' \
  "$target_triple" "$execution_mode" "$app_dir" "$report_json"
