#!/usr/bin/env python3
"""Build the public capability ledger from reviewed manifests and Git history."""

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import sys

from progress_catalog import load_catalog
from progress_targets import PUBLIC_STATUSES, load_targets, share

ROOT = Path(__file__).resolve().parents[1]
MANIFESTS = ("product.json",)
STATUSES = ("implemented", "partial", "planned")
ITEM_STATUSES = ("working", "partial", "planned")
LANGUAGES = (("as2", "AS2"), ("as3", "AS3"))
REPOSITORY = "https://github.com/jazzymiles/NeoFlash"
REPORT_START = "<!-- progress:start -->"
REPORT_END = "<!-- progress:end -->"
STATUS_DEFINITIONS = ("**Working**: the main behavior from the Adobe reference runs and a linked test proves it. "
                      "**Partial**: it runs, with the missing behavior named. **Missing**: no runtime path yet. "
                      "**Needs review**: not yet classified. Statuses change only through test-backed reviews; "
                      "this is not a Flash compatibility score.")


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def methodology(root, revision=None):
    raw = git(root, "show", f"{revision}:docs/progress/methodology.json") if revision else (root / "docs/progress/methodology.json").read_text()
    version = json.loads(raw)["version"]
    if type(version) is not int or version < 1:
        raise ValueError("Methodology version must be a positive integer")
    return version


def read_inventory(root, revision=None):
    categories, features = [], []
    files = set(git(root, "ls-tree", "--name-only", revision, "docs/progress/").splitlines()) if revision else None
    # Revisions before methodology v3 also kept supplementary AS family notes; they are not counted.
    for name in MANIFESTS:
        path = f"docs/progress/{name}"
        if files is not None and path not in files:
            raise ValueError(f"Incomplete historical inventory at {revision}")
        content = git(root, "show", f"{revision}:{path}") if revision else (root / path).read_text()
        data = json.loads(content)
        if data.get("schemaVersion") != 1:
            raise ValueError(f"{path}: unsupported schemaVersion")
        categories.extend(data["categories"])
        features.extend(data["features"])
    validate_inventory(root, categories, features, revision, require_items=methodology(root, revision) >= 3)
    return categories, features


def derived_status(items):
    statuses = {item["status"] for item in items}
    return "implemented" if statuses == {"working"} else "planned" if statuses == {"planned"} else "partial"


def validate_items(feature):
    items = feature.get("items")
    fid = feature["id"]
    if not isinstance(items, list) or not items:
        raise ValueError(f"{fid}: list the group's user-visible functions in items")
    seen = set()
    for item in items:
        if set(item) != {"id", "title", "status"}:
            raise ValueError(f"{fid}: each item needs exactly id, title and status")
        if not re.fullmatch(r"[a-z][a-z0-9-]*", item["id"]) or item["id"] in seen:
            raise ValueError(f"{fid}: invalid/duplicate item id {item['id']}")
        seen.add(item["id"])
        if not isinstance(item["title"], str) or not item["title"].strip():
            raise ValueError(f"{fid}/{item['id']}: missing title")
        if item["status"] not in ITEM_STATUSES:
            raise ValueError(f"{fid}/{item['id']}: status must be one of {', '.join(ITEM_STATUSES)}")
    if derived_status(items) != feature["status"]:
        raise ValueError(f"{fid}: status must be {derived_status(items)} for these items "
                         "(implemented = all working, planned = all planned, otherwise partial)")


