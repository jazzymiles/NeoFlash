#!/usr/bin/env python3
"""Stage and archive fail-closed Windows/Linux NeoFlash preview packages.

The platform wrappers build NeoFlash and its web runtime, stage the exact Rust
license closure, then call this tool in two phases.  ``stage`` validates and
copies immutable inputs.  This leaves a narrow window for optional platform
signing.  ``finalize`` hashes the final bytes and creates a deterministic ZIP
or tar.gz plus a checksum file covering both the application archive and the
bundled-media corresponding-source archive.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import re
import shutil
import stat
import tarfile
import tempfile
import zipfile
from pathlib import Path, PurePosixPath
from typing import Any, Iterable


DETERMINISTIC_MTIME = 315_532_800  # 1980-01-01 UTC, valid for ZIP.
SHA_LINE = re.compile(r"^([0-9a-fA-F]{64})  (.+)$")
PINS_PATH = Path(__file__).with_name("portable-media-pins.json")
PINS = json.loads(PINS_PATH.read_text(encoding="utf-8"))
MAX_MANIFEST_BYTES = 64 * 1024 * 1024
CACHE_REFERENCE = re.compile(
    r"\.cache/downloads/([A-Za-z0-9_.+-]+_[0-9a-f]{64}\.tar\.xz)"
)


class PortablePackageError(RuntimeError):
    """The requested portable package cannot be proven complete."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_json(value: Any) -> str:
    return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def safe_relative_path(raw: str) -> PurePosixPath:
    normalized = raw.replace("\\", "/")
    path = PurePosixPath(normalized)
    if (
        not normalized
        or path.is_absolute()
        or any(part in ("", ".", "..") for part in path.parts)
        or (path.parts and ":" in path.parts[0])
    ):
        raise PortablePackageError(f"unsafe relative path: {raw!r}")
    return path


def safe_archive_root(raw: str) -> str:
    path = safe_relative_path(raw)
    if len(path.parts) != 1 or "/" in raw or "\\" in raw:
        raise PortablePackageError(f"archive root must be one safe path component: {raw!r}")
    return path.name


def require_output_child(path: Path, root: Path, label: str) -> None:
    resolved = path.resolve()
    allowed = root.resolve()
    if resolved == allowed or not resolved.is_relative_to(allowed) or ".git" in resolved.parts:
        raise PortablePackageError(
            f"refusing unsafe {label}; expected a child of {allowed}: {resolved}"
        )


def parse_sha_manifest(text: str, label: str) -> dict[str, str]:
    result: dict[str, str] = {}
    for line_number, raw_line in enumerate(text.splitlines(), 1):
        if not raw_line.strip():
            continue
        match = SHA_LINE.fullmatch(raw_line)
        if not match:
            raise PortablePackageError(
                f"{label}:{line_number}: expected '<sha256>  <relative-path>'"
            )
        digest, raw_path = match.groups()
        path = safe_relative_path(raw_path).as_posix()
        if path in result:
            raise PortablePackageError(f"{label}: duplicate checksum path {path!r}")
        result[path] = digest.lower()
    if not result:
        raise PortablePackageError(f"{label}: checksum manifest is empty")
    return result


def require_file(path: Path, label: str) -> None:
    if not path.is_file() or path.stat().st_size == 0:
        raise PortablePackageError(f"missing or empty {label}: {path}")


def check_binary_magic(path: Path, platform: str, label: str) -> None:
    require_file(path, label)
    with path.open("rb") as handle:
        magic = handle.read(4)
    if platform == "windows-x64" and magic[:2] != b"MZ":
        raise PortablePackageError(f"{label} is not a Windows PE executable: {path}")
    if platform == "linux-x64" and magic != b"\x7fELF":
        raise PortablePackageError(f"{label} is not a Linux ELF executable: {path}")


def load_json(path: Path, label: str) -> dict[str, Any]:
    require_file(path, label)
    try:
        value = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise PortablePackageError(f"invalid {label} {path}: {error}") from error
    if not isinstance(value, dict):
        raise PortablePackageError(f"{label} must be a JSON object: {path}")
    return value


