"""Tests for LC_RPATH stripping and relocation verification in stage-bundled-media.py."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest
from unittest import mock


MODULE_PATH = Path(__file__).resolve().parents[1] / "stage-bundled-media.py"
SPEC = importlib.util.spec_from_file_location("stage_bundled_media", MODULE_PATH)
stage_bundled_media = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage_bundled_media)


SAMPLE_OTOOL_L = """/tmp/media/lib/libjpeg.8.dylib:
Load command 0
      cmd LC_SEGMENT_64
  cmdsize 72
Load command 17
          cmd LC_RPATH
      cmdsize 56
         path /opt/homebrew/Cellar/jpeg-turbo/3.2.1/lib (offset 12)
Load command 18
          cmd LC_RPATH
      cmdsize 32
         path @loader_path (offset 12)
Load command 19
          cmd LC_RPATH
      cmdsize 40
         path @executable_path/../lib (offset 12)
"""


class StageBundledMediaTests(unittest.TestCase):
    def test_parse_otool_rpaths_extracts_all_lc_rpath_values(self):
        self.assertEqual(
            stage_bundled_media.parse_otool_rpaths(SAMPLE_OTOOL_L),
            [
                "/opt/homebrew/Cellar/jpeg-turbo/3.2.1/lib",
                "@loader_path",
                "@executable_path/../lib",
            ],
        )

    def test_is_allowed_rpath_permits_only_loader_and_executable_paths(self):
        for allowed in (
            "@loader_path",
            "@loader_path/plugins",
            "@executable_path",
            "@executable_path/../lib",
        ):
            self.assertTrue(stage_bundled_media.is_allowed_rpath(allowed), allowed)

        for disallowed in (
            "/opt/homebrew/Cellar/jpeg-turbo/3.2.1/lib",
            "/opt/homebrew/lib",
            "/usr/local/lib",
            "/Users/builder/deps/lib",
            "@rpath/libfoo.dylib",
            "@loader_path/../../Users/builder/lib",
            "@executable_path/../../opt/homebrew/lib",
        ):
            self.assertFalse(
                stage_bundled_media.is_allowed_rpath(disallowed), disallowed
            )

    def test_strip_external_rpaths_deletes_only_disallowed_entries(self):
        target = Path("/tmp/media/lib/libjpeg.8.dylib")
        calls: list[list[str]] = []

        def fake_run(cmd: list[str], env=None) -> str:
            calls.append(cmd)
            if cmd[:2] == ["otool", "-l"]:
                return SAMPLE_OTOOL_L
            if cmd[:2] == ["install_name_tool", "-delete_rpath"]:
                return ""
            raise AssertionError(f"Unexpected command: {cmd}")

        with mock.patch.object(stage_bundled_media, "run_checked", side_effect=fake_run):
            removed = stage_bundled_media.strip_external_rpaths(target)

        self.assertEqual(removed, ["/opt/homebrew/Cellar/jpeg-turbo/3.2.1/lib"])
        self.assertEqual(
            calls,
            [
                ["otool", "-l", str(target)],
                [
                    "install_name_tool",
                    "-delete_rpath",
                    "/opt/homebrew/Cellar/jpeg-turbo/3.2.1/lib",
                    str(target),
                ],
            ],
        )

    def test_verify_binary_relocation_rejects_remaining_homebrew_rpath(self):
        target = Path("/tmp/media/lib/libjpeg.8.dylib")

        def fake_run(cmd: list[str], env=None) -> str:
            if cmd[:2] == ["otool", "-L"]:
                return (
                    f"{target}:\n"
                    "\t@loader_path/libjpeg.8.dylib (compatibility version 8.0.0, current version 8.3.2)\n"
                    "\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1351.0.0)\n"
                )
            if cmd[:2] == ["otool", "-l"]:
                return SAMPLE_OTOOL_L
            raise AssertionError(f"Unexpected command: {cmd}")

        with mock.patch.object(stage_bundled_media, "run_checked", side_effect=fake_run):
            with self.assertRaisesRegex(RuntimeError, "Disallowed LC_RPATH"):
                stage_bundled_media.verify_binary_relocation(target)


if __name__ == "__main__":
    unittest.main()