def validate_inventory(root, categories, features, revision=None, require_items=False):
    ids, feature_ids, paths = set(), set(), set()
    for category in categories:
        cid = category["id"]
        if not re.fullmatch(r"[a-z][a-z0-9-]*", cid) or cid in ids:
            raise ValueError(f"Invalid/duplicate category: {cid}")
        ids.add(cid)
        for field in ("title", "description"):
            if not isinstance(category.get(field), str) or not category[field].strip():
                raise ValueError(f"{cid}: missing {field}")
    for feature in features:
        fid = feature["id"]
        if not re.fullmatch(r"[a-z][a-z0-9-]*", fid) or fid in feature_ids:
            raise ValueError(f"Invalid/duplicate feature: {fid}")
        feature_ids.add(fid)
        if feature.get("category") not in ids or feature.get("status") not in STATUSES:
            raise ValueError(f"{fid}: invalid category/status")
        for field in ("title", "scope", "limitations"):
            if not isinstance(feature.get(field), str) or not feature[field].strip():
                raise ValueError(f"{fid}: missing {field}")
        if require_items or "items" in feature:
            validate_items(feature)
        if not isinstance(feature.get("evidence"), list) or not feature["evidence"]:
            raise ValueError(f"{fid}: evidence is required, including a roadmap for planned work")
        for path in feature["evidence"]:
            if not isinstance(path, str) or path.startswith("/") or ".." in Path(path).parts or ":" in path:
                raise ValueError(f"{fid}: unsafe evidence path")
            paths.add(path)
    if not features or ids != {f["category"] for f in features}:
        raise ValueError("Every category must contain at least one capability")
    for path in sorted(paths):
        if revision:
            if git(root, "cat-file", "-t", f"{revision}:{path}") != "blob":
                raise ValueError(f"Evidence must be a file: {path}")
        elif not (root / path).is_file():
            raise ValueError(f"Missing evidence: {path}")


def counts(features):
    result = dict.fromkeys(STATUSES, 0)
    for feature in features:
        result[feature["status"]] += 1
    return {**result, "total": len(features)}


def item_counts(features):
    result = dict.fromkeys(ITEM_STATUSES, 0)
    for feature in features:
        for item in feature.get("items", []):
            result[item["status"]] += 1
    return {**result, "total": sum(result.values())}


def summary_table(categories, features):
    lines = ["| Area | Functions working | Partial | Planned | Capability groups (closed / partial / planned) |",
             "| --- | ---: | ---: | ---: | ---: |"]
    rows = [(category["title"], [f for f in features if f["category"] == category["id"]]) for category in categories]
    for title, members in rows + [("**All areas**", features)]:
        items, groups = item_counts(members), counts(members)
        lines.append(f"| {title} | {items['working']} of {items['total']} | {items['partial']} | {items['planned']} | "
                     f"{groups['implemented']} / {groups['partial']} / {groups['planned']} |")
    return "\n".join(lines)


def target_row(title, row):
    return (f"| {title} | {row['total']:,} | {row['working']:,} | {row['partial']:,} | {row['missing']:,} | {row['needs_review']:,} | "
            f"{share(row, 'working'):.1f}% | {share(row, 'working', 'partial'):.1f}% |")


def target_table(view):
    lines = ["| Flash Player area | Entries | Working | Partial | Missing | Needs review | Working | Working or partial |",
             "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"]
    lines.extend(target_row(f"{stage['order']}. {stage['title']}", stage["counts"]) for stage in view["stages"])
    last_now = max(stage["order"] for stage in view["stages"] if stage["now"])
    lines.append(target_row(f"**Now: areas 1–{last_now}**", view["now"]))
    lines.append(target_row("**All Flash Player areas**", view["target"]))
    outside = " · ".join(f"{item['title']} {item['counts']['total']:,}" for item in view["outside"])
    lines.extend(["", f"Outside the target and never in a percentage: {outside}."])
    return "\n".join(lines)


def last_review(catalog):
    meta = catalog["metadata"]
    dates = [run["verifiedAt"][:10] for run in meta.get("independentVerifications", [])]
    dates += [review["date"] for review in meta.get("statusReviews", [])]
    return max(dates, default=meta["auditedAt"][:10])


def targets_markdown(targets, heading="###"):
    lines = []
    for language in ("AS3", "AS2"):
        if language in targets:
            lines.extend([f"{heading} {targets[language]['title']}", "", target_table(targets[language]), ""])
    return lines


