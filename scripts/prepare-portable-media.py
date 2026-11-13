#!/usr/bin/env python3
"""Prepare pinned FFmpeg media payloads for Windows/Linux previews.

FFmpeg's official download page links BtbN's builds for Windows and Linux.
NeoFlash pins a retained monthly BtbN release, its build-repository commit, the
exact FFmpeg commit and every downloaded byte.  No floating ``latest`` asset is
accepted.  ``binary`` stages the two helpers and their provenance.  ``source``
assembles the corresponding-source archive after BtbN's exact ``download.sh``
has populated its content-addressed dependency cache.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import lzma
import os
import re
import shutil
import stat
import tarfile
import tempfile
import urllib.error
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath


PINS_PATH = Path(__file__).with_name("portable-media-pins.json")
PINS = json.loads(PINS_PATH.read_text(encoding="utf-8"))
PROJECT_ROOT = Path(__file__).resolve().parent.parent
BTBN_TAG = PINS["btbn_build_repository"]["release_tag"]
BTBN_COMMIT = PINS["btbn_build_repository"]["commit"]
BTBN_ARCHIVE_NAME = f"FFmpeg-Builds-{BTBN_COMMIT}.tar.gz"
BTBN_ARCHIVE_URL = f"https://github.com/BtbN/FFmpeg-Builds/archive/{BTBN_COMMIT}.tar.gz"
BTBN_ARCHIVE_SHA256 = PINS["btbn_build_repository"]["archive_sha256"]
FFMPEG_COMMIT = PINS["ffmpeg"]["commit"]
FFMPEG_ARCHIVE_NAME = f"FFmpeg-{FFMPEG_COMMIT}.tar.gz"
FFMPEG_ARCHIVE_URL = f"https://github.com/FFmpeg/FFmpeg/archive/{FFMPEG_COMMIT}.tar.gz"
FFMPEG_ARCHIVE_SHA256 = PINS["ffmpeg"]["archive_sha256"]
ASSETS = PINS["assets"]
DETERMINISTIC_MTIME = 315_532_800
LICENSE_BASENAME = re.compile(
    r"^(?:licen[cs]e|copying|copyright|notice)(?:$|[._-].*)", re.IGNORECASE
)
LICENSE_DIRECTORY = re.compile(
    r"^(?:licen[cs]es?|copying|copyright|notices?)$", re.IGNORECASE
)
MAX_LICENSE_BYTES = 4 * 1024 * 1024
MAX_LICENSE_FILES_PER_COMPONENT = 256
MAX_LICENSE_TOTAL_BYTES_PER_COMPONENT = 64 * 1024 * 1024
CACHE_REFERENCE = re.compile(
    r"\.cache/downloads/([A-Za-z0-9_.+-]+_[0-9a-f]{64}\.tar\.xz)"
)
CACHE_ENTRY = re.compile(
    r"^[A-Za-z0-9_.+-]+(?:_[0-9a-f]{64})?\.tar\.xz$"
)
CACHE_STAGE = re.compile(r"^([A-Za-z0-9_.+-]+)_[0-9a-f]{64}\.tar\.xz$")
SCRIPT_SOURCE_REFERENCE = re.compile(
    r"(?:^|[;&(]\s*)(?:source|\.)\s+[\"']?(scripts\.d/[^\"'\s;)]+\.sh)",
    re.MULTILINE,
)


class MediaPreparationError(RuntimeError):
    """Pinned media inputs are absent, changed or incomplete."""


def require_target_output(path: Path, label: str) -> Path:
    resolved = path.resolve()
    target_root = (PROJECT_ROOT / "target").resolve()
    if (
        resolved == target_root
        or not resolved.is_relative_to(target_root)
        or ".git" in resolved.parts
    ):
        raise MediaPreparationError(
            f"refusing unsafe {label}; output must be a child of {target_root}: {resolved}"
        )
    return resolved


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def validate_component_license_bundle(root: Path) -> None:
    manifest_path = root / "component-license-manifest.json"
    if not manifest_path.is_file():
        raise MediaPreparationError("component license bundle manifest is missing")
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise MediaPreparationError(f"component license manifest is invalid: {error}") from error
    if (
        not isinstance(manifest, dict)
        or manifest.get("btbn_build_commit") != BTBN_COMMIT
        or manifest.get("ffmpeg_commit") != FFMPEG_COMMIT
        or manifest.get("build_variant") != PINS["build_variant"]
        or not isinstance(manifest.get("components"), list)
        or not manifest["components"]
    ):
        raise MediaPreparationError("component license manifest does not match pinned inputs")
    expected_files = {"component-license-manifest.json"}
    for component in manifest["components"]:
        if (
            not isinstance(component, dict)
            or not isinstance(component.get("license_evidence"), list)
            or not component["license_evidence"]
        ):
            raise MediaPreparationError("component license manifest entry is incomplete")
        for evidence in component["license_evidence"]:
            if not isinstance(evidence, dict) or not isinstance(evidence.get("path"), str):
                raise MediaPreparationError("component license evidence entry is incomplete")
            relative = safe_member(evidence["path"]).as_posix()
            if relative in expected_files:
                raise MediaPreparationError(f"duplicate component license evidence {relative}")
            expected_files.add(relative)
            path = root / relative
            if (
                not path.is_file()
                or path.stat().st_size != evidence.get("bytes")
                or sha256_file(path) != evidence.get("sha256")
            ):
                raise MediaPreparationError(f"component license evidence mismatch: {relative}")
    actual_files = {
        path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file()
    }
    if actual_files != expected_files:
        raise MediaPreparationError("component license bundle contains unrecorded or missing files")


def canonical_json(value: object) -> str:
    return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def fetch_pinned(url: str, expected: str, expected_size: int, destination: Path) -> Path:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if (
        destination.is_file()
        and destination.stat().st_size == expected_size
        and sha256_file(destination) == expected
    ):
        return destination
    destination.unlink(missing_ok=True)
    temporary = destination.with_suffix(destination.suffix + ".part")
    temporary.unlink(missing_ok=True)
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "NeoFlash-T289/1"})
        with urllib.request.urlopen(request, timeout=60) as response, temporary.open("wb") as output:
            content_length = response.headers.get("Content-Length")
            if content_length is not None and int(content_length) != expected_size:
                raise MediaPreparationError(
                    f"pinned input size header mismatch for {url}; expected {expected_size}, got {content_length}"
                )
            received = 0
            while True:
                chunk = response.read(min(1024 * 1024, expected_size + 1 - received))
                if not chunk:
                    break
                received += len(chunk)
                if received > expected_size:
                    raise MediaPreparationError(f"pinned input exceeds size limit for {url}")
                output.write(chunk)
    except (OSError, urllib.error.URLError, MediaPreparationError) as error:
        temporary.unlink(missing_ok=True)
        raise MediaPreparationError(f"could not download pinned input {url}: {error}") from error
    actual = sha256_file(temporary)
    actual_size = temporary.stat().st_size
    if actual_size != expected_size or actual != expected:
        temporary.unlink(missing_ok=True)
        raise MediaPreparationError(
            f"pinned input mismatch for {url}; expected {expected_size} bytes/{expected}, "
            f"got {actual_size} bytes/{actual}"
        )
    temporary.replace(destination)
    return destination


def asset_url(name: str) -> str:
    return f"https://github.com/BtbN/FFmpeg-Builds/releases/download/{BTBN_TAG}/{name}"


def safe_member(name: str) -> PurePosixPath:
    path = PurePosixPath(name.replace("\\", "/"))
    if path.is_absolute() or ".." in path.parts or not path.parts:
        raise MediaPreparationError(f"unsafe archive member {name!r}")
    return path


def read_asset_members(asset: Path) -> dict[str, bytes]:
    members: dict[str, bytes] = {}
    if asset.suffix == ".zip":
        with zipfile.ZipFile(asset) as archive:
            for info in archive.infolist():
                path = safe_member(info.filename)
                if info.is_dir():
                    continue
                leaf = path.name
                if leaf in ("ffmpeg.exe", "ffprobe.exe", "LICENSE.txt"):
                    members[leaf] = archive.read(info)
    elif asset.name.endswith(".tar.xz"):
        with tarfile.open(asset, "r:xz") as archive:
            for member in archive.getmembers():
                path = safe_member(member.name)
                if not member.isfile():
                    continue
                leaf = path.name
                if leaf in ("ffmpeg", "ffprobe", "LICENSE.txt"):
                    handle = archive.extractfile(member)
                    if handle is not None:
                        members[leaf] = handle.read()
    else:
        raise MediaPreparationError(f"unsupported BtbN asset format: {asset}")
    return members


def extract_named_member(archive_path: Path, suffix: str) -> bytes:
    with tarfile.open(archive_path, "r:*") as archive:
        matches = []
        for member in archive.getmembers():
            safe_member(member.name)
            if member.isfile() and member.name.endswith(suffix):
                matches.append(member)
        if len(matches) != 1:
            raise MediaPreparationError(
                f"expected one {suffix!r} in {archive_path.name}, found {len(matches)}"
            )
        handle = archive.extractfile(matches[0])
        if handle is None:
            raise MediaPreparationError(f"could not read {suffix!r} from {archive_path}")
        return handle.read()


def prepare_build_repository(cache: Path, destination: Path) -> None:
    require_target_output(cache, "media cache")
    require_target_output(destination, "BtbN build-repository destination")
    archive_path = fetch_pinned(
        BTBN_ARCHIVE_URL,
        BTBN_ARCHIVE_SHA256,
        int(PINS["btbn_build_repository"]["archive_bytes"]),
        cache / BTBN_ARCHIVE_NAME,
    )
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True)
    with tarfile.open(archive_path, "r:gz") as archive:
        for member in archive.getmembers():
            safe = safe_member(member.name)
            if len(safe.parts) == 1:
                if not member.isdir():
                    raise MediaPreparationError("BtbN archive root is not a directory")
                continue
            relative = PurePosixPath(*safe.parts[1:])
            output = destination.joinpath(*relative.parts)
            if member.isdir():
                output.mkdir(parents=True, exist_ok=True)
                continue
            if not member.isfile():
                raise MediaPreparationError(
                    f"BtbN build archive contains unsupported member {member.name}"
                )
            output.parent.mkdir(parents=True, exist_ok=True)
            handle = archive.extractfile(member)
            if handle is None:
                raise MediaPreparationError(f"could not read {member.name}")
            with output.open("wb") as target:
                shutil.copyfileobj(handle, target, length=1024 * 1024)
            output.chmod(member.mode & 0o777)


def prepare_binary(
    platform: str,
    cache: Path,
    destination: Path,
    component_licenses: Path,
) -> None:
    require_target_output(cache, "media cache")
    require_target_output(destination, "portable media destination")
    spec = ASSETS[platform]
    name = str(spec["name"])
    asset = fetch_pinned(
        asset_url(name), str(spec["sha256"]), int(spec["bytes"]), cache / name
    )
    build_archive = fetch_pinned(
        BTBN_ARCHIVE_URL,
        BTBN_ARCHIVE_SHA256,
        int(PINS["btbn_build_repository"]["archive_bytes"]),
        cache / BTBN_ARCHIVE_NAME,
    )
    members = read_asset_members(asset)
    required = (str(spec["ffmpeg"]), str(spec["ffprobe"]), "LICENSE.txt")
    absent = [name for name in required if not members.get(name)]
    if absent:
        raise MediaPreparationError(f"BtbN asset is missing required files: {absent}")

    if destination.exists():
        shutil.rmtree(destination)
    bin_dir = destination / "bin"
    licenses = destination / "licenses"
    provenance = destination / "provenance"
    bin_dir.mkdir(parents=True)
    licenses.mkdir(parents=True)
    provenance.mkdir(parents=True)

    records = []
    for tool in (str(spec["ffmpeg"]), str(spec["ffprobe"])):
        path = bin_dir / tool
        path.write_bytes(members[tool])
        path.chmod(0o755)
        expected_tool_hash = str(
            spec["ffmpeg_sha256"] if Path(tool).stem == "ffmpeg" else spec["ffprobe_sha256"]
        )
        actual_tool_hash = sha256_file(path)
        if actual_tool_hash != expected_tool_hash:
            raise MediaPreparationError(
                f"pinned {tool} hash mismatch; expected {expected_tool_hash}, got {actual_tool_hash}"
            )
        records.append(
            {
                "license": "GPL-3.0-or-later",
                "name": Path(tool).stem,
                "relative_path": f"bin/{tool}",
                "sha256": actual_tool_hash,
                "source_commit": FFMPEG_COMMIT,
                "version": PINS["ffmpeg"]["version"],
            }
        )
    ffmpeg_license = members["LICENSE.txt"]
    btbn_license = extract_named_member(build_archive, "/LICENSE")
    if hashlib.sha256(ffmpeg_license).hexdigest() != PINS["ffmpeg"]["license_sha256"]:
        raise MediaPreparationError("pinned FFmpeg license text changed")
    if (
        hashlib.sha256(btbn_license).hexdigest()
        != PINS["btbn_build_repository"]["license_sha256"]
    ):
        raise MediaPreparationError("pinned BtbN license text changed")
    (licenses / "FFmpeg-COPYING.GPLv3.txt").write_bytes(ffmpeg_license)
    (licenses / "BtbN-FFmpeg-Builds-LICENSE.txt").write_bytes(btbn_license)
    validate_component_license_bundle(component_licenses)
    shutil.copytree(component_licenses, licenses / "components")
    notice = f"""NeoFlash portable media helper notice
