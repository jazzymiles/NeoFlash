#!/usr/bin/env python3
"""Build the public capability ledger from reviewed manifests and Git history."""

import argparse
from datetime import datetime, timezone
import json
import hashlib
from pathlib import Path
import re
import subprocess

from progress_catalog import load_catalog

ROOT = Path(__file__).resolve().parents[1]
MANIFESTS = ("as2.json", "as3.json", "product.json")
STATUSES = ("implemented", "partial", "planned")
REPOSITORY = "https://github.com/jazzymiles/NeoFlash"
REPORT_START = "<!-- progress:start -->"
REPORT_END = "<!-- progress:end -->"


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def read_inventory(root, revision=None):
    categories, features = [], []
    for name in MANIFESTS:
        path = f"docs/progress/{name}"
        content = git(root, "show", f"{revision}:{path}") if revision else (root / path).read_text()
        data = json.loads(content)
        if data.get("schemaVersion") != 1:
            raise ValueError(f"{path}: unsupported schemaVersion")
        if "category" in data:
            category = data["category"]
            categories.append(category)
            features.extend({**f, "category": category["id"]} for f in data["features"])
        else:
            categories.extend(data["categories"])
            features.extend(data["features"])
    validate_inventory(root, categories, features, revision)
    return categories, features


def validate_inventory(root, categories, features, revision=None):
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


def summary_table(categories, features):
    lines = ["| Area | Implemented | Partial | Planned | Registry coverage |", "| --- | ---: | ---: | ---: | ---: |"]
    for category in categories:
        count = counts([f for f in features if f["category"] == category["id"]])
        percentage = 100 * count["implemented"] / count["total"]
        lines.append(f"| {category['title']} | {count['implemented']}/{count['total']} | {count['partial']} | {count['planned']} | {percentage:.1f}% |")
    return "\n".join(lines)


def catalog_table(catalog):
    lines = ["| Language | Documented entries | Implemented subset | Partial | Missing | Needs review |", "| --- | ---: | ---: | ---: | ---: | ---: |"]
    for language, row in catalog["summary"]["byLanguage"].items():
        lines.append(f"| {language} | {row['total']:,} | {row['implemented_subset']:,} | {row['partial']:,} | {row['missing']:,} | {row['needs_review']:,} |")
    return "\n".join(lines)


def source_snapshot(root, revision, catalog):
    changed_code, changed_docs = [], []
    hashes = catalog["metadata"]["implementationSourceHashes"]
    for path, expected in hashes.items():
        if revision:
            result = subprocess.run(["git", "-C", str(root), "show", f"{revision}:{path}"], capture_output=True)
            actual = hashlib.sha256(result.stdout).hexdigest() if result.returncode == 0 else None
        else:
            actual = hashlib.sha256((root / path).read_bytes()).hexdigest() if (root / path).is_file() else None
        if actual != expected:
            (changed_docs if path.startswith("docs/") else changed_code).append(path)
    return {"implementationMatches": not changed_code, "checkedFiles": len(hashes),
            "changedImplementationFiles": sorted(changed_code), "changedDocumentationFiles": sorted(changed_docs)}


def catalog_report(catalog):
    meta = catalog["metadata"]
    lines = ["# ActionScript declaration coverage", "", "Generated by `python3 scripts/progress.py --write-reports` from the complete normalized catalogue and its frozen audit.", "",
             f"Audit: `{meta['auditId']}` · {meta['auditedAt']} (UTC). Recorded baseline HEAD: `{meta['baselineRevision']}`; the implementation fingerprints in [the audit metadata](actionscript-audit.json) identify the actual source bytes.", "",
             catalog_table(catalog), "", "These are documented declarations, not thousands of independently executed tests or equally sized features. **Implemented subset** means only the linked rule's bounded behavior. All entries lack full Flash compatibility certification; this does not mean nothing works. Most missing entries are architectural absence inferences. Original Flash differential testing was not performed.", "",
             "## Scope breakdown", "", "The full reference includes AIR, Flex and component libraries. Even the core/language/runtime view includes some AIR declarations; it is not a Flash Player-only API list.", "",
             "| Language | Scope | Entries | Implemented subset | Partial | Missing | Needs review |", "| --- | --- | ---: | ---: | ---: | ---: | ---: |"]
    for language, scopes in catalog["summary"]["byLanguageScope"].items():
        for scope, row in scopes.items():
            lines.append(f"| {language} | {scope} | {row['total']:,} | {row['implemented_subset']} | {row['partial']} | {row['missing']:,} | {row['needs_review']} |")
    lines.extend(["", "## Inspect individual declarations", "", "[Complete normalized catalogue](actionscript-catalog.json) · [Audit rules, source hashes and provenance](actionscript-audit.json)", "",
                  "The normalized catalogue preserves every original ID, language, owner, name, kind, scope, source URL and audited status. Query it locally without loading the full list in the public page:", "", "```sh", "python3 scripts/progress_catalog.py --summary", "python3 scripts/progress_catalog.py --language AS3 --scope runtime --query MovieClip", "```", "",
                  "The public page checks the baseline source fingerprints against its published revision. Changed implementation files make the audit a visibly dated snapshot; documentation changes are reported separately. Later working-tree reviews in the original audit are not promoted to shipped implementation or fresh test passes.", "", "## Evidence rules", ""])
    for rule in catalog["rules"]:
        lines.extend([f"### {rule['id']} — {rule['title']}", "", rule["reason"], "", rule["limitations"], ""])
        refs = [f"[{r['path']}](../../{r['path']})" for r in rule["evidence"]]
        if refs:
            lines.extend(["Evidence: " + ", ".join(refs), ""])
    return "\n".join(lines) + "\n"


