#!/usr/bin/env python3
"""Survey binary Figma (.fig) and REST/plugin (.figma.json) import over local corpora.

Runs `neoflash-import inspect` and `neoflash-import import` (to an ephemeral
`.nflash` file) on every `*.fig` and `*.figma.json` under the target folder(s)
(default: `interface/figma` and `target/figma-corpus`) and prints aggregate
results only: importable count, format versions, timing/size totals, first
blocking cause per file, and approximation warning frequencies. Private file
names are printed only with `--files`. Nothing is written into the repository;
use `--out` for raw JSON reports (keep under ignored `target/`).
"""

import argparse
import collections
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def resolve_common_root() -> Path:
    try:
        proc = subprocess.run(
            ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        common_git = Path(proc.stdout.strip())
        if common_git.name == ".git" and common_git.parent.is_dir():
            return common_git.parent
    except Exception:
        pass
    return ROOT


def default_folders() -> list[Path]:
    common = resolve_common_root()
    candidates = [
        ROOT / "interface/figma",
        common / "interface/figma",
        ROOT / "target/figma-corpus",
        common / "target/figma-corpus",
    ]
    out: list[Path] = []
    seen: set[Path] = set()
    for path in candidates:
        try:
            resolved = path.resolve()
        except Exception:
            continue
        if resolved.is_dir() and resolved not in seen:
            seen.add(resolved)
            out.append(resolved)
    return out


def collect_sources(folders: list[Path]) -> list[tuple[Path, Path]]:
    found: list[tuple[Path, Path]] = []
    seen: set[Path] = set()
    for folder in folders:
        if not folder.is_dir():
            continue
        for path in sorted(folder.rglob("*"), key=lambda p: str(p).lower()):
            if not path.is_file():
                continue
            low = path.name.lower()
            if not (low.endswith(".fig") or low.endswith(".figma.json")):
                continue
            resolved = path.resolve()
            if resolved not in seen:
                seen.add(resolved)
                found.append((folder, resolved))
    return found


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "folders",
        nargs="*",
        type=Path,
        help="folders to scan (default: interface/figma and target/figma-corpus)",
    )
    parser.add_argument("--files", action="store_true", help="list each file's result")
    parser.add_argument("--out", type=Path, help="write raw reports here (under target/)")
    parser.add_argument("--no-build", action="store_true", help="reuse existing release binary")
    parser.add_argument(
        "--strict",
        action="store_true",
        help="exit non-zero if no files are found or any document fails import/validation",
    )
    args = parser.parse_args()

    folders = [p.resolve() for p in args.folders] if args.folders else default_folders()
    sources = collect_sources(folders)
    if not sources:
        msg = "No .fig or .figma.json files found in target folders."
        if args.strict:
            sys.exit(msg)
        print(msg)
        return

    if not args.no_build:
        subprocess.run(
            ["cargo", "build", "--locked", "--release", "-q", "-p", "neoflash-import"],
            cwd=ROOT,
            check=True,
        )
    binary = ROOT / "target/release/neoflash-import"
    if not binary.is_file():
        sys.exit(f"Missing binary: {binary}")

    blocked: collections.Counter[str] = collections.Counter()
    warned_docs: collections.Counter[str] = collections.Counter()
    warned_total: collections.Counter[str] = collections.Counter()
    versions: collections.Counter[str] = collections.Counter()

    importable = 0
    total_src_bytes = 0
    total_nflash_bytes = 0
    max_nflash_bytes = 0
    total_symbols = 0
    total_layers = 0
    total_elements = 0
    total_elapsed = 0.0
    max_elapsed = 0.0
    rows: list[tuple[str, str, str, float, int]] = []

    with tempfile.TemporaryDirectory(prefix="neoflash-figma-survey-") as tmp_dir:
        tmp_root = Path(tmp_dir)
        for idx, (base_folder, path) in enumerate(sources):
            src_bytes = path.stat().st_size
            total_src_bytes += src_bytes
            rel_name = str(path.relative_to(base_folder))
            t0 = time.perf_counter()
            inspect_run = subprocess.run(
                [str(binary), "inspect", str(path)],
                capture_output=True,
                text=True,
                timeout=300,
                check=False,
            )
            inspect_dt = time.perf_counter() - t0

            if args.out:
                out_file = args.out / f"doc_{idx:03d}.json"
                out_file.parent.mkdir(parents=True, exist_ok=True)
                out_file.write_text(inspect_run.stdout or inspect_run.stderr)

            if inspect_run.returncode != 0:
                total_elapsed += inspect_dt
                max_elapsed = max(max_elapsed, inspect_dt)
                first_line = (inspect_run.stderr.strip().splitlines() or ["unknown error"])[0]
                # Scrub source path if present so default output never leaks private names.
                sanitized = first_line.replace(str(path), "<file>").replace(path.name, "<file>")
                cause = f"inspect_error: {sanitized[:140]}"
                blocked[cause] += 1
                rows.append((rel_name, "-", cause, inspect_dt, 0))
                continue

            report = json.loads(inspect_run.stdout)
            ver = report.get("version") or "unknown"
            versions[ver] += 1

            diagnostics = report.get("diagnostics", [])
            errors = [d for d in diagnostics if d.get("severity") == "error"]
            doc_warn_codes = set()
            for d in diagnostics:
                if d.get("severity") == "warning":
                    code = d.get("code", "unknown")
                    doc_warn_codes.add(code)
                    warned_total[code] += 1
            for code in doc_warn_codes:
                warned_docs[code] += 1

            if not report.get("can_import"):
                total_elapsed += inspect_dt
                max_elapsed = max(max_elapsed, inspect_dt)
                cause = (
                    f"{errors[0]['code']}: {errors[0]['message'][:120]}"
                    if errors
                    else "can_import=false"
                )
                blocked[cause] += 1
                rows.append((rel_name, ver, cause, inspect_dt, 0))
                continue

            # Also verify full `.nflash` serialization & size budget (`<= 128 MiB`).
            dest_nflash = tmp_root / f"imported_{idx:04d}.nflash"
            t1 = time.perf_counter()
            import_run = subprocess.run(
                [str(binary), "import", str(path), str(dest_nflash)],
                capture_output=True,
                text=True,
                timeout=300,
                check=False,
            )
            import_dt = time.perf_counter() - t1
            file_dt = max(inspect_dt, import_dt)
            total_elapsed += file_dt
            max_elapsed = max(max_elapsed, file_dt)

            if import_run.returncode != 0 or not dest_nflash.is_file():
                first_line = (import_run.stderr.strip().splitlines() or ["nflash export failed"])[0]
                sanitized = first_line.replace(str(path), "<file>").replace(path.name, "<file>")
                cause = f"nflash_export: {sanitized[:140]}"
                blocked[cause] += 1
                rows.append((rel_name, ver, cause, file_dt, 0))
                continue

            nflash_bytes = dest_nflash.stat().st_size
            dest_nflash.unlink(missing_ok=True)

            stats = report.get("statistics", {})
            total_nflash_bytes += nflash_bytes
            max_nflash_bytes = max(max_nflash_bytes, nflash_bytes)
            total_symbols += int(stats.get("symbols", 0))
            total_layers += int(stats.get("layers", 0))
            total_elements += int(stats.get("elements", 0))
            importable += 1
            rows.append((rel_name, ver, "imports", file_dt, nflash_bytes))

    print(
        f"{importable} of {len(sources)} Figma documents import and serialize to valid .nflash"
    )
    print(
        f"Aggregate source: {total_src_bytes / (1024 * 1024):.2f} MiB | "
        f".nflash total: {total_nflash_bytes / (1024 * 1024):.2f} MiB "
        f"(max {max_nflash_bytes / (1024 * 1024):.2f} MiB) | "
        f"symbols: {total_symbols} | layers: {total_layers} | elements: {total_elements} | "
        f"time: {total_elapsed:.2f}s (max {max_elapsed:.2f}s/doc)"
    )
    if versions:
        ver_summary = ", ".join(f"{v}: {c}" for v, c in versions.most_common())
        print(f"Versions: {ver_summary}")

    if blocked:
        print("\nFirst blocking cause (documents):")
        for cause, count in blocked.most_common():
            print(f"  {count:4}  {cause}")

    if warned_docs:
        print("\nReported approximations (documents / total diagnostics):")
        for code, count in warned_docs.most_common():
            print(f"  {count:4} docs ({warned_total[code]:6} total)  {code}")

    if args.files:
        print()
        for name, ver, result, dt, nf_bytes in rows:
            print(
                f"{name}\t{ver}\t{result}\t{dt:.2f}s\t{nf_bytes / (1024 * 1024):.2f} MiB"
            )

    if args.strict and importable != len(sources):
        sys.exit(1)


if __name__ == "__main__":
    main()
