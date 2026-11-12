"""Tests for the fail-closed Rust dependency license bundle."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock


MODULE_PATH = (
    Path(__file__).resolve().parents[1] / "stage-rust-dependency-licenses.py"
)
SPEC = importlib.util.spec_from_file_location("stage_rust_licenses", MODULE_PATH)
stage_rust_licenses = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage_rust_licenses)


class RustDependencyLicenseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(
            prefix="neoflash-rust-license-test-"
        )
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        (self.root / "LICENSE-MIT").write_text("workspace MIT\n", encoding="utf-8")
        (self.root / "LICENSE-APACHE").write_text(
            "workspace Apache\n", encoding="utf-8"
        )

    def package(
        self,
        name,
        version,
        relative,
        *,
        source=None,
        license_expression=None,
        license_file=None,
    ):
        package_root = self.root / relative
        package_root.mkdir(parents=True, exist_ok=True)
        manifest = package_root / "Cargo.toml"
        manifest.write_text(f"[package]\nname='{name}'\nversion='{version}'\n")
        package_id = (
            f"{source}#{name}@{version}"
            if source
            else f"path+file://{package_root}#{name}@{version}"
        )
        return {
            "id": package_id,
            "name": name,
            "version": version,
            "source": source,
            "manifest_path": str(manifest),
            "license": license_expression,
            "license_file": license_file,
        }

    def metadata(self, packages, dependencies, root_id):
        return {
            "workspace_root": str(self.root),
            "workspace_members": [root_id],
            "packages": packages,
            "resolve": {
                "nodes": [
                    {
                        "id": package["id"],
                        "dependencies": dependencies.get(package["id"], []),
                    }
                    for package in packages
                ]
            },
        }

    def metadata_with_dependency_kinds(self, packages, edges, root_id):
        return {
            "workspace_root": str(self.root),
            "workspace_members": [root_id],
            "packages": packages,
            "resolve": {
                "nodes": [
                    {
                        "id": package["id"],
                        "dependencies": [edge["pkg"] for edge in edges.get(package["id"], [])],
                        "deps": edges.get(package["id"], []),
                    }
                    for package in packages
                ]
            },
        }

    def lock_packages(self, packages, checksums=None):
        checksums = checksums or {}
        result = []
        for package in packages:
            entry = {"name": package["name"], "version": package["version"]}
            if package["source"] is not None:
                entry["source"] = package["source"]
            if package["id"] in checksums:
                entry["checksum"] = checksums[package["id"]]
            result.append(entry)
        return result

    def test_closure_preserves_compound_spdx_checksums_and_is_deterministic(self):
        editor = self.package(
            "neoflash-editor",
            "0.1.0-beta.1",
            "crates/editor",
            license_expression="MIT OR Apache-2.0",
        )
        ring = self.package(
            "ring",
            "0.17.14",
            "registry/ring",
            source="registry+https://github.com/rust-lang/crates.io-index",
            license_expression="Apache-2.0 AND ISC",
        )
        (self.root / "registry/ring/LICENSE").write_text(
            "Apache and ISC evidence\n", encoding="utf-8"
        )
        roots = self.package(
            "webpki-roots",
            "1.0.12",
            "registry/webpki-roots",
            source="registry+https://github.com/rust-lang/crates.io-index",
            license_expression="CDLA-Permissive-2.0",
            license_file="COPYING",
        )
        (self.root / "registry/webpki-roots/COPYING").write_text(
            "CDLA evidence\n", encoding="utf-8"
        )
        unused = self.package(
            "unused",
            "9.9.9",
            "registry/unused",
            source="registry+https://github.com/rust-lang/crates.io-index",
            license_expression=None,
        )
        packages = [editor, ring, roots, unused]
        metadata = self.metadata(
            packages,
            {
                editor["id"]: [ring["id"], roots["id"]],
                ring["id"]: [],
                roots["id"]: [],
                unused["id"]: [],
            },
            editor["id"],
        )
        checksums = {ring["id"]: "a" * 64, roots["id"]: "b" * 64}
        manifest, files = stage_rust_licenses.build_manifest(
            metadata,
            self.lock_packages(packages, checksums),
            "neoflash-editor",
            self.root,
        )

        self.assertEqual(manifest["package_count"], 3)
        by_name = {package["name"]: package for package in manifest["packages"]}
        self.assertNotIn("unused", by_name)
        self.assertEqual(by_name["ring"]["spdx"], "Apache-2.0 AND ISC")
        self.assertEqual(by_name["ring"]["checksum"], "a" * 64)
        self.assertEqual(
            by_name["webpki-roots"]["spdx"], "CDLA-Permissive-2.0"
        )
        self.assertTrue(by_name["webpki-roots"]["license_evidence"])
        self.assertGreaterEqual(len(files), 4)  # two workspace texts + ring + roots

        first = self.root / "bundle-one"
        second = self.root / "bundle-two"
        stage_rust_licenses.stage_bundle(first, manifest, files)
        stage_rust_licenses.stage_bundle(second, manifest, files)
        self.assertEqual(
            (first / stage_rust_licenses.MANIFEST_JSON).read_bytes(),
            (second / stage_rust_licenses.MANIFEST_JSON).read_bytes(),
        )
        self.assertEqual(
            (first / stage_rust_licenses.MANIFEST_MARKDOWN).read_bytes(),
            (second / stage_rust_licenses.MANIFEST_MARKDOWN).read_bytes(),
        )
        markdown = (first / stage_rust_licenses.MANIFEST_MARKDOWN).read_text()
        self.assertIn("Apache-2.0 AND ISC", markdown)
        self.assertIn("CDLA-Permissive-2.0", markdown)

    def test_missing_declared_license_and_evidence_fails_closed(self):
        editor = self.package(
            "neoflash-editor",
            "0.1.0-beta.1",
            "crates/editor",
            license_expression="MIT",
        )
        unknown = self.package(
            "unknown-license",
            "1.2.3",
            "registry/unknown",
            source="registry+https://github.com/rust-lang/crates.io-index",
        )
        metadata = self.metadata(
            [editor, unknown],
            {editor["id"]: [unknown["id"]], unknown["id"]: []},
            editor["id"],
        )
        with self.assertRaisesRegex(
            stage_rust_licenses.LicenseBundleError,
            "unknown-license 1.2.3: neither a declared license",
        ):
            stage_rust_licenses.build_manifest(
                metadata,
                self.lock_packages([editor, unknown]),
                "neoflash-editor",
                self.root,
            )

    def test_development_only_dependency_is_not_part_of_runtime_closure(self):
        editor = self.package(
            "neoflash-editor",
            "0.1.0-beta.1",
            "crates/editor",
            license_expression="MIT",
        )
        runtime = self.package(
            "runtime-dependency",
            "1.0.0",
            "registry/runtime",
            source="registry+https://github.com/rust-lang/crates.io-index",
            license_expression="MIT",
        )
        (self.root / "registry/runtime/LICENSE").write_text(
            "runtime MIT evidence\n", encoding="utf-8"
        )
        dev_only = self.package(
            "test-helper",
            "1.0.0",
            "registry/test-helper",
            source="registry+https://github.com/rust-lang/crates.io-index",
            license_expression=None,
        )
        packages = [editor, runtime, dev_only]
        metadata = self.metadata_with_dependency_kinds(
            packages,
            {
                editor["id"]: [
                    {
                        "pkg": runtime["id"],
                        "dep_kinds": [{"kind": None, "target": None}],
                    },
                    {
                        "pkg": dev_only["id"],
                        "dep_kinds": [{"kind": "dev", "target": None}],
                    },
                ],
                runtime["id"]: [],
                dev_only["id"]: [],
            },
            editor["id"],
        )
        manifest, _ = stage_rust_licenses.build_manifest(
            metadata,
            self.lock_packages(packages, {runtime["id"]: "c" * 64}),
            "neoflash-editor",
            self.root,
        )
        self.assertEqual(
            [package["name"] for package in manifest["packages"]],
            ["neoflash-editor", "runtime-dependency"],
        )

    def test_missing_declared_license_file_fails_even_with_spdx(self):
        editor = self.package(
            "neoflash-editor",
            "0.1.0-beta.1",
            "crates/editor",
            license_expression="MIT",
        )
        dependency = self.package(
            "broken-license-file",
            "2.0.0",
            "registry/broken",
            source="registry+https://github.com/rust-lang/crates.io-index",
            license_expression="MIT",
            license_file="LEGAL-NOTICE",
        )
        metadata = self.metadata(
            [editor, dependency],
            {editor["id"]: [dependency["id"]], dependency["id"]: []},
            editor["id"],
        )
        with self.assertRaisesRegex(
            stage_rust_licenses.LicenseBundleError,
            "declared license-file 'LEGAL-NOTICE' is missing",
        ):
            stage_rust_licenses.build_manifest(
                metadata,
                self.lock_packages([editor, dependency]),
                "neoflash-editor",
                self.root,
            )

    def test_spdx_only_package_requires_and_bundles_pinned_canonical_text(self):
        editor = self.package(
            "neoflash-editor",
            "0.1.0-beta.1",
            "crates/editor",
            license_expression="MIT",
        )
        dependency = self.package(
            "spdx-only",
            "1.0.0",
            "registry/spdx-only",
            source="registry+https://github.com/rust-lang/crates.io-index",
            license_expression="MIT OR Apache-2.0",
        )
        metadata = self.metadata(
            [editor, dependency],
            {editor["id"]: [dependency["id"]], dependency["id"]: []},
            editor["id"],
        )
        checksums = {dependency["id"]: "d" * 64}
        with self.assertRaisesRegex(
            stage_rust_licenses.LicenseBundleError,
            "pinned canonical text is unavailable",
        ):
            stage_rust_licenses.build_manifest(
                metadata,
                self.lock_packages([editor, dependency], checksums),
                "neoflash-editor",
                self.root,
            )

        canonical = {
            "MIT": b"complete MIT text\n",
            "Apache-2.0": b"complete Apache text\n",
        }
        manifest, files = stage_rust_licenses.build_manifest(
            metadata,
            self.lock_packages([editor, dependency], checksums),
            "neoflash-editor",
            self.root,
            canonical,
        )
        record = next(item for item in manifest["packages"] if item["name"] == "spdx-only")
        self.assertEqual(len(record["license_evidence"]), 2)
        self.assertEqual(files["canonical/MIT.txt"], canonical["MIT"])
        self.assertEqual(files["canonical/Apache-2.0.txt"], canonical["Apache-2.0"])

    def test_registry_checksum_and_git_revision_are_required(self):
        for label, source, expected in (
            (
                "registry",
                "registry+https://github.com/rust-lang/crates.io-index",
                "registry package has no Cargo.lock checksum",
            ),
            (
                "git",
                "git+https://example.invalid/dependency.git",
                "Git source is not pinned to an exact revision",
            ),
        ):
            with self.subTest(label=label):
                editor = self.package(
                    "neoflash-editor",
                    "0.1.0-beta.1",
                    f"{label}/editor",
                    license_expression="MIT",
                )
                dependency = self.package(
                    f"{label}-dependency",
                    "1.0.0",
                    f"{label}/dependency",
                    source=source,
                    license_expression="MIT",
                )
                metadata = self.metadata(
                    [editor, dependency],
                    {editor["id"]: [dependency["id"]], dependency["id"]: []},
                    editor["id"],
                )
                with self.assertRaisesRegex(
                    stage_rust_licenses.LicenseBundleError, expected
                ):
                    stage_rust_licenses.build_manifest(
                        metadata,
                        self.lock_packages([editor, dependency]),
                        "neoflash-editor",
                        self.root,
                    )

    def test_cli_fixture_writes_manifest_and_evidence(self):
        editor = self.package(
            "neoflash-editor",
            "0.1.0-beta.1",
            "crates/editor",
            license_expression="MIT OR Apache-2.0",
        )
        metadata = self.metadata([editor], {editor["id"]: []}, editor["id"])
        metadata_path = self.root / "metadata.json"
        metadata_path.write_text(json.dumps(metadata), encoding="utf-8")
        lock_path = self.root / "Cargo.lock"
        lock_path.write_text(
            'version = 4\n\n[[package]]\nname = "neoflash-editor"\n'
            'version = "0.1.0-beta.1"\n',
            encoding="utf-8",
        )
        destination = self.root / "cli-bundle"
        with mock.patch.object(stage_rust_licenses, "load_spdx_texts", return_value={}):
            result = stage_rust_licenses.main(
                [
                    "--project-root",
                    str(self.root),
                    "--metadata-json",
                    str(metadata_path),
                    "--lockfile",
                    str(lock_path),
                    "--dest",
                    str(destination),
                ]
            )
        self.assertEqual(result, 0)
        payload = json.loads(
            (destination / stage_rust_licenses.MANIFEST_JSON).read_text()
        )
        self.assertEqual(payload["root_package"], "neoflash-editor")
        self.assertEqual(payload["package_count"], 1)

    def test_stage_refuses_project_like_destination(self):
        destination = self.root / "existing-project"
        destination.mkdir()
        (destination / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        with self.assertRaisesRegex(
            stage_rust_licenses.LicenseBundleError,
            "Refusing to replace project-like destination",
        ):
            stage_rust_licenses.stage_bundle(
                destination,
                {
                    "schema_version": 1,
                    "root_package": "neoflash-editor",
                    "package_count": 0,
                    "packages": [],
                },
                {},
                replace=True,
            )
        self.assertTrue((destination / "Cargo.toml").is_file())


if __name__ == "__main__":
    unittest.main()
