#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
import shutil
import stat
import struct
import subprocess
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


def build_synthetic_pe64(
    dll_names: list[str],
    *,
    delay_dll_names: list[str] | None = None,
    machine: int = 0x8664,
    opt_magic: int = 0x020B,
) -> bytes:
    """Build a minimal valid PE32+ image carrying an .idata import directory."""
    e_lfanew = 0x80
    size_of_optional_header = 240  # Standard 112-byte PE32+ header + 16 data directories
    num_sections = 1
    sec_table_off = e_lfanew + 24 + size_of_optional_header
    size_of_headers = 0x200
    section_va = 0x1000
    section_raw_ptr = size_of_headers

    idata = bytearray()
    import_table_rva = section_va
    if dll_names:
        desc_table_len = (len(dll_names) + 1) * 20
        idata.extend(b"\x00" * desc_table_len)
        for index, dll in enumerate(dll_names):
            name_rva = section_va + len(idata)
            idata.extend(dll.encode("ascii") + b"\x00")
            if len(idata) % 2:
                idata.append(0)
            thunk_rva = section_va + len(idata)
            idata.extend(b"\x00" * 16)
            struct.pack_into(
                "<IIIII",
                idata,
                index * 20,
                thunk_rva,
                0,
                0,
                name_rva,
                thunk_rva,
            )
        import_size = desc_table_len
    else:
        import_table_rva = 0
        import_size = 0

    delay_rva = 0
    delay_size = 0
    if delay_dll_names:
        while len(idata) % 4:
            idata.append(0)
        delay_offset = len(idata)
        delay_rva = section_va + delay_offset
        delay_size = (len(delay_dll_names) + 1) * 32
        idata.extend(b"\x00" * delay_size)
        for index, dll in enumerate(delay_dll_names):
            name_rva = section_va + len(idata)
            idata.extend(dll.encode("ascii") + b"\x00")
            struct.pack_into(
                "<8I",
                idata,
                delay_offset + index * 32,
                1,
                name_rva,
                0,
                0,
                0,
                0,
                0,
                0,
            )

    raw_size = max(0x200, ((len(idata) + 0x1FF) // 0x200) * 0x200)
    idata.extend(b"\x00" * (raw_size - len(idata)))

    image = bytearray(size_of_headers)
    image[0:2] = b"MZ"
    struct.pack_into("<I", image, 0x3C, e_lfanew)
    image[e_lfanew : e_lfanew + 4] = b"PE\x00\x00"
    struct.pack_into(
        "<HHIIIHH",
        image,
        e_lfanew + 4,
        machine,
        num_sections,
        0,
        0,
        0,
        size_of_optional_header,
        0x0022,
    )
    opt_off = e_lfanew + 24
    struct.pack_into("<H", image, opt_off, opt_magic)
    struct.pack_into("<I", image, opt_off + 56, section_va + raw_size)  # SizeOfImage
    struct.pack_into("<I", image, opt_off + 60, size_of_headers)  # SizeOfHeaders
    struct.pack_into("<H", image, opt_off + 68, 2)  # Subsystem = WINDOWS_GUI
    struct.pack_into("<I", image, opt_off + 108, 16)  # NumberOfRvaAndSizes
    if import_size:
        struct.pack_into("<II", image, opt_off + 112 + 8, import_table_rva, import_size)
    if delay_size:
        struct.pack_into("<II", image, opt_off + 112 + 13 * 8, delay_rva, delay_size)

    image[sec_table_off : sec_table_off + 8] = b".idata\x00\x00"
    struct.pack_into(
        "<IIII",
        image,
        sec_table_off + 8,
        len(idata),
        section_va,
        raw_size,
        section_raw_ptr,
    )
    struct.pack_into("<I", image, sec_table_off + 36, 0x40000040)
    image.extend(idata)
    return bytes(image)


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
            root = tarfile.TarInfo(".")
            root.type = tarfile.DIRTYPE
            root.mode = 0o755
            archive.addfile(root)
            for index in range(300):
                data = b"same license text"
                info = tarfile.TarInfo(f"source/LICENSE-{index:03d}")
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
        records = prepare_media.extract_component_licenses(
            duplicates, "duplicates", self.root / "duplicate-evidence"
        )
        self.assertEqual(len(records), 1)

        british = self.root / "british-licence.tar.xz"
        with tarfile.open(british, "w:xz") as archive:
            data = b"British licence spelling"
            info = tarfile.TarInfo("source/LICENCE")
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
        records = prepare_media.extract_component_licenses(
            british, "british", self.root / "british-evidence"
        )
        self.assertEqual(len(records), 1)

        embedded = self.root / "embedded-mit.tar.xz"
        embedded_text = b'''/*\n * Copyright (c) fixture\n * Permission is hereby granted, free of charge, to any person obtaining a copy.\n * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND.\n */\nint fixture;\n'''
        with tarfile.open(embedded, "w:xz") as archive:
            info = tarfile.TarInfo("source/include/fixture.h")
            info.size = len(embedded_text)
            archive.addfile(info, io.BytesIO(embedded_text))
        records = prepare_media.extract_component_licenses(
            embedded, "embedded", self.root / "embedded-evidence"
        )
        self.assertEqual(len(records), 1)
        evidence = self.root / "embedded-evidence" / records[0]["path"]
        self.assertIn(b"Permission is hereby granted", evidence.read_bytes())

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
        tool_name = "tool.exe" if os.name == "nt" else "tool"
        tool = self.write(stage, tool_name, b"executable")
        tool.chmod(0o755)
        first, second = self.root / "first.zip", self.root / "second.zip"
        with mock.patch("os.access", return_value=True):
            package_portable.deterministic_zip(stage, first, "NeoFlash")
            package_portable.deterministic_zip(stage, second, "NeoFlash")
        self.assertEqual(digest_file(first), digest_file(second))
        with zipfile.ZipFile(first) as archive:
            self.assertEqual(archive.getinfo("NeoFlash/large.bin").file_size, large.stat().st_size)
            self.assertEqual(archive.getinfo("NeoFlash/large.bin").external_attr >> 16, stat.S_IFREG | 0o644)
            self.assertEqual(archive.getinfo(f"NeoFlash/{tool_name}").external_attr >> 16, stat.S_IFREG | 0o755)
        with self.assertRaisesRegex(package_portable.PortablePackageError, "unsafe|archive root"):
            package_portable.deterministic_zip(stage, self.root / "escape.zip", "../escape")
        first_tar, second_tar = self.root / "first.tar.gz", self.root / "second.tar.gz"
        with mock.patch("os.access", return_value=True):
            package_portable.deterministic_tar_gz(stage, first_tar, "NeoFlash")
            package_portable.deterministic_tar_gz(stage, second_tar, "NeoFlash")
        self.assertEqual(digest_file(first_tar), digest_file(second_tar))
        with tarfile.open(first_tar, "r:gz") as archive:
            self.assertEqual(archive.getmember("NeoFlash/large.bin").mode, 0o644)
            self.assertEqual(archive.getmember(f"NeoFlash/{tool_name}").mode, 0o755)

    def test_end_to_end_windows_package_has_guides_hashes_and_shared_source(self):
        source_root, _ = self.source_tree()
        source_archive = self.archive_tree(source_root, "portable-source.tar.xz")
        media = self.media_root(source_root)
        project = self.root / "project"
        self.write(project, "LICENSE-MIT", b"MIT\n")
        self.write(project, "LICENSE-APACHE", b"Apache\n")
        self.write(project, "THIRD_PARTY_NOTICES.txt", b"third-party\n")
        executable = self.write(
            project,
            "target/release/neoflash-editor.exe",
            build_synthetic_pe64(["KERNEL32.dll", "USER32.dll", "ntdll.dll", "bcrypt.dll"]),
        )
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

    def test_windows_pe64_import_audit_accepts_system_dlls_and_rejects_dynamic_crt(self):
        clean_pe = self.write(
            self.root,
            "pe/clean-editor.exe",
            build_synthetic_pe64(["KERNEL32.dll", "USER32.dll", "ntdll.dll", "GDI32.dll"]),
        )
        self.assertEqual(
            package_portable.audit_windows_pe_imports(clean_pe),
            ["KERNEL32.dll", "USER32.dll", "ntdll.dll", "GDI32.dll"],
        )

        for bad_dll in (
            "VCRUNTIME140.dll",
            "vcruntime140_1.dll",
            "MSVCP140.dll",
            "ucrtbase_d.dll",
            "api-ms-win-crt-runtime-l1-1-0.dll",
            "concrt140.dll",
        ):
            bad_pe = self.write(
                self.root,
                f"pe/bad-{bad_dll}.exe",
                build_synthetic_pe64(["KERNEL32.dll", bad_dll, "USER32.dll"]),
            )
            with self.assertRaisesRegex(
                package_portable.PortablePackageError,
                rf"forbidden MSVC/UCRT runtime DLL.*{bad_dll}",
            ):
                package_portable.audit_windows_pe_imports(bad_pe)

        delay_bad = self.write(
            self.root,
            "pe/delay-vcruntime.exe",
            build_synthetic_pe64(
                ["KERNEL32.dll"],
                delay_dll_names=["VCRUNTIME140_1.dll"],
            ),
        )
        with self.assertRaisesRegex(
            package_portable.PortablePackageError, "VCRUNTIME140_1.dll"
        ):
            package_portable.audit_windows_pe_imports(delay_bad)

        stub_mz = self.write(self.root, "pe/stub.exe", b"MZnot-a-real-pe64")
        with self.assertRaisesRegex(
            package_portable.PortablePackageError, "IMAGE_DOS_HEADER"
        ):
            package_portable.audit_windows_pe_imports(stub_mz)

    @unittest.skipIf(
        os.name == "nt",
        "Linux AppImage shell packager requires POSIX userland",
    )
    def test_linux_appimage_packager_stages_complete_appdir_and_emits_verified_appimage(self):
        source_root, _ = self.source_tree()
        source_archive = self.archive_tree(source_root, "linux-source.tar.xz")
        linux_ffmpeg = b"\x7fELF" + b"linux-ffmpeg"
        linux_ffprobe = b"\x7fELF" + b"linux-ffprobe"
        self.pins["assets"]["linux-x64"]["ffmpeg_sha256"] = digest_bytes(linux_ffmpeg)
        self.pins["assets"]["linux-x64"]["ffprobe_sha256"] = digest_bytes(linux_ffprobe)

        media = self.root / "linux-media"
        ffmpeg = self.write(media, "bin/ffmpeg", linux_ffmpeg)
        ffprobe = self.write(media, "bin/ffprobe", linux_ffprobe)
        ffmpeg.chmod(0o755)
        ffprobe.chmod(0o755)
        self.write(media, "licenses/FFmpeg-COPYING.GPLv3.txt", self.ffmpeg_license)
        self.write(media, "licenses/BtbN-FFmpeg-Builds-LICENSE.txt", self.btbn_license)
        self.write(media, "licenses/README-CORRESPONDING-SOURCE.txt", b"source beside package")
        shutil.copytree(source_root / "component-licenses", media / "licenses/components")
        asset = self.pins["assets"]["linux-x64"]
        repo = self.pins["btbn_build_repository"]
        sbom = {
            "binary_asset": {
                "name": asset["name"],
                "sha256": asset["sha256"],
                "url": f"https://github.com/BtbN/FFmpeg-Builds/releases/download/{repo['release_tag']}/{asset['name']}",
            },
            "build_repository": {
                "commit": repo["commit"],
                "sha256": repo["archive_sha256"],
                "url": f"https://github.com/BtbN/FFmpeg-Builds/archive/{repo['commit']}.tar.gz",
            },
            "build_variant": self.pins["build_variant"],
            "ffmpeg_commit": self.ffmpeg_commit,
            "libraries": [],
            "platform": "linux-x64",
            "executables": [
                {
                    "license": "GPL-3.0-or-later",
                    "name": name,
                    "relative_path": f"bin/{name}",
                    "sha256": digest_file(path),
                    "source_commit": self.ffmpeg_commit,
                    "version": self.pins["ffmpeg"]["version"],
                }
                for name, path in (("ffmpeg", ffmpeg), ("ffprobe", ffprobe))
            ],
        }
        self.write(
            media,
            "provenance/media-sbom.json",
            (json.dumps(sbom, indent=2, sort_keys=True) + "\n").encode(),
        )
        self.write(
            media,
            "provenance/SHA256SUMS.txt",
            (f"{digest_file(ffmpeg)}  bin/ffmpeg\n{digest_file(ffprobe)}  bin/ffprobe\n").encode(),
        )

        project = self.root / "linux-project"
        self.write(project, "LICENSE-MIT", b"MIT\n")
        self.write(project, "LICENSE-APACHE", b"Apache\n")
        self.write(project, "THIRD_PARTY_NOTICES.txt", b"third-party\n")
        executable = self.write(
            project, "target/release/neoflash-editor", b"\x7fELF" + b"linux-editor"
        )
        executable.chmod(0o755)
        runtime = project / "target/web-runtime"
        self.write(runtime, "neoflash_web.js", b"runtime")
        self.write(runtime, "neoflash_web_bg.wasm", b"wasm")
        rust_licenses = project / "target/rust-licenses"
        evidence = self.write(rust_licenses, "evidence/fixture-LICENSE", b"license")
        self.write(rust_licenses, "rust-dependency-licenses.md", b"fixture summary")
        rust_manifest = {
            "package_count": 1,
            "packages": [
                {
                    "license_evidence": [
                        {
                            "bytes": evidence.stat().st_size,
                            "bundle_path": "evidence/fixture-LICENSE",
                            "sha256": digest_file(evidence),
                        }
                    ],
                    "name": "fixture",
                    "version": "1.0.0",
                }
            ],
        }
        self.write(
            rust_licenses,
            "rust-dependency-licenses.json",
            (json.dumps(rust_manifest) + "\n").encode(),
        )

        version = "0.1.0-beta.1"
        package_name = f"NeoFlash-{version}-linux-x64-preview"
        dist_dir = project / "dist"
        checksum_file = dist_dir / f"NeoFlash-{version}-linux-x64-SHA256SUMS.txt"
        args = SimpleNamespace(
            archive=dist_dir / f"{package_name}.tar.gz",
            build_number=629,
            checksum_file=checksum_file,
            commit="a" * 40,
            executable=executable,
            media_root=media,
            media_source_archive=source_archive,
            package_name=package_name,
            platform="linux-x64",
            profile="release",
            project_root=project,
            runtime_dir=runtime,
            rust_licenses=rust_licenses,
            source_output=dist_dir / f"NeoFlash-{version}-portable-media-corresponding-source.tar.xz",
            stage_dir=project / "target/package" / package_name,
            version=version,
        )
        package_portable.stage_package(args)
        package_portable.finalize_package(args)

        script = ROOT / "scripts/package-linux-appimage.sh"
        appdir_out = self.root / "appdir-stage"
        appdir_out.mkdir()
        res = subprocess.run(
            ["bash", str(script), "--stage-only", str(args.stage_dir), str(appdir_out)],
            check=True,
            capture_output=True,
            text=True,
        )
        appdir = Path(res.stdout.strip())
        self.assertEqual(appdir, appdir_out / "NeoFlash.AppDir")
        self.assertTrue((appdir / "AppRun").is_file())
        self.assertTrue((appdir / "AppRun").stat().st_mode & stat.S_IXUSR)
        apprun_text = (appdir / "AppRun").read_text(encoding="utf-8")
        self.assertIn("NEOFLASH_WEB_RUNTIME_DIR", apprun_text)
        self.assertIn("NEOFLASH_FFMPEG", apprun_text)
        self.assertIn("NEOFLASH_FFPROBE", apprun_text)
        self.assertNotIn("LD_LIBRARY_PATH", apprun_text)
        self.assertTrue((appdir / "neoflash.desktop").is_file())
        self.assertIn(
            f"X-AppImage-Version={version}",
            (appdir / "neoflash.desktop").read_text(encoding="utf-8"),
        )
        self.assertTrue((appdir / "neoflash.png").is_file())
        self.assertTrue((appdir / ".DirIcon").is_file())
        self.assertTrue((appdir / "usr/share/mime/packages/neoflash.xml").is_file())
        self.assertTrue((appdir / "usr/lib/neoflash/neoflash-editor").is_file())
        self.assertTrue((appdir / "usr/lib/neoflash/runtime/neoflash_web.js").is_file())
        self.assertTrue((appdir / "usr/lib/neoflash/media/bin/ffmpeg").is_file())
        self.assertTrue(
            (appdir / "usr/lib/neoflash/licenses/rust/rust-dependency-licenses.json").is_file()
        )
        self.assertTrue(
            (appdir / "usr/lib/neoflash/media/licenses/FFmpeg-COPYING.GPLv3.txt").is_file()
        )

        mock_tool = self.write(
            self.root,
            "mock-appimagetool.sh",
            b'#!/bin/sh\nset -eu\n[ "$1" = "--no-appstream" ]\n[ -x "$2/AppRun" ]\nprintf "APPIMAGE-MOCK:%s\\n" "$ARCH" > "$3"\n',
        )
        mock_tool.chmod(0o755)
        env = os.environ.copy()
        env["NEOFLASH_APPIMAGETOOL"] = str(mock_tool)
        res_pkg = subprocess.run(
            ["bash", str(script), str(args.stage_dir), str(dist_dir), str(checksum_file)],
            check=True,
            capture_output=True,
            text=True,
            env=env,
        )
        appimage_path = Path(res_pkg.stdout.strip())
        self.assertEqual(appimage_path.name, f"NeoFlash-{version}-linux-x86_64.AppImage")
        self.assertTrue(appimage_path.is_file())
        sums = package_portable.parse_sha_manifest(
            checksum_file.read_text(encoding="utf-8"), "linux sums with AppImage"
        )
        self.assertEqual(sums[appimage_path.name], digest_file(appimage_path))

    def _append_synthetic_authenticode_cert(self, pe_path: Path) -> None:
        data = bytearray(pe_path.read_bytes())
        (e_lfanew,) = struct.unpack_from("<I", data, 0x3C)
        opt_off = e_lfanew + 24
        cert_offset = len(data)
        cert_blob = struct.pack("<IHH", 32, 0x0200, 0x0002) + (b"PKCS7-SIGNED-STUB-BYTES!" )
        data.extend(cert_blob)
        # IMAGE_DIRECTORY_ENTRY_SECURITY (index 4): file offset + byte size.
        struct.pack_into("<II", data, opt_off + 112 + 4 * 8, cert_offset, len(cert_blob))
        pe_path.write_bytes(bytes(data))

    def test_windows_per_user_installer_and_uninstaller_scripts_generated_and_packaged(self):
        source_root, _ = self.source_tree()
        source_archive = self.archive_tree(source_root, "win-inst-source.tar.xz")
        media = self.media_root(source_root)
        project = self.root / "win-inst-project"
        self.write(project, "LICENSE-MIT", b"MIT\n")
        self.write(project, "LICENSE-APACHE", b"Apache\n")
        self.write(project, "THIRD_PARTY_NOTICES.txt", b"third-party\n")
        executable = self.write(
            project,
            "target/release/neoflash-editor.exe",
            build_synthetic_pe64(["KERNEL32.dll", "USER32.dll", "ntdll.dll"]),
        )
        runtime = project / "target/web-runtime"
        self.write(runtime, "neoflash_web.js", b"runtime")
        self.write(runtime, "neoflash_web_bg.wasm", b"wasm")
        rust_licenses = project / "target/rust-licenses"
        evidence = self.write(rust_licenses, "evidence/fixture-LICENSE", b"license")
        self.write(rust_licenses, "rust-dependency-licenses.md", b"fixture summary")
        rust_manifest = {
            "package_count": 1,
            "packages": [
                {
                    "license_evidence": [
                        {
                            "bytes": evidence.stat().st_size,
                            "bundle_path": "evidence/fixture-LICENSE",
                            "sha256": digest_file(evidence),
                        }
                    ],
                    "name": "fixture",
                    "version": "1.0.0",
                }
            ],
        }
        self.write(
            rust_licenses,
            "rust-dependency-licenses.json",
            (json.dumps(rust_manifest) + "\n").encode(),
        )
        version = "0.1.0-beta.2"
        package_name = f"NeoFlash-{version}-windows-x64-preview"
        args = SimpleNamespace(
            archive=project / "dist" / f"{package_name}.zip",
            build_number=640,
            checksum_file=project / "dist/win-inst-SHA256SUMS.txt",
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
            source_output=project / "dist/win-inst-portable-media-corresponding-source.tar.xz",
            stage_dir=project / "target/package" / package_name,
            version=version,
        )
        package_portable.stage_package(args)
        package_portable.finalize_package(args)

        install_ps1 = (args.stage_dir / "Install-NeoFlash.ps1").read_text(encoding="utf-8")
        uninstall_ps1 = (args.stage_dir / "Uninstall-NeoFlash.ps1").read_text(encoding="utf-8")
        provenance = json.loads((args.stage_dir / "BUNDLE-PROVENANCE.json").read_text(encoding="utf-8"))

        # Per-user install root, Start Menu shortcut, HKCU .nflash & Installed apps registry, zero UAC.
        self.assertIn('Programs\\NeoFlash', install_ps1)
        self.assertIn("NeoFlash.lnk", install_ps1)
        self.assertIn("HKCU:\\Software\\Classes\\.nflash", install_ps1)
        self.assertIn("HKCU:\\Software\\Classes\\NeoFlash.Document", install_ps1)
        self.assertIn(
            "HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\NeoFlash",
            install_ps1,
        )
        self.assertIn(version, install_ps1)
        self.assertNotIn("-Verb RunAs", install_ps1)
        self.assertNotIn("HKLM:", install_ps1)

        self.assertIn("NeoFlash.lnk", uninstall_ps1)
        self.assertIn("HKCU:\\Software\\Classes\\.nflash", uninstall_ps1)
        self.assertIn("HKCU:\\Software\\Classes\\NeoFlash.Document", uninstall_ps1)
        self.assertIn(
            "HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\NeoFlash",
            uninstall_ps1,
        )
        self.assertNotIn("HKLM:", uninstall_ps1)
        self.assertNotIn("cmd.exe", uninstall_ps1)
        self.assertNotIn("rmdir /s", uninstall_ps1)
        self.assertIn("BUILD_METADATA.json", uninstall_ps1)
        self.assertIn("PACKAGE-SHA256SUMS.txt", uninstall_ps1)
        self.assertIn("$env:NEOFLASH_UNINSTALL_TARGET = $TargetDir", uninstall_ps1)
        # Verify zero string interpolation of $TargetDir inside Start-Process / -Command.
        for line in uninstall_ps1.splitlines():
            if "Start-Process" in line or "$DeferredCleanup =" in line:
                self.assertNotIn("$TargetDir", line)

        self.assertFalse(provenance["authenticode"]["signed"])
        internal_sums = package_portable.parse_sha_manifest(
            (args.stage_dir / "PACKAGE-SHA256SUMS.txt").read_text(encoding="utf-8"),
            "windows internal manifest",
        )
        self.assertIn("Install-NeoFlash.ps1", internal_sums)
        self.assertIn("Uninstall-NeoFlash.ps1", internal_sums)
        self.assertIn("BUNDLE-PROVENANCE.json", internal_sums)
        with zipfile.ZipFile(args.archive) as zf:
            names = set(zf.namelist())
            self.assertIn(f"{package_name}/Install-NeoFlash.ps1", names)
            self.assertIn(f"{package_name}/Uninstall-NeoFlash.ps1", names)
            self.assertIn(f"{package_name}/BUNDLE-PROVENANCE.json", names)

    def test_windows_authenticode_config_validation_and_signed_provenance(self):
        # 1. Unsigned default when -Sign is not requested and no env vars are set.
        cfg = package_portable.resolve_windows_authenticode_config({}, sign_requested=False)
        self.assertFalse(cfg["enabled"])
        self.assertEqual(cfg["mode"], "unsigned")
        self.assertEqual(cfg["timestamp_url"], "http://timestamp.digicert.com")

        # 2. Fail-closed validation errors.
        with self.assertRaisesRegex(package_portable.PortablePackageError, "neither"):
            package_portable.resolve_windows_authenticode_config({}, sign_requested=True)

        pfx_file = self.write(self.root, "secrets/release-cert.pfx", b"PKCS12-DUMMY-BYTES")
        with self.assertRaisesRegex(package_portable.PortablePackageError, "ambiguous"):
            package_portable.resolve_windows_authenticode_config(
                {
                    "NEOFLASH_WINDOWS_CERT_SHA1": "a" * 40,
                    "NEOFLASH_WINDOWS_PFX_PATH": str(pfx_file),
                },
                sign_requested=True,
            )

        with self.assertRaisesRegex(package_portable.PortablePackageError, "without NEOFLASH_WINDOWS_PFX_PATH"):
            package_portable.resolve_windows_authenticode_config(
                {"NEOFLASH_WINDOWS_PFX_PASSWORD": "super-secret-password"},
                sign_requested=False,
            )

        with self.assertRaisesRegex(package_portable.PortablePackageError, "40-hex"):
            package_portable.resolve_windows_authenticode_config(
                {"NEOFLASH_WINDOWS_CERT_SHA1": "not-a-valid-sha1"},
                sign_requested=True,
            )

        with self.assertRaisesRegex(package_portable.PortablePackageError, "missing or empty"):
            package_portable.resolve_windows_authenticode_config(
                {"NEOFLASH_WINDOWS_PFX_PATH": str(self.root / "missing.pfx")},
                sign_requested=True,
            )

        with self.assertRaisesRegex(package_portable.PortablePackageError, "timestamp URL"):
            package_portable.resolve_windows_authenticode_config(
                {
                    "NEOFLASH_WINDOWS_CERT_SHA1": "a" * 40,
                    "NEOFLASH_WINDOWS_TIMESTAMP_URL": "http://user:secret@timestamp.example.com",
                },
                sign_requested=True,
            )

        # 3. Valid SHA-1 thumbprint and valid PFX configurations.
        sha_cfg = package_portable.resolve_windows_authenticode_config(
            {
                "NEOFLASH_WINDOWS_CERT_SHA1": "ab" * 20,
                "NEOFLASH_WINDOWS_TIMESTAMP_URL": "https://timestamp.digicert.com",
            },
            sign_requested=True,
        )
        self.assertTrue(sha_cfg["enabled"])
        self.assertEqual(sha_cfg["mode"], "cert_store_sha1")
        self.assertEqual(sha_cfg["cert_sha1"], "AB" * 20)
        self.assertEqual(sha_cfg["timestamp_url"], "https://timestamp.digicert.com")

        pfx_cfg = package_portable.resolve_windows_authenticode_config(
            {
                "NEOFLASH_WINDOWS_PFX_PATH": str(pfx_file),
                "NEOFLASH_WINDOWS_PFX_PASSWORD": "top-secret-pfx-password-xyz",
            },
            sign_requested=True,
        )
        self.assertTrue(pfx_cfg["enabled"])
        self.assertEqual(pfx_cfg["mode"], "pfx_file")
        self.assertTrue(pfx_cfg["has_pfx_password"])

        # 4. End-to-end signed Windows stage -> record_windows_authenticode_signing -> finalize_package,
        #    verifying zero secret leakage and fail-closed post-signing hash/table checks.
        self.win_tool = build_synthetic_pe64(["KERNEL32.dll"])
        self.win_probe = build_synthetic_pe64(["KERNEL32.dll", "USER32.dll"])
        self.pins["assets"]["windows-x64"]["ffmpeg_sha256"] = digest_bytes(self.win_tool)
        self.pins["assets"]["windows-x64"]["ffprobe_sha256"] = digest_bytes(self.win_probe)

        source_root, _ = self.source_tree()
        source_archive = self.archive_tree(source_root, "win-signed-source.tar.xz")
        media = self.media_root(source_root)
        project = self.root / "win-signed-project"
        self.write(project, "LICENSE-MIT", b"MIT\n")
        self.write(project, "LICENSE-APACHE", b"Apache\n")
        self.write(project, "THIRD_PARTY_NOTICES.txt", b"third-party\n")
        executable = self.write(
            project,
            "target/release/neoflash-editor.exe",
            build_synthetic_pe64(["KERNEL32.dll", "USER32.dll", "ntdll.dll"]),
        )
        runtime = project / "target/web-runtime"
        self.write(runtime, "neoflash_web.js", b"runtime")
        self.write(runtime, "neoflash_web_bg.wasm", b"wasm")
        rust_licenses = project / "target/rust-licenses"
        evidence = self.write(rust_licenses, "evidence/fixture-LICENSE", b"license")
        self.write(rust_licenses, "rust-dependency-licenses.md", b"fixture summary")
        rust_manifest = {
            "package_count": 1,
            "packages": [
                {
                    "license_evidence": [
                        {
                            "bytes": evidence.stat().st_size,
                            "bundle_path": "evidence/fixture-LICENSE",
                            "sha256": digest_file(evidence),
                        }
                    ],
                    "name": "fixture",
                    "version": "1.0.0",
                }
            ],
        }
        self.write(
            rust_licenses,
            "rust-dependency-licenses.json",
            (json.dumps(rust_manifest) + "\n").encode(),
        )
        package_name = "NeoFlash-0.1.0-beta.2-windows-x64-preview"
        args = SimpleNamespace(
            archive=project / "dist" / f"{package_name}.zip",
            build_number=641,
            checksum_file=project / "dist/win-signed-SHA256SUMS.txt",
            commit="f" * 40,
            executable=executable,
            media_root=media,
            media_source_archive=source_archive,
            package_name=package_name,
            platform="windows-x64",
            profile="release",
            project_root=project,
            runtime_dir=runtime,
            rust_licenses=rust_licenses,
            source_output=project / "dist/win-signed-portable-media-corresponding-source.tar.xz",
            stage_dir=project / "target/package" / package_name,
            version="0.1.0-beta.2",
        )
        package_portable.stage_package(args)

        targets = package_portable.collect_windows_signable_binaries(args.stage_dir)
        self.assertEqual(
            [p.relative_to(args.stage_dir).as_posix() for p in targets],
            ["neoflash-editor.exe", "media/bin/ffmpeg.exe", "media/bin/ffprobe.exe"],
        )
        pre_hashes = {
            p.relative_to(args.stage_dir).as_posix(): digest_file(p) for p in targets
        }

        # Recording signing BEFORE binaries actually carry an Authenticode certificate table must fail closed!
        with self.assertRaisesRegex(
            package_portable.PortablePackageError, "Authenticode certificate table missing"
        ):
            package_portable.record_windows_authenticode_signing(
                args.stage_dir,
                credential_source="pfx_file",
                timestamp_url="http://timestamp.digicert.com",
                pre_hashes=pre_hashes,
            )

        for target_pe in targets:
            self._append_synthetic_authenticode_cert(target_pe)

        prov = package_portable.record_windows_authenticode_signing(
            args.stage_dir,
            credential_source="pfx_file",
            timestamp_url="http://timestamp.digicert.com",
            pre_hashes=pre_hashes,
        )
        self.assertTrue(prov["authenticode"]["signed"])
        self.assertTrue(prov["authenticode"]["verified"])
        self.assertEqual(prov["authenticode"]["verify_policy"], "signtool verify /pa /v")
        prov_text = (args.stage_dir / "BUNDLE-PROVENANCE.json").read_text(encoding="utf-8")
        self.assertNotIn("top-secret-pfx-password-xyz", prov_text)
        self.assertNotIn("release-cert.pfx", prov_text)
        self.assertNotIn("password", prov_text.lower())

        package_portable.finalize_package(args)
        self.assertTrue(args.archive.is_file())

    def test_windows_uninstall_script_marker_guard_and_metacharacter_safety(self):
        script = package_portable.render_windows_uninstall_script("0.1.0-beta.2")
        self.assertNotIn("cmd.exe", script)
        self.assertIn("Test-Path -LiteralPath $BuildMetaMarker", script)
        self.assertIn("Test-Path -LiteralPath $SumsMarker", script)
        self.assertIn("$env:NEOFLASH_UNINSTALL_TARGET = $TargetDir", script)
        self.assertIn('"powershell.exe"', script)
        self.assertIn('"-NoProfile"', script)
        self.assertIn('"-NonInteractive"', script)
        self.assertIn('"-WindowStyle", "Hidden"', script)
        # Deferred cleanup literal must be single-quoted with zero $TargetDir interpolation
        # so paths containing `&`, `%TEMP%`, `;`, or quotes can never inject commands.
        deferred_lines = [
            line for line in script.splitlines() if line.strip().startswith("$DeferredCleanup =")
        ]
        self.assertEqual(len(deferred_lines), 1)
        rhs = deferred_lines[0].split("=", 1)[1].strip()
        self.assertTrue(rhs.startswith("'") and rhs.endswith("'"), rhs)
        self.assertNotIn("$TargetDir", rhs)
        self.assertIn("$env:NEOFLASH_UNINSTALL_TARGET", rhs)
        self.assertIn("Remove-Item -LiteralPath $t -Recurse -Force", rhs)

        # If pwsh is available on the host, execute the marker guard directly against a path
        # containing `&` and `%TEMP%` without markers and verify it refuses deletion!
        pwsh = shutil.which("pwsh")
        if pwsh:
            tricky_dir = self.root / "Tricky & %TEMP% Dir"
            sentinel = self.write(tricky_dir, "keep-user-file.txt", b"do not delete")
            ps1_path = self.root / "Uninstall-NeoFlash.ps1"
            ps1_path.write_text(script, encoding="utf-8")
            env = os.environ.copy()
            env["APPDATA"] = ""
            env["LOCALAPPDATA"] = str(self.root / "LocalAppData")
            proc = subprocess.run(
                [pwsh, "-NoProfile", "-NonInteractive", "-File", str(ps1_path), "-InstallDir", str(tricky_dir), "-Quiet"],
                capture_output=True,
                text=True,
                env=env,
            )
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("BUILD_METADATA.json", proc.stderr)
            self.assertTrue(sentinel.is_file())


if __name__ == "__main__":
    unittest.main()
