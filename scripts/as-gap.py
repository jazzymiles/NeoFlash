#!/usr/bin/env python3
"""Compact ActionScript gap queries: one line per declaration, no JSON dumps.

Use this instead of opening docs/progress/actionscript-catalog.json (20 MB) or the reference
snapshot (78 MB). Output is sized for an agent's context window.

  python3 scripts/as-gap.py flash.geom.Point            # one class: every member, missing first
  python3 scripts/as-gap.py flash.display --classes     # package: per-class counts, most missing first
  python3 scripts/as-gap.py --stage 4 --classes         # AS3 priority stage (1-8) per-class counts
  python3 scripts/as-gap.py MovieClip --lang AS2        # AS2 class
  python3 scripts/as-gap.py flash.geom --missing        # only missing members of a package

Signatures come from docs/actionscript-reference/*.json when that local snapshot exists.
"""
import argparse
import collections
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CATALOG = ROOT / "docs/progress/actionscript-catalog.json"
REFERENCE = ROOT / "docs/actionscript-reference"
PRIORITIES = ROOT / "docs/progress/as3-priorities.json"
ORDER = {"missing": 0, "needs_review": 1, "partial": 2, "implemented_subset": 3}
SHORT = {"missing": "MISS", "needs_review": "REVW", "partial": "PART", "implemented_subset": "SUBS"}


def signatures():
    found = {}
    for name in ["as2.json", "as2-components.json", "as2-components-supplement.json", "as3.json"]:
        path = REFERENCE / name
        if path.exists():
            for ref in json.loads(path.read_text())["entries"]:
                key = (ref["language"].upper(), ref.get("owner") or "", ref["name"], ref["kind"])
                found.setdefault(key, ref.get("signature") or "")
    return found


def owner_of(entry):
    return entry.get("owner") or ("" if entry["kind"] != "class" else entry["name"])


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("target", nargs="?", default="", help="class or package prefix (qualified)")
    parser.add_argument("--lang", default="AS3", choices=["AS2", "AS3"])
    parser.add_argument("--stage", type=int, help="AS3 priority stage 1-8 (docs/progress/ACTIONSCRIPT.md)")
    parser.add_argument("--classes", action="store_true", help="per-class status counts instead of members")
    parser.add_argument("--missing", action="store_true", help="only missing / needs-review members")
    parser.add_argument("--limit", type=int, default=400)
    args = parser.parse_args()

    entries = [e for e in json.loads(CATALOG.read_text())["entries"] if e["language"] == args.lang]
    if args.stage:
        priorities = json.loads(PRIORITIES.read_text())
        stage_id = next(s["id"] for s in priorities["stages"] if s["order"] == args.stage)
        owners = {o for o, (_, stage) in priorities["ownerDefaults"].items() if stage == stage_id}
        entries = [e for e in entries if owner_of(e) in owners]
    if args.target:
        t = args.target
        entries = [e for e in entries if owner_of(e) == t or owner_of(e).startswith(t + ".")]
    if args.missing:
        entries = [e for e in entries if e["status"] in ("missing", "needs_review")]

    if args.classes:
        counts = collections.defaultdict(collections.Counter)
        for e in entries:
            counts[owner_of(e) or "<global>"][e["status"]] += 1
        rows = sorted(counts.items(), key=lambda kv: (-kv[1]["missing"], kv[0]))
        print(f"{'class':48} SUBS PART MISS REVW")
        for owner, c in rows[: args.limit]:
            print(f"{owner:48} {c['implemented_subset']:4} {c['partial']:4} {c['missing']:4} {c['needs_review']:4}")
        total = sum(counts.values(), collections.Counter())
        print(f"{len(rows)} classes · subset {total['implemented_subset']} · partial {total['partial']} · missing {total['missing']} · review {total['needs_review']}")
        return

    sigs = signatures()
    entries.sort(key=lambda e: (ORDER[e["status"]], owner_of(e), e["kind"], e["name"]))
    for e in entries[: args.limit]:
        sig = sigs.get((e["language"], e.get("owner") or "", e["name"], e["kind"]), "")
        print(f"{SHORT[e['status']]} {e['kind']:11} {owner_of(e)}.{e['name']}  {sig}".rstrip())
    if len(entries) > args.limit:
        print(f"… {len(entries) - args.limit} more (raise --limit or narrow the target)")


if __name__ == "__main__":
    main()