def validate_rust_licenses(root: Path) -> None:
    manifest_path = root / "rust-dependency-licenses.json"
    manifest = load_json(manifest_path, "Rust dependency license manifest")
    packages = manifest.get("packages")
    if not isinstance(packages, list) or not packages:
        raise PortablePackageError("Rust dependency license manifest has no packages")
    if manifest.get("package_count") != len(packages):
        raise PortablePackageError("Rust dependency license package count does not match")
    evidence_hashes: dict[str, str] = {}
    for package in packages:
        if not isinstance(package, dict):
            raise PortablePackageError("Rust dependency license package entry is invalid")
        identity = f"{package.get('name', '?')} {package.get('version', '?')}"
        evidence = package.get("license_evidence")
        if not isinstance(evidence, list) or not evidence:
            raise PortablePackageError(f"Rust package {identity} has no license evidence")
        for entry in evidence:
            if not isinstance(entry, dict):
                raise PortablePackageError(f"Rust package {identity} has invalid evidence")
            relative = (
                entry.get("bundle_path")
                or entry.get("bundled_path")
                or entry.get("relative_path")
            )
            expected = entry.get("sha256")
            size = entry.get("bytes")
            if (
                not isinstance(relative, str)
                or not isinstance(expected, str)
                or not re.fullmatch(r"[0-9a-f]{64}", expected)
                or not isinstance(size, int)
                or size <= 0
            ):
                raise PortablePackageError(f"Rust package {identity} evidence is incomplete")
            normalized = safe_relative_path(relative).as_posix()
            previous = evidence_hashes.setdefault(normalized, expected)
            if previous != expected:
                raise PortablePackageError(
                    f"Rust package {identity} has conflicting shared license evidence"
                )
            evidence_path = root / normalized
            require_file(evidence_path, f"license evidence for {identity}")
            if evidence_path.stat().st_size != size or sha256_file(evidence_path) != expected:
                raise PortablePackageError(f"Rust package {identity} license hash mismatch")
    require_file(root / "rust-dependency-licenses.md", "Rust dependency license summary")
    expected_files = set(evidence_hashes) | {
        "rust-dependency-licenses.json",
        "rust-dependency-licenses.md",
    }
    actual_files: set[str] = set()
    for path in root.rglob("*"):
        if path.is_symlink():
            raise PortablePackageError(f"Rust license bundle contains a symlink: {path}")
        if path.is_file():
            actual_files.add(path.relative_to(root).as_posix())
    if actual_files != expected_files:
        raise PortablePackageError("Rust license bundle file inventory is incomplete or stale")


def expected_media_names(platform: str) -> tuple[str, str]:
    suffix = ".exe" if platform == "windows-x64" else ""
    return f"ffmpeg{suffix}", f"ffprobe{suffix}"


def validate_component_licenses(root: Path) -> dict[str, Any]:
    manifest_path = root / "component-license-manifest.json"
    manifest = load_json(manifest_path, "portable media component license manifest")
    expected_repo = PINS["btbn_build_repository"]
    if (
        manifest.get("btbn_build_commit") != expected_repo["commit"]
        or manifest.get("ffmpeg_commit") != PINS["ffmpeg"]["commit"]
        or manifest.get("build_variant") != PINS["build_variant"]
    ):
        raise PortablePackageError("component license manifest does not match pinned media inputs")
    components = manifest.get("components")
    if not isinstance(components, list) or not components:
        raise PortablePackageError("component license manifest has no components")
    source_hashes: dict[str, str] = {}
    evidence_paths: set[str] = set()
    for component in components:
        if not isinstance(component, dict):
            raise PortablePackageError("component license entry is invalid")
        name = component.get("component")
        source_path = component.get("source_path")
        source_sha = component.get("source_sha256")
        evidence = component.get("license_evidence")
        if (
            not isinstance(name, str)
            or not isinstance(source_path, str)
            or not isinstance(source_sha, str)
            or not re.fullmatch(r"[0-9a-f]{64}", source_sha)
            or not isinstance(evidence, list)
            or not evidence
        ):
            raise PortablePackageError("component license entry is incomplete")
        safe_source = safe_relative_path(source_path).as_posix()
        if safe_source in source_hashes:
            raise PortablePackageError(f"duplicate component license source {safe_source}")
        source_hashes[safe_source] = source_sha
        for record in evidence:
            if not isinstance(record, dict):
                raise PortablePackageError(f"invalid license evidence for {name}")
            relative = record.get("path")
            expected = record.get("sha256")
            size = record.get("bytes")
            if (
                not isinstance(relative, str)
                or not isinstance(expected, str)
                or not isinstance(size, int)
                or size <= 0
            ):
                raise PortablePackageError(f"incomplete license evidence for {name}")
            evidence_path = root / safe_relative_path(relative)
            normalized_evidence = safe_relative_path(relative).as_posix()
            if normalized_evidence in evidence_paths:
                raise PortablePackageError(f"duplicate component license evidence {relative}")
            evidence_paths.add(normalized_evidence)
            require_file(evidence_path, f"component license evidence for {name}")
            if evidence_path.stat().st_size != size or sha256_file(evidence_path) != expected:
                raise PortablePackageError(f"component license evidence mismatch for {name}")
    file_hashes = {
        path.relative_to(root).as_posix(): sha256_file(path)
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }
    if set(file_hashes) != evidence_paths | {"component-license-manifest.json"}:
        raise PortablePackageError("component license file inventory is incomplete or stale")
    return {
        "manifest_sha256": sha256_file(manifest_path),
        "source_hashes": source_hashes,
        "file_hashes": file_hashes,
    }


