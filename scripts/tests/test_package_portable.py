#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import shutil
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]


def load_script(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / filename)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


package_portable = load_script("package_portable", "package-portable.py")
prepare_media = load_script("prepare_portable_media", "prepare-portable-media.py")


def digest_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digest_file(path: Path) -> str:
    return package_portable.sha256_file(path)


class PortablePackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.repo_commit = "b" * 40
        self.ffmpeg_commit = "c" * 40
        self.dep_name = f"50-codec_{'d' * 64}.tar.xz"
        self.win_tool = b"MZ" + b"win-ffmpeg"
        self.win_probe = b"MZ" + b"win-ffprobe"
        self.ffmpeg_license = b"GPL version 3 fixture\n"
        self.btbn_license = b"MIT fixture\n"
        self.pins = {
            "schema_version": 1,
            "assets": {
                "windows-x64": {
                    "name": "fixture-win64-gpl.zip",
                    "sha256": "1" * 64,
                    "ffmpeg": "ffmpeg.exe",
                    "ffmpeg_sha256": digest_bytes(self.win_tool),
                    "ffprobe": "ffprobe.exe",
                    "ffprobe_sha256": digest_bytes(self.win_probe),
                },
                "linux-x64": {
                    "name": "fixture-linux64-gpl.tar.xz",
                    "sha256": "2" * 64,
                    "ffmpeg": "ffmpeg",
                    "ffmpeg_sha256": "3" * 64,
                    "ffprobe": "ffprobe",
                    "ffprobe_sha256": "4" * 64,
                },
            },
            "btbn_build_repository": {
                "archive_sha256": "",
                "commit": self.repo_commit,
                "license_sha256": digest_bytes(self.btbn_license),
                "release_tag": "fixture-retained-tag",
            },
            "build_variant": "gpl-9.0-static",
            "ffmpeg": {
                "archive_sha256": "",
                "commit": self.ffmpeg_commit,
                "license_sha256": digest_bytes(self.ffmpeg_license),
                "short_commit": self.ffmpeg_commit[:10],
                "version": "fixture-version",
            },
            "generated_dockerfiles": {},
        }
        self.old_package_pins = package_portable.PINS
        self.old_prepare_pins = prepare_media.PINS
        package_portable.PINS = self.pins
        prepare_media.PINS = self.pins

    def tearDown(self):
        package_portable.PINS = self.old_package_pins
        prepare_media.PINS = self.old_prepare_pins
        self.temp.cleanup()

    def write(self, root: Path, relative: str, data: bytes) -> Path:
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return path

    def source_tree(self, tamper_after_manifest: bool = False):
        root = self.root / ("source-tamper" if tamper_after_manifest else "source")
        root.mkdir()
        build_relative = f"components/btbn-ffmpeg-builds/{self.repo_commit}/sources/FFmpeg-Builds-{self.repo_commit}.tar.gz"
        ffmpeg_relative = f"components/ffmpeg/{self.ffmpeg_commit}/sources/FFmpeg-{self.ffmpeg_commit}.tar.gz"
        dep_relative = f"components/btbn-static-dependencies/{self.repo_commit}/sources/{self.dep_name}"
        build_source = self.write(root, build_relative, b"build-source")
        ffmpeg_source = self.write(root, ffmpeg_relative, b"ffmpeg-source")
        dependency = root / dep_relative
        dependency.parent.mkdir(parents=True)
        with dependency.open("wb") as handle:
            handle.truncate(8 * 1024 * 1024)
        self.write(root, f"components/btbn-ffmpeg-builds/{self.repo_commit}/licenses/LICENSE", b"MIT")
        self.write(root, f"components/ffmpeg/{self.ffmpeg_commit}/licenses/COPYING.GPLv3", b"GPL")
        self.write(root, "REBUILD.md", b"rebuild boundary")
        self.write(root, "release-build-scripts/package-portable.py", b"script")

        docker_records = []
        for target in ("linux64", "win64"):
            name = f"{target}-gpl-9.0.Dockerfile"
            data = (f"ENV TARGET={target} VARIANT=gpl REPO=btbn/ffmpeg-builds ADDINS_STR=9.0 FFVER=900\n"
                    f"RUN --mount=src=.cache/downloads/{self.dep_name},dst=/cache.tar.xz true\n").encode()
            path = self.write(root, f"release-build-scripts/btbn-generated/{name}", data)
            sha = digest_file(path)
            self.pins["generated_dockerfiles"][name] = sha
            docker_records.append({"name": name, "sha256": sha, "source_mounts": [self.dep_name]})

        component_sources = {
            build_relative: digest_file(build_source),
            ffmpeg_relative: digest_file(ffmpeg_source),
            dep_relative: digest_file(dependency),
        }
        self.pins["btbn_build_repository"]["archive_sha256"] = component_sources[build_relative]
        self.pins["ffmpeg"]["archive_sha256"] = component_sources[ffmpeg_relative]
        license_components = []
        for index, (source_path, sha) in enumerate(component_sources.items()):
            evidence_path = f"evidence/component-{index}/001-LICENSE"
            evidence = self.write(root, f"component-licenses/{evidence_path}", f"license {index}".encode())
            license_components.append({
                "component": f"component-{index}", "source_path": source_path,
                "source_sha256": sha, "license_evidence": [{
                    "bytes": evidence.stat().st_size, "path": evidence_path,
                    "sha256": digest_file(evidence), "source_path": "LICENSE"}]})
        license_manifest = {
            "btbn_build_commit": self.repo_commit, "build_variant": self.pins["build_variant"],
            "components": license_components, "ffmpeg_commit": self.ffmpeg_commit, "schema_version": 1}
        self.write(root, "component-licenses/component-license-manifest.json",
                   (json.dumps(license_manifest, indent=2, sort_keys=True) + "\n").encode())

        source_records = [{"bytes": path.stat().st_size, "path": path.relative_to(root).as_posix(),
                           "sha256": digest_file(path)}
                          for path in sorted(root.rglob("*")) if path.is_file()]
        source_manifest = {
            "btbn_build_commit": self.repo_commit,
            "btbn_release_tag": self.pins["btbn_build_repository"]["release_tag"],
            "build_variant": self.pins["build_variant"], "ffmpeg_commit": self.ffmpeg_commit,
            "generated_dockerfiles": docker_records, "schema_version": 1,
            "source_files": source_records}
        self.write(root, "SOURCE-MANIFEST.json",
                   (json.dumps(source_manifest, indent=2, sort_keys=True) + "\n").encode())
        sums = [f"{digest_file(path)}  {path.relative_to(root).as_posix()}"
                for path in sorted(root.rglob("*")) if path.is_file() and path.name != "SHA256SUMS.txt"]
        self.write(root, "SHA256SUMS.txt", ("\n".join(sums) + "\n").encode())
        if tamper_after_manifest:
            dependency.write_bytes(b"tampered")
        return root, component_sources

    def archive_tree(self, root: Path, name: str, outside: bool = False) -> Path:
        archive_path = self.root / name
        with tarfile.open(archive_path, "w:xz") as archive:
            archive.add(root, arcname="media-corresponding-source")
            if outside:
                extra = self.write(self.root, "outside.txt", b"not checksummed")
                archive.add(extra, arcname="outside.txt")
        return archive_path

    def media_root(self, source_root: Path) -> Path:
        root = self.root / "media"
        if root.exists():
            shutil.rmtree(root)
        ffmpeg = self.write(root, "bin/ffmpeg.exe", self.win_tool)
        ffprobe = self.write(root, "bin/ffprobe.exe", self.win_probe)
        self.write(root, "licenses/FFmpeg-COPYING.GPLv3.txt", self.ffmpeg_license)
        self.write(root, "licenses/BtbN-FFmpeg-Builds-LICENSE.txt", self.btbn_license)
        self.write(root, "licenses/README-CORRESPONDING-SOURCE.txt", b"source beside package")
        shutil.copytree(source_root / "component-licenses", root / "licenses/components")
        asset = self.pins["assets"]["windows-x64"]
        repo = self.pins["btbn_build_repository"]
        sbom = {
            "binary_asset": {"name": asset["name"], "sha256": asset["sha256"],
                "url": f"https://github.com/BtbN/FFmpeg-Builds/releases/download/{repo['release_tag']}/{asset['name']}"},
            "build_repository": {"commit": repo["commit"], "sha256": repo["archive_sha256"],
                "url": f"https://github.com/BtbN/FFmpeg-Builds/archive/{repo['commit']}.tar.gz"},
            "build_variant": self.pins["build_variant"], "ffmpeg_commit": self.ffmpeg_commit,
            "libraries": [], "platform": "windows-x64",
            "executables": [{"license": "GPL-3.0-or-later", "name": name,
                "relative_path": f"bin/{name}.exe", "sha256": digest_file(path),
                "source_commit": self.ffmpeg_commit, "version": self.pins["ffmpeg"]["version"]}
                for name, path in (("ffmpeg", ffmpeg), ("ffprobe", ffprobe))]}
        self.write(root, "provenance/media-sbom.json",
                   (json.dumps(sbom, indent=2, sort_keys=True) + "\n").encode())
        self.write(root, "provenance/SHA256SUMS.txt",
                   (f"{digest_file(ffmpeg)}  bin/ffmpeg.exe\n{digest_file(ffprobe)}  bin/ffprobe.exe\n").encode())
        return root

    def test_large_source_and_exact_media_pair_pass(self):
        source_root, expected_sources = self.source_tree()
        source_info = package_portable.validate_source_archive(self.archive_tree(source_root, "source.tar.xz"))
        self.assertEqual(source_info["component_sources"], expected_sources)
        media_info = package_portable.validate_media_root(self.media_root(source_root), "windows-x64")
        package_portable.validate_media_source_pair(media_info, source_info)

    def test_source_rejects_outside_root_and_tampering(self):
        source_root, _ = self.source_tree()
        with self.assertRaisesRegex(package_portable.PortablePackageError, "outside"):
            package_portable.validate_source_archive(self.archive_tree(source_root, "outside.tar.xz", True))
        tampered_root, _ = self.source_tree(True)
        with self.assertRaisesRegex(package_portable.PortablePackageError, "hash mismatch"):
            package_portable.validate_source_archive(self.archive_tree(tampered_root, "tampered.tar.xz"))

    def test_media_rejects_stale_sum_binary_and_license(self):
        source_root, _ = self.source_tree()
        media = self.media_root(source_root)
        with (media / "provenance/SHA256SUMS.txt").open("a", encoding="utf-8") as handle:
            handle.write(f"{'0' * 64}  bin/stale.exe\n")
        with self.assertRaisesRegex(package_portable.PortablePackageError, "inventory"):
            package_portable.validate_media_root(media, "windows-x64")
        media = self.media_root(source_root)
        (media / "bin/ffmpeg.exe").write_bytes(b"MZtampered")
        with self.assertRaisesRegex(package_portable.PortablePackageError, "hash mismatch"):
            package_portable.validate_media_root(media, "windows-x64")
        media = self.media_root(source_root)
        next((media / "licenses/components/evidence").rglob("*LICENSE")).write_text("changed")
        with self.assertRaisesRegex(package_portable.PortablePackageError, "evidence mismatch"):
            package_portable.validate_media_root(media, "windows-x64")
        media = self.media_root(source_root)
        self.write(media, "unrecorded-payload.bin", b"hidden")
        with self.assertRaisesRegex(package_portable.PortablePackageError, "root file inventory"):
            package_portable.validate_media_root(media, "windows-x64")

    def test_dockerfile_union_excludes_stale_and_nonfree(self):
        cache = self.root / "downloads"
        cache.mkdir()
        source = self.write(cache, self.dep_name, b"source")
        dockerfiles = []
        for target in ("linux64", "win64"):
            name = f"{target}-gpl-9.0.Dockerfile"
            data = (f"ENV TARGET={target} VARIANT=gpl REPO=btbn/ffmpeg-builds ADDINS_STR=9.0 FFVER=900\n"
                    f"RUN --mount=src=.cache/downloads/{self.dep_name},dst=/cache.tar.xz true\n").encode()
            path = self.write(self.root, name, data)
            self.pins["generated_dockerfiles"][name] = digest_file(path)
            dockerfiles.append(path)
        self.write(cache, "stale_" + "e" * 64 + ".tar.xz", b"stale")
        selected, records = prepare_media.selected_cache_sources(dockerfiles, cache)
        self.assertEqual(selected, [source])
        self.assertEqual(len(records), 2)
        bad = dockerfiles[0]
        bad.write_text(bad.read_text().replace(self.dep_name, f"50-fdk-aac_{'f' * 64}.tar.xz"))
        self.pins["generated_dockerfiles"][bad.name] = digest_file(bad)
        with self.assertRaisesRegex(prepare_media.MediaPreparationError, "nonfree"):
            prepare_media.selected_cache_sources(dockerfiles, cache)

    def test_download_stage_filter_keeps_only_exact_mount_stages(self):
        dockerfiles = []
        for target in ("linux64", "win64"):
            name = f"{target}-gpl-9.0.Dockerfile"
            data = (f"ENV TARGET={target} VARIANT=gpl REPO=btbn/ffmpeg-builds ADDINS_STR=9.0 FFVER=900\n"
                    f"RUN --mount=src=.cache/downloads/{self.dep_name},dst=/cache.tar.xz true\n").encode()
            path = self.write(self.root, f"filter/{name}", data)
            self.pins["generated_dockerfiles"][name] = digest_file(path)
            dockerfiles.append(path)
        repository = self.root / "btbn"
        self.write(repository, "download.sh", b"#!/bin/bash\n")
        selected = self.write(repository, "scripts.d/50-codec.sh", b"ffbuild_dockerdl() { :; }\n")
        skipped = self.write(repository, "scripts.d/50-fdk-aac.sh", b"ffbuild_dockerdl() { :; }\n")
        with mock.patch.object(
            prepare_media, "require_target_output", side_effect=lambda path, _label: path.resolve()
        ):
            enabled, disabled = prepare_media.filter_download_stages(repository, dockerfiles)
        self.assertEqual((enabled, disabled), (1, 1))
        self.assertTrue(selected.is_file())
        self.assertFalse(skipped.exists())
        self.assertTrue(skipped.with_name(skipped.name + ".neoflash-disabled").is_file())

    def test_component_license_cap_applies_after_content_deduplication(self):
        duplicates = self.root / "duplicate-licenses.tar.xz"
        with tarfile.open(duplicates, "w:xz") as archive:
            for index in range(300):
                data = b"same license text"
                info = tarfile.TarInfo(f"source/LICENSE-{index:03d}")
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
        records = prepare_media.extract_component_licenses(
            duplicates, "duplicates", self.root / "duplicate-evidence"
        )
        self.assertEqual(len(records), 1)

        unique = self.root / "unique-licenses.tar.xz"
        with tarfile.open(unique, "w:xz") as archive:
            for index in range(prepare_media.MAX_LICENSE_FILES_PER_COMPONENT + 1):
                data = f"unique license text {index}".encode()
                info = tarfile.TarInfo(f"source/LICENSE-{index:03d}")
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
        with self.assertRaisesRegex(prepare_media.MediaPreparationError, "too many unique"):
            prepare_media.extract_component_licenses(
                unique, "unique", self.root / "unique-evidence"
            )

    def test_deterministic_archives_stream_large_file_and_normalize_modes(self):
        stage = self.root / "stage"
        stage.mkdir()
        large = stage / "large.bin"
        with large.open("wb") as handle:
            handle.truncate(12 * 1024 * 1024)
        tool = self.write(stage, "tool", b"executable")
        tool.chmod(0o755)
        first, second = self.root / "first.zip", self.root / "second.zip"
        package_portable.deterministic_zip(stage, first, "NeoFlash")
        package_portable.deterministic_zip(stage, second, "NeoFlash")
        self.assertEqual(digest_file(first), digest_file(second))
        with zipfile.ZipFile(first) as archive:
            self.assertEqual(archive.getinfo("NeoFlash/large.bin").file_size, large.stat().st_size)
        with self.assertRaisesRegex(package_portable.PortablePackageError, "unsafe|archive root"):
            package_portable.deterministic_zip(stage, self.root / "escape.zip", "../escape")
        first_tar, second_tar = self.root / "first.tar.gz", self.root / "second.tar.gz"
        package_portable.deterministic_tar_gz(stage, first_tar, "NeoFlash")
        package_portable.deterministic_tar_gz(stage, second_tar, "NeoFlash")
        self.assertEqual(digest_file(first_tar), digest_file(second_tar))
        with tarfile.open(first_tar, "r:gz") as archive:
            self.assertEqual(archive.getmember("NeoFlash/large.bin").mode, 0o644)
            self.assertEqual(archive.getmember("NeoFlash/tool").mode, 0o755)

    def test_end_to_end_windows_package_has_guides_hashes_and_shared_source(self):
        source_root, _ = self.source_tree()
        source_archive = self.archive_tree(source_root, "portable-source.tar.xz")
        media = self.media_root(source_root)
        project = self.root / "project"
        self.write(project, "LICENSE-MIT", b"MIT\n")
        self.write(project, "LICENSE-APACHE", b"Apache\n")
        self.write(project, "THIRD_PARTY_NOTICES.txt", b"third-party\n")
        executable = self.write(project, "target/release/neoflash-editor.exe", b"MZeditor")
        runtime = project / "target/web-runtime"
        self.write(runtime, "neoflash_web.js", b"runtime")
        self.write(runtime, "neoflash_web_bg.wasm", b"wasm")
        rust_licenses = project / "target/rust-licenses"
        evidence = self.write(rust_licenses, "evidence/fixture-LICENSE", b"license")
        self.write(rust_licenses, "rust-dependency-licenses.md", b"fixture summary")
        rust_manifest = {
            "package_count": 1,
            "packages": [{
                "license_evidence": [{
                    "bytes": evidence.stat().st_size,
                    "bundle_path": "evidence/fixture-LICENSE",
                    "sha256": digest_file(evidence),
                }],
                "name": "fixture",
                "version": "1.0.0",
            }],
        }
        self.write(
            rust_licenses,
            "rust-dependency-licenses.json",
            (json.dumps(rust_manifest) + "\n").encode(),
        )
        package_name = "NeoFlash-fixture-windows-x64-preview"
        args = SimpleNamespace(
            archive=project / "dist" / f"{package_name}.zip",
            build_number=42,
            checksum_file=project / "dist/fixture-SHA256SUMS.txt",
            commit="a" * 40,
            executable=executable,
            media_root=media,
            media_source_archive=source_archive,
            package_name=package_name,
            platform="windows-x64",
            profile="release",
            project_root=project,
            runtime_dir=runtime,
            rust_licenses=rust_licenses,
            source_output=project / "dist/fixture-portable-media-corresponding-source.tar.xz",
            stage_dir=project / "target/package" / package_name,
            version="fixture",
        )
        package_portable.stage_package(args)
        package_portable.finalize_package(args)
        self.assertTrue((args.stage_dir / "README.txt").is_file())
        self.assertTrue((args.stage_dir / "LICENSES-AND-SOURCE.txt").is_file())
        self.assertTrue((args.stage_dir / "PACKAGE-SHA256SUMS.txt").is_file())
        self.assertEqual(digest_file(args.source_output), digest_file(source_archive))
        sums = package_portable.parse_sha_manifest(
            args.checksum_file.read_text(), "fixture external sums"
        )
        self.assertEqual(set(sums), {args.archive.name, args.source_output.name})
        with zipfile.ZipFile(args.archive) as archive:
            self.assertIn(f"{package_name}/README.txt", archive.namelist())

    def test_pinned_download_size_mismatch_is_bounded_and_actionable(self):
        class Response(io.BytesIO):
            headers = {}

        destination = self.root / "download.bin"
        with mock.patch.object(
            prepare_media.urllib.request, "urlopen", return_value=Response(b"short")
        ):
            with self.assertRaisesRegex(prepare_media.MediaPreparationError, "got 5 bytes"):
                prepare_media.fetch_pinned(
                    "https://example.invalid/pinned", "0" * 64, 10, destination
                )
        self.assertFalse(destination.with_suffix(".bin.part").exists())

    def test_destructive_output_guards_reject_broad_paths(self):
        with self.assertRaisesRegex(prepare_media.MediaPreparationError, "unsafe"):
            prepare_media.require_target_output(Path("/"), "fixture output")
        with self.assertRaisesRegex(package_portable.PortablePackageError, "unsafe"):
            package_portable.require_output_child(
                self.root / "target", self.root / "target", "fixture stage"
            )
        with self.assertRaisesRegex(package_portable.PortablePackageError, "unsafe"):
            package_portable.require_output_child(
                self.root / "elsewhere", self.root / "target", "fixture stage"
            )


if __name__ == "__main__":
    unittest.main()