def catalog_report(catalog, targets):
    meta = catalog["metadata"]
    lines = ["# ActionScript declaration coverage", "", "Generated by `python3 scripts/progress.py --write-reports`. Counting rules: [README.md](README.md).", "",
             f"{STATUS_DEFINITIONS} Last status review: {last_review(catalog)}.", "",
             "## Flash Player target", "",
             "Both languages are counted against the same eight Flash Player areas. Areas 1–6 are the current priority; 7–8 follow. "
             "AS3 areas come from the [AS3 priority manifest](as3-priorities.json); AS2 areas from `scripts/progress_targets.py`.", ""]
    lines.extend(targets_markdown(targets))
    lines.extend(["## Full reference catalogue", "",
                  "Every documented declaration, including AIR, Flex and components, for provenance. These totals are not progress measures.", "",
                  "| Language | Scope | Entries | Working | Partial | Missing | Needs review |", "| --- | --- | ---: | ---: | ---: | ---: | ---: |"])
    for language, scopes in catalog["summary"]["byLanguageScope"].items():
        for scope, row in scopes.items():
            lines.append(f"| {language} | {scope} | {row['total']:,} | {row['implemented_subset']:,} | {row['partial']:,} | {row['missing']:,} | {row['needs_review']:,} |")
    lines.extend(["", f"Original audit `{meta['auditId']}` ({meta['auditedAt']}, baseline `{meta['baselineRevision'][:12]}`) set the starting statuses; later reviews below change them.", ""])
    reviews = meta.get("statusReviews", [])
    runs = meta.get("independentVerifications", [])
    if reviews or runs:
        lines.extend(["## Status reviews", "", "| Review | Date | Commit | Changed entries |", "| --- | --- | --- | --- |"])
        for review in reversed(reviews):
            changed = ", ".join(f"{count} → {status}" for status, count in review["changes"].items())
            lines.append(f"| `{review['id']}` | {review['date']} | `{review['commit'][:12]}` | {changed} |")
        for run in reversed(runs):
            lines.append(f"| `{run['id']}` | {run['verifiedAt'][:10]} | `{run['recordedHead'][:12]}` | earlier format; see its rule |")
        lines.append("")
    lines.extend(["## Inspect individual declarations", "", "[Complete normalized catalogue](actionscript-catalog.json) · [Rules and provenance](actionscript-audit.json)", "",
                  "```sh", "python3 scripts/progress_catalog.py --summary", "python3 scripts/progress_catalog.py --language AS3 --scope runtime --query MovieClip", "```", "",
                  "## Evidence rules", ""])
    for rule in catalog["rules"]:
        lines.extend([f"### {rule['id']} — {rule['title']}", "", rule["reason"], "", rule["limitations"], ""])
        for item in rule.get("reviewedEntries", []):
            status = "working" if item["status"] == "implemented_subset" else item["status"]
            lines.append(f"- `{item['catalogId']}` — **{status}**: {item['scopeNote']}")
        if rule.get("reviewedEntries"):
            lines.append("")
        refs = [f"[{r['path']}](../../{r['path']})" for r in rule["evidence"]]
        if refs:
            lines.extend(["Evidence: " + ", ".join(refs), ""])
    return "\n".join(lines).rstrip() + "\n"


