#!/usr/bin/env python3
"""Offline integrity and importer regression checks for examples/fla-corpus."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "examples/fla-corpus"


def corpus_path(relative):
    path = (CORPUS / relative).resolve()
    if not path.is_relative_to(CORPUS.resolve()):
        raise ValueError(f"Path escapes corpus: {relative}")
    return path


def run_importer(binary, *args):
    return subprocess.run(
        [str(binary), *map(str, args)], cwd=ROOT, text=True,
        capture_output=True, timeout=30, check=False,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--integrity-only", action="store_true",
                        help="check source hashes without building/running Rust")
    parser.add_argument("--importer", type=Path,
                        help="use an already built CLI instead of cargo build")
    args = parser.parse_args()
    manifest = json.loads((CORPUS / "manifest.json").read_text())
    if manifest["schema_version"] != 1:
        raise ValueError("Unsupported corpus manifest version")
    seen = set()
    total = 0
    for item in manifest["files"]:
        name = item["path"]
        if name in seen:
            raise ValueError(f"Duplicate file in manifest: {name}")
        seen.add(name)
        data = corpus_path(name).read_bytes()
        if len(data) != item["bytes"] or hashlib.sha256(data).hexdigest() != item["sha256"]:
            raise ValueError(f"Source integrity failed: {name}; restore the pinned original")
        total += len(data)
    # Every source/companion/license belongs to the manifest. Local prose and
    # provenance metadata are exempt; generated build/report files belong in target/.
    for path in CORPUS.rglob("*"):
        if path.is_file() and path.name not in {"README.md", "metadata.json", "manifest.json", "VERIFICATION.md"}:
            if path.relative_to(CORPUS).as_posix() not in seen:
                raise ValueError(f"Unlisted corpus file: {path.relative_to(CORPUS)}")
    print(f"Integrity: {len(seen)} pinned files, {total:,} bytes", flush=True)
    if args.integrity_only:
        return 0
    if args.importer:
        binary = args.importer.resolve()
    else:
        cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")
        subprocess.run([cargo, "build", "--locked", "-p", "neoflash-import"],
                       cwd=ROOT, check=True)
        target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
        if not target.is_absolute():
            target = ROOT / target
        binary = target / "debug/neoflash-import"
    reports = ROOT / "target/fla-corpus/reports"
    reports.mkdir(parents=True, exist_ok=True)
    summary = []
    case_ids = set()
    with tempfile.TemporaryDirectory(prefix="imports-", dir=reports.parent) as temporary:
        for case in manifest["cases"]:
            case_id = case["id"]
            if (case_id in case_ids or not case_id or
                    any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-_" for c in case_id)):
                raise ValueError(f"Invalid or duplicate case id: {case_id}")
            case_ids.add(case_id)
            source = corpus_path(case["path"])
            result = run_importer(binary, "inspect", source)
            if result.returncode:
                raise ValueError(f"{case_id}: inspect failed: {result.stderr.strip()}")
            report = json.loads(result.stdout)
            (reports / f"{case_id}.json").write_text(result.stdout)
            expected = case["expected"]
            for key in ("format", "can_import"):
                if report[key] != expected[key]:
                    raise ValueError(f"{case_id}: {key}: expected {expected[key]!r}, got {report[key]!r}; review compatibility before updating manifest")
            diagnostics = report["diagnostics"]
            codes = {d["code"] for d in diagnostics}
            missing = set(expected.get("required_diagnostics", [])) - codes
            if missing:
                raise ValueError(f"{case_id}: missing actionable diagnostics {sorted(missing)}")
            for diagnostic in diagnostics:
                if not all(diagnostic.get(k) for k in ("code", "location", "message")):
                    raise ValueError(f"{case_id}: incomplete source-located diagnostic")
            output = Path(temporary) / f"{case_id}.nflash"
            conversion = run_importer(binary, "import", source, output)
            if expected["can_import"]:
                if conversion.returncode or not output.is_file():
                    raise ValueError(f"{case_id}: expected import: {conversion.stderr.strip()}")
                actual = json.loads(output.read_text())
                native = json.loads(corpus_path(case["native_project"]).read_text())
                if actual != native:
                    raise ValueError(f"{case_id}: imported document differs from checked-in native project")
                if report["statistics"] != expected["statistics"]:
                    raise ValueError(f"{case_id}: imported element/frame/script counts changed")
                # An attempted overwrite must leave the destination intact.
                before = output.read_bytes()
                retry = run_importer(binary, "import", source, output)
                if (retry.returncode != 1 or "exist" not in retry.stderr.lower()
                        or output.read_bytes() != before):
                    raise ValueError(f"{case_id}: import overwrote an existing project")
            else:
                if not any(d["severity"] == "error" for d in diagnostics):
                    raise ValueError(f"{case_id}: blocked input has no error diagnostic")
                if (conversion.returncode != 1 or output.exists()
                        or not conversion.stderr.startswith("Import blocked:")):
                    raise ValueError(f"{case_id}: blocked import produced a project or no explanation")
            summary.append({"id": case_id, "path": case["path"],
                            "format": report["format"], "version": report.get("version"),
                            "can_import": report["can_import"],
                            "diagnostic_codes": sorted(codes)})
            print(f"PASS {case_id}: {report['format']}, "
                  f"{'imported and compared' if report['can_import'] else 'explicitly blocked'}", flush=True)
    # Also ensure the read-only importer did not modify any corpus input.
    for item in manifest["files"]:
        if hashlib.sha256(corpus_path(item["path"]).read_bytes()).hexdigest() != item["sha256"]:
            raise ValueError(f"Importer changed source file: {item['path']}")
    (reports / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(f"Passed {len(summary)} cases. Reports: {reports.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"Corpus check failed: {error}", file=sys.stderr)
        sys.exit(1)