def reports(root, categories, features, write=False):
    version = json.loads((root / "docs/progress/methodology.json").read_text())["version"]
    catalog = load_catalog(root) if version >= 2 else None
    product_categories = [c for c in categories if c["id"] not in ("as2", "as3")] if catalog else categories
    product_features = [f for f in features if f["category"] not in ("as2", "as3")] if catalog else features
    coverage = (("### ActionScript reference coverage\n\n" + catalog_table(catalog) +
                 "\n\nCounts use the complete selected documentation catalogue, including AIR, Flex and components. “Implemented subset” covers only the audited rule, not the whole API contract. Missing and unreviewed declarations remain separate. These are **not Flash compatibility percentages**. See the [dated audit and scope breakdown](docs/progress/ACTIONSCRIPT.md).\n\n### Product capabilities\n\n") if catalog else "") + summary_table(product_categories, product_features)
    block = "\n".join([
        REPORT_START,
        "## Where we are",
        "",
        "The editor foundation works: nested timelines, vector tools, scripting and standalone HTML5. Full AS2/AS3, FLA/SWF import, portable media and DOM applications remain in development.",
        "",
        coverage,
        "",
        "Implemented means the item's stated scope is complete; partial items do not contribute to the percentage. This measures our capability registry, **not Flash compatibility or test line coverage**. Items differ in size, so there is no overall product completion percentage.",
        "",
        "[Current coverage](https://jazzymiles.github.io/NeoFlash/progress.html) · [Daily history](https://jazzymiles.github.io/NeoFlash/history.html) · [Detailed registry](docs/progress/DETAILS.md) · [Counting rules](docs/progress/README.md)",
        "",
        "This summary is generated from `docs/progress/*.json`; GitHub Pages rebuilds on every push to `main`.",
        REPORT_END,
    ])
    readme_path = root / "README.md"
    readme = readme_path.read_text()
    if REPORT_START not in readme or REPORT_END not in readme:
        raise ValueError("README needs progress:start and progress:end markers")
    before, rest = readme.split(REPORT_START, 1)
    _, after = rest.split(REPORT_END, 1)
    updated = before + block + after
    labels = {"implemented": "Implemented", "partial": "Partial", "planned": "Planned"}
    detail = ["# NeoFlash detailed capability notes", "", "Generated by `python3 scripts/progress.py --write-reports`. Edit the JSON inventories, not this file.", "", "[Methodology](README.md). Each status records a source review within an explicit scope; links to tests do not imply those tests run during the page build. This summary does not measure full Flash compatibility.", "", "ActionScript coverage uses the [complete declaration catalogue and audit](ACTIONSCRIPT.md). The AS2/AS3 capability notes below explain behavior; their 35/36 groups are **not a coverage denominator**.", "", summary_table(product_categories, product_features)]
    for category in categories:
        detail.extend(["", f"## {category['title']}", "", category["description"]])
        for feature in [f for f in features if f["category"] == category["id"]]:
            detail.extend(["", f"### {feature['title']}", "", f"`{feature['id']}` · **{labels[feature['status']]}**", "", f"**Scope:** {feature['scope']}", "", f"**Limitations:** {feature['limitations']}", "", "**Evidence:** " + ", ".join(f"[{p}](../../{p})" for p in feature["evidence"])])
    details = "\n".join(detail) + "\n"
    detail_path = root / "docs/progress/DETAILS.md"
    audit_path = root / "docs/progress/ACTIONSCRIPT.md"
    audit_report = catalog_report(catalog) if catalog else None
    if write:
        readme_path.write_text(updated)
        detail_path.write_text(details)
        if audit_report:
            audit_path.write_text(audit_report)
    elif readme != updated or not detail_path.exists() or detail_path.read_text() != details:
        raise ValueError("Progress reports are stale: python3 scripts/progress.py --write-reports")
    elif audit_report and (not audit_path.exists() or audit_path.read_text() != audit_report):
        raise ValueError("ActionScript report is stale: python3 scripts/progress.py --write-reports")


