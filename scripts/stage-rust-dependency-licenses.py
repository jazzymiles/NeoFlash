#!/usr/bin/env python3
"""Build a fail-closed Rust dependency license bundle for a Cargo package.

The default audit resolves ``neoflash-editor`` with ``cargo metadata --locked``.
For every package in that dependency closure it records the exact Cargo source,
lockfile checksum and declared SPDX expression, and discovers/copies packaged
``LICENSE*``, ``COPYING*`` and ``NOTICE*`` evidence. When a published crate omits
those files, the complete declared terms are supplied from a content-addressed,
pinned SPDX license-list revision. Missing text or source identity fails closed.

The generated JSON and Markdown contain no timestamps or host-specific registry
paths, so identical Cargo inputs and license files produce identical manifests.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError as error:  # pragma: no cover - Python < 3.11
    raise SystemExit("Python 3.11 or newer is required (tomllib is unavailable).") from error


SCHEMA_VERSION = 1
MANIFEST_JSON = "rust-dependency-licenses.json"
MANIFEST_MARKDOWN = "rust-dependency-licenses.md"
LICENSE_NAME = re.compile(r"^(?:license|copying|notice)(?:$|[._-].*)", re.IGNORECASE)
LICENSE_DIRECTORY = re.compile(r"^(?:licenses?|copying|notices?)$", re.IGNORECASE)
MAX_LICENSE_FILE_BYTES = 4 * 1024 * 1024
DETERMINISTIC_MTIME = 315_532_800  # 1980-01-01, also safe for ZIP archives.
SPDX_LICENSE_LIST_REVISION = "31846481de7ea17d92f2f46fad76abe58b2e9bb0"
SPDX_TEXT_SHA256 = {
    "Apache-2.0": "074e6e32c86a4c0ef8b3ed25b721ca23aca83df277cd88106ef7177c354615ff",
    "BSL-1.0": "84c6ef3ea9e3254a54d0acf5d3e0c61ae011b8fef7dd6940591cf060e6380a8f",
    "CC0-1.0": "a2010f343487d3f7618affe54f789f5487602331c0a8d03f49e9a7c547cf0499",
    "LGPL-2.1-or-later": "5749785c8bdefafcb5d798270ed0a967036fe2ca63dcedade1627565dfef81d2",
    "LLVM-exception": "e34c58338bd89d43e709e226610d8f32b3e3c47f4ad9a99a8dc1d4ac7842488e",
    "MIT": "b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5",
    "MPL-2.0": "66a3107d5ad6a058aab753eaac2047ccb2ed0e39465dd0fe5844da3e300d5172",
    "OFL-1.1": "8eea8287e5876b539670cadb82e99f9a7afddec6f6730811be1daf25d2e9bcfd",
    "Ubuntu-font-1.0": "2f0015108d68627bd788d313f529c21ff4da2c2c42a5e1f3883acc83480f9002",
    "Zlib": "bfb1112d49db5b1daecdfef24bd7e2f3ea0bafb33aa67aa0ab51e2bf8407c03d",
}
SPDX_OPERATORS = {"AND", "OR", "WITH"}


class LicenseBundleError(RuntimeError):
    """A complete license bundle cannot be proven from the current inputs."""


def declared_license_ids(expression: str) -> list[str]:
    """Return every license/exception identifier in Cargo's declared expression.

    Older crates still use ``MIT/Apache-2.0``. Treating the slash as a choice is
    conservative for notice staging: both complete texts are bundled.
    """

    tokens = re.findall(r"[A-Za-z0-9][A-Za-z0-9.+-]*", expression)
    return sorted({token for token in tokens if token.upper() not in SPDX_OPERATORS})


def spdx_text_url(identifier: str) -> str:
    return (
        "https://raw.githubusercontent.com/spdx/license-list-data/"
        f"{SPDX_LICENSE_LIST_REVISION}/text/{identifier}.txt"
    )


def load_spdx_texts(cache_dir: Path, allow_download: bool) -> dict[str, bytes]:
    """Load pinned canonical license texts, verifying the cache on every use."""

    cache_dir.mkdir(parents=True, exist_ok=True)
    texts: dict[str, bytes] = {}
    issues: list[str] = []
    for identifier, expected in sorted(SPDX_TEXT_SHA256.items()):
        target = cache_dir / f"{expected}--{safe_component(identifier)}.txt"
        content: bytes | None = None
        if target.is_file():
            candidate = target.read_bytes()
            if sha256_bytes(candidate) == expected:
                content = candidate
            else:
                target.unlink()
        if content is None and allow_download:
            try:
                with urllib.request.urlopen(spdx_text_url(identifier), timeout=30) as response:
                    candidate = response.read(MAX_LICENSE_FILE_BYTES + 1)
            except (OSError, urllib.error.URLError) as error:
                issues.append(f"{identifier}: could not download pinned SPDX text: {error}")
                continue
            if len(candidate) > MAX_LICENSE_FILE_BYTES:
                issues.append(f"{identifier}: pinned SPDX text exceeds the size limit")
                continue
            actual = sha256_bytes(candidate)
            if actual != expected:
                issues.append(
                    f"{identifier}: pinned SPDX text hash mismatch; expected {expected}, got {actual}"
                )
                continue
            deterministic_write(target, candidate)
            content = candidate
        if content is None:
            issues.append(
                f"{identifier}: pinned SPDX text is not cached; rerun with --download-license-texts"
            )
        else:
            texts[identifier] = content
    if issues:
        raise LicenseBundleError(
            "Canonical license texts are incomplete:\n  - " + "\n  - ".join(issues)
        )
    return texts


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def locate_cargo() -> str:
    override = os.environ.get("CARGO", "").strip()
    if override:
        candidate = Path(override).expanduser()
        if candidate.is_file():
            return str(candidate)
        raise LicenseBundleError(f"CARGO={override} does not point to a Cargo executable.")
    discovered = shutil.which("cargo")
    if discovered:
        return discovered
    user_cargo = Path.home() / ".cargo" / "bin" / "cargo"
    if user_cargo.is_file():
        return str(user_cargo)
    raise LicenseBundleError("Could not locate Cargo; set CARGO to its executable path.")


def load_cargo_metadata(project_root: Path, metadata_json: Path | None = None) -> dict[str, Any]:
    if metadata_json is not None:
        try:
            return json.loads(metadata_json.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise LicenseBundleError(
                f"Could not read Cargo metadata fixture '{metadata_json}': {error}"
            ) from error

    command = [
        locate_cargo(),
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--manifest-path",
        str(project_root / "Cargo.toml"),
    ]
    result = subprocess.run(
        command,
        cwd=project_root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "no Cargo diagnostic"
        raise LicenseBundleError(
            f"cargo metadata --locked failed ({result.returncode}): {detail}"
        )
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise LicenseBundleError(f"cargo metadata returned invalid JSON: {error}") from error


def load_cargo_lock(lockfile: Path) -> list[dict[str, Any]]:
    try:
        payload = tomllib.loads(lockfile.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise LicenseBundleError(f"Could not read Cargo.lock '{lockfile}': {error}") from error
    packages = payload.get("package")
    if not isinstance(packages, list):
        raise LicenseBundleError(f"Cargo.lock '{lockfile}' has no [[package]] entries.")
    return packages


def dependency_closure(metadata: dict[str, Any], root_package: str) -> list[dict[str, Any]]:
    packages = metadata.get("packages")
    resolve = metadata.get("resolve")
    if not isinstance(packages, list) or not isinstance(resolve, dict):
        raise LicenseBundleError("Cargo metadata is missing packages or the resolved graph.")

    packages_by_id = {
        package.get("id"): package
        for package in packages
        if isinstance(package, dict) and isinstance(package.get("id"), str)
    }
    workspace_members = set(metadata.get("workspace_members") or [])
    candidates = [
        package
        for package in packages_by_id.values()
        if package.get("name") == root_package
    ]
    workspace_candidates = [
        package for package in candidates if package.get("id") in workspace_members
    ]
    if len(workspace_candidates) == 1:
        root = workspace_candidates[0]
    elif len(candidates) == 1:
        root = candidates[0]
    else:
        labels = ", ".join(sorted(str(package.get("id")) for package in candidates))
        raise LicenseBundleError(
            f"Expected one Cargo package named '{root_package}', found {len(candidates)}"
            + (f": {labels}" if labels else ".")
        )

    nodes = resolve.get("nodes")
    if not isinstance(nodes, list):
        raise LicenseBundleError("Cargo metadata resolve graph has no nodes.")
    nodes_by_id = {
        node.get("id"): node
        for node in nodes
        if isinstance(node, dict) and isinstance(node.get("id"), str)
    }

    pending = [root["id"]]
    visited: set[str] = set()
    while pending:
        package_id = pending.pop()
        if package_id in visited:
            continue
        if package_id not in packages_by_id:
            raise LicenseBundleError(
                f"Resolved dependency '{package_id}' is absent from Cargo metadata packages."
            )
        node = nodes_by_id.get(package_id)
        if node is None:
            raise LicenseBundleError(
                f"Cargo metadata has no resolved node for package '{package_id}'."
            )
        visited.add(package_id)
        detailed_dependencies = node.get("deps")
        if isinstance(detailed_dependencies, list):
            dependencies = []
            for dependency in detailed_dependencies:
                if not isinstance(dependency, dict) or not isinstance(
                    dependency.get("pkg"), str
                ):
                    raise LicenseBundleError(
                        f"Cargo metadata node '{package_id}' has an invalid dependency edge."
                    )
                dependency_kinds = dependency.get("dep_kinds") or []
                if not isinstance(dependency_kinds, list):
                    raise LicenseBundleError(
                        f"Cargo metadata node '{package_id}' has invalid dependency kinds."
                    )
                if not all(isinstance(kind, dict) for kind in dependency_kinds):
                    raise LicenseBundleError(
                        f"Cargo metadata node '{package_id}' has a malformed dependency kind."
                    )
                # Development-only crates are not part of a packaged runtime. Normal and build
                # dependencies remain included for every declared target so the same bundle can
                # certify macOS and the Windows/Linux portability builds.
                if dependency_kinds and all(
                    kind.get("kind") == "dev" for kind in dependency_kinds
                ):
                    continue
                dependencies.append(dependency["pkg"])
        else:
            # Cargo metadata fixtures and older compatible producers may expose only the flat
            # dependency list. It is still a complete fail-closed closure, just without the
            # opportunity to remove development-only edges.
            dependencies = node.get("dependencies") or []
            if not isinstance(dependencies, list) or not all(
                isinstance(dependency, str) for dependency in dependencies
            ):
                raise LicenseBundleError(
                    f"Cargo metadata node '{package_id}' has invalid dependencies."
                )
        pending.extend(dependencies)

    return [packages_by_id[package_id] for package_id in visited]


def lock_entry_for(
    package: dict[str, Any], lock_packages: list[dict[str, Any]]
) -> dict[str, Any]:
    name = package.get("name")
    version = package.get("version")
    source = package.get("source")
    candidates = [
        entry
        for entry in lock_packages
        if entry.get("name") == name and str(entry.get("version")) == str(version)
    ]
    exact = [entry for entry in candidates if entry.get("source") == source]
    if len(exact) == 1:
        return exact[0]
    if source is None:
        source_less = [entry for entry in candidates if entry.get("source") is None]
        if len(source_less) == 1:
            return source_less[0]
    labels = ", ".join(sorted(str(entry.get("source")) for entry in candidates))
    raise LicenseBundleError(
        f"Cargo.lock does not uniquely match {name} {version} from {source!r}"
        + (f"; candidates: {labels}" if labels else ".")
    )


def package_root(package: dict[str, Any]) -> Path:
    manifest = package.get("manifest_path")
    if not isinstance(manifest, str) or not manifest:
        raise LicenseBundleError(
            f"Package {package.get('name')} {package.get('version')} has no manifest_path."
        )
    path = Path(manifest)
    if not path.is_file():
        raise LicenseBundleError(
            f"Package {package.get('name')} {package.get('version')} manifest is missing: {path}"
        )
    return path.parent.resolve()


def relative_to_or_none(path: Path, parent: Path) -> Path | None:
    try:
        return path.relative_to(parent)
    except ValueError:
        return None


def source_descriptor(
    package: dict[str, Any], root: Path, workspace_root: Path
) -> tuple[str, str]:
    source = package.get("source")
    if isinstance(source, str) and source:
        if source.startswith("registry+"):
            return "registry", source
        if source.startswith("git+"):
            return "git", source
        return "external", source
    relative = relative_to_or_none(root, workspace_root)
    if relative is not None:
        display = "." if not relative.parts else relative.as_posix()
        return "path", f"path+workspace:{display}"
    return "path", f"path+file:{root.as_posix()}"


def evidence_origin(path: Path, root: Path, workspace_root: Path, declared: str | None) -> str:
    relative = relative_to_or_none(path, root)
    if relative is not None:
        return relative.as_posix()
    workspace_relative = relative_to_or_none(path, workspace_root)
    if workspace_relative is not None:
        return f"workspace:{workspace_relative.as_posix()}"
    if declared:
        return f"declared-license-file:{declared}"
    return f"external:{path.name}"


def is_license_name(path: Path) -> bool:
    return bool(LICENSE_NAME.match(path.name))


def discover_license_files(
    package: dict[str, Any], root: Path, workspace_root: Path
) -> tuple[list[tuple[Path, str]], list[str]]:
    discovered: dict[Path, str] = {}
    issues: list[str] = []
    declared_raw = package.get("license_file")
    declared = declared_raw.strip() if isinstance(declared_raw, str) else None

    if declared:
        candidate = Path(declared)
        if not candidate.is_absolute():
            candidate = root / candidate
        candidate = candidate.resolve()
        if not candidate.is_file():
            issues.append(
                f"{package.get('name')} {package.get('version')}: declared license-file "
                f"'{declared}' is missing"
            )
        else:
            discovered[candidate] = evidence_origin(
                candidate, root, workspace_root, declared
            )

    def add_root_candidates(directory: Path) -> None:
        if not directory.is_dir():
            return
        for child in sorted(directory.iterdir(), key=lambda value: value.name.casefold()):
            if child.is_file() and is_license_name(child):
                resolved = child.resolve()
                discovered.setdefault(
                    resolved, evidence_origin(resolved, root, workspace_root, declared)
                )
            elif child.is_dir() and LICENSE_DIRECTORY.match(child.name):
                for nested in sorted(
                    (path for path in child.rglob("*") if path.is_file()),
                    key=lambda value: value.as_posix().casefold(),
                ):
                    resolved = nested.resolve()
                    discovered.setdefault(
                        resolved,
                        evidence_origin(resolved, root, workspace_root, declared),
                    )

    add_root_candidates(root)
    if package.get("source") is None and root != workspace_root:
        add_root_candidates(workspace_root)

    files: list[tuple[Path, str]] = []
    for path, origin in sorted(discovered.items(), key=lambda item: item[1].casefold()):
        try:
            size = path.stat().st_size
        except OSError as error:
            issues.append(
                f"{package.get('name')} {package.get('version')}: cannot inspect "
                f"license evidence '{origin}': {error}"
            )
            continue
        if size > MAX_LICENSE_FILE_BYTES:
            issues.append(
                f"{package.get('name')} {package.get('version')}: license evidence "
                f"'{origin}' is {size} bytes (limit {MAX_LICENSE_FILE_BYTES})"
            )
            continue
        files.append((path, origin))
    return files, issues


def safe_component(value: str) -> str:
    normalized = re.sub(r"[^A-Za-z0-9._+-]+", "-", value).strip("-.")
    return normalized or "package"


def package_bundle_name(name: str, version: str, source: str) -> str:
    source_hash = hashlib.sha256(source.encode("utf-8")).hexdigest()[:12]
    return f"{safe_component(name)}-{safe_component(version)}-{source_hash}"


def build_manifest(
    metadata: dict[str, Any],
    lock_packages: list[dict[str, Any]],
    root_package: str,
    project_root: Path,
    canonical_license_texts: dict[str, bytes] | None = None,
) -> tuple[dict[str, Any], dict[str, bytes]]:
    workspace_raw = metadata.get("workspace_root")
    workspace_root = (
        Path(workspace_raw).resolve()
        if isinstance(workspace_raw, str) and workspace_raw
        else project_root.resolve()
    )
    packages = dependency_closure(metadata, root_package)
    issues: list[str] = []
    records: list[dict[str, Any]] = []
    bundled_files: dict[str, bytes] = {}

    for package in packages:
        name = str(package.get("name"))
        version = str(package.get("version"))
        try:
            root = package_root(package)
            lock_entry = lock_entry_for(package, lock_packages)
            source_kind, source = source_descriptor(package, root, workspace_root)
        except LicenseBundleError as error:
            issues.append(str(error))
            continue

        license_expression = package.get("license")
        if isinstance(license_expression, str):
            license_expression = license_expression.strip() or None
        else:
            license_expression = None
        evidence, evidence_issues = discover_license_files(
            package, root, workspace_root
        )
        issues.extend(evidence_issues)
        if not license_expression and not evidence:
            issues.append(
                f"{name} {version}: neither a declared license nor "
                "LICENSE*/COPYING*/NOTICE* evidence is available"
            )

        package_dir = package_bundle_name(name, version, source)
        evidence_records: list[dict[str, Any]] = []
        for index, (license_path, origin) in enumerate(evidence, start=1):
            try:
                content = license_path.read_bytes()
            except OSError as error:
                issues.append(
                    f"{name} {version}: cannot read license evidence '{origin}': {error}"
                )
                continue
            filename = safe_component(license_path.name)
            bundle_path = f"packages/{package_dir}/{index:02d}-{filename}"
            bundled_files[bundle_path] = content
            evidence_records.append(
                {
                    "source_path": origin,
                    "bundle_path": bundle_path,
                    "sha256": sha256_bytes(content),
                    "bytes": len(content),
                }
            )

        if not evidence_records and license_expression:
            identifiers = declared_license_ids(license_expression)
            unavailable = [
                identifier
                for identifier in identifiers
                if not canonical_license_texts
                or identifier not in canonical_license_texts
            ]
            if not identifiers or unavailable:
                detail = ", ".join(unavailable or identifiers) or license_expression
                issues.append(
                    f"{name} {version}: no packaged license evidence and pinned canonical "
                    f"text is unavailable for {detail}"
                )
            else:
                for identifier in identifiers:
                    content = canonical_license_texts[identifier]
                    bundle_path = f"canonical/{safe_component(identifier)}.txt"
                    existing = bundled_files.get(bundle_path)
                    if existing is not None and existing != content:
                        issues.append(
                            f"{name} {version}: conflicting canonical text for {identifier}"
                        )
                        continue
                    bundled_files[bundle_path] = content
                    evidence_records.append(
                        {
                            "source_path": (
                                f"SPDX-License-List-Data@{SPDX_LICENSE_LIST_REVISION}:"
                                f"text/{identifier}.txt"
                            ),
                            "bundle_path": bundle_path,
                            "sha256": sha256_bytes(content),
                            "bytes": len(content),
                        }
                    )

        checksum = lock_entry.get("checksum")
        if checksum is not None:
            checksum = str(checksum)
        source_revision = None
        if source_kind == "git" and "#" in source:
            source_revision = source.rsplit("#", 1)[1] or None
        if source_kind == "registry" and not checksum:
            issues.append(
                f"{name} {version}: registry package has no Cargo.lock checksum"
            )
        if source_kind == "git" and not source_revision:
            issues.append(
                f"{name} {version}: Git source is not pinned to an exact revision"
            )
        records.append(
            {
                "name": name,
                "version": version,
                "source_kind": source_kind,
                "source": source,
                "source_revision": source_revision,
                "checksum": checksum,
                # Preserve Cargo's complete expression verbatim. In particular this must
                # never collapse `ring`'s `Apache-2.0 AND ISC` into an OR choice.
                "spdx": license_expression,
                "declared_license_file": package.get("license_file"),
                "license_evidence": evidence_records,
            }
        )

    if issues:
        formatted = "\n".join(f"  - {issue}" for issue in sorted(set(issues)))
        raise LicenseBundleError(
            "Rust dependency license audit failed; no bundle was written:\n" + formatted
        )

    records.sort(
        key=lambda record: (
            record["name"].casefold(),
            record["version"],
            record["source"],
        )
    )
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "root_package": root_package,
        "package_count": len(records),
        "packages": records,
    }
    return manifest, bundled_files


def markdown_escape(value: Any) -> str:
    if value is None:
        return "—"
    return str(value).replace("|", "\\|").replace("\n", " ")


def render_markdown(manifest: dict[str, Any]) -> str:
    lines = [
        "# Rust runtime dependency licenses",
        "",
        (
            f"Resolved fail-closed from `cargo metadata --locked` for "
            f"`{manifest['root_package']}`: **{manifest['package_count']} packages**."
        ),
        "",
        "The SPDX column preserves Cargo's complete declared expression; `AND` terms are cumulative.",
        "Checksums are Cargo.lock package checksums. Git revisions remain pinned in the source URL.",
        "",
        "| Package | Version | Source | Checksum | SPDX / declared license | Evidence |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for package in manifest["packages"]:
        evidence = package["license_evidence"]
        evidence_text = "<br>".join(
            f"[{markdown_escape(item['source_path'])}]({markdown_escape(item['bundle_path'])})"
            for item in evidence
        ) or "declared expression only"
        lines.append(
            "| "
            + " | ".join(
                [
                    markdown_escape(package["name"]),
                    markdown_escape(package["version"]),
                    markdown_escape(package["source"]),
                    markdown_escape(package["checksum"]),
                    markdown_escape(package["spdx"]),
                    evidence_text,
                ]
            )
            + " |"
        )
    lines.append("")
    return "\n".join(lines)


def deterministic_write(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    path.chmod(0o644)
    os.utime(path, (DETERMINISTIC_MTIME, DETERMINISTIC_MTIME))


def stage_bundle(
    destination: Path,
    manifest: dict[str, Any],
    bundled_files: dict[str, bytes],
    replace: bool = False,
) -> None:
    destination = destination.resolve()
    protected = {Path("/").resolve(), Path.home().resolve(), Path.cwd().resolve()}
    if destination in protected or relative_to_or_none(Path.cwd().resolve(), destination) is not None:
        raise LicenseBundleError(
            f"Refusing unsafe license bundle destination '{destination}'."
        )
    if destination.exists() and (
        (destination / ".git").exists() or (destination / "Cargo.toml").is_file()
    ):
        raise LicenseBundleError(
            f"Refusing to replace project-like destination '{destination}'."
        )
    if destination.exists() and not replace:
        raise LicenseBundleError(
            f"Destination '{destination}' already exists; pass --replace to replace that exact directory."
        )
    destination.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(
        tempfile.mkdtemp(prefix=f".{destination.name}.tmp-", dir=destination.parent)
    )
    try:
        for relative, content in sorted(bundled_files.items()):
            deterministic_write(staging / relative, content)
        json_bytes = (
            json.dumps(manifest, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
        ).encode("utf-8")
        deterministic_write(staging / MANIFEST_JSON, json_bytes)
        deterministic_write(
            staging / MANIFEST_MARKDOWN,
            render_markdown(manifest).encode("utf-8"),
        )
        if destination.exists():
            shutil.rmtree(destination)
        os.replace(staging, destination)
        os.utime(destination, (DETERMINISTIC_MTIME, DETERMINISTIC_MTIME))
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--project-root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="Cargo workspace root (default: repository root).",
    )
    parser.add_argument(
        "--package",
        default="neoflash-editor",
        help="Root Cargo package whose resolved dependency closure is bundled.",
    )
    parser.add_argument(
        "--lockfile",
        type=Path,
        help="Cargo.lock path (default: <project-root>/Cargo.lock).",
    )
    parser.add_argument(
        "--metadata-json",
        type=Path,
        help="Use pre-generated Cargo metadata JSON (fixtures/offline verification only).",
    )
    parser.add_argument(
        "--license-cache",
        type=Path,
        help="Pinned canonical SPDX text cache (default: target/rust-license-text-cache).",
    )
    parser.add_argument(
        "--download-license-texts",
        action="store_true",
        help="Download missing canonical texts from the pinned SPDX license-list revision.",
    )
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument(
        "--audit",
        action="store_true",
        help="Resolve, hash and validate the complete closure without writing a bundle.",
    )
    mode.add_argument(
        "--dest",
        type=Path,
        help="Write the complete deterministic bundle into this directory.",
    )
    parser.add_argument(
        "--replace",
        action="store_true",
        help="Replace the exact --dest directory if it already exists.",
    )
    args = parser.parse_args(argv)
    if args.replace and args.dest is None:
        parser.error("--replace requires --dest")
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    project_root = args.project_root.resolve()
    lockfile = (args.lockfile or project_root / "Cargo.lock").resolve()
    try:
        metadata = load_cargo_metadata(project_root, args.metadata_json)
        lock_packages = load_cargo_lock(lockfile)
        license_cache = (
            args.license_cache or project_root / "target" / "rust-license-text-cache"
        ).resolve()
        canonical_license_texts = load_spdx_texts(
            license_cache, args.download_license_texts
        )
        manifest, bundled_files = build_manifest(
            metadata,
            lock_packages,
            args.package,
            project_root,
            canonical_license_texts,
        )
        if args.dest is not None:
            stage_bundle(args.dest, manifest, bundled_files, args.replace)
            print(
                f"Staged {manifest['package_count']} Rust dependency license records "
                f"and {len(bundled_files)} evidence files into {args.dest.resolve()}."
            )
        else:
            evidence_count = sum(
                len(package["license_evidence"])
                for package in manifest["packages"]
            )
            print(
                f"Rust dependency license audit passed: {manifest['package_count']} packages, "
                f"{evidence_count} license evidence files."
            )
        return 0
    except LicenseBundleError as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