=====================================

This directory contains ffmpeg and ffprobe programs from the retained
BtbN FFmpeg-Builds monthly release {BTBN_TAG}. FFmpeg's official download page
links BtbN as a provider of Windows builds and 64-bit Linux static builds.

The BtbN static-dependency profile links codec libraries into the helpers. The
Linux ELF still uses the normal glibc/system runtime (GLIBC 2.28 or newer); it
is not claimed to be a fully static executable.

Binary asset: {name}
Binary SHA-256: {spec['sha256']}
BtbN build repository commit: {BTBN_COMMIT}
FFmpeg source commit: {FFMPEG_COMMIT}
License: GPL-3.0-or-later (complete text is beside this notice).

The exact BtbN build scripts, FFmpeg source and dependency sources are shipped
in the separately checksummed NeoFlash platform media corresponding-source
archive. No nonfree NeoFlash media profile is used.
"""
    (licenses / "README-CORRESPONDING-SOURCE.txt").write_text(
        notice, encoding="utf-8", newline="\n"
    )
    sbom = {
        "aggregation_boundary": (
            "Separate subprocess executables from BtbN's static-dependency profile, invoked "
            "through standard command-line pipes; "
            "NeoFlash does not link against FFmpeg."
        ),
        "binary_asset": {
            "name": name,
            "sha256": spec["sha256"],
            "url": asset_url(name),
        },
        "build_repository": {
            "commit": BTBN_COMMIT,
            "sha256": BTBN_ARCHIVE_SHA256,
            "url": BTBN_ARCHIVE_URL,
        },
        "component": "neoflash-portable-media",
        "build_variant": PINS["build_variant"],
        "executables": records,
        "ffmpeg_commit": FFMPEG_COMMIT,
        "helper_license": "GPL-3.0-or-later",
        "libraries": [],
        "platform": platform,
        "schema_version": 1,
    }
    (provenance / "media-sbom.json").write_text(
        canonical_json(sbom), encoding="utf-8", newline="\n"
    )
    sums = [f"{record['sha256']}  {record['relative_path']}" for record in records]
    (provenance / "SHA256SUMS.txt").write_text(
        "\n".join(sums) + "\n", encoding="utf-8", newline="\n"
    )


def deterministic_source_archive(root: Path, destination: Path) -> None:
    temp = destination.with_suffix(destination.suffix + ".tmp")
    temp.unlink(missing_ok=True)
    with temp.open("wb") as raw:
        # The payload is mostly already-compressed .tar.xz/.tar.gz input. A low
        # deterministic preset avoids hours of redundant single-threaded work.
        with lzma.LZMAFile(raw, mode="wb", preset=1) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
                    relative = path.relative_to(root).as_posix()
                    info = archive.gettarinfo(str(path), arcname=f"media-corresponding-source/{relative}")
                    info.uid = 0
                    info.gid = 0
                    info.uname = "root"
                    info.gname = "root"
                    info.mtime = DETERMINISTIC_MTIME
                    if info.isfile():
                        info.mode = 0o644
                        with path.open("rb") as handle:
                            archive.addfile(info, handle)
                    elif info.isdir():
                        info.mode = 0o755
                        archive.addfile(info)
                    else:
                        raise MediaPreparationError(f"unsupported source input type: {path}")
    temp.replace(destination)


def pinned_dockerfile_sources(
    dockerfiles: list[Path],
) -> tuple[set[str], list[dict[str, object]]]:
    expected_dockerfiles = set(PINS["generated_dockerfiles"])
    actual_dockerfiles = {path.name for path in dockerfiles}
    if actual_dockerfiles != expected_dockerfiles:
        raise MediaPreparationError(
            "generated BtbN Dockerfile set does not match the pinned Windows/Linux GPL profile; "
            f"expected {sorted(expected_dockerfiles)}, got {sorted(actual_dockerfiles)}"
        )
    selected_names: set[str] = set()
    dockerfile_records: list[dict[str, object]] = []
    for dockerfile in sorted(dockerfiles, key=lambda path: path.name):
        if not dockerfile.is_file():
            raise MediaPreparationError(f"generated BtbN Dockerfile is missing: {dockerfile}")
        data = dockerfile.read_bytes()
        actual_dockerfile_hash = hashlib.sha256(data).hexdigest()
        if actual_dockerfile_hash != PINS["generated_dockerfiles"][dockerfile.name]:
            raise MediaPreparationError(
                f"generated Dockerfile hash mismatch for pinned profile: {dockerfile}"
            )
        text = data.decode("utf-8")
        expected_target = dockerfile.name.split("-gpl-9.0.Dockerfile", 1)[0]
        expected_env = (
            f"ENV TARGET={expected_target} VARIANT=gpl REPO=btbn/ffmpeg-builds "
            "ADDINS_STR=9.0 FFVER=900"
        )
        if text.count(expected_env) != 1:
            raise MediaPreparationError(
                f"generated Dockerfile does not identify pinned {expected_target}/gpl/9.0: {dockerfile}"
            )
        references = sorted(set(CACHE_REFERENCE.findall(text)))
        raw_mounts = set(re.findall(r"\.cache/downloads/([^,\"\s]+)", text))
        if raw_mounts != set(references):
            raise MediaPreparationError(
                f"generated Dockerfile has unparsed or non-content-addressed source mounts: {dockerfile}"
            )
        if any("fdk-aac" in reference.lower() for reference in references):
            raise MediaPreparationError(
                f"nonfree-only fdk-aac unexpectedly entered GPL source inventory: {dockerfile}"
            )
        if not references:
            raise MediaPreparationError(
                f"generated BtbN Dockerfile has no content-addressed source mounts: {dockerfile}"
            )
        selected_names.update(references)
        dockerfile_records.append(
            {
                "name": dockerfile.name,
                "sha256": actual_dockerfile_hash,
                "source_mounts": references,
            }
        )
    return selected_names, dockerfile_records


def selected_cache_sources(
    dockerfiles: list[Path], dependency_downloads: Path
) -> tuple[list[Path], list[dict[str, object]]]:
    selected_names, dockerfile_records = pinned_dockerfile_sources(dockerfiles)
    selected: list[Path] = []
    for name in sorted(selected_names):
        source = dependency_downloads / name
        if source.is_symlink() or not source.is_file() or not source.stat().st_size:
            raise MediaPreparationError(
                f"required content-addressed BtbN source is absent or an alias: {source}"
            )
        selected.append(source)
    return selected, dockerfile_records


def filter_download_stages(repository: Path, dockerfiles: list[Path]) -> tuple[int, int]:
    """Disable upstream download stages absent from the two pinned Dockerfiles.

    BtbN's ``download.sh`` intentionally downloads every stage. The portable
    source kit needs only the exact content-addressed mounts selected by the
    generated Windows/Linux GPL Dockerfiles, so an ephemeral checkout under
    ``target/`` is narrowed before invoking the unchanged upstream script.
    Disabled files are renamed rather than deleted.
    """

    repository = require_target_output(repository, "BtbN download-stage repository")
    scripts_root = repository / "scripts.d"
    if not (repository / "download.sh").is_file() or not scripts_root.is_dir():
        raise MediaPreparationError(
            f"BtbN download-stage repository is incomplete: {repository}"
        )
    selected_names, _ = pinned_dockerfile_sources(dockerfiles)
    selected_stages: set[str] = set()
    for name in selected_names:
        match = CACHE_STAGE.fullmatch(name)
        if match is None:
            raise MediaPreparationError(f"invalid content-addressed cache name: {name}")
        selected_stages.add(match.group(1))

    scripts = sorted(scripts_root.rglob("*.sh"))
    by_stem: dict[str, list[Path]] = {}
    for script in scripts:
        by_stem.setdefault(script.stem, []).append(script)
    enabled: set[Path] = set()
    for stage in sorted(selected_stages):
        matches = by_stem.get(stage, [])
        if len(matches) != 1:
            raise MediaPreparationError(
                f"expected one upstream download stage for {stage!r}, found {len(matches)}"
            )
        enabled.add(matches[0])

    pending = list(enabled)
    while pending:
        script = pending.pop()
        text = script.read_text(encoding="utf-8")
        for reference in SCRIPT_SOURCE_REFERENCE.findall(text):
            referenced = repository / PurePosixPath(reference)
            if not referenced.is_file() or not referenced.resolve().is_relative_to(scripts_root):
                raise MediaPreparationError(
                    f"selected download stage has unsafe or missing source reference: {reference}"
                )
            if referenced not in enabled:
                enabled.add(referenced)
                pending.append(referenced)

    disabled = 0
    for script in scripts:
        if script in enabled:
            continue
        output = script.with_name(script.name + ".neoflash-disabled")
        if output.exists():
            raise MediaPreparationError(f"download-stage backup already exists: {output}")
        script.rename(output)
        disabled += 1
    remaining = set(scripts_root.rglob("*.sh"))
    if remaining != enabled:
        raise MediaPreparationError("BtbN selected download-stage filter is incomplete")
    return len(enabled), disabled


def extract_component_licenses(
    source_archive: Path,
    component_name: str,
    license_root: Path,
) -> list[dict[str, object]]:
    component_slug = re.sub(r"[^A-Za-z0-9._+-]+", "_", component_name)[:32]
    component_slug += "-" + hashlib.sha256(component_name.encode()).hexdigest()[:12]
    destination = license_root / "evidence" / component_slug
    destination.mkdir(parents=True, exist_ok=True)
    candidates: dict[str, tuple[str, bytes]] = {}
    unique_bytes = 0

    def record_candidate(source_path: str, data: bytes) -> None:
        nonlocal unique_bytes
        digest = hashlib.sha256(data).hexdigest()
        if digest in candidates:
            return
        if len(candidates) >= MAX_LICENSE_FILES_PER_COMPONENT:
            raise MediaPreparationError(
                f"too many unique license evidence files in {source_archive.name}"
            )
        unique_bytes += len(data)
        if unique_bytes > MAX_LICENSE_TOTAL_BYTES_PER_COMPONENT:
            raise MediaPreparationError(
                f"unique license evidence exceeds size limit in {source_archive.name}"
            )
        candidates[digest] = (source_path, data)

    try:
        with tarfile.open(source_archive, "r:*") as archive:
            members = archive.getmembers()
            for member in members:
                normalized_name = member.name.replace("\\", "/").rstrip("/")
                if normalized_name in ("", "."):
                    if member.isdir():
                        continue
                    raise MediaPreparationError(
                        f"source archive root is not a directory: {source_archive.name}"
                    )
                path = safe_member(member.name)
                in_license_directory = any(
                    LICENSE_DIRECTORY.match(part) for part in path.parts[:-1]
                )
                if not member.isfile() or not (
                    LICENSE_BASENAME.match(path.name) or in_license_directory
                ):
                    continue
                if member.size <= 0 or member.size > MAX_LICENSE_BYTES:
                    continue
                handle = archive.extractfile(member)
                if handle is None:
                    continue
                data = handle.read(MAX_LICENSE_BYTES + 1)
                if len(data) != member.size or len(data) > MAX_LICENSE_BYTES:
                    raise MediaPreparationError(
                        f"license evidence size changed while reading {member.name}"
                    )
                record_candidate(path.as_posix(), data)

            # Some source distributions (notably the pinned nv-codec-headers
            # inputs) put their complete MIT grant only in source-file header
            # comments. Preserve those exact bytes when no standalone licence
            # file exists instead of either omitting the component or treating
            # an unrelated README as legal evidence.
            if not candidates:
                for member in members:
                    normalized_name = member.name.replace("\\", "/").rstrip("/")
                    if normalized_name in ("", ".") or not member.isfile():
                        continue
                    path = safe_member(member.name)
                    if path.suffix.lower() not in {
                        ".c",
                        ".cc",
                        ".cpp",
                        ".h",
                        ".hh",
                        ".hpp",
                        ".m",
                        ".mm",
                        ".rs",
                    } or not (0 < member.size <= MAX_LICENSE_BYTES):
                        continue
                    handle = archive.extractfile(member)
                    if handle is None:
                        continue
                    data = handle.read(MAX_LICENSE_BYTES + 1)
                    if len(data) != member.size or len(data) > MAX_LICENSE_BYTES:
                        raise MediaPreparationError(
                            f"embedded licence evidence changed size while reading {member.name}"
                        )
                    grant = data.find(b"Permission is hereby granted, free of charge")
                    warranty = data.find(b'THE SOFTWARE IS PROVIDED "AS IS"', grant)
                    if grant < 0 or warranty < 0:
                        continue
                    start = data.rfind(b"/*", 0, grant)
                    end = data.find(b"*/", warranty)
                    if start < 0 or end < 0:
                        continue
                    record_candidate(path.as_posix(), data[start : end + 2])
    except tarfile.TarError as error:
        raise MediaPreparationError(
            f"could not inspect component source {source_archive}: {error}"
        ) from error
    if not candidates:
        raise MediaPreparationError(
            f"selected component source {source_archive.name} contains no LICENSE/COPYING/NOTICE evidence"
        )

    records: list[dict[str, object]] = []
    ordered_candidates = sorted(candidates.items(), key=lambda item: item[1][0])
    for index, (digest, (source_path, data)) in enumerate(ordered_candidates, 1):
        original_leaf = re.sub(
            r"[^A-Za-z0-9._+-]+", "_", PurePosixPath(source_path).name
        )
        suffix = "".join(PurePosixPath(original_leaf).suffixes)[-16:]
        stem_budget = max(8, 48 - len(suffix))
        bounded_leaf = PurePosixPath(original_leaf).stem[:stem_budget] + suffix
        path_tag = hashlib.sha256(source_path.encode()).hexdigest()[:10]
        output = destination / f"{index:03d}-{path_tag}-{bounded_leaf}"
        output.write_bytes(data)
        records.append(
            {
                "bytes": len(data),
                "path": output.relative_to(license_root).as_posix(),
                "sha256": digest,
                "source_path": source_path,
            }
        )
    return records


def prepare_source(
    project_root: Path,
    cache: Path,
    dependency_downloads: Path,
    dockerfiles: list[Path],
    license_output: Path,
    destination: Path,
    prune_unselected: bool,
) -> None:
    require_target_output(cache, "media cache")
    require_target_output(license_output, "component-license destination")
    require_target_output(destination, "corresponding-source archive")
    build_archive = fetch_pinned(
        BTBN_ARCHIVE_URL,
        BTBN_ARCHIVE_SHA256,
        int(PINS["btbn_build_repository"]["archive_bytes"]),
        cache / BTBN_ARCHIVE_NAME,
    )
    ffmpeg_archive = fetch_pinned(
        FFMPEG_ARCHIVE_URL,
        FFMPEG_ARCHIVE_SHA256,
        int(PINS["ffmpeg"]["archive_bytes"]),
        cache / FFMPEG_ARCHIVE_NAME,
    )
    dependency_sources, dockerfile_records = selected_cache_sources(
        dockerfiles, dependency_downloads
    )
    if prune_unselected:
        safe_cache_root = (project_root / "target").resolve()
        resolved_downloads = dependency_downloads.resolve()
        if (
            resolved_downloads.name != "downloads"
            or resolved_downloads.parent.name != ".cache"
            or not resolved_downloads.is_relative_to(safe_cache_root)
        ):
            raise MediaPreparationError(
                "refusing to prune outside target/**/.cache/downloads"
            )
        selected_names = {path.name for path in dependency_sources}
        for path in dependency_downloads.iterdir():
            if (
                path.name not in selected_names
                and CACHE_ENTRY.fullmatch(path.name)
                and (path.is_file() or path.is_symlink())
            ):
                path.unlink()

    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix="neoflash-media-source-", dir=destination.parent
    ) as temporary:
        root = Path(temporary)
        scripts_dir = root / "release-build-scripts"
        btbn_sources = root / "components" / "btbn-ffmpeg-builds" / BTBN_COMMIT / "sources"
        btbn_licenses = root / "components" / "btbn-ffmpeg-builds" / BTBN_COMMIT / "licenses"
        ffmpeg_sources = root / "components" / "ffmpeg" / FFMPEG_COMMIT / "sources"
        ffmpeg_licenses = root / "components" / "ffmpeg" / FFMPEG_COMMIT / "licenses"
        dependency_dest = root / "components" / "btbn-static-dependencies" / BTBN_COMMIT / "sources"
        dependency_licenses = root / "components" / "btbn-static-dependencies" / BTBN_COMMIT / "licenses"
        generated_dir = scripts_dir / "btbn-generated"
        component_licenses = root / "component-licenses"
        for directory in (
            scripts_dir,
            btbn_sources,
            btbn_licenses,
            ffmpeg_sources,
            ffmpeg_licenses,
            dependency_dest,
            dependency_licenses,
            generated_dir,
            component_licenses,
        ):
            directory.mkdir(parents=True)

        for script_name in (
            "prepare-portable-media.py",
            "package-portable.py",
            "portable-media-pins.json",
        ):
            shutil.copy2(project_root / "scripts" / script_name, scripts_dir / script_name)
        for dockerfile in dockerfiles:
            shutil.copy2(dockerfile, generated_dir / dockerfile.name)
        def link_or_copy(source: Path, output: Path) -> None:
            try:
                os.link(source, output)
            except OSError:
                shutil.copy2(source, output)

        link_or_copy(build_archive, btbn_sources / build_archive.name)
        link_or_copy(ffmpeg_archive, ffmpeg_sources / ffmpeg_archive.name)
        (btbn_licenses / "LICENSE").write_bytes(extract_named_member(build_archive, "/LICENSE"))
        (ffmpeg_licenses / "COPYING.GPLv3").write_bytes(
            extract_named_member(ffmpeg_archive, "/COPYING.GPLv3")
        )
        for source in dependency_sources:
            link_or_copy(source, dependency_dest / source.name)

        component_records = []
        component_inputs = [
            (
                "btbn-ffmpeg-builds",
                btbn_sources / build_archive.name,
                build_archive,
            ),
            (
                "ffmpeg",
                ffmpeg_sources / ffmpeg_archive.name,
                ffmpeg_archive,
            ),
        ]
        component_inputs.extend(
            (
                source.name.removesuffix(".tar.xz"),
                dependency_dest / source.name,
                source,
            )
            for source in dependency_sources
        )
        for component_name, staged_source, original_source in component_inputs:
            evidence = extract_component_licenses(
                original_source, component_name, component_licenses
            )
            component_records.append(
                {
                    "component": component_name,
                    "license_evidence": evidence,
                    "source_path": staged_source.relative_to(root).as_posix(),
                    "source_sha256": sha256_file(original_source),
                }
            )
        license_manifest = {
            "btbn_build_commit": BTBN_COMMIT,
            "build_variant": PINS["build_variant"],
            "components": component_records,
            "ffmpeg_commit": FFMPEG_COMMIT,
            "schema_version": 1,
        }
        (component_licenses / "component-license-manifest.json").write_text(
            canonical_json(license_manifest), encoding="utf-8", newline="\n"
        )
        if license_output.exists():
            shutil.rmtree(license_output)
        shutil.copytree(component_licenses, license_output)
        (dependency_licenses / "README.txt").write_text(
            "Exact extracted license evidence for every selected static dependency is in "
            "../../../../component-licenses/.\n",
            encoding="utf-8",
            newline="\n",
        )
        (root / "REBUILD.md").write_text(
            f"""# NeoFlash portable media rebuild