def validate_media_root(root: Path, platform: str) -> dict[str, Any]:
    ffmpeg_name, ffprobe_name = expected_media_names(platform)
    ffmpeg = root / "bin" / ffmpeg_name
    ffprobe = root / "bin" / ffprobe_name
    check_binary_magic(ffmpeg, platform, "bundled ffmpeg")
    check_binary_magic(ffprobe, platform, "bundled ffprobe")
    if platform == "linux-x64":
        for helper in (ffmpeg, ffprobe):
            if not helper.stat().st_mode & stat.S_IXUSR:
                raise PortablePackageError(f"bundled helper is not executable: {helper}")

    licenses = root / "licenses"
    if not licenses.is_dir() or not any(path.is_file() for path in licenses.rglob("*")):
        raise PortablePackageError("bundled media license directory is missing or empty")
    require_file(
        licenses / "README-CORRESPONDING-SOURCE.txt",
        "bundled media corresponding-source notice",
    )
    if (
        sha256_file(licenses / "FFmpeg-COPYING.GPLv3.txt")
        != PINS["ffmpeg"]["license_sha256"]
        or sha256_file(licenses / "BtbN-FFmpeg-Builds-LICENSE.txt")
        != PINS["btbn_build_repository"]["license_sha256"]
    ):
        raise PortablePackageError("required pinned FFmpeg/BtbN license text is missing or changed")
    component_license_info = validate_component_licenses(licenses / "components")

    provenance = root / "provenance"
    sums_path = provenance / "SHA256SUMS.txt"
    sbom_path = provenance / "media-sbom.json"
    sums = parse_sha_manifest(
        sums_path.read_text(encoding="utf-8") if sums_path.is_file() else "",
        str(sums_path),
    )
    sbom = load_json(sbom_path, "bundled media SBOM")
    platform_pins = PINS["assets"][platform]
    expected_repo = PINS["btbn_build_repository"]
    expected_asset_url = (
        "https://github.com/BtbN/FFmpeg-Builds/releases/download/"
        f"{expected_repo['release_tag']}/{platform_pins['name']}"
    )
    expected_build_url = (
        "https://github.com/BtbN/FFmpeg-Builds/archive/"
        f"{expected_repo['commit']}.tar.gz"
    )
    if (
        sbom.get("platform") != platform
        or sbom.get("build_variant") != PINS["build_variant"]
        or sbom.get("ffmpeg_commit") != PINS["ffmpeg"]["commit"]
        or sbom.get("binary_asset")
        != {
            "name": platform_pins["name"],
            "sha256": platform_pins["sha256"],
            "url": expected_asset_url,
        }
        or sbom.get("build_repository")
        != {
            "commit": expected_repo["commit"],
            "sha256": expected_repo["archive_sha256"],
            "url": expected_build_url,
        }
    ):
        raise PortablePackageError("bundled media SBOM does not match canonical pinned inputs")
    records: list[dict[str, Any]] = []
    for key in ("executables", "libraries"):
        entries = sbom.get(key, [])
        if not isinstance(entries, list):
            raise PortablePackageError(f"bundled media SBOM field {key!r} is invalid")
        for entry in entries:
            if not isinstance(entry, dict):
                raise PortablePackageError(f"bundled media SBOM field {key!r} has invalid entry")
            records.append(entry)

    by_path: dict[str, str] = {}
    for record in records:
        raw_path = record.get("relative_path")
        expected = record.get("sha256")
        if not isinstance(raw_path, str) or not isinstance(expected, str):
            raise PortablePackageError("bundled media SBOM record lacks path or SHA-256")
        relative = safe_relative_path(raw_path).as_posix()
        if relative in by_path:
            raise PortablePackageError(f"duplicate bundled media SBOM path {relative!r}")
        payload = root / relative
        require_file(payload, "bundled media payload")
        actual = sha256_file(payload)
        if actual != expected.lower() or sums.get(relative) != actual:
            raise PortablePackageError(f"bundled media hash mismatch for {relative}")
        by_path[relative] = actual

    required = {f"bin/{ffmpeg_name}", f"bin/{ffprobe_name}"}
    if not required.issubset(by_path):
        missing = ", ".join(sorted(required - by_path))
        raise PortablePackageError(f"bundled media SBOM omits required helpers: {missing}")

    binary_payloads: set[str] = set()
    for directory in (root / "bin", root / "lib"):
        if directory.is_dir():
            binary_payloads.update(
                path.relative_to(root).as_posix()
                for path in directory.rglob("*")
                if path.is_file()
            )
    if binary_payloads != set(by_path):
        missing = sorted(binary_payloads - set(by_path))
        stale = sorted(set(by_path) - binary_payloads)
        raise PortablePackageError(
            f"bundled media SBOM/file inventory mismatch; unrecorded={missing}, absent={stale}"
        )
    if set(sums) != set(by_path):
        raise PortablePackageError("bundled media SHA256SUMS inventory is incomplete or stale")
    expected_helper_hashes = {
        f"bin/{ffmpeg_name}": platform_pins["ffmpeg_sha256"],
        f"bin/{ffprobe_name}": platform_pins["ffprobe_sha256"],
    }
    if by_path != expected_helper_hashes:
        raise PortablePackageError("bundled helpers do not match the pinned BtbN release bytes")
    expected_version = PINS["ffmpeg"]["version"]
    for record in sbom.get("executables", []):
        if (
            record.get("version") != expected_version
            or record.get("source_commit") != PINS["ffmpeg"]["commit"]
            or record.get("license") != "GPL-3.0-or-later"
        ):
            raise PortablePackageError("bundled helper identity does not match canonical pins")
    if sbom.get("libraries") != []:
        raise PortablePackageError(
            "pinned portable media profile must not contain bundled shared-library payloads"
        )
    expected_files = set(by_path) | {
        "licenses/FFmpeg-COPYING.GPLv3.txt",
        "licenses/BtbN-FFmpeg-Builds-LICENSE.txt",
        "licenses/README-CORRESPONDING-SOURCE.txt",
        "provenance/SHA256SUMS.txt",
        "provenance/media-sbom.json",
    }
    expected_files.update(
        f"licenses/components/{relative}"
        for relative in component_license_info["file_hashes"]
    )
    actual_files: set[str] = set()
    for path in root.rglob("*"):
        if path.is_symlink():
            raise PortablePackageError(f"bundled media root contains a symlink: {path}")
        if path.is_file():
            actual_files.add(path.relative_to(root).as_posix())
    if actual_files != expected_files:
        raise PortablePackageError("bundled media root file inventory is incomplete or stale")
    return component_license_info


