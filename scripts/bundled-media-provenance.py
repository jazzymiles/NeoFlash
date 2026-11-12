#!/usr/bin/env python3
"""Build and verify the corresponding-source bundle for packaged media helpers.

The macOS media payload is assembled from Homebrew bottles.  Copying the dylibs is not
enough for a public GPL distribution: every staged binary must remain traceable to the
exact keg, bottle, formula recipe, upstream source, patches and licence evidence used to
produce it.  This tool enforces that boundary and fails closed with one complete missing
evidence list.

It does not edit the media payload unless ``--refresh-staged-hashes`` is explicitly
used after a verified code-signing operation. A successful run writes a deterministic
provenance directory (and, optionally, a deterministic tar.xz archive) to a new output
path. An incomplete run leaves the requested output absent and returns a non-zero status.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any


HASH_RE = re.compile(r"^[0-9a-f]{64}$")
LICENCE_TOKENS = (
    "LICENSE",
    "LICENCE",
    "COPYING",
    "COPYRIGHT",
    "NOTICE",
)
SPDX_LICENCE_PREFIXES = (
    "GPL-",
    "LGPL-",
    "AGPL-",
    "APACHE-",
    "BSD-",
    "MPL-",
    "ZLIB-",
)
MAX_LICENCE_FILE_BYTES = 2 * 1024 * 1024
MAX_DOWNLOAD_BYTES = 2 * 1024 * 1024 * 1024


@dataclass(frozen=True)
class ArtifactSpec:
    owner: str
    kind: str
    name: str
    url: str
    sha256: str | None
    revision: str | None = None


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_json(value: Any) -> str:
    return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def safe_leaf(raw: str) -> str:
    leaf = re.sub(r"[^A-Za-z0-9._@+-]+", "-", raw).strip("-.")
    return leaf or "artifact"


def validate_relative_path(raw: str) -> PurePosixPath:
    rel = PurePosixPath(raw)
    if rel.is_absolute() or not rel.parts or any(part in ("", ".", "..") for part in rel.parts):
        raise ValueError(f"unsafe relative path: {raw!r}")
    return rel


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object in {path}")
    return value


def spdx_sha256(package: dict[str, Any]) -> str | None:
    values = [
        str(item.get("checksumValue", "")).lower()
        for item in package.get("checksums", [])
        if item.get("algorithm") == "SHA256"
    ]
    valid = [value for value in values if HASH_RE.fullmatch(value)]
    return valid[0] if len(valid) == 1 else None


def package_by_prefix(sbom: dict[str, Any], prefix: str) -> list[dict[str, Any]]:
    return [
        item
        for item in sbom.get("packages", [])
        if isinstance(item, dict) and str(item.get("SPDXID", "")).startswith(prefix)
    ]


def media_inventory(media_root: Path, missing: list[dict[str, str]]) -> dict[tuple[str, str], list[dict[str, str]]]:
    sbom_path = media_root / "provenance" / "media-sbom.json"
    if not sbom_path.is_file():
        missing.append({"code": "media-sbom-missing", "path": str(sbom_path)})
        return {}
    try:
        media_sbom = load_json(sbom_path)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        missing.append({"code": "media-sbom-invalid", "detail": str(error)})
        return {}

    entries: list[dict[str, Any]] = []
    for executable in media_sbom.get("executables", []):
        if not isinstance(executable, dict):
            continue
        item = dict(executable)
        item["source_formula"] = "ffmpeg"
        entries.append(item)
    entries.extend(item for item in media_sbom.get("libraries", []) if isinstance(item, dict))

    components: dict[tuple[str, str], list[dict[str, str]]] = {}
    declared_paths: set[str] = set()
    for item in entries:
        rel_raw = str(item.get("relative_path", ""))
        formula = str(item.get("source_formula", ""))
        version = str(item.get("version", ""))
        expected_hash = str(item.get("sha256", "")).lower()
        try:
            rel = validate_relative_path(rel_raw)
        except ValueError as error:
            missing.append({"code": "media-path-invalid", "detail": str(error)})
            continue
        if not formula or not version:
            missing.append({"code": "component-origin-missing", "path": rel_raw})
            continue
        if not HASH_RE.fullmatch(expected_hash):
            missing.append({"code": "component-hash-missing", "path": rel_raw})
            continue
        path = media_root.joinpath(*rel.parts)
        if not path.is_file():
            missing.append({"code": "staged-file-missing", "path": rel_raw})
            continue
        actual_hash = sha256_file(path)
        if actual_hash != expected_hash:
            missing.append(
                {
                    "code": "staged-file-hash-mismatch",
                    "path": rel_raw,
                    "expected": expected_hash,
                    "actual": actual_hash,
                }
            )
            continue
        declared_paths.add(rel.as_posix())
        components.setdefault((formula, version), []).append(
            {"relative_path": rel.as_posix(), "sha256": actual_hash}
        )

    actual_paths = {
        path.relative_to(media_root).as_posix()
        for dirname in ("bin", "lib")
        for path in (media_root / dirname).glob("*")
        if path.is_file()
    }
    for path in sorted(actual_paths - declared_paths):
        missing.append({"code": "staged-file-unaccounted", "path": path})
    for path in sorted(declared_paths - actual_paths):
        missing.append({"code": "declared-file-not-staged", "path": path})

    for files in components.values():
        files.sort(key=lambda item: item["relative_path"])
    return components


def refresh_staged_hashes(media_root: Path) -> None:
    """Refresh whole-file hashes after code signing, without changing provenance origins."""

    sbom_path = media_root / "provenance" / "media-sbom.json"
    media_sbom = load_json(sbom_path)
    entries = [
        item
        for group in ("executables", "libraries")
        for item in media_sbom.get(group, [])
        if isinstance(item, dict)
    ]
    declared: set[str] = set()
    sums: list[str] = []
    for item in entries:
        rel = validate_relative_path(str(item.get("relative_path", "")))
        rel_text = rel.as_posix()
        if rel_text in declared:
            raise ValueError(f"duplicate staged media path: {rel_text}")
        path = media_root.joinpath(*rel.parts)
        if not path.is_file():
            raise ValueError(f"staged media file is missing: {rel_text}")
        digest = sha256_file(path)
        item["sha256"] = digest
        declared.add(rel_text)
        sums.append(f"{digest}  {rel_text}")
    actual = {
        path.relative_to(media_root).as_posix()
        for dirname in ("bin", "lib")
        for path in (media_root / dirname).glob("*")
        if path.is_file()
    }
    if actual != declared:
        extra = sorted(actual - declared)
        absent = sorted(declared - actual)
        raise ValueError(f"staged media inventory changed; extra={extra}, absent={absent}")
    sbom_path.write_text(canonical_json(media_sbom), encoding="utf-8")
    (media_root / "provenance" / "SHA256SUMS.txt").write_text(
        "\n".join(sorted(sums)) + "\n", encoding="utf-8"
    )


def formula_recipe(keg: Path, formula: str) -> Path | None:
    candidates = sorted((keg / ".brew").glob("*.rb"))
    exact = keg / ".brew" / f"{formula}.rb"
    if exact in candidates:
        return exact
    return candidates[0] if len(candidates) == 1 else None


def extract_recipe_resources(
    formula: str, recipe_text: str
) -> tuple[list[ArtifactSpec], list[str]]:
    resources: list[ArtifactSpec] = []
    unresolved: list[str] = []
    # Homebrew also permits resources inside `test do`; those fixtures are not build inputs and
    # are not part of the corresponding source for the shipped binary.
    build_recipe = recipe_text.split("\n  test do", 1)[0]
    for match in re.finditer(r'^\s*resource\s+"([^"]+)"\s+do\s*$', build_recipe, re.MULTILINE):
        tail = build_recipe[match.end() :]
        end = re.search(r"^\s*end\s*$", tail, re.MULTILINE)
        block = tail[: end.start()] if end else tail
        url_match = re.search(r'^\s*url\s+"([^"]+)"', block, re.MULTILINE)
        hash_match = re.search(r'^\s*sha256\s+"([0-9a-f]{64})"', block, re.MULTILINE)
        revision_match = re.search(r'revision:\s*"([0-9a-f]{7,64})"', block)
        if url_match and (hash_match or revision_match):
            resources.append(
                ArtifactSpec(
                    owner=formula,
                    kind="resource",
                    name=match.group(1),
                    url=url_match.group(1),
                    sha256=hash_match.group(1) if hash_match else None,
                    revision=revision_match.group(1) if revision_match else None,
                )
            )
        else:
            unresolved.append(match.group(1))
    return resources, unresolved


def extract_recipe_patches(formula: str, recipe_text: str) -> tuple[list[ArtifactSpec], list[str]]:
    patches: list[ArtifactSpec] = []
    local_files: list[str] = []
    for index, match in enumerate(re.finditer(r"^\s*patch\s+do\s*$", recipe_text, re.MULTILINE), 1):
        tail = recipe_text[match.end() :]
        end = re.search(r"^\s*end\s*$", tail, re.MULTILINE)
        block = tail[: end.start()] if end else tail
        url_match = re.search(r'^\s*url\s+"([^"]+)"', block, re.MULTILINE)
        hash_match = re.search(r'^\s*sha256\s+"([0-9a-f]{64})"', block, re.MULTILINE)
        file_match = re.search(r'^\s*file\s+"([^"]+)"', block, re.MULTILINE)
        if url_match and hash_match:
            patches.append(
                ArtifactSpec(
                    owner=formula,
                    kind="patch",
                    name=f"patch-{index}",
                    url=url_match.group(1),
                    sha256=hash_match.group(1),
                )
            )
        elif file_match:
            local_files.append(file_match.group(1))
        else:
            local_files.append(f"<unparsed patch block {index}>")
    return patches, local_files


def primary_git_revision(recipe_text: str) -> tuple[str | None, str | None]:
    head = recipe_text.split("\n  head ", 1)[0]
    url_match = re.search(r'^\s*url\s+"([^"]+\.git)"', head, re.MULTILINE)
    revision_match = re.search(r'revision:\s*"([0-9a-f]{7,64})"', head)
    if url_match and revision_match:
        return url_match.group(1), revision_match.group(1)
    return None, None


def brew_formula_info(formula: str) -> dict[str, Any] | None:
    try:
        result = subprocess.run(
            ["brew", "info", "--json=v2", formula],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except OSError:
        return None
    if result.returncode != 0:
        return None
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError:
        return None
    formulae = payload.get("formulae", [])
    return formulae[0] if len(formulae) == 1 and isinstance(formulae[0], dict) else None


def formula_pkg_version(info: dict[str, Any]) -> str:
    stable = str(info.get("versions", {}).get("stable", ""))
    revision = int(info.get("revision", 0) or 0)
    return f"{stable}_{revision}" if revision else stable


def receipt_bottle_tag(receipt: dict[str, Any]) -> str | None:
    source_path = str(receipt.get("source", {}).get("path", ""))
    match = re.search(r"packages\.([A-Za-z0-9_]+)\.jws\.json$", source_path)
    return match.group(1) if match else None


def resolve_bottle_provenance(
    formula: str,
    version: str,
    receipt: dict[str, Any],
    bottle_packages: list[dict[str, Any]],
) -> tuple[str | None, str | None, str]:
    """Return exact bottle URL/hash, using formula API metadata only when version-exact.

    Homebrew's newer internal-package installs occasionally omit `SPDXRef-Bottle-*` from the keg
    SBOM even though the receipt says the keg was poured from a bottle.  The signed formula API
    still records the exact tag and digest; using it is safe only while its package version equals
    the installed keg version.
    """
    if len(bottle_packages) == 1:
        package = bottle_packages[0]
        url = str(package.get("downloadLocation", ""))
        digest = spdx_sha256(package)
        if url not in ("", "NOASSERTION", "NONE") and digest:
            return url, digest, "keg-spdx"

    info = brew_formula_info(formula)
    if not info or formula_pkg_version(info) != version:
        return None, None, "unresolved"
    tag = receipt_bottle_tag(receipt)
    files = info.get("bottle", {}).get("stable", {}).get("files", {})
    if not tag or not isinstance(files, dict):
        return None, None, "unresolved"
    bottle = files.get(tag)
    if not isinstance(bottle, dict):
        return None, None, "unresolved"
    url = str(bottle.get("url", ""))
    digest = str(bottle.get("sha256", "")).lower()
    if url and HASH_RE.fullmatch(digest):
        return url, digest, f"formula-api:{info.get('tap_git_head') or 'signed-api'}:{tag}"
    return None, None, "unresolved"


def resolve_local_patches(
    formula: str,
    version: str,
    local_files: list[str],
    missing: list[dict[str, str]],
) -> tuple[list[ArtifactSpec], dict[str, Any]]:
    if not local_files:
        return [], {}
    info = brew_formula_info(formula)
    if not info or formula_pkg_version(info) != version:
        for file_name in local_files:
            missing.append(
                {
                    "code": "formula-local-patch-unresolved",
                    "component": f"{formula}@{version}",
                    "patch": file_name,
                    "action": "install/tap the exact Homebrew formula revision or provide its patch manually",
                }
            )
        return [], {}
    tap_head = str(info.get("tap_git_head", ""))
    checksums = info.get("local_patch_checksums", {})
    if not re.fullmatch(r"[0-9a-f]{40,64}", tap_head):
        tap_head = ""
    specs: list[ArtifactSpec] = []
    for file_name in local_files:
        expected = str(checksums.get(file_name, "")).lower()
        if not tap_head or not HASH_RE.fullmatch(expected) or file_name.startswith("<"):
            missing.append(
                {
                    "code": "formula-local-patch-unresolved",
                    "component": f"{formula}@{version}",
                    "patch": file_name,
                    "action": "restore exact tap commit and local_patch_checksums metadata",
                }
            )
            continue
        specs.append(
            ArtifactSpec(
                owner=formula,
                kind="patch",
                name=PurePosixPath(file_name).name,
                url=f"https://raw.githubusercontent.com/Homebrew/homebrew-core/{tap_head}/{file_name}",
                sha256=expected,
                revision=tap_head,
            )
        )
    return specs, {
        "tap_git_head": tap_head,
        "ruby_source_path": info.get("ruby_source_path"),
        "ruby_source_checksum": info.get("ruby_source_checksum", {}).get("sha256"),
    }


def cache_name(spec: ArtifactSpec) -> str:
    parsed_name = PurePosixPath(urllib.parse.urlparse(spec.url).path).name
    leaf = safe_leaf(parsed_name or spec.name)
    identity = spec.sha256 or spec.revision or hashlib.sha256(spec.url.encode()).hexdigest()
    return f"{identity}--{leaf}"


def fetch_http(spec: ArtifactSpec, cache_dir: Path, allow_download: bool) -> tuple[Path | None, str | None]:
    cache_dir.mkdir(parents=True, exist_ok=True)
    target = cache_dir / cache_name(spec)
    if target.is_file():
        actual = sha256_file(target)
        if spec.sha256 is None or actual == spec.sha256:
            return target, actual
        target.unlink()
    if not allow_download:
        return None, None

    temp = target.with_name(target.name + ".part")
    temp.unlink(missing_ok=True)
    digest = hashlib.sha256()
    total = 0
    try:
        with urllib.request.urlopen(spec.url, timeout=90) as response, temp.open("wb") as output:
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                if total > MAX_DOWNLOAD_BYTES:
                    raise RuntimeError(f"download exceeds {MAX_DOWNLOAD_BYTES} bytes")
                digest.update(chunk)
                output.write(chunk)
        actual = digest.hexdigest()
        if spec.sha256 is not None and actual != spec.sha256:
            raise RuntimeError(f"SHA-256 mismatch: expected {spec.sha256}, got {actual}")
        temp.replace(target)
        return target, actual
    except Exception:
        temp.unlink(missing_ok=True)
        raise


def recover_resource_from_keg(spec: ArtifactSpec, keg: Path, cache_dir: Path) -> tuple[Path | None, str | None]:
    """Recover a vanished upstream resource from the exact poured keg by verified SHA-256.

    This is limited to non-code formula resources (for example Tesseract trained data).  The
    original URL and declared digest remain in the manifest; the keg copy is accepted only when
    its bytes match that digest exactly.
    """
    if spec.kind != "resource" or spec.sha256 is None:
        return None, None
    basename = PurePosixPath(urllib.parse.unquote(urllib.parse.urlparse(spec.url).path)).name
    if not basename:
        return None, None
    for candidate in sorted(keg.rglob(basename)):
        if candidate.is_file() and sha256_file(candidate) == spec.sha256:
            cache_dir.mkdir(parents=True, exist_ok=True)
            target = cache_dir / cache_name(spec)
            shutil.copy2(candidate, target)
            return target, spec.sha256
    return None, None


def deterministic_tree_archive(source: Path, destination: Path) -> None:
    with tarfile.open(destination, "w:xz", format=tarfile.PAX_FORMAT) as archive:
        for path in sorted(source.rglob("*"), key=lambda item: item.relative_to(source).as_posix()):
            rel = path.relative_to(source).as_posix()
            if ".git" in PurePosixPath(rel).parts:
                continue
            info = archive.gettarinfo(str(path), arcname=rel)
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mtime = 0
            if info.isfile():
                with path.open("rb") as payload:
                    archive.addfile(info, payload)
            else:
                archive.addfile(info)


def fetch_git_source(spec: ArtifactSpec, cache_dir: Path, allow_download: bool) -> tuple[Path | None, str | None, list[dict[str, str]]]:
    if not spec.revision:
        return None, None, []
    prefix = f"git-{safe_leaf(spec.owner)}-{spec.revision}"
    cache_dir.mkdir(parents=True, exist_ok=True)
    repository_cache = cache_dir / (
        "git-repository-" + hashlib.sha256(spec.url.encode("utf-8")).hexdigest()[:24] + ".git"
    )
    if repository_cache.exists() and not (repository_cache / "HEAD").is_file():
        raise RuntimeError(f"invalid cached Git repository: {repository_cache}")
    repository_created = False
    if not repository_cache.exists():
        if not allow_download:
            return None, None, []
        subprocess.run(["git", "init", "--quiet", "--bare", str(repository_cache)], check=True)
        repository_created = True
    if not repository_created:
        fsck = subprocess.run(
            ["git", "-C", str(repository_cache), "fsck", "--full", "--no-dangling"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        if fsck.returncode != 0:
            raise RuntimeError(
                f"cached Git repository verification failed: {fsck.stderr.strip()}"
            )
    cache_ref = f"refs/neoflash/{safe_leaf(spec.revision)}"
    if allow_download:
        subprocess.run(
            [
                "git",
                "-C",
                str(repository_cache),
                "fetch",
                "--quiet",
                "--force",
                "--depth=1",
                spec.url,
                f"{spec.revision}:{cache_ref}",
            ],
            check=True,
        )
        revision_ref = cache_ref
    else:
        revision_ref = spec.revision
    try:
        resolved = subprocess.check_output(
            ["git", "-C", str(repository_cache), "rev-parse", f"{revision_ref}^{{commit}}"],
            text=True,
        ).strip()
    except subprocess.CalledProcessError:
        return None, None, []
    if not resolved.startswith(spec.revision):
        raise RuntimeError(f"resolved Git revision {resolved} does not match {spec.revision}")
    with tempfile.TemporaryDirectory(prefix="neoflash-media-git-") as temp_raw:
        checkout = Path(temp_raw) / "checkout"
        subprocess.run(
            [
                "git",
                "-C",
                str(repository_cache),
                "worktree",
                "add",
                "--quiet",
                "--force",
                "--detach",
                str(checkout),
                resolved,
            ],
            check=True,
        )
        try:
            subprocess.run(
                ["git", "-C", str(checkout), "submodule", "update", "--init", "--recursive", "--quiet"],
                check=True,
            )
            submodules: list[dict[str, str]] = []
            status = subprocess.run(
                ["git", "-C", str(checkout), "submodule", "status", "--recursive"],
                check=True,
                stdout=subprocess.PIPE,
                text=True,
            ).stdout
            for line in status.splitlines():
                if not line:
                    continue
                if line[0] in "-+U":
                    raise RuntimeError(f"unresolved Git submodule state: {line}")
                fields = line[1:].split()
                if len(fields) >= 2:
                    submodules.append({"revision": fields[0], "path": fields[1]})
            provisional = cache_dir / f"{prefix}.part.tar.xz"
            provisional.unlink(missing_ok=True)
            deterministic_tree_archive(checkout, provisional)
            actual = sha256_file(provisional)
            target = cache_dir / f"{prefix}--{actual}.tar.xz"
            if target.is_file() and sha256_file(target) == actual:
                provisional.unlink()
            else:
                provisional.replace(target)
        finally:
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(repository_cache),
                    "worktree",
                    "remove",
                    "--force",
                    str(checkout),
                ],
                check=False,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
        return target, actual, submodules


def is_licence_name(name: str) -> bool:
    upper = PurePosixPath(name).name.upper()
    stem = upper.rsplit(".", 1)[0]
    return (
        any(token in upper for token in LICENCE_TOKENS)
        or upper.startswith(SPDX_LICENCE_PREFIXES)
        or stem in {"MIT", "ZLIB", "BSD", "GPL", "LGPL", "AGPL", "MPL"}
    )


def read_archive_licences(archive_path: Path) -> list[tuple[str, bytes]]:
    licences: list[tuple[str, bytes]] = []

    def add_member(name: str, payload: bytes) -> None:
        if len(payload) <= MAX_LICENCE_FILE_BYTES:
            licences.append((PurePosixPath(name).as_posix(), payload))

    try:
        if tarfile.is_tarfile(archive_path):
            with tarfile.open(archive_path, "r:*") as archive:
                for member in sorted(archive.getmembers(), key=lambda item: item.name):
                    if member.isfile() and is_licence_name(member.name) and member.size <= MAX_LICENCE_FILE_BYTES:
                        source = archive.extractfile(member)
                        if source:
                            add_member(member.name, source.read(MAX_LICENCE_FILE_BYTES + 1))
        elif zipfile.is_zipfile(archive_path):
            with zipfile.ZipFile(archive_path) as archive:
                for member in sorted(archive.infolist(), key=lambda item: item.filename):
                    if not member.is_dir() and is_licence_name(member.filename) and member.file_size <= MAX_LICENCE_FILE_BYTES:
                        with archive.open(member) as source:
                            add_member(member.filename, source.read(MAX_LICENCE_FILE_BYTES + 1))
    except (OSError, tarfile.TarError, zipfile.BadZipFile):
        return []
    unique = {(name, hashlib.sha256(payload).hexdigest()): (name, payload) for name, payload in licences}
    return [unique[key] for key in sorted(unique)]


def inspect_archive_licences(archive_path: Path) -> list[dict[str, str]]:
    return [
        {"archive_member": name, "sha256": hashlib.sha256(payload).hexdigest()}
        for name, payload in read_archive_licences(archive_path)
    ]


def extract_licences(archive_path: Path, destination: Path) -> list[dict[str, str]]:
    destination.mkdir(parents=True, exist_ok=True)
    extracted: list[dict[str, str]] = []

    def write_member(name: str, payload: bytes) -> None:
        if len(payload) > MAX_LICENCE_FILE_BYTES:
            return
        base = safe_leaf(PurePosixPath(name).name)
        target = destination / base
        suffix = 2
        while target.exists() and target.read_bytes() != payload:
            target = destination / f"{base}.{suffix}"
            suffix += 1
        if not target.exists():
            target.write_bytes(payload)
        extracted.append({"path": target.as_posix(), "sha256": sha256_file(target)})

    for name, payload in read_archive_licences(archive_path):
        write_member(name, payload)
    unique = {(item["path"], item["sha256"]): item for item in extracted}
    return [unique[key] for key in sorted(unique)]


def copy_with_record(source: Path, destination: Path, root: Path) -> dict[str, str]:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    return {
        "path": destination.relative_to(root).as_posix(),
        "sha256": sha256_file(destination),
    }


def write_sums(root: Path) -> None:
    files = [path for path in root.rglob("*") if path.is_file() and path.name != "SHA256SUMS.txt"]
    lines = [f"{sha256_file(path)}  {path.relative_to(root).as_posix()}" for path in sorted(files)]
    (root / "SHA256SUMS.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")


def deterministic_bundle_archive(root: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temp = destination.with_name(destination.name + ".part")
    temp.unlink(missing_ok=True)
    with tarfile.open(temp, "w:xz", format=tarfile.PAX_FORMAT) as archive:
        for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
            rel = f"{root.name}/{path.relative_to(root).as_posix()}"
            info = archive.gettarinfo(str(path), arcname=rel)
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mtime = 0
            if info.isfile():
                with path.open("rb") as payload:
                    archive.addfile(info, payload)
            else:
                archive.addfile(info)
    temp.replace(destination)


def build_provenance(
    media_root: Path,
    cellar: Path,
    cache_dir: Path,
    output_dir: Path | None,
    archive_path: Path | None,
    allow_download: bool,
    build_scripts: list[Path] | None = None,
) -> dict[str, Any]:
    missing: list[dict[str, str]] = []
    components = media_inventory(media_root, missing)
    temp_root_path: Path | None = None
    if output_dir:
        if output_dir.exists():
            missing.append(
                {
                    "code": "output-exists",
                    "path": str(output_dir),
                    "action": "choose a new output path",
                }
            )
        else:
            output_dir.parent.mkdir(parents=True, exist_ok=True)
            temp_root_path = Path(tempfile.mkdtemp(prefix=output_dir.name + ".tmp-", dir=output_dir.parent))

    component_records: list[dict[str, Any]] = []
    build_input_records: list[dict[str, str]] = []
    try:
        for script_path in build_scripts or []:
            if not script_path.is_file():
                missing.append({"code": "release-build-script-missing", "path": str(script_path)})
                continue
            record = {"name": script_path.name, "sha256": sha256_file(script_path)}
            if temp_root_path:
                record.update(
                    copy_with_record(
                        script_path,
                        temp_root_path / "release-build-scripts" / script_path.name,
                        temp_root_path,
                    )
                )
            build_input_records.append(record)

        for (formula, version), staged_files in sorted(components.items()):
            keg = cellar / formula / version
            component_name = f"{formula}@{version}"
            if not keg.is_dir():
                missing.append({"code": "exact-keg-missing", "component": component_name, "path": str(keg)})
                continue
            receipt_path = keg / "INSTALL_RECEIPT.json"
            sbom_path = keg / "sbom.spdx.json"
            recipe_path = formula_recipe(keg, formula)
            for code, path in (
                ("install-receipt-missing", receipt_path),
                ("homebrew-sbom-missing", sbom_path),
            ):
                if not path.is_file():
                    missing.append({"code": code, "component": component_name, "path": str(path)})
            if recipe_path is None:
                missing.append({"code": "formula-recipe-missing", "component": component_name})
            if not receipt_path.is_file() or not sbom_path.is_file() or recipe_path is None:
                continue

            try:
                receipt = load_json(receipt_path)
                homebrew_sbom = load_json(sbom_path)
                recipe_text = recipe_path.read_text(encoding="utf-8")
            except (OSError, ValueError, json.JSONDecodeError, UnicodeError) as error:
                missing.append({"code": "keg-metadata-invalid", "component": component_name, "detail": str(error)})
                continue

            source_packages = package_by_prefix(homebrew_sbom, "SPDXRef-Archive-")
            bottle_packages = package_by_prefix(homebrew_sbom, "SPDXRef-Bottle-")
            if len(source_packages) != 1:
                missing.append({"code": "source-package-ambiguous", "component": component_name})
                continue
            source_package = source_packages[0]
            bottle_url, bottle_hash, bottle_evidence = resolve_bottle_provenance(
                formula, version, receipt, bottle_packages
            )
            if bottle_hash is None or bottle_url is None:
                missing.append({"code": "bottle-provenance-incomplete", "component": component_name})

            source_url = str(source_package.get("downloadLocation", ""))
            source_hash = spdx_sha256(source_package)
            source_license = str(source_package.get("licenseConcluded", ""))
            git_url: str | None = None
            git_revision: str | None = None
            if source_hash is None and source_url.endswith(".git"):
                git_url, git_revision = primary_git_revision(recipe_text)
            if source_url in ("", "NOASSERTION", "NONE") or source_license in ("", "NOASSERTION", "NONE"):
                missing.append({"code": "upstream-source-metadata-incomplete", "component": component_name})
                continue
            if source_hash is None and not (git_url and git_revision and source_url == git_url):
                missing.append(
                    {
                        "code": "upstream-source-hash-missing",
                        "component": component_name,
                        "action": "supply a pinned Git revision or SHA-256 verified source archive",
                    }
                )
                continue

            source_spec = ArtifactSpec(
                owner=formula,
                kind="source",
                name=f"{formula}-{version}",
                url=source_url,
                sha256=source_hash,
                revision=git_revision,
            )
            resources, unresolved_resources = extract_recipe_resources(formula, recipe_text)
            for resource in unresolved_resources:
                missing.append(
                    {
                        "code": "formula-resource-unresolved",
                        "component": component_name,
                        "resource": resource,
                        "action": "record an exact SHA-256 archive or pinned Git revision",
                    }
                )
            patches, local_patch_files = extract_recipe_patches(formula, recipe_text)
            local_patches, tap_metadata = resolve_local_patches(formula, version, local_patch_files, missing)
            patches.extend(local_patches)

            record: dict[str, Any] = {
                "formula": formula,
                "version": version,
                "license_expression": source_license,
                "staged_files": staged_files,
                "homebrew": {
                    "homebrew_version": receipt.get("homebrew_version"),
                    "arch": receipt.get("arch"),
                    "built_as_bottle": receipt.get("built_as_bottle"),
                    "poured_from_bottle": receipt.get("poured_from_bottle"),
                    "bottle_url": bottle_url,
                    "bottle_sha256": bottle_hash,
                    "bottle_evidence": bottle_evidence,
                    **tap_metadata,
                },
                "formula_recipe": {"sha256": sha256_file(recipe_path)},
                "upstream_source": {
                    "url": source_url,
                    "declared_sha256": source_hash,
                    "revision": git_revision,
                },
                "resources": [],
                "patches": [],
                "license_evidence": [],
            }

            component_root = temp_root_path / "components" / safe_leaf(formula) / safe_leaf(version) if temp_root_path else None
            if component_root:
                record["formula_recipe"].update(
                    copy_with_record(recipe_path, component_root / "formula.rb", temp_root_path)
                )
                record["homebrew"]["install_receipt"] = copy_with_record(
                    receipt_path, component_root / "INSTALL_RECEIPT.json", temp_root_path
                )
                record["homebrew"]["spdx_sbom"] = copy_with_record(
                    sbom_path, component_root / "sbom.spdx.json", temp_root_path
                )

            all_specs: list[ArtifactSpec] = [source_spec, *resources, *patches]
            source_artifact: Path | None = None
            for spec in all_specs:
                recovered_from_keg = False
                try:
                    if spec.revision and spec.sha256 is None:
                        cached, actual_hash, submodules = fetch_git_source(spec, cache_dir, allow_download)
                    else:
                        cached, actual_hash = fetch_http(spec, cache_dir, allow_download)
                        submodules = []
                except (OSError, RuntimeError, subprocess.CalledProcessError, urllib.error.URLError) as error:
                    cached, actual_hash = recover_resource_from_keg(spec, keg, cache_dir)
                    submodules = []
                    recovered_from_keg = cached is not None
                    if not recovered_from_keg:
                        missing.append(
                            {
                                "code": "artifact-fetch-failed",
                                "component": component_name,
                                "artifact": f"{spec.kind}:{spec.name}",
                                "detail": str(error),
                            }
                        )
                        continue
                if cached is None or actual_hash is None:
                    missing.append(
                        {
                            "code": "artifact-not-cached",
                            "component": component_name,
                            "artifact": f"{spec.kind}:{spec.name}",
                            "url": spec.url,
                            "expected_sha256": spec.sha256 or "generated-from-pinned-git-revision",
                            "action": "rerun with --download or pre-populate --cache-dir",
                        }
                    )
                    continue
                item = {
                    "name": spec.name,
                    "url": spec.url,
                    "sha256": actual_hash,
                    "revision": spec.revision,
                }
                if submodules:
                    item["submodules"] = submodules
                if recovered_from_keg:
                    item["retrieval"] = "exact-hash copy recovered from installed keg after upstream URL failure"
                if spec is source_spec:
                    source_artifact = cached
                if component_root:
                    folder = "sources" if spec.kind in ("source", "resource") else "patches"
                    destination = component_root / folder / cached.name
                    copied = copy_with_record(cached, destination, temp_root_path)
                    item.update(copied)
                if spec.kind == "source":
                    record["upstream_source"].update(item)
                elif spec.kind == "resource":
                    record["resources"].append(item)
                else:
                    record["patches"].append(item)

            if "patch :DATA" in recipe_text:
                marker = recipe_text.find("\n__END__\n")
                if marker < 0:
                    missing.append({"code": "embedded-patch-missing", "component": component_name})
                elif component_root:
                    embedded = recipe_text[marker + len("\n__END__\n") :].encode()
                    target = component_root / "patches" / "embedded-DATA.patch"
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(embedded)
                    record["patches"].append(
                        {
                            "name": "embedded-DATA",
                            "url": "formula.rb#__END__",
                            "sha256": sha256_file(target),
                            "path": target.relative_to(temp_root_path).as_posix(),
                            "revision": None,
                        }
                    )

            keg_licences = sorted(
                path for path in keg.iterdir() if path.is_file() and is_licence_name(path.name)
            )
            if component_root:
                for licence in keg_licences:
                    record["license_evidence"].append(
                        copy_with_record(
                            licence,
                            component_root / "licenses" / safe_leaf(licence.name),
                            temp_root_path,
                        )
                    )
                if not keg_licences and source_artifact:
                    extracted = extract_licences(source_artifact, component_root / "licenses")
                    for item in extracted:
                        item["path"] = Path(item["path"]).relative_to(temp_root_path).as_posix()
                    record["license_evidence"].extend(extracted)
            elif not keg_licences and source_artifact:
                record["license_evidence"].extend(inspect_archive_licences(source_artifact))
            if not keg_licences and not record["license_evidence"]:
                missing.append(
                    {
                        "code": "license-text-missing",
                        "component": component_name,
                        "action": "bundle the verified upstream source and extract its LICENSE/COPYING text",
                    }
                )

            component_records.append(record)

        report = {
            "schema_version": 1,
            "release_ready": not missing,
            "media_root": media_root.name,
            "component_count": len(component_records),
            "staged_file_count": sum(len(item["staged_files"]) for item in component_records),
            "release_build_scripts": build_input_records,
            "components": component_records,
            "missing": sorted(missing, key=lambda item: canonical_json(item)),
        }

        if temp_root_path and not missing and output_dir:
            (temp_root_path / "manifest.json").write_text(canonical_json(report), encoding="utf-8")
            write_sums(temp_root_path)
            temp_root_path.replace(output_dir)
            temp_root_path = None
            if archive_path:
                if archive_path.exists():
                    raise FileExistsError(f"archive output already exists: {archive_path}")
                deterministic_bundle_archive(output_dir, archive_path)
        return report
    finally:
        if temp_root_path and temp_root_path.exists():
            shutil.rmtree(temp_root_path)


def write_report(path: Path | None, report: dict[str, Any]) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(canonical_json(report), encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    project_root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(
        description="Fail-closed GPL corresponding-source/provenance gate for bundled FFmpeg media."
    )
    parser.add_argument(
        "--media-root",
        type=Path,
        default=project_root / "target" / "bundled-media",
        help="staged media payload containing bin/, lib/ and provenance/media-sbom.json",
    )
    parser.add_argument(
        "--cellar",
        type=Path,
        default=Path("/opt/homebrew/Cellar"),
        help="Homebrew Cellar containing the exact staged kegs",
    )
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=project_root / "target" / "bundled-media-source-cache",
        help="verified upstream source/patch cache",
    )
    parser.add_argument("--output-dir", type=Path, help="new corresponding-source bundle directory")
    parser.add_argument("--archive", type=Path, help="optional deterministic .tar.xz output")
    parser.add_argument("--report", type=Path, help="write the audit report even when the gate fails")
    parser.add_argument(
        "--download",
        action="store_true",
        help="download missing SHA-256 sources/patches and archive pinned Git revisions",
    )
    parser.add_argument(
        "--refresh-staged-hashes",
        action="store_true",
        help="refresh media SBOM hashes after an already-audited code-signing operation",
    )
    args = parser.parse_args(argv)
    if args.archive and not args.output_dir:
        parser.error("--archive requires --output-dir")

    try:
        if args.refresh_staged_hashes:
            refresh_staged_hashes(args.media_root.resolve())
        report = build_provenance(
            media_root=args.media_root.resolve(),
            cellar=args.cellar.resolve(),
            cache_dir=args.cache_dir.resolve(),
            output_dir=args.output_dir.resolve() if args.output_dir else None,
            archive_path=args.archive.resolve() if args.archive else None,
            allow_download=args.download,
            build_scripts=[
                project_root / "scripts" / "build-bundled-ffmpeg.sh",
                project_root / "scripts" / "build-bundled-ffmpeg-lgpl.sh",
                project_root / "scripts" / "stage-bundled-media.py",
                project_root / "scripts" / "bundled-media-provenance.py",
            ],
        )
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        report = {
            "schema_version": 1,
            "release_ready": False,
            "components": [],
            "missing": [{"code": "provenance-gate-error", "detail": str(error)}],
        }
    write_report(args.report.resolve() if args.report else None, report)
    if report["release_ready"]:
        print(
            f"Bundled media provenance ready: {report['component_count']} components, "
            f"{report['staged_file_count']} staged files."
        )
        return 0

    print("Bundled media provenance is incomplete; release refused:", file=sys.stderr)
    for item in report.get("missing", []):
        print("  - " + canonical_json(item).strip().replace("\n", " "), file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
