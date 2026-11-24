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

    def test_package_defaults_to_arm64_app_and_guards_complete_release_set(self):
        text = PACKAGE.read_text(encoding="utf-8")
        self.assertIn("git status --porcelain=v1 --untracked-files=all", text)
        self.assertIn("[release|debug] [--beta-release]", text)
        self.assertIn('beta_release=0', text)
        self.assertIn('if [ "$beta_release" -eq 0 ]; then', text)
        self.assertIn('lipo -archs "$app_binary"', text)
        self.assertIn('if [ "$app_archs" != "arm64" ]; then', text)
        self.assertIn("App-only build complete", text)
        self.assertIn("release --beta-release", text)
        self.assertEqual(text.count("--local-payload-only"), 2)
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
        app_only_exit = text.index('if [ "$beta_release" -eq 0 ]; then')
        archive = text.index('ditto -c -k --norsrc --keepParent "$app_dir"')
        dmg_sign = text.index('codesign --force --sign - "$dmg_path"')
        dmg_verify = text.index('codesign --verify --verbose=2 "$dmg_path"')
        sums = text.index("shasum -a 256")
        self.assertLess(nested_sign, refresh)
        self.assertLess(refresh, xattr_scrub)
        self.assertLess(xattr_scrub, outer_sign)
        self.assertLess(chmod_norm, outer_sign)
        self.assertLess(outer_sign, perm_audit)
        self.assertLess(perm_audit, app_only_exit)
        self.assertLess(app_only_exit, archive)
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

    def test_notarization_requires_explicit_beta_release_package(self):
        text = NOTARIZE.read_text(encoding="utf-8")
        self.assertIn("package-macos.sh release --beta-release first", text)

    def test_package_editor_binary_and_plist_use_the_same_git_metadata(self):
        text = PACKAGE.read_text(encoding="utf-8")
        commit = text.index('commit="$(git rev-parse --verify HEAD)"')
        build = text.index('build_number="$(git rev-list --count HEAD)"')
        export = text.index(
            'export NEOFLASH_GIT_COMMIT="$commit" NEOFLASH_BUILD_NUMBER="$build_number"'
        )
        self.assertLess(max(commit, build), export)
        self.assertLess(export, text.index("cargo build --locked --release -p neoflash-editor"))
        self.assertLess(export, text.index("cargo build --locked -p neoflash-editor"))
        self.assertIn('<key>NeoFlashGitCommit</key><string>$commit</string>', text)
        self.assertIn('<key>CFBundleVersion</key><string>$build_number</string>', text)

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

        imported_utis = plist["UTImportedTypeDeclarations"]
        self.assertEqual(len(imported_utis), 1)
        self.assertEqual(
            imported_utis[0]["UTTypeIdentifier"], "com.adobe.actionscript-source"
        )
        self.assertEqual(
            imported_utis[0]["UTTypeConformsTo"],
            ["public.source-code", "public.plain-text"],
        )
        self.assertEqual(
            imported_utis[0]["UTTypeTagSpecification"]["public.filename-extension"],
            ["as"],
        )
        self.assertEqual(
            imported_utis[0]["UTTypeTagSpecification"]["public.mime-type"],
            ["text/x-actionscript", "application/x-actionscript"],
        )

        doc_types = {entry["CFBundleTypeName"]: entry for entry in plist["CFBundleDocumentTypes"]}
        for name, entry in doc_types.items():
            with self.subTest(doc_type=name):
                self.assertTrue(entry.get("LSItemContentTypes"))
                self.assertTrue(entry.get("CFBundleTypeExtensions"))

        owner = doc_types["NeoFlash Document"]
        self.assertEqual(owner["CFBundleTypeRole"], "Editor")
        self.assertEqual(owner["LSHandlerRank"], "Owner")
        self.assertEqual(owner["LSItemContentTypes"], ["app.neoflash.project"])
        self.assertEqual(owner["CFBundleTypeExtensions"], ["nflash"])
        self.assertEqual(owner["CFBundleTypeIconFile"], "AppIcon")

        as_source = doc_types["ActionScript Source"]
        self.assertEqual(as_source["CFBundleTypeRole"], "Editor")
        self.assertEqual(as_source["LSHandlerRank"], "Alternate")
        self.assertEqual(as_source["CFBundleTypeExtensions"], ["as"])
        self.assertEqual(
            as_source["LSItemContentTypes"],
            [
                "com.adobe.actionscript-source",
                "com.apple.applesingle-archive",
                "public.source-code",
                "public.script",
                "public.plain-text",
            ],
        )

        viewer = doc_types["Flash & Design Interchange Documents"]
        self.assertEqual(viewer["CFBundleTypeRole"], "Viewer")
        self.assertEqual(viewer["LSHandlerRank"], "Alternate")
        self.assertEqual(
            viewer["LSItemContentTypes"],
            [
                "com.apple.applesingle-archive",
                "com.adobe.actionscript-source",
                "public.source-code",
                "public.script",
                "public.plain-text",
                "public.data",
                "public.content",
                "public.item",
                "public.folder",
                "public.archive",
                "com.adobe.pdf",
                "com.adobe.illustrator.ai-image",
                "com.adobe.encapsulated-postscript",
                "com.adobe.photoshop-image",
                "public.svg-image",
            ],
        )
        self.assertEqual(
            viewer["CFBundleTypeExtensions"],
            [
                "fla",
                "xfl",
                "as",
                "swf",
                "swc",
                "fig",
                "figma",
                "svg",
                "psd",
                "psb",
                "pdf",
                "ai",
                "eps",
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
        self.assertIn(
            'restore_backup_path "$backup_dir/NeoFlash.app" "$app_dir" tree',
            text,
        )
        self.assertIn('/bin/mv "$backup_path" "$live_path"', text)
        outer_sign = text.index('  "$staged_app"\ncodesign --verify')
        self.assertGreater(text.index("media-provenance-final.json"), outer_sign)


if __name__ == "__main__":
    unittest.main()
