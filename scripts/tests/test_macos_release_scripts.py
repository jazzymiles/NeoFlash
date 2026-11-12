"""Structural regression checks for the fail-closed macOS release scripts."""

from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[2]
PACKAGE = ROOT / "scripts" / "package-macos.sh"
NOTARIZE = ROOT / "scripts" / "notarize-macos-beta.sh"


class MacosReleaseScriptTests(unittest.TestCase):
    def test_shell_syntax(self):
        for script in (PACKAGE, NOTARIZE):
            with self.subTest(script=script.name):
                subprocess.run(["bash", "-n", str(script)], check=True)

    def test_package_refuses_non_head_sources_and_requires_complete_release_set(self):
        text = PACKAGE.read_text(encoding="utf-8")
        self.assertIn("git status --porcelain=v1 --untracked-files=all", text)
        self.assertIn("hdiutil is required for the macOS release DMG", text)
        self.assertIn('"$(basename "$source_archive_path")"', text)
        nested_sign = text.index('codesign --force --sign - "$dylib"')
        refresh = text.index("--refresh-staged-hashes")
        outer_sign = text.index('codesign --force --sign - "$app_dir"')
        archive = text.index('ditto -c -k --sequesterRsrc --keepParent "$app_dir"')
        self.assertLess(nested_sign, refresh)
        self.assertLess(refresh, outer_sign)
        self.assertLess(outer_sign, archive)
        self.assertNotIn('codesign --force --deep --sign - "$app_dir"', text)
        self.assertGreater(text.index("media-provenance-final.json"), outer_sign)
        self.assertNotIn(
            'hdiutil convert "$dmg_rw" -format UDZO -ov -o "$dmg_path" >/dev/null 2>&1 ||',
            text,
        )

    def test_notarization_is_staged_and_preserves_source_and_dmg_wrapper(self):
        text = NOTARIZE.read_text(encoding="utf-8")
        self.assertIn('staged_app="$work_root/NeoFlash.app"', text)
        self.assertIn("media-provenance-before-signing.json", text)
        self.assertIn("--refresh-staged-hashes", text)
        self.assertIn("media-provenance-final.json", text)
        self.assertIn('"$(basename "$staged_source")"', text)
        self.assertIn('ln -s /Applications "$dmg_stage/Applications"', text)
        self.assertIn("Licenses and source.txt", text)
        self.assertIn("commit_started=1", text)
        self.assertIn("commit_complete=1", text)
        self.assertIn('mv "$backup_dir/NeoFlash.app" "$app_dir"', text)
        outer_sign = text.index('  "$staged_app"\ncodesign --verify')
        self.assertGreater(text.index("media-provenance-final.json"), outer_sign)


if __name__ == "__main__":
    unittest.main()