def methodology_version(root, revision):
    version = json.loads(git(root, "show", f"{revision}:docs/progress/methodology.json"))["version"]
    if type(version) is not int or version < 1:
        raise ValueError("Methodology version must be a positive integer")
    return version


def snapshot(categories, features, date, revision, methodology_version=1, catalog=None):
    record = {
        "date": date,
        "revision": revision,
        "methodologyVersion": methodology_version,
        "categories": {c["id"]: counts([f for f in features if f["category"] == c["id"]]) for c in categories},
        "total": counts(features),
    }
    if catalog:
        for language, row in catalog["summary"]["byLanguage"].items():
            record["categories"][language.lower()] = {
                "implemented": row["implemented_subset"], "partial": row["partial"],
                "planned": row["missing"], "missing": row["missing"], "needs_review": row["needs_review"],
                "total": row["total"], "metric": "catalogue",
            }
        record["auditId"] = catalog["metadata"]["auditId"]
        record["total"] = counts([f for f in features if f["category"] not in ("as2", "as3")])
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
    current_version = methodology_version(root, revision)
    for day, sha in daily_revisions(root, revision):
        files = set(git(root, "ls-tree", "-r", "--name-only", sha, "--", "docs/progress/").splitlines())
        if not files:
            continue  # No measurements existed yet; do not invent zeroes.
        required = {f"docs/progress/{name}" for name in (*MANIFESTS, "methodology.json")}
        if not required.issubset(files):
            raise ValueError(f"Incomplete historical inventory at {sha}")
        categories, features = read_inventory(root, sha)
        version = methodology_version(root, sha)
        catalog = load_catalog(root, sha) if version >= 2 else None
        record = snapshot(categories, features, day, sha, version, catalog)
        if current_version >= 2 and version < 2:
            # The former 35/36 capability groups are not declaration measurements.
            for category in ("as2", "as3"):
                record["categories"].pop(category, None)
        records.append(record)
    return records


def build(root, output, revision):
    if git(root, "rev-parse", "--is-shallow-repository") == "true":
        raise ValueError("History needs a full checkout: git fetch --unshallow")
    revision = git(root, "rev-parse", "--verify", f"{revision}^{{commit}}")
    categories, features = read_inventory(root, revision)
    version = methodology_version(root, revision)
    catalog = load_catalog(root, revision) if version >= 2 else None
    if catalog:
        features = [f for f in features if f["category"] not in ("as2", "as3")]
        categories = [{**c, "description": "Documented declarations in the complete selected reference catalogue; implemented subset, partial, missing and needs review are separate."} if c["id"] in ("as2", "as3") else c for c in categories]
    timestamp = git(root, "show", "-s", "--format=%ct", revision)
    data = {
        "schemaVersion": 2 if catalog else 1,
        "generatedAt": datetime.fromtimestamp(int(timestamp), timezone.utc).isoformat().replace("+00:00", "Z"),
        "revision": revision,
        "repository": REPOSITORY,
        "methodology": {
            "version": version,
            "description": "ActionScript counts come from the full declaration catalogue and its dated audit, not capability groups. Product percentages use bounded capabilities. Neither is a Flash compatibility score. History uses the last main commit per UTC day; obsolete AS family counts are excluded from declaration history.",
        },
        "categories": categories,
        "features": features,
        "history": history(root, revision),
    }
    if catalog:
        meta = catalog["metadata"]
        data["actionscript"] = {
            "auditId": meta["auditId"], "auditedAt": meta["auditedAt"], "sourceRevision": meta["baselineRevision"],
            "counts": catalog["summary"]["byLanguage"], "byScope": catalog["summary"]["byLanguageScope"],
            "sourceSnapshot": source_snapshot(root, revision, catalog),
            "note": "Dated declaration audit, not full Flash conformance. Most missing entries are inferred from absent libraries. Later unshipped working-tree reviews are excluded.",
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
        version = json.loads((ROOT / "docs/progress/methodology.json").read_text())["version"]
        if type(version) is not int or version < 1:
            raise ValueError("Methodology version must be a positive integer")
        categories, features = read_inventory(ROOT)
        reports(ROOT, categories, features, write=args.write_reports)
        if version >= 2:
            catalog = load_catalog(ROOT)
            product = [f for f in features if f["category"] not in ("as2", "as3")]
            print(f"Valid inventory: {catalog['summary']['total']['total']:,} ActionScript declarations, {len(product)} product capabilities; supplementary AS notes are excluded from coverage counts")
        else:
            print(f"Valid inventory: {len(categories)} categories, {len(features)} capabilities; {counts(features)}")
    else:
        data = build(ROOT, args.output, args.revision)
        print(f"Built {args.output}: {len(data['features'])} capabilities, {len(data['history'])} daily snapshots ({data['revision'][:7]})")


if __name__ == "__main__":
    main()