def read_tar_member_bounded(
    archive: tarfile.TarFile,
    member: tarfile.TarInfo,
    label: str,
) -> bytes:
    if member.size <= 0 or member.size > MAX_MANIFEST_BYTES:
        raise PortablePackageError(f"{label} is empty or exceeds the size limit")
    handle = archive.extractfile(member)
    if handle is None:
        raise PortablePackageError(f"could not read {label}")
    data = handle.read(MAX_MANIFEST_BYTES + 1)
    if len(data) != member.size or len(data) > MAX_MANIFEST_BYTES:
        raise PortablePackageError(f"{label} changed size while reading")
    return data


def sha256_tar_member(archive: tarfile.TarFile, member: tarfile.TarInfo) -> str:
    handle = archive.extractfile(member)
    if handle is None:
        raise PortablePackageError(f"could not read source member {member.name}")
    digest = hashlib.sha256()
    read_bytes = 0
    for chunk in iter(lambda: handle.read(1024 * 1024), b""):
        digest.update(chunk)
        read_bytes += len(chunk)
    if read_bytes != member.size:
        raise PortablePackageError(f"source member changed size while reading: {member.name}")
    return digest.hexdigest()


def validate_source_archive(path: Path) -> dict[str, Any]:
    require_file(path, "bundled media corresponding-source archive")
    if not path.name.endswith(".tar.xz"):
        raise PortablePackageError("bundled media source archive must be a .tar.xz file")
    try:
        with tarfile.open(path, "r:xz") as archive:
            members = archive.getmembers()
            files: dict[str, tarfile.TarInfo] = {}
            all_names: set[str] = set()
            root_name = "media-corresponding-source"
            for member in members:
                safe = safe_relative_path(member.name)
                normalized = safe.as_posix()
                if normalized in all_names:
                    raise PortablePackageError(f"duplicate source archive member: {normalized}")
                all_names.add(normalized)
                if safe.parts[0] != root_name:
                    raise PortablePackageError(
                        f"source archive contains payload outside {root_name}/: {normalized}"
                    )
                if member.issym() or member.islnk() or member.isdev() or not (
                    member.isfile() or member.isdir()
                ):
                    raise PortablePackageError(
                        f"source archive contains unsupported member type: {normalized}"
                    )
                if member.isfile():
                    if len(safe.parts) == 1:
                        raise PortablePackageError("source archive root must be a directory")
                    relative = PurePosixPath(*safe.parts[1:]).as_posix()
                    files[relative] = member

            if "SHA256SUMS.txt" not in files or "SOURCE-MANIFEST.json" not in files:
                raise PortablePackageError(
                    "source archive must contain root SHA256SUMS.txt and SOURCE-MANIFEST.json"
                )
            sums_data = read_tar_member_bounded(
                archive, files["SHA256SUMS.txt"], "source SHA256SUMS.txt"
            )
            manifest_data = read_tar_member_bounded(
                archive, files["SOURCE-MANIFEST.json"], "source manifest"
            )
            try:
                sums = parse_sha_manifest(sums_data.decode("utf-8"), "source SHA256SUMS.txt")
                source_manifest = json.loads(manifest_data.decode("utf-8"))
            except (UnicodeError, json.JSONDecodeError) as error:
                raise PortablePackageError(f"invalid source archive manifest: {error}") from error
            if not isinstance(source_manifest, dict):
                raise PortablePackageError("source manifest must be a JSON object")

            expected_files = set(files) - {"SHA256SUMS.txt"}
            if set(sums) != expected_files:
                raise PortablePackageError("source archive checksum inventory is incomplete or stale")
            actual_hashes: dict[str, str] = {}
            for relative, expected in sums.items():
                actual = sha256_tar_member(archive, files[relative])
                if actual != expected:
                    raise PortablePackageError(f"source archive hash mismatch for {relative}")
                actual_hashes[relative] = actual
            archived_docker_mounts: dict[str, list[str]] = {}
            for docker_name in PINS["generated_dockerfiles"]:
                docker_path = f"release-build-scripts/btbn-generated/{docker_name}"
                if docker_path not in files:
                    raise PortablePackageError(f"source archive lacks {docker_name}")
                docker_text = read_tar_member_bounded(
                    archive, files[docker_path], f"generated Dockerfile {docker_name}"
                ).decode("utf-8")
                parsed = sorted(set(CACHE_REFERENCE.findall(docker_text)))
                raw = set(re.findall(r"\.cache/downloads/([^,\"\s]+)", docker_text))
                if not parsed or raw != set(parsed):
                    raise PortablePackageError(
                        f"generated Dockerfile source mounts are not exact for {docker_name}"
                    )
                if any("fdk-aac" in item.lower() for item in parsed):
                    raise PortablePackageError(
                        f"nonfree-only fdk-aac entered GPL source inventory: {docker_name}"
                    )
                archived_docker_mounts[docker_name] = parsed
    except (OSError, tarfile.TarError) as error:
        raise PortablePackageError(f"invalid bundled media source archive: {error}") from error

    expected_repo = PINS["btbn_build_repository"]
    if (
        source_manifest.get("btbn_build_commit") != expected_repo["commit"]
        or source_manifest.get("btbn_release_tag") != expected_repo["release_tag"]
        or source_manifest.get("build_variant") != PINS["build_variant"]
        or source_manifest.get("ffmpeg_commit") != PINS["ffmpeg"]["commit"]
    ):
        raise PortablePackageError("source manifest does not match canonical pinned inputs")
    source_records = source_manifest.get("source_files")
    if not isinstance(source_records, list) or not source_records:
        raise PortablePackageError("source manifest has no source file inventory")
    recorded: dict[str, tuple[str, int]] = {}
    for record in source_records:
        if not isinstance(record, dict):
            raise PortablePackageError("source manifest file record is invalid")
        relative = record.get("path")
        digest = record.get("sha256")
        size = record.get("bytes")
        if (
            not isinstance(relative, str)
            or not isinstance(digest, str)
            or not isinstance(size, int)
            or size <= 0
        ):
            raise PortablePackageError("source manifest file record is incomplete")
        relative = safe_relative_path(relative).as_posix()
        if relative in recorded:
            raise PortablePackageError(f"duplicate source manifest path {relative}")
        recorded[relative] = (digest, size)
    expected_recorded = set(files) - {"SHA256SUMS.txt", "SOURCE-MANIFEST.json"}
    if set(recorded) != expected_recorded:
        raise PortablePackageError("source manifest file inventory is incomplete or stale")
    for relative, (digest, size) in recorded.items():
        if actual_hashes.get(relative) != digest or files[relative].size != size:
            raise PortablePackageError(f"source manifest record mismatch for {relative}")

    dockerfiles = source_manifest.get("generated_dockerfiles")
    if not isinstance(dockerfiles, list):
        raise PortablePackageError("source manifest lacks generated Dockerfile inventory")
    docker_by_name: dict[str, dict[str, Any]] = {}
    selected_mounts: set[str] = set()
    for record in dockerfiles:
        if not isinstance(record, dict) or not isinstance(record.get("name"), str):
            raise PortablePackageError("generated Dockerfile record is invalid")
        name = record["name"]
        if name in docker_by_name:
            raise PortablePackageError(f"duplicate generated Dockerfile record {name}")
        docker_by_name[name] = record
        mounts = record.get("source_mounts")
        if not isinstance(mounts, list) or not mounts or not all(
            isinstance(item, str) for item in mounts
        ):
            raise PortablePackageError(f"generated Dockerfile source mounts are invalid for {name}")
        if sorted(mounts) != archived_docker_mounts.get(name):
            raise PortablePackageError(
                f"generated Dockerfile source manifest disagrees with archived content for {name}"
            )
        selected_mounts.update(mounts)
        docker_path = f"release-build-scripts/btbn-generated/{name}"
        if (
            name not in PINS["generated_dockerfiles"]
            or record.get("sha256") != PINS["generated_dockerfiles"][name]
            or actual_hashes.get(docker_path) != record.get("sha256")
        ):
            raise PortablePackageError(f"generated Dockerfile hash mismatch for {name}")
    if set(docker_by_name) != set(PINS["generated_dockerfiles"]):
        raise PortablePackageError("generated Dockerfile set does not match canonical pins")

    build_source = (
        f"components/btbn-ffmpeg-builds/{expected_repo['commit']}/sources/"
        f"FFmpeg-Builds-{expected_repo['commit']}.tar.gz"
    )
    ffmpeg_source = (
        f"components/ffmpeg/{PINS['ffmpeg']['commit']}/sources/"
        f"FFmpeg-{PINS['ffmpeg']['commit']}.tar.gz"
    )
    if actual_hashes.get(build_source) != expected_repo["archive_sha256"]:
        raise PortablePackageError("pinned BtbN build source is missing or changed")
    if actual_hashes.get(ffmpeg_source) != PINS["ffmpeg"]["archive_sha256"]:
        raise PortablePackageError("pinned FFmpeg source is missing or changed")

    component_sources = {
        relative: digest
        for relative, digest in actual_hashes.items()
        if len(PurePosixPath(relative).parts) >= 4
        and PurePosixPath(relative).parts[0] == "components"
        and "sources" in PurePosixPath(relative).parts
    }
    dependency_prefix = (
        f"components/btbn-static-dependencies/{expected_repo['commit']}/sources/"
    )
    dependency_names = {
        PurePosixPath(relative).name
        for relative in component_sources
        if relative.startswith(dependency_prefix)
    }
    if dependency_names != selected_mounts:
        raise PortablePackageError(
            "corresponding-source dependency set does not match generated GPL build inputs"
        )
    required_families = (
        any(name.startswith("release-build-scripts/") for name in actual_hashes),
        bool(component_sources),
        any("licenses" in PurePosixPath(name).parts for name in actual_hashes),
    )
    if not all(required_families):
        raise PortablePackageError(
            "source archive lacks build scripts, component sources, or component licenses"
        )
    component_license_files = {
        relative.removeprefix("component-licenses/"): digest
        for relative, digest in actual_hashes.items()
        if relative.startswith("component-licenses/")
    }
    if "component-license-manifest.json" not in component_license_files:
        raise PortablePackageError("source archive lacks component license manifest")
    return {
        "component_sources": component_sources,
        "component_license_files": component_license_files,
    }


