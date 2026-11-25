"""Unit and structural regression tests for scripts/verify-macos-intel.sh."""

from __future__ import annotations

import json
import os
from pathlib import Path
import plistlib
import re
import stat
import struct
import subprocess
import tempfile
import unittest
import zlib


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "verify-macos-intel.sh"


def make_minimal_png(width: int, height: int) -> bytes:
    def chunk(tag: bytes, payload: bytes) -> bytes:
        crc = zlib.crc32(tag + payload) & 0xFFFFFFFF
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", crc)

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    # Minimal valid 1x1 compressed scanline placeholder (script inspects PNG signature + IHDR).
    idat = zlib.compress(b"\x00\x10\x14\x1e\xff")
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")


def write_exec(path: Path, body: str) -> Path:
    path.write_text(body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IRUSR)
    return path


class MacosIntelVerifyScriptTests(unittest.TestCase):
    def test_executable_and_shell_syntax(self):
        self.assertTrue(SCRIPT.is_file(), f"missing {SCRIPT}")
        self.assertTrue(os.access(SCRIPT, os.X_OK), "verify-macos-intel.sh must be executable")
        subprocess.run(["bash", "-n", str(SCRIPT)], check=True)

    def test_structural_invariants_and_info_plist(self):
        text = SCRIPT.read_text(encoding="utf-8")
        self.assertIn('deployment_target="${NEOFLASH_INTEL_DEPLOYMENT_TARGET:-12.0}"', text)
        self.assertIn('export MACOSX_DEPLOYMENT_TARGET="$deployment_target"', text)
        self.assertIn('target_triple="x86_64-apple-darwin"', text)
        self.assertIn(
            'cargo build --locked --release --target "$target_triple" --config "env.MACOSX_DEPLOYMENT_TARGET=\\"$deployment_target\\"" -p neoflash-editor',
            text,
        )
        self.assertIn('target/artifacts/macos-x86_64', text)
        self.assertIn('PATH="/usr/bin:/bin:/usr/sbin:/sbin"', text)
        self.assertIn('--verify --deep --strict --verbose=2', text)
        self.assertNotIn('"$project_root/dist/NeoFlash', text)

        match = re.search(
            r'cat > "\$app_dir/Contents/Info\.plist" <<PLIST\n(.*?)\nPLIST',
            text,
            re.DOTALL,
        )
        self.assertIsNotNone(match)
        rendered = (
            match.group(1)
            .replace("$icon_plist_keys", "<key>CFBundleIconFile</key><string>NeoFlash</string>")
            .replace("$version", "0.1.0-beta.1")
            .replace("$build_number", "584")
            .replace("$commit", "deadbeef")
            .replace("$profile", "release")
            .replace("$target_triple", "x86_64-apple-darwin")
            .replace("$deployment_target", "12.0")
        )
        plist = plistlib.loads(rendered.encode("utf-8"))
        self.assertEqual(plist["LSMinimumSystemVersion"], "12.0")
        self.assertEqual(plist["LSArchitecturePriority"], ["x86_64"])
        self.assertEqual(plist["NeoFlashTargetTriple"], "x86_64-apple-darwin")
        imported_utis = [u["UTTypeIdentifier"] for u in plist.get("UTImportedTypeDeclarations", [])]
        self.assertIn("com.adobe.actionscript-source", imported_utis)
        doc_types = {d["CFBundleTypeName"]: d for d in plist.get("CFBundleDocumentTypes", [])}
        self.assertIn("ActionScript Source", doc_types)
        self.assertIn("as", doc_types["ActionScript Source"]["CFBundleTypeExtensions"])
        self.assertIn(
            "com.apple.applesingle-archive",
            doc_types["ActionScript Source"]["LSItemContentTypes"],
        )
        self.assertIn("Flash & Design Interchange Documents", doc_types)
        self.assertIn(
            "fla",
            doc_types["Flash & Design Interchange Documents"]["CFBundleTypeExtensions"],
        )
        self.assertIn(
            "swf",
            doc_types["Flash & Design Interchange Documents"]["CFBundleTypeExtensions"],
        )
        with tempfile.NamedTemporaryFile("wb", suffix=".plist") as handle:
            handle.write(rendered.encode("utf-8"))
            handle.flush()
            subprocess.run(["/usr/bin/plutil", "-lint", handle.name], check=True)

    def test_cli_help_and_invalid_flag_rejection(self):
        help_res = subprocess.run(
            [str(SCRIPT), "--help"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        self.assertEqual(help_res.returncode, 0)
        self.assertIn("Usage: scripts/verify-macos-intel.sh", help_res.stdout)
        self.assertIn("--keep-deps", help_res.stdout)
        self.assertIn("--skip-gui-smoke", help_res.stdout)

        for bad_args in (["--unknown-flag"], ["release", "debug"]):
            with self.subTest(args=bad_args):
                res = subprocess.run(
                    [str(SCRIPT), *bad_args],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                    check=False,
                )
                self.assertEqual(res.returncode, 2)

    def test_refuses_staging_inside_top_level_dist(self):
        with tempfile.TemporaryDirectory() as tmp:
            project = Path(tmp) / "proj"
            (project / "dist").mkdir(parents=True)
            env = os.environ.copy()
            env["NEOFLASH_INTEL_PROJECT_ROOT"] = str(project)
            env["NEOFLASH_INTEL_ARTIFACTS_DIR"] = str(project / "dist" / "intel")
            res = subprocess.run(
                [str(SCRIPT)],
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertEqual(res.returncode, 2)
            self.assertIn("refusing to stage Intel verification artifacts inside top-level dist/", res.stderr)
            self.assertFalse(
                (project / "dist" / "intel").exists(),
                "dist/ guard must run before mkdir -p creates any subdirectory inside dist/",
            )

    def _build_hermetic_fixture(
        self,
        root: Path,
        *,
        lipo_arch: str = "x86_64",
        file_desc: str = "Mach-O 64-bit executable x86_64",
        rpath: str | None = None,
        extra_dylib: str = "/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit",
        rosetta_ok: bool = True,
    ) -> dict[str, str]:
        project = root / "project"
        (project / "crates/neoflash-editor/assets/app-icon").mkdir(parents=True)
        (project / "target/web-runtime").mkdir(parents=True)
        (project / "dist").mkdir(parents=True)
        (project / "dist/sentinel.txt").write_text("untouched\n", encoding="utf-8")

        (project / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.0-beta.1"\n', encoding="utf-8")
        (project / "THIRD_PARTY_NOTICES.txt").write_text("notices\n", encoding="utf-8")
        (project / "LICENSE-MIT").write_text("MIT\n", encoding="utf-8")
        (project / "LICENSE-APACHE").write_text("Apache\n", encoding="utf-8")
        (project / "crates/neoflash-editor/assets/app-icon/NeoFlash.icns").write_bytes(b"icns")
        (project / "target/web-runtime/neoflash_web.js").write_text("// js\n", encoding="utf-8")
        (project / "target/web-runtime/neoflash_web_bg.wasm").write_bytes(b"\x00asm\x01\x00\x00\x00")

        tools = root / "tools"
        tools.mkdir()
        png_fixture = root / "fixture-shot.png"
        png_fixture.write_bytes(make_minimal_png(1280, 800))

        mock_editor = write_exec(
            root / "mock-neoflash-editor",
            f"""#!/bin/sh
set -eu
if [ -n "${{NEOFLASH_SCREENSHOT:-}}" ]; then
  /bin/cp "{png_fixture}" "$NEOFLASH_SCREENSHOT"
  exit 0
fi
case "${{1:-}}" in
  --version)
    printf '%s\\n' 'neoflash-editor 0.1.0-beta.1 (build 1, commit abcdef0)'
    exit 0
    ;;
  --headless)
    sub="${{2:-}}"
    case "$sub" in
      capabilities)
        printf '%s\\n' '{{"ok":true,"command":"capabilities","subcommands":{{"new":true,"validate":true}}}}'
        ;;
      new)
        out=""
        while [ "$#" -gt 0 ]; do
          if [ "$1" = "--output" ]; then out="$2"; shift 2; else shift; fi
        done
        printf '%s\\n' '{{"mock":true}}' > "$out"
        printf '%s\\n' '{{"ok":true,"command":"new","format_version":6}}'
        ;;
      batch)
        /bin/cat >/dev/null
        printf '%s\\n' '{{"ok":true,"command":"batch"}}'
        ;;
      validate)
        printf '%s\\n' '{{"ok":true,"command":"validate","valid":true,"validation_issue_count":0,"script_error_count":0,"script_warning_count":0}}'
        ;;
      compile)
        printf '%s\\n' '{{"ok":true,"command":"compile","error_count":0,"warning_count":0,"compiled_scripts_count":1}}'
        ;;
      eval)
        printf '%s\\n' '{{"ok":true,"command":"eval","trace_lines":["intel-rosetta-as3-ok"]}}'
        ;;
      export)
        target=""
        out=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --target) target="$2"; shift 2 ;;
            --out) out="$2"; shift 2 ;;
            *) shift ;;
          esac
        done
        if [ "$target" = "html5" ]; then
          /bin/mkdir -p "$out"
          printf '%s\\n' '<html></html>' > "$out/index.html"
          printf '%s\\n' '// bundle' > "$out/bundle.js"
          printf '%s\\n' '// runtime' > "$out/neoflash_web.js"
          printf '%s\\n' '{{"ok":true}}' > "$out/neoflash-export.json"
        elif [ "$target" = "swf" ]; then
          printf 'FWS\\x0a0123456789012345678901234567890123456789' > "$out"
        fi
        printf '%s\\n' '{{"ok":true,"command":"export"}}'
        ;;
    esac
    ;;
esac
""",
        )

        mock_lipo = write_exec(
            tools / "lipo",
            f"#!/bin/sh\nprintf '%s\\n' '{lipo_arch}'\n",
        )
        mock_file = write_exec(
            tools / "file",
            f"#!/bin/sh\nprintf '%s\\n' '{file_desc}'\n",
        )
        rpath_block = ""
        if rpath is not None:
            rpath_block = f"Load command 9\n          cmd LC_RPATH\n      cmdsize 32\n         path {rpath} (offset 12)\n"
        mock_otool = write_exec(
            tools / "otool",
            f"""#!/bin/sh
if [ "$1" = "-l" ]; then
  cat <<'OUT'
Load command 0
      cmd LC_BUILD_VERSION
  cmdsize 32
 platform 1
    minos 15.0
      sdk 26.2
{rpath_block}OUT
else
  cat <<'OUT'
$2:
\t{extra_dylib} (compatibility version 1.0.0, current version 1.0.0)
\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1356.0.0)
OUT
fi
""",
        )
        mock_codesign = write_exec(
            tools / "codesign",
            "#!/bin/sh\nexit 0\n",
        )
        if rosetta_ok:
            mock_arch = write_exec(
                tools / "arch",
                """#!/bin/sh
if [ "${1:-}" = "-x86_64" ]; then shift; fi
exec "$@"
""",
            )
        else:
            mock_arch = write_exec(
                tools / "arch",
                "#!/bin/sh\nexit 1\n",
            )

        env = os.environ.copy()
        env.update(
            {
                "NEOFLASH_INTEL_PROJECT_ROOT": str(project),
                "NEOFLASH_INTEL_ARTIFACTS_DIR": str(project / "target/artifacts/macos-x86_64"),
                "NEOFLASH_INTEL_PREBUILT_BINARY": str(mock_editor),
                "NEOFLASH_INTEL_SKIP_RUST_LICENSES": "1",
                "NEOFLASH_INTEL_SKIP_ACTOOL": "1",
                "NEOFLASH_HOST_ARCH_OVERRIDE": "arm64",
                "NEOFLASH_LIPO_BIN": str(mock_lipo),
                "NEOFLASH_FILE_BIN": str(mock_file),
                "NEOFLASH_OTOOL_BIN": str(mock_otool),
                "NEOFLASH_CODESIGN_BIN": str(mock_codesign),
                "NEOFLASH_ARCH_BIN": str(mock_arch),
            }
        )
        return env

    def test_hermetic_arch_rpath_dylib_rosetta_and_success_gates(self):
        cases = [
            ("arm64_rejected", {"lipo_arch": "arm64"}, [], 1, "expected lipo -archs strictly equal to x86_64"),
            (
                "universal_rejected",
                {"lipo_arch": "x86_64 arm64"},
                [],
                1,
                "expected lipo -archs strictly equal to x86_64",
            ),
            (
                "bad_file_desc_rejected",
                {"file_desc": "Mach-O 64-bit executable arm64"},
                [],
                1,
                "Mach-O 64-bit executable x86_64",
            ),
            (
                "external_rpath_rejected",
                {"rpath": "/opt/homebrew/lib"},
                [],
                1,
                "disallowed external LC_RPATH",
            ),
            (
                "non_system_dylib_rejected",
                {"extra_dylib": "/usr/local/lib/libcustom.dylib"},
                [],
                1,
                "non-system dynamic libraries",
            ),
            (
                "missing_rosetta_rejected_by_default",
                {"rosetta_ok": False},
                [],
                1,
                "cannot execute x86_64 binaries",
            ),
        ]

        for label, kwargs, extra_args, expected_rc, expected_err in cases:
            with self.subTest(case=label), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                env = self._build_hermetic_fixture(root, **kwargs)
                res = subprocess.run(
                    [str(SCRIPT), *extra_args],
                    env=env,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                    check=False,
                )
                self.assertEqual(res.returncode, expected_rc, res.stderr)
                self.assertIn(expected_err, res.stderr)
                artifacts = Path(env["NEOFLASH_INTEL_ARTIFACTS_DIR"])
                self.assertFalse((artifacts / "NeoFlash.app").exists(), "failed run must clean incomplete bundle")
                self.assertFalse((artifacts / "verification-report.json").exists())

        # Full hermetic success writes NeoFlash.app, intel-dark.png, and verification-report.json without touching dist/.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            env = self._build_hermetic_fixture(root, rpath="@executable_path/../Frameworks")
            res = subprocess.run(
                [str(SCRIPT)],
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertEqual(res.returncode, 0, res.stderr)
            artifacts = Path(env["NEOFLASH_INTEL_ARTIFACTS_DIR"])
            self.assertTrue((artifacts / "NeoFlash.app/Contents/MacOS/neoflash-editor").is_file())
            self.assertTrue((artifacts / "intel-dark.png").is_file())
            receipt = json.loads((artifacts / "verification-report.json").read_text(encoding="utf-8"))
            self.assertEqual(receipt["status"], "passed")
            self.assertEqual(receipt["target_triple"], "x86_64-apple-darwin")
            self.assertEqual(receipt["execution_mode"], "rosetta2")
            self.assertEqual(receipt["mach_o"]["lipo_archs"], "x86_64")
            self.assertEqual(receipt["mach_o"]["external_lc_rpath_count"], 0)
            self.assertTrue(receipt["rosetta_cli_checks"]["executed"])
            self.assertEqual(receipt["gui_screenshot_smoke"]["status"], "passed")
            self.assertEqual(receipt["gui_screenshot_smoke"]["width"], 1280)
            self.assertEqual(receipt["gui_screenshot_smoke"]["height"], 800)

            dist_entries = [p.name for p in (Path(env["NEOFLASH_INTEL_PROJECT_ROOT"]) / "dist").iterdir()]
            self.assertEqual(dist_entries, ["sentinel.txt"])


if __name__ == "__main__":
    unittest.main()