This bundle corresponds to BtbN FFmpeg-Builds `{BTBN_TAG}` at commit
`{BTBN_COMMIT}` and FFmpeg commit `{FFMPEG_COMMIT}`. The shipped helpers use
BtbN's GPL 9.0 static-dependency profiles generated for `linux64` and
`win64`; no `nonfree` variant is used. The Linux programs still use the
normal glibc/system runtime.

1. Verify every file with `SHA256SUMS.txt` and inspect `SOURCE-MANIFEST.json`.
2. Extract the BtbN build-repository source under `components/`.
3. Place the selected hash-named dependency archives in its `.cache/downloads/`.
4. Run `./generate.sh linux64 gpl 9.0` or `./generate.sh win64 gpl 9.0`.
5. Build the generated Dockerfile, checking out the pinned FFmpeg commit above.

The two generated Dockerfiles and their exact content-addressed source mounts
are retained under `release-build-scripts/btbn-generated/`.

BtbN's generated Dockerfiles refer to a moving `:latest` build image. This
bundle therefore preserves complete corresponding source and build recipes,
but does not claim that a future rebuild has a byte-identical compiler
environment. The shipped binary bytes themselves are fixed by published and
per-executable SHA-256 hashes.
""",
            encoding="utf-8",
            newline="\n",
        )

        source_records = []
        for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
            if path.is_file():
                source_records.append(
                    {
                        "path": path.relative_to(root).as_posix(),
                        "sha256": sha256_file(path),
                        "bytes": path.stat().st_size,
                    }
                )
        manifest = {
            "btbn_build_commit": BTBN_COMMIT,
            "btbn_release_tag": BTBN_TAG,
            "build_variant": PINS["build_variant"],
            "generated_dockerfiles": dockerfile_records,
            "ffmpeg_commit": FFMPEG_COMMIT,
            "schema_version": 1,
            "source_files": source_records,
        }
        (root / "SOURCE-MANIFEST.json").write_text(
            canonical_json(manifest), encoding="utf-8", newline="\n"
        )
        checksum_lines = [
            f"{sha256_file(path)}  {path.relative_to(root).as_posix()}"
            for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix())
            if path.is_file() and path.name != "SHA256SUMS.txt"
        ]
        (root / "SHA256SUMS.txt").write_text(
            "\n".join(checksum_lines) + "\n", encoding="utf-8", newline="\n"
        )
        deterministic_source_archive(root, destination)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", required=True, type=Path)
    subparsers = parser.add_subparsers(dest="command", required=True)
    repository = subparsers.add_parser("build-repository")
    repository.add_argument("--dest", required=True, type=Path)
    stage_filter = subparsers.add_parser("filter-download-stages")
    stage_filter.add_argument("--repository", required=True, type=Path)
    stage_filter.add_argument("--dockerfile", required=True, action="append", type=Path)
    binary = subparsers.add_parser("binary")
    binary.add_argument("--platform", required=True, choices=tuple(ASSETS))
    binary.add_argument("--dest", required=True, type=Path)
    binary.add_argument("--component-licenses", required=True, type=Path)
    source = subparsers.add_parser("source")
    source.add_argument("--project-root", required=True, type=Path)
    source.add_argument("--dependency-downloads", required=True, type=Path)
    source.add_argument("--dockerfile", required=True, action="append", type=Path)
    source.add_argument("--component-licenses", required=True, type=Path)
    source.add_argument("--prune-unselected", action="store_true")
    source.add_argument("--archive", required=True, type=Path)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "build-repository":
            prepare_build_repository(args.cache.resolve(), args.dest.resolve())
            print(args.dest.resolve())
        elif args.command == "filter-download-stages":
            enabled, disabled = filter_download_stages(
                args.repository.resolve(), [path.resolve() for path in args.dockerfile]
            )
            print(f"enabled {enabled} exact download stages; disabled {disabled}")
        elif args.command == "binary":
            prepare_binary(
                args.platform,
                args.cache.resolve(),
                args.dest.resolve(),
                args.component_licenses.resolve(),
            )
            print(args.dest.resolve())
        else:
            prepare_source(
                args.project_root.resolve(),
                args.cache.resolve(),
                args.dependency_downloads.resolve(),
                [path.resolve() for path in args.dockerfile],
                args.component_licenses.resolve(),
                args.archive.resolve(),
                args.prune_unselected,
            )
            print(args.archive.resolve())
        return 0
    except (OSError, MediaPreparationError, tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"Portable media preparation refused: {error}", file=os.sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