def copy_tree_exact(source: Path, destination: Path) -> None:
    if not source.is_dir():
        raise PortablePackageError(f"required directory does not exist: {source}")
    shutil.copytree(source, destination, copy_function=shutil.copy2)


def validate_media_source_pair(
    media_info: dict[str, Any], source_info: dict[str, Any]
) -> None:
    if media_info["source_hashes"] != source_info["component_sources"]:
        raise PortablePackageError(
            "component license/source inventory does not match corresponding source"
        )
    if media_info["file_hashes"] != source_info["component_license_files"]:
        raise PortablePackageError(
            "packaged component license evidence does not match corresponding source"
        )


def stage_package(args: argparse.Namespace) -> None:
    if args.stage_dir.exists():
        shutil.rmtree(args.stage_dir)
    args.stage_dir.mkdir(parents=True)

    check_binary_magic(args.executable, args.platform, "NeoFlash executable")
    require_file(args.runtime_dir / "neoflash_web.js", "web runtime JavaScript")
    require_file(args.runtime_dir / "neoflash_web_bg.wasm", "web runtime WebAssembly")
    validate_media_root(args.media_root, args.platform)
    validate_rust_licenses(args.rust_licenses)

    executable_name = "neoflash-editor.exe" if args.platform == "windows-x64" else "neoflash-editor"
    shutil.copy2(args.executable, args.stage_dir / executable_name)
    if args.platform == "linux-x64":
        (args.stage_dir / executable_name).chmod(0o755)
    copy_tree_exact(args.runtime_dir, args.stage_dir / "runtime")
    copy_tree_exact(args.media_root, args.stage_dir / "media")
    (args.stage_dir / "licenses" / "neoflash").mkdir(parents=True)
    for name in ("LICENSE-MIT", "LICENSE-APACHE"):
        shutil.copy2(args.project_root / name, args.stage_dir / "licenses" / "neoflash" / name)
    copy_tree_exact(args.rust_licenses, args.stage_dir / "licenses" / "rust")
    shutil.copy2(
        args.project_root / "THIRD_PARTY_NOTICES.txt",
        args.stage_dir / "THIRD_PARTY_NOTICES.txt",
    )

    metadata = {
        "build_number": args.build_number,
        "channel": f"{args.platform}-technical-preview",
        "commit": args.commit,
        "name": "NeoFlash",
        "profile": args.profile,
        "target": (
            "x86_64-pc-windows-msvc"
            if args.platform == "windows-x64"
            else "x86_64-unknown-linux-gnu"
        ),
        "version": args.version,
    }
    (args.stage_dir / "BUILD_METADATA.json").write_text(
        canonical_json(metadata), encoding="utf-8", newline="\n"
    )
    notice = f"""NeoFlash {args.version} — Licenses and bundled-media source
========================================================

NeoFlash is free and open-source software licensed under MIT OR Apache-2.0.
The complete NeoFlash license texts are in licenses/neoflash/.

Third-party notices are in THIRD_PARTY_NOTICES.txt. Exact Rust dependency
license records and evidence are in licenses/rust/. Bundled FFmpeg/FFprobe
notices, provenance and hashes are in media/licenses/ and media/provenance/.

The complete, hash-verified corresponding source for the shipped media helpers
is distributed beside this archive as:

  {args.source_output.name}

Verify both artifacts with {args.checksum_file.name}.
"""
    (args.stage_dir / "LICENSES-AND-SOURCE.txt").write_text(
        notice, encoding="utf-8", newline="\n"
    )
    if args.platform == "windows-x64":
        run_guide = f"""NeoFlash {args.version} — Windows x64 Technical Preview
============================================================

Run neoflash-editor.exe directly; no installer is required. This preview ZIP
is unsigned unless its checksum set also lists a signed MSIX, so Windows
SmartScreen may ask you to confirm that you trust the file.

Ordinary Test Movie works. The private-pipe interactive script debugger is not
available in this first Windows preview and reports that boundary explicitly.
The compatible build is produced on Windows Server 2022; final acceptance on
an actual Windows 11 x64 machine remains part of the preview test.

All application and media license texts are already included. The complete
media corresponding-source archive and {args.checksum_file.name} are supplied
beside this ZIP; users do not need to search for licenses themselves.
"""
    else:
        run_guide = f"""NeoFlash {args.version} — Linux x64 Technical Preview
==========================================================

Run ./neoflash-editor from this directory. The executable bit is preserved by
the tar.gz archive. This preview is built and tested on Ubuntu 24.04 x64 with
X11 and Wayland, using Vulkan first and GLES as the fallback. It is not an
AppImage or a deb package.

The editor uses normal system runtime libraries, including glibc, the desktop
X11/Wayland stack and ALSA. Bundled FFmpeg/FFprobe use the BtbN
static-dependency GPL profile but still require the normal glibc system
runtime; the package does not claim fully static Linux executables.

All application and media license texts are already included. The complete
media corresponding-source archive and {args.checksum_file.name} are supplied
beside this tarball; users do not need to search for licenses themselves.
"""
    (args.stage_dir / "README.txt").write_text(
        run_guide, encoding="utf-8", newline="\n"
    )


