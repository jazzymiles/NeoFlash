"""Structural regression checks for the fail-closed macOS release scripts."""

from pathlib import Path
import plistlib
import re
import subprocess
import tempfile
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
        xattr_scrub = text.rindex('xattr -cr "$app_dir"')
        chmod_norm = text.rindex('chmod -R u+rwX,go+rX "$app_dir"')
        outer_sign = text.index('codesign --force --sign - "$app_dir"')
        perm_audit = text.index(
            'find "$app_dir" \\( -type d ! -perm -0555 -o -type f ! -perm -0444 \\)'
        )
        archive = text.index('ditto -c -k --norsrc --keepParent "$app_dir"')
        dmg_sign = text.index('codesign --force --sign - "$dmg_path"')
        dmg_verify = text.index('codesign --verify --verbose=2 "$dmg_path"')
        sums = text.index("shasum -a 256")
        self.assertLess(nested_sign, refresh)
        self.assertLess(refresh, xattr_scrub)
        self.assertLess(xattr_scrub, outer_sign)
        self.assertLess(chmod_norm, outer_sign)
        self.assertLess(outer_sign, perm_audit)
        self.assertLess(perm_audit, archive)
        self.assertLess(archive, dmg_sign)
        self.assertLess(dmg_sign, dmg_verify)
        self.assertLess(dmg_verify, sums)
        self.assertNotIn("--sequesterRsrc", text)
        self.assertNotIn('codesign --force --deep --sign - "$app_dir"', text)
        self.assertNotIn('rm -rf "$project_root/dist"', text)
        self.assertNotIn("portable-media-corresponding-source.tar.xz\"\nrm", text)
        self.assertGreater(text.index("media-provenance-final.json"), outer_sign)
        self.assertNotIn(
            'hdiutil convert "$dmg_rw" -format UDZO -ov -o "$dmg_path" >/dev/null 2>&1 ||',
            text,
        )

    def test_package_info_plist_declares_exported_nflash_uti_and_document_types(self):
        text = PACKAGE.read_text(encoding="utf-8")
        match = re.search(
            r'cat > "\$app_dir/Contents/Info\.plist" <<PLIST\n(.*?)\nPLIST',
            text,
            re.DOTALL,
        )
        self.assertIsNotNone(match)
        rendered = (
            match.group(1)
            .replace(
                "$icon_plist_keys",
                "<key>CFBundleIconFile</key><string>NeoFlash</string>",
            )
            .replace("$version", "0.1.0-beta.1")
            .replace("$build_number", "100")
            .replace("$commit", "deadbeef")
            .replace("$profile", "release")
        )
        plist = plistlib.loads(rendered.encode("utf-8"))
        with tempfile.NamedTemporaryFile("wb", suffix=".plist") as handle:
            handle.write(rendered.encode("utf-8"))
            handle.flush()
            subprocess.run(["plutil", "-lint", handle.name], check=True)

        utis = plist["UTExportedTypeDeclarations"]
        self.assertEqual(len(utis), 1)
        self.assertEqual(utis[0]["UTTypeIdentifier"], "app.neoflash.project")
        self.assertEqual(
            utis[0]["UTTypeConformsTo"],
            ["public.data", "public.composite-content"],
        )
        self.assertEqual(
            utis[0]["UTTypeTagSpecification"]["public.filename-extension"],
            ["nflash"],
        )
        self.assertEqual(
            utis[0]["UTTypeTagSpecification"]["public.mime-type"],
            ["application/vnd.neoflash.project+json"],
        )

        doc_types = {entry["CFBundleTypeName"]: entry for entry in plist["CFBundleDocumentTypes"]}
        owner = doc_types["NeoFlash Document"]
        self.assertEqual(owner["CFBundleTypeRole"], "Editor")
        self.assertEqual(owner["LSHandlerRank"], "Owner")
        self.assertEqual(owner["LSItemContentTypes"], ["app.neoflash.project"])
        self.assertEqual(owner["CFBundleTypeExtensions"], ["nflash"])
        self.assertEqual(owner["CFBundleTypeIconFile"], "AppIcon")

        viewer = doc_types["Flash & Design Interchange Documents"]
        self.assertEqual(viewer["CFBundleTypeRole"], "Viewer")
        self.assertEqual(viewer["LSHandlerRank"], "Alternate")
        self.assertEqual(
            viewer["CFBundleTypeExtensions"],
            [
                "fla",
                "xfl",
                "swf",
                "swc",
                "fig",
                "figma",
                "svg",
                "psd",
                "psb",
                "pdf",
                "ai",
                "fxg",
            ],
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
