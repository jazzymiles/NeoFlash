import hashlib
import importlib.util
import json
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "scripts" / "bundled-media-provenance.py"
SPEC = importlib.util.spec_from_file_location("bundled_media_provenance", MODULE_PATH)
provenance = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = provenance
SPEC.loader.exec_module(provenance)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class BundledMediaProvenanceTests(unittest.TestCase):
    def make_fixture(self, root: Path, *, keg_license: bool = False):
        media = root / "media"
        binary = media / "bin" / "ffmpeg"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"fixture-ffmpeg")

        source_tree = root / "upstream"
        source_tree.mkdir()
        (source_tree / "LICENSE.txt").write_text("Fixture permissive license\n", encoding="utf-8")
        (source_tree / "source.c").write_text("int main(void) { return 0; }\n", encoding="utf-8")
        source_archive = root / "fixture-1.0.tar.gz"
        with tarfile.open(source_archive, "w:gz") as archive:
            archive.add(source_tree, arcname="fixture-1.0")
        source_hash = digest(source_archive)

        media_sbom = {
            "schema_version": 1,
            "executables": [
                {
                    "name": "ffmpeg",
                    "relative_path": "bin/ffmpeg",
                    "version": "1.0",
                    "sha256": digest(binary),
                    "license": "GPL-3.0-or-later",
                    "source_url": source_archive.as_uri(),
                }
            ],
            "libraries": [],
        }
        prov = media / "provenance"
        prov.mkdir()
        (prov / "media-sbom.json").write_text(json.dumps(media_sbom), encoding="utf-8")

        keg = root / "Cellar" / "ffmpeg" / "1.0"
        (keg / ".brew").mkdir(parents=True)
        receipt = {
            "homebrew_version": "test-brew",
            "arch": "arm64",
            "built_as_bottle": True,
            "poured_from_bottle": True,
            "source": {
                "versions": {"stable": "1.0"},
                "path": "/cache/packages.arm64_sequoia.jws.json",
            },
        }
        (keg / "INSTALL_RECEIPT.json").write_text(json.dumps(receipt), encoding="utf-8")
        sbom = {
            "packages": [
                {
                    "SPDXID": "SPDXRef-Archive-ffmpeg-src",
                    "name": "ffmpeg",
                    "versionInfo": "1.0",
                    "licenseConcluded": "GPL-3.0-or-later",
                    "downloadLocation": source_archive.as_uri(),
                    "checksums": [{"algorithm": "SHA256", "checksumValue": source_hash}],
                },
                {
                    "SPDXID": "SPDXRef-Bottle-ffmpeg",
                    "name": "ffmpeg",
                    "versionInfo": "1.0",
                    "licenseConcluded": "GPL-3.0-or-later",
                    "downloadLocation": "https://example.invalid/ffmpeg.bottle.tar.gz",
                    "checksums": [{"algorithm": "SHA256", "checksumValue": "b" * 64}],
                },
            ]
        }
        (keg / "sbom.spdx.json").write_text(json.dumps(sbom), encoding="utf-8")
        (keg / ".brew" / "ffmpeg.rb").write_text(
            'class Ffmpeg < Formula\n  url "' + source_archive.as_uri() + '"\n'
            f'  sha256 "{source_hash}"\n  license "GPL-3.0-or-later"\nend\n',
            encoding="utf-8",
        )
        if keg_license:
            (keg / "COPYING").write_text("GPL fixture\n", encoding="utf-8")
        return media, root / "Cellar"

    def test_builds_complete_deterministic_bundle_and_extracts_missing_keg_license(self):
        with tempfile.TemporaryDirectory() as raw:
            base = Path(raw)
            media, cellar = self.make_fixture(base)
            archives = []
            for run in ("a", "b"):
                parent = base / run
                parent.mkdir()
                output = parent / "corresponding-source"
                archive = parent / "corresponding-source.tar.xz"
                report = provenance.build_provenance(
                    media_root=media,
                    cellar=cellar,
                    cache_dir=base / "cache",
                    output_dir=output,
                    archive_path=archive,
                    allow_download=True,
                )
                self.assertTrue(report["release_ready"], report["missing"])
                self.assertEqual(report["component_count"], 1)
                self.assertEqual(report["staged_file_count"], 1)
                self.assertTrue((output / "manifest.json").is_file())
                self.assertTrue((output / "SHA256SUMS.txt").is_file())
                licenses = list((output / "components" / "ffmpeg" / "1.0" / "licenses").iterdir())
                self.assertTrue(any("LICENSE" in path.name for path in licenses))
                archives.append(digest(archive))
            self.assertEqual(archives[0], archives[1])

    def test_missing_sources_and_license_fail_closed_without_output(self):
        with tempfile.TemporaryDirectory() as raw:
            base = Path(raw)
            media, cellar = self.make_fixture(base)
            output = base / "result"
            report = provenance.build_provenance(
                media_root=media,
                cellar=cellar,
                cache_dir=base / "empty-cache",
                output_dir=output,
                archive_path=None,
                allow_download=False,
            )
            self.assertFalse(report["release_ready"])
            self.assertFalse(output.exists())
            codes = {item["code"] for item in report["missing"]}
            self.assertIn("artifact-not-cached", codes)
            self.assertIn("license-text-missing", codes)

    def test_read_only_audit_finds_license_in_cached_source_archive(self):
        with tempfile.TemporaryDirectory() as raw:
            base = Path(raw)
            media, cellar = self.make_fixture(base)
            report = provenance.build_provenance(
                media_root=media,
                cellar=cellar,
                cache_dir=base / "cache",
                output_dir=None,
                archive_path=None,
                allow_download=True,
            )
            self.assertTrue(report["release_ready"], report["missing"])
            self.assertTrue(
                any(
                    evidence.get("archive_member", "").endswith("LICENSE.txt")
                    for evidence in report["components"][0]["license_evidence"]
                )
            )

    def test_unlisted_staged_binary_refuses_release(self):
        with tempfile.TemporaryDirectory() as raw:
            base = Path(raw)
            media, cellar = self.make_fixture(base, keg_license=True)
            (media / "lib").mkdir()
            (media / "lib" / "libhidden.dylib").write_bytes(b"untracked")
            report = provenance.build_provenance(
                media_root=media,
                cellar=cellar,
                cache_dir=base / "cache",
                output_dir=None,
                archive_path=None,
                allow_download=False,
            )
            self.assertFalse(report["release_ready"])
            self.assertTrue(
                any(
                    item["code"] == "staged-file-unaccounted"
                    and item["path"] == "lib/libhidden.dylib"
                    for item in report["missing"]
                )
            )

    def test_refresh_staged_hashes_updates_signed_bytes_but_refuses_inventory_changes(self):
        with tempfile.TemporaryDirectory() as raw:
            base = Path(raw)
            media, cellar = self.make_fixture(base, keg_license=True)
            binary = media / "bin" / "ffmpeg"
            binary.write_bytes(b"fixture-ffmpeg-after-codesign")
            provenance.refresh_staged_hashes(media)
            sbom = json.loads((media / "provenance" / "media-sbom.json").read_text())
            self.assertEqual(sbom["executables"][0]["sha256"], digest(binary))
            self.assertIn(digest(binary), (media / "provenance" / "SHA256SUMS.txt").read_text())
            output = base / "signed-source"
            report = provenance.build_provenance(
                media_root=media,
                cellar=cellar,
                cache_dir=base / "cache",
                output_dir=output,
                archive_path=base / "signed-source.tar.xz",
                allow_download=True,
            )
            self.assertTrue(report["release_ready"], report["missing"])
            manifest = json.loads((output / "manifest.json").read_text())
            self.assertEqual(
                manifest["components"][0]["staged_files"][0]["sha256"],
                digest(binary),
            )

            hidden = media / "lib" / "libhidden.dylib"
            hidden.parent.mkdir()
            hidden.write_bytes(b"hidden")
            with self.assertRaisesRegex(ValueError, "inventory changed"):
                provenance.refresh_staged_hashes(media)

    def test_missing_spdx_bottle_uses_only_version_exact_formula_api_tag(self):
        receipt = {"source": {"path": "/cache/packages.arm64_sequoia.jws.json"}}
        exact_info = {
            "versions": {"stable": "1.0"},
            "revision": 0,
            "tap_git_head": "a" * 40,
            "bottle": {
                "stable": {
                    "files": {
                        "arm64_sequoia": {
                            "url": "https://example.invalid/exact.tar.gz",
                            "sha256": "c" * 64,
                        }
                    }
                }
            },
        }
        with mock.patch.object(provenance, "brew_formula_info", return_value=exact_info):
            url, sha, evidence = provenance.resolve_bottle_provenance(
                "fixture", "1.0", receipt, []
            )
        self.assertEqual(url, "https://example.invalid/exact.tar.gz")
        self.assertEqual(sha, "c" * 64)
        self.assertIn("arm64_sequoia", evidence)

        newer_info = dict(exact_info)
        newer_info["versions"] = {"stable": "2.0"}
        with mock.patch.object(provenance, "brew_formula_info", return_value=newer_info):
            self.assertEqual(
                provenance.resolve_bottle_provenance("fixture", "1.0", receipt, []),
                (None, None, "unresolved"),
            )

    def test_recipe_scanner_keeps_build_resources_and_excludes_test_fixtures(self):
        build = hashlib.sha256(b"build").hexdigest()
        test = hashlib.sha256(b"test").hexdigest()
        recipe = f'''class Fixture < Formula
  resource "compiler-input" do
    url "https://example.invalid/build.tar.gz"
    sha256 "{build}"
  end
  test do
    resource "runtime-fixture" do
      url "https://example.invalid/test.dat"
      sha256 "{test}"
    end
  end
end
'''
        resources, unresolved = provenance.extract_recipe_resources("fixture", recipe)
        self.assertEqual([item.name for item in resources], ["compiler-input"])
        self.assertEqual(unresolved, [])

    def test_recipe_scanner_keeps_pinned_git_resources_and_rejects_unpinned_ones(self):
        recipe = '''class Fixture < Formula
  resource "sjpeg" do
    url "https://example.invalid/sjpeg.git",
        revision: "94e0df6d0f8b44228de5be0ff35efb9f946a13c9"
  end
  resource "unknown" do
    url "https://example.invalid/floating.git"
  end
end
'''
        resources, unresolved = provenance.extract_recipe_resources("fixture", recipe)
        self.assertEqual(len(resources), 1)
        self.assertEqual(resources[0].name, "sjpeg")
        self.assertEqual(resources[0].revision, "94e0df6d0f8b44228de5be0ff35efb9f946a13c9")
        self.assertEqual(unresolved, ["unknown"])

    def test_git_cache_is_rebuilt_from_verified_repository_objects(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            repository = root / "repository"
            repository.mkdir()
            provenance.subprocess.run(["git", "init", "--quiet", str(repository)], check=True)
            provenance.subprocess.run(
                ["git", "-C", str(repository), "config", "user.email", "fixture@example.invalid"],
                check=True,
            )
            provenance.subprocess.run(
                ["git", "-C", str(repository), "config", "user.name", "Fixture"],
                check=True,
            )
            (repository / "LICENSE").write_text("fixture license\n", encoding="utf-8")
            (repository / "source.c").write_text("int fixture;\n", encoding="utf-8")
            provenance.subprocess.run(["git", "-C", str(repository), "add", "."], check=True)
            provenance.subprocess.run(
                ["git", "-C", str(repository), "commit", "--quiet", "-m", "fixture"],
                check=True,
            )
            revision = provenance.subprocess.check_output(
                ["git", "-C", str(repository), "rev-parse", "HEAD"], text=True
            ).strip()
            spec = provenance.ArtifactSpec(
                owner="fixture",
                kind="resource",
                name="fixture-source",
                url=str(repository),
                sha256=None,
                revision=revision,
            )
            cache = root / "cache"
            archive, expected, _ = provenance.fetch_git_source(spec, cache, True)
            self.assertTrue(tarfile.is_tarfile(archive))
            archive.write_bytes(b"forged cache entry")

            rebuilt, actual, _ = provenance.fetch_git_source(spec, cache, False)
            self.assertEqual(actual, expected)
            self.assertTrue(tarfile.is_tarfile(rebuilt))

    def test_vanished_non_code_resource_can_be_recovered_only_by_exact_keg_hash(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            keg_resource = root / "keg" / "share" / "model.bin"
            keg_resource.parent.mkdir(parents=True)
            keg_resource.write_bytes(b"exact trained model")
            expected = digest(keg_resource)
            spec = provenance.ArtifactSpec(
                owner="fixture",
                kind="resource",
                name="model",
                url="https://example.invalid/model.bin",
                sha256=expected,
            )
            recovered, actual = provenance.recover_resource_from_keg(
                spec, root / "keg", root / "cache"
            )
            self.assertEqual(actual, expected)
            self.assertEqual(recovered.read_bytes(), b"exact trained model")
            wrong = provenance.ArtifactSpec(
                owner="fixture",
                kind="resource",
                name="model",
                url=spec.url,
                sha256="0" * 64,
            )
            self.assertEqual(
                provenance.recover_resource_from_keg(wrong, root / "keg", root / "other-cache"),
                (None, None),
            )

    def test_license_name_detection_accepts_spdx_files_without_matching_unrelated_gplot(self):
        self.assertTrue(provenance.is_licence_name("LGPL-2.1-or-later.txt"))
        self.assertTrue(provenance.is_licence_name("COPYING.LESSER"))
        self.assertTrue(provenance.is_licence_name("license.md"))
        self.assertFalse(provenance.is_licence_name("gplot.c"))
        self.assertFalse(provenance.is_licence_name("binding-example.jpg"))


if __name__ == "__main__":
    unittest.main()
