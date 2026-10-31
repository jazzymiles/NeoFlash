#!/usr/bin/env python3
"""Guard the NF2 design system against unreviewed edits.

The design system (Figma export, its Rust form and its rules) is owned by the design track.
Implementation agents (Codex) may READ it and USE it, never change it. Needed changes are written
as requests in docs/NF2-REQUESTS.md.

  python3 scripts/design-lock.py            # check: fail if a locked file differs from the lock
  python3 scripts/design-lock.py --write    # design owner only: re-pin after an approved change

The lock lists every versioned file under the locked roots with its SHA-256.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LOCK = ROOT / "interface/design-system/DESIGN-LOCK.json"
LOCKED = [
    "interface/design-system",
    "crates/neoflash-ui",
    "docs/DESIGN-SYSTEM.md",
]
EXCLUDED = {"interface/design-system/DESIGN-LOCK.json"}


def tracked_files() -> list[str]:
    """Versioned + new (not ignored) files under the locked roots."""
    out = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", *LOCKED],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    files = sorted({line for line in out.splitlines() if line and line not in EXCLUDED})
    return [f for f in files if (ROOT / f).is_file()]


def digest(path: str) -> str:
    return hashlib.sha256((ROOT / path).read_bytes()).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--write", action="store_true", help="re-pin the lock (design owner only)")
    args = parser.parse_args()
    current = {path: digest(path) for path in tracked_files()}
    if args.write:
        LOCK.write_text(
            json.dumps(
                {
                    "note": "NF2 design-system lock. Do not edit by hand. Implementation agents must not "
                    "change locked files; write requests to docs/NF2-REQUESTS.md.",
                    "roots": LOCKED,
                    "files": current,
                },
                indent=1,
                sort_keys=True,
            )
            + "\n"
        )
        print(f"design lock written: {len(current)} files")
        return 0
    if not LOCK.exists():
        print("design lock missing: interface/design-system/DESIGN-LOCK.json", file=sys.stderr)
        return 1
    pinned = json.loads(LOCK.read_text())["files"]
    changed = sorted(p for p in current if pinned.get(p) != current[p] and p in pinned)
    added = sorted(p for p in current if p not in pinned)
    removed = sorted(p for p in pinned if p not in current)
    if not (changed or added or removed):
        print(f"design lock ok: {len(current)} files")
        return 0
    for label, items in (("changed", changed), ("added", added), ("removed", removed)):
        for item in items:
            print(f"design lock: {label}: {item}", file=sys.stderr)
    print(
        "\nThe NF2 design system is locked. Revert these edits and describe what you need in "
        "docs/NF2-REQUESTS.md (see AGENTS.md › Design system).",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