def iter_stage_files(stage_dir: Path) -> Iterable[Path]:
    return sorted(
        (
            path
            for path in stage_dir.rglob("*")
            if path.is_file() and path.name != "PACKAGE-SHA256SUMS.txt"
        ),
        key=lambda path: path.relative_to(stage_dir).as_posix(),
    )


def validate_staged_package(stage_dir: Path, platform: str) -> dict[str, Any]:
    executable_name = "neoflash-editor.exe" if platform == "windows-x64" else "neoflash-editor"
    check_binary_magic(stage_dir / executable_name, platform, "staged NeoFlash executable")
    require_file(stage_dir / "runtime" / "neoflash_web.js", "staged runtime JavaScript")
    require_file(stage_dir / "runtime" / "neoflash_web_bg.wasm", "staged runtime WebAssembly")
    media_info = validate_media_root(stage_dir / "media", platform)
    validate_rust_licenses(stage_dir / "licenses" / "rust")
    require_file(stage_dir / "LICENSES-AND-SOURCE.txt", "license/source notice")
    require_file(stage_dir / "README.txt", "platform run guide")
    require_file(stage_dir / "BUILD_METADATA.json", "build metadata")
    return media_info


def write_internal_manifest(stage_dir: Path) -> None:
    lines = [
        f"{sha256_file(path)}  {path.relative_to(stage_dir).as_posix()}"
        for path in iter_stage_files(stage_dir)
    ]
    if not lines:
        raise PortablePackageError("portable package staging directory is empty")
    (stage_dir / "PACKAGE-SHA256SUMS.txt").write_text(
        "\n".join(lines) + "\n", encoding="utf-8", newline="\n"
    )