def reports(root, categories, features, write=False):
    catalog = load_catalog(root)
    targets = load_targets(root, catalog)
    block = "\n".join([
        REPORT_START,
        "## Where we are",
        "",
        "The editor foundation works: nested timelines, vector tools, scripting and standalone HTML5. Full AS2/AS3, FLA/SWF import, portable media and DOM applications remain in development.",
        "",
        "### ActionScript: Flash Player areas",
        "",
        f"{STATUS_DEFINITIONS} Last status review: {last_review(catalog)}.",
        "",
        *targets_markdown(targets, "####"),
        "### Product functions",
        "",
        summary_table(categories, features),
        "",
        "Each capability group lists its user-visible functions; a group is closed only when all of them work. Counts are functions, not effort or overall product completion.",
        "",
        "[Current coverage](https://jazzymiles.github.io/NeoFlash/progress.html) · [Daily history](https://jazzymiles.github.io/NeoFlash/history.html) · [Detailed registry](docs/progress/DETAILS.md) · [ActionScript details](docs/progress/ACTIONSCRIPT.md) · [Counting rules](docs/progress/README.md)",
        "",
        "Generated from `docs/progress/`; GitHub Pages rebuilds on every push to `main`.",
        REPORT_END,
    ])
    readme_path = root / "README.md"
    readme = readme_path.read_text()
    if REPORT_START not in readme or REPORT_END not in readme:
        raise ValueError("README needs progress:start and progress:end markers")
    before, rest = readme.split(REPORT_START, 1)
    _, after = rest.split(REPORT_END, 1)
    updated = before + block + after
    labels = {"implemented": "Closed", "partial": "Partial", "planned": "Planned"}
    detail = ["# NeoFlash detailed capability notes", "", "Generated by `python3 scripts/progress.py --write-reports`. Edit `product.json`, not this file. [Counting rules](README.md); ActionScript uses the [declaration catalogue](ACTIONSCRIPT.md).", "", summary_table(categories, features)]
    for category in categories:
        detail.extend(["", f"## {category['title']}", "", category["description"]])
        for feature in [f for f in features if f["category"] == category["id"]]:
            detail.extend(["", f"### {feature['title']}", "", f"`{feature['id']}` · **{labels[feature['status']]}**", ""])
            detail.extend(f"- {item['status'].capitalize()}: {item['title']}" for item in feature.get("items", []))
            detail.extend(["", f"**Scope:** {feature['scope']}", "", f"**Limitations:** {feature['limitations']}", "", "**Evidence:** " + ", ".join(f"[{p}](../../{p})" for p in feature["evidence"])])
    details = "\n".join(detail) + "\n"
    detail_path = root / "docs/progress/DETAILS.md"
    audit_path = root / "docs/progress/ACTIONSCRIPT.md"
    audit_report = catalog_report(catalog, targets)
    if write:
        readme_path.write_text(updated)
        detail_path.write_text(details)
        audit_path.write_text(audit_report)
    elif readme != updated or not detail_path.exists() or detail_path.read_text() != details:
        raise ValueError("Progress reports are stale: python3 scripts/progress.py --write-reports")
    elif not audit_path.exists() or audit_path.read_text() != audit_report:
        raise ValueError("ActionScript report is stale: python3 scripts/progress.py --write-reports")


def target_record(view):
    """History counts for one language's Flash Player target."""
    row = view["target"]
    return {"implemented": row["working"], "partial": row["partial"], "planned": row["missing"],
            **{status: row[status] for status in PUBLIC_STATUSES}, "total": row["total"],
            "now": view["now"], "metric": "flash-player-target", "methodologyVersion": 3}


def snapshot(categories, features, date, revision, methodology_version=1, targets=None):
    record = {"date": date, "revision": revision, "methodologyVersion": methodology_version, "categories": {}, "total": counts(features)}
    for category in categories:
        members = [f for f in features if f["category"] == category["id"]]
        if methodology_version >= 3:
            # Functions inside each group; the group status counts stay alongside.
            items = item_counts(members)
            record["categories"][category["id"]] = {"implemented": items["working"], "partial": items["partial"], "planned": items["planned"],
                                                    "total": items["total"], "groups": counts(members), "metric": "functions"}
        else:
            record["categories"][category["id"]] = counts(members)
    # Every committed catalogue is recounted with the current target rules, so
    # the ActionScript lines stay continuous across methodology versions.
    for key, language in LANGUAGES:
        if targets and language in targets:
            record["categories"][key] = target_record(targets[language])
    return record


def daily_revisions(root, revision):
    # Keep the last commit in first-parent ancestry order on each UTC date.
    # Author timestamps may be rewritten or retain an older timezone/date.
    days = {}
    for line in git(root, "log", "--first-parent", "--reverse", "--format=%H %ct", revision).splitlines():
        sha, timestamp = line.split()
        day = datetime.fromtimestamp(int(timestamp), timezone.utc).date().isoformat()
        days[day] = sha
    return sorted(days.items())


