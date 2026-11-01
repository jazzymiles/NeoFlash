#!/usr/bin/env python3
"""Survey binary/XFL FLA import over a local folder of real documents.

Runs `neoflash-import inspect` on every *.fla below the folder (default: the
git-ignored `examples/old flash files`) and prints aggregate results only:
importable count, first blocking cause per file, and which warnings (lossy
conversions) occur in how many documents. Private file names are printed only
with --files. Nothing is written into the repository; use --out for raw JSON
(keep it under ignored target/).
"""

import argparse
import collections
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
DEFAULT = ROOT / "examples/old flash files"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("folder", nargs="?", type=Path, default=DEFAULT)
    parser.add_argument("--files", action="store_true", help="list each file's result")
    parser.add_argument("--out", type=Path, help="write raw reports here (under target/)")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    if not args.folder.is_dir():
        sys.exit(f"No such folder: {args.folder}")
    if not args.no_build:
        subprocess.run(
            ["cargo", "build", "--release", "-q", "-p", "neoflash-import"], cwd=ROOT, check=True
        )
    binary = ROOT / "target/release/neoflash-import"
    sources = sorted(args.folder.rglob("*.fla"), key=lambda p: str(p).lower())
    blocked = collections.Counter()
    warned = collections.Counter()
    importable = 0
    rows = []
    for path in sources:
        run = subprocess.run(
            [str(binary), "inspect", str(path)],
            capture_output=True, text=True, timeout=300, check=False,
        )
        name = str(path.relative_to(args.folder))
        if args.out:
            target = args.out / (name.replace("/", "__") + ".json")
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(run.stdout or run.stderr)
        if run.returncode != 0:
            cause = "container: " + run.stderr.strip()[:120]
            blocked[cause] += 1
            rows.append((name, None, cause))
            continue
        report = json.loads(run.stdout)
        errors = [d for d in report["diagnostics"] if d["severity"] != "warning"]
        for code in {d["code"] for d in report["diagnostics"] if d["severity"] == "warning"}:
            warned[code] += 1
        if report["can_import"]:
            importable += 1
            rows.append((name, report.get("version"), "imports"))
        else:
            cause = f"{errors[0]['code']}: {errors[0]['message'][:100]}" if errors else "?"
            blocked[cause] += 1
            rows.append((name, report.get("version"), cause))
    print(f"{importable} of {len(sources)} documents import")
    if blocked:
        print("\nFirst blocking cause (documents):")
        for cause, count in blocked.most_common():
            print(f"  {count:4}  {cause}")
    if warned:
        print("\nReported approximations (documents):")
        for code, count in warned.most_common():
            print(f"  {count:4}  {code}")
    if args.files:
        print()
        for name, version, result in rows:
            print(f"{name}\t{version or '-'}\t{result}")


if __name__ == "__main__":
    main()