def deterministic_zip(stage_dir: Path, archive_path: Path, root_name: str) -> None:
    root_name = safe_archive_root(root_name)
    archive_path.parent.mkdir(parents=True, exist_ok=True)
    temp = archive_path.with_suffix(archive_path.suffix + ".tmp")
    if temp.exists():
        temp.unlink()
    with zipfile.ZipFile(temp, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for path in sorted(stage_dir.rglob("*"), key=lambda item: item.relative_to(stage_dir).as_posix()):
            if not path.is_file():
                continue
            relative = path.relative_to(stage_dir).as_posix()
            info = zipfile.ZipInfo(f"{root_name}/{relative}", date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            mode = 0o755 if os.access(path, os.X_OK) else 0o644
            info.external_attr = (stat.S_IFREG | mode) << 16
            info.create_system = 3
            with path.open("rb") as source, archive.open(info, "w", force_zip64=True) as output:
                shutil.copyfileobj(source, output, length=1024 * 1024)
    temp.replace(archive_path)


def deterministic_tar_gz(stage_dir: Path, archive_path: Path, root_name: str) -> None:
    root_name = safe_archive_root(root_name)
    archive_path.parent.mkdir(parents=True, exist_ok=True)
    temp = archive_path.with_suffix(archive_path.suffix + ".tmp")
    if temp.exists():
        temp.unlink()
    with temp.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                for path in sorted(stage_dir.rglob("*"), key=lambda item: item.relative_to(stage_dir).as_posix()):
                    relative = path.relative_to(stage_dir).as_posix()
                    name = f"{root_name}/{relative}"
                    info = archive.gettarinfo(str(path), arcname=name)
                    info.uid = 0
                    info.gid = 0
                    info.uname = "root"
                    info.gname = "root"
                    info.mtime = DETERMINISTIC_MTIME
                    if info.isfile():
                        info.mode = 0o755 if os.access(path, os.X_OK) else 0o644
                        with path.open("rb") as handle:
                            archive.addfile(info, handle)
                    elif info.isdir():
                        info.mode = 0o755
                        archive.addfile(info)
                    else:
                        raise PortablePackageError(f"unsupported staged file type: {path}")
    temp.replace(archive_path)


def finalize_package(args: argparse.Namespace) -> None:
    media_info = validate_staged_package(args.stage_dir, args.platform)
    source_info = validate_source_archive(args.media_source_archive)
    validate_media_source_pair(media_info, source_info)
    write_internal_manifest(args.stage_dir)

    if args.archive.suffix == ".zip":
        deterministic_zip(args.stage_dir, args.archive, args.package_name)
    elif args.archive.name.endswith(".tar.gz"):
        deterministic_tar_gz(args.stage_dir, args.archive, args.package_name)
    else:
        raise PortablePackageError("portable archive must end in .zip or .tar.gz")

    args.source_output.parent.mkdir(parents=True, exist_ok=True)
    source_temp = args.source_output.with_suffix(args.source_output.suffix + ".tmp")
    source_temp.unlink(missing_ok=True)
    try:
        os.link(args.media_source_archive, source_temp)
    except OSError:
        shutil.copyfile(args.media_source_archive, source_temp)
    source_temp.replace(args.source_output)
    lines = [
        f"{sha256_file(args.archive)}  {args.archive.name}",
        f"{sha256_file(args.source_output)}  {args.source_output.name}",
    ]
    args.checksum_file.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("stage", "finalize", "all"))
    parser.add_argument("--platform", required=True, choices=("windows-x64", "linux-x64"))
    parser.add_argument("--project-root", required=True, type=Path)
    parser.add_argument("--version", default="0.1.0-beta.1")
    parser.add_argument("--profile", choices=("release", "debug"), default="release")
    parser.add_argument("--commit", required=True)
    parser.add_argument("--build-number", required=True, type=int)
    parser.add_argument("--executable", required=True, type=Path)
    parser.add_argument("--runtime-dir", required=True, type=Path)
    parser.add_argument("--media-root", required=True, type=Path)
    parser.add_argument("--media-source-archive", required=True, type=Path)
    parser.add_argument("--rust-licenses", required=True, type=Path)
    parser.add_argument("--stage-dir", required=True, type=Path)
    parser.add_argument("--package-name", required=True)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--source-output", required=True, type=Path)
    parser.add_argument("--checksum-file", required=True, type=Path)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    for field in (
        "project_root",
        "executable",
        "runtime_dir",
        "media_root",
        "media_source_archive",
        "rust_licenses",
        "stage_dir",
        "archive",
        "source_output",
        "checksum_file",
    ):
        setattr(args, field, getattr(args, field).resolve())
    try:
        safe_archive_root(args.package_name)
        require_output_child(args.stage_dir, args.project_root / "target", "staging directory")
        require_output_child(args.archive, args.project_root / "dist", "portable archive")
        require_output_child(args.source_output, args.project_root / "dist", "source archive")
        require_output_child(args.checksum_file, args.project_root / "dist", "checksum file")
        if args.action in ("stage", "all"):
            stage_package(args)
        if args.action in ("finalize", "all"):
            finalize_package(args)
    except (OSError, PortablePackageError, json.JSONDecodeError) as error:
        print(f"Portable package refused: {error}", file=os.sys.stderr)
        return 1
    print(args.archive if args.action != "stage" else args.stage_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