def history(root, revision):
    records = []
    for day, sha in daily_revisions(root, revision):
        files = set(git(root, "ls-tree", "-r", "--name-only", sha, "--", "docs/progress/").splitlines())
        if not files:
            continue  # No measurements existed yet; do not invent zeroes.
        if "docs/progress/methodology.json" not in files:
            raise ValueError(f"Incomplete historical inventory at {sha}")
        categories, features = read_inventory(root, sha)
        version = methodology(root, sha)
        # Before v2 there was no declaration catalogue; the old 35/36 family counts are never shown.
        targets = load_targets(root, load_catalog(root, sha), sha) if version >= 2 else None
        product = [c for c in categories if c["id"] not in ("as2", "as3")]
        records.append(snapshot(product, [f for f in features if f["category"] not in ("as2", "as3")], day, sha, version, targets))
    return records


def build(root, output, revision):
    if git(root, "rev-parse", "--is-shallow-repository") == "true":
        raise ValueError("History needs a full checkout: git fetch --unshallow")
    revision = git(root, "rev-parse", "--verify", f"{revision}^{{commit}}")
    categories, features = read_inventory(root, revision)
    version = methodology(root, revision)
    catalog = load_catalog(root, revision)
    targets = load_targets(root, catalog, revision)
    timestamp = git(root, "show", "-s", "--format=%ct", revision)
    language_categories = [{"id": key, "title": targets[language]["title"],
                            "description": "Declarations in the eight Flash Player areas; AIR, Flex and components are outside the target."}
                           for key, language in LANGUAGES if language in targets]
    data = {
        "schemaVersion": 3,
        "generatedAt": datetime.fromtimestamp(int(timestamp), timezone.utc).isoformat().replace("+00:00", "Z"),
        "revision": revision,
        "repository": REPOSITORY,
        "methodology": {
            "version": version,
            "description": "ActionScript: declarations in eight Flash Player areas, statuses changed only by test-backed reviews. Product: user-visible functions listed inside each capability group. Neither is a Flash compatibility score. History uses the last main commit per UTC day.",
        },
        "categories": categories + language_categories,
        "features": features,
        "history": history(root, revision),
        "actionscript": {
            "targets": targets, "lastReview": last_review(catalog), "definitions": STATUS_DEFINITIONS,
            "auditId": catalog["metadata"]["auditId"], "counts": catalog["summary"]["byLanguage"],
            "byScope": catalog["summary"]["byLanguageScope"],
        },
    }
    output.mkdir(parents=True, exist_ok=True)
    for name in ("index.html", "history.html", "style.css", "app.js"):
        # Source and evidence links always describe the same immutable revision.
        content = git(root, "show", f"{revision}:web/progress/{name}")
        (output / name).write_text(content + "\n")
    (output / "progress.html").write_text((output / "index.html").read_text())
    (output / ".nojekyll").write_text("")
    (output / "progress.json").write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
    (output / "history.json").write_text(json.dumps(data["history"], ensure_ascii=False, indent=2) + "\n")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="validate the working-tree inventory without publishing")
    parser.add_argument("--write-reports", action="store_true", help="regenerate the README summary and local detailed ledger")
    parser.add_argument("--revision", default="HEAD", help="committed revision to build (default HEAD)")
    parser.add_argument("--output", type=Path, default=ROOT / "dist/progress")
    args = parser.parse_args()
    if args.check or args.write_reports:
        categories, features = read_inventory(ROOT)
        reports(ROOT, categories, features, write=args.write_reports)
        # The Actions editor toolbox/code hints follow the declaration statuses.
        index = [sys.executable, str(ROOT / "scripts/editor-api-index.py")]
        if args.write_reports and (ROOT / "docs/actionscript-reference/as3.json").exists():
            subprocess.run(index, check=True)
        else:
            subprocess.run([*index, "--check"], check=True)
        catalog = load_catalog(ROOT)
        items = item_counts(features)
        print(f"Valid inventory: {catalog['summary']['total']['total']:,} ActionScript declarations; "
              f"{len(features)} product groups with {items['total']} functions ({items['working']} working)")
    else:
        data = build(ROOT, args.output, args.revision)
        print(f"Built {args.output}: {len(data['features'])} capability groups, {len(data['history'])} daily snapshots ({data['revision'][:7]})")


if __name__ == "__main__":
    main()
