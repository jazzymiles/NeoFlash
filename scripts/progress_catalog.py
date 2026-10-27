#!/usr/bin/env python3
"""Import and read the complete, frozen ActionScript declaration audit.

The importer verifies the original catalogue/evidence hashes before joining every
catalogue ID to its audited decision. It does not inspect current implementation,
rerun the audit or promote any previously partial API to full compatibility.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from functools import lru_cache
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
SOURCES = ("as2.json", "as2-components.json", "as2-components-supplement.json", "as3.json")
CATALOG_PATH = "docs/progress/actionscript-catalog.json"
AUDIT_PATH = "docs/progress/actionscript-audit.json"
STATUSES = ("implemented_subset", "partial", "missing", "needs_review")
ENTRY_FIELDS = ("catalog_id", "language", "owner", "name", "kind", "scope", "source_url", "status", "rule_id", "evidence_kind")
HASH = re.compile(r"[0-9a-f]{64}")


def sha256(raw):
    return hashlib.sha256(raw).hexdigest()


def safe_path(path):
    if not isinstance(path, str) or not path or "\\" in path or ":" in path:
        raise ValueError(f"Invalid repository-relative evidence path: {path!r}")
    parsed = PurePosixPath(path)
    if parsed.is_absolute() or ".." in parsed.parts or path.startswith("~"):
        raise ValueError(f"Invalid repository-relative evidence path: {path!r}")
    return path


def counter(rows):
    observed = Counter(row["status"] for row in rows)
    return {**{status: observed[status] for status in STATUSES}, "total": sum(observed.values())}


def aggregate(entries):
    """Count declarations, including unreviewed entries, without fractional credit."""
    by_language, by_scope, by_pair = defaultdict(list), defaultdict(list), defaultdict(lambda: defaultdict(list))
    for entry in entries:
        by_language[entry["language"]].append(entry)
        by_scope[entry["scope"]].append(entry)
        by_pair[entry["language"]][entry["scope"]].append(entry)
    return {
        "total": counter(entries),
        "byLanguage": {key: counter(rows) for key, rows in sorted(by_language.items())},
        "byScope": {key: counter(rows) for key, rows in sorted(by_scope.items())},
        "byLanguageScope": {language: {scope: counter(rows) for scope, rows in sorted(scopes.items())}
                            for language, scopes in sorted(by_pair.items())},
    }


def checked_hash(raw, expected, label):
    if not isinstance(expected, str) or not HASH.fullmatch(expected) or sha256(raw) != expected:
        raise ValueError(f"{label}: hash differs from its audited snapshot; review the source audit before importing")


def reference(item, source_hashes):
    """Retain only a relative path, source line and recorded content fingerprint."""
    if isinstance(item, str):
        match = re.fullmatch(r"([^:]+)(?::([0-9]+))?", item)
        if not match:
            raise ValueError(f"Invalid source reference: {item!r}")
        path, line = match.groups()
        line = int(line) if line else None
    elif isinstance(item, dict):
        path, line = item.get("file"), item.get("line")
    else:
        raise ValueError("Source reference must be text or an object")
    result = {"path": safe_path(path)}
    if line is not None:
        if not isinstance(line, int) or line < 1:
            raise ValueError("Source line must be a positive integer")
        result["line"] = line
    if path in source_hashes:
        result["sha256"] = source_hashes[path]
    return result


def reviewed_rules(evidence, entries):
    """Allowlist only explanatory text and safe code references; omit probes/logs."""
    source_hashes = evidence["source_files"]
    definitions = {}

    def add(identifier, title, reason, limitations, refs=()):
        definitions[identifier] = {
            "id": identifier,
            "title": title,
            "reason": reason,
            "limitations": limitations,
            "evidence": [reference(item, source_hashes) for item in refs],
        }

    baseline = evidence["baseline_evidence"]
    add(baseline["id"], "Architectural absence of platform libraries", baseline["reason"],
        "Architectural inference; declarations were not individually executed.", baseline.get("evidence", []))
    for item in evidence["compiler"]["findings"]:
        add(item["id"], item["title"], item["observed"], item.get("verification", ""), item.get("evidence", []))
    for item in evidence["html5"]["capabilities"]:
        add("host:" + item["id"], item["id"].replace("-", " ").capitalize(), item["behavior"],
            " ".join(item.get("limitations", [])), item.get("evidence", []))
    for item in evidence["html5"]["findings"]:
        add(item["id"], item["id"], item["problem"],
            "Observed evidence: " + item["status"] + ". This is not an original Flash differential result.",
            item.get("evidence", []))
    for item in evidence["manual_rules"]:
        add(item["id"], item["id"].replace("-", " ").capitalize(), item["reason"],
            "Use only within the exact reviewed rule; no full Flash compatibility is claimed.", item.get("evidence", []))
    used = {entry["rule_id"] for entry in entries}
    if not used.issubset(definitions):
        raise ValueError(f"Audit rules have no safe explanation: {sorted(used - definitions.keys())}")
    return [definitions[identifier] for identifier in sorted(used)]


def catalogue_bytes(entries):
    # One declaration per line keeps the complete public inventory inspectable.
    return ('{"schemaVersion":1,"entries":[\n' +
            ',\n'.join(json.dumps(row, ensure_ascii=False, separators=(",", ":")) for row in entries) +
            '\n]}\n').encode("utf-8")


def import_catalog(source, root=ROOT):
    """Import the four original catalogue files and their exact audited mapping."""
    source, root = Path(source), Path(root)
    audit_raw = (source / "implementation-audit.json").read_bytes()
    audit = json.loads(audit_raw)
    evidence_raw = (source / "implementation-evidence.json").read_bytes()
    checked_hash(evidence_raw, audit["evidence_sha256"], "implementation-evidence.json")
    evidence = json.loads(evidence_raw)
    if audit.get("schema_version") != 1 or evidence.get("schema_version") != 1:
        raise ValueError("Unsupported original audit/evidence schema")
    if audit["audit_id"] != evidence["audit_id"] or audit["head"] != evidence["head"]:
        raise ValueError("Audit and evidence identify different source snapshots")
    originals, sources = {}, []
    for name in SOURCES:
        raw = (source / name).read_bytes()
        checked_hash(raw, audit["catalog_sha256"][name], name)
        data = json.loads(raw)
        if data.get("schema_version") != 1:
            raise ValueError(f"{name}: unsupported source schema")
        for entry in data["entries"]:
            if entry["id"] in originals:
                raise ValueError(f"Duplicate source catalogue ID: {entry['id']}")
            originals[entry["id"]] = entry
        coverage = data.get("coverage", {})
        sources.append({
            "file": name, "sha256": sha256(raw), "entries": len(data["entries"]),
            "title": data.get("reference_title") or data.get("title") or data.get("catalog") or name,
            "referenceGeneration": data.get("reference_generation") or data.get("source_baseline", {}).get("reference_generation_date"),
            "claim": coverage.get("claim", "Complete import of entries in this selected source catalogue; source coverage limitations still apply."),
            "limitations": coverage.get("limitations", []),
            "excludedSurfaces": coverage.get("excluded_surfaces", []),
        })
    decisions = {row["catalog_id"]: row for row in audit["entries"]}
    if len(decisions) != len(audit["entries"]) or decisions.keys() != originals.keys():
        raise ValueError("Audit must map every unique catalogue ID exactly once")
    if audit["total_entries"] != len(originals):
        raise ValueError("Audit total differs from source catalogue count")
    entries = []
    for identifier in sorted(originals):
        original, decision = originals[identifier], decisions[identifier]
        entries.append({
            "catalog_id": identifier,
            **{key: original.get(key) for key in ("language", "owner", "name", "kind", "scope", "source_url")},
            **{key: decision[key] for key in ("status", "rule_id", "evidence_kind")},
        })
    summary = aggregate(entries)
    for expected, actual, label in ((audit["counts"], summary["total"], "all declarations"),
                                    *((audit["by_language"][key], value, key) for key, value in summary["byLanguage"].items()),
                                    *((audit["by_scope"][key], value, key) for key, value in summary["byScope"].items())):
        if any(expected.get(status, 0) != actual[status] for status in STATUSES):
            raise ValueError(f"Status totals differ from source audit: {label}")
    source_hashes = {safe_path(path): digest for path, digest in sorted(evidence["source_files"].items())}
    if any(not HASH.fullmatch(digest) for digest in source_hashes.values()):
        raise ValueError("Invalid source evidence fingerprint")
    payload = catalogue_bytes(entries)
    metadata = {
        "auditId": audit["audit_id"], "auditedAt": evidence["created_at_utc"],
        "baselineRevision": audit["head"], "currentSourceVerified": False,
        "classificationNote": audit["classification_note"], "versionNote": audit["version_note"],
        "sourceSnapshotNote": "Imported historical audit of a hash-identified working-tree snapshot. The recorded HEAD alone does not identify all audited source bytes. This import verifies catalogue integrity, not the current NeoFlash implementation.",
        "sourceWorkingTreeNote": evidence["working_tree_note"],
        "originalFlashDifferential": evidence["original_flash_differential"],
        "browserMatrix": evidence["browser_matrix"],
        "lateReviewExecutionPolicy": evidence["late_review_execution_policy"],
        "sourceFiles": sources,
        "inputHashes": {"implementation-audit.json": sha256(audit_raw), "implementation-evidence.json": sha256(evidence_raw), **audit["catalog_sha256"]},
        "implementationSourceHashes": source_hashes,
        "sourceChangesAfterExecution": [
            {"path": safe_path(item["path"]), "baseline_sha256": item["baseline_sha256"], "observed_sha256": item["observed_sha256"]}
            for item in evidence.get("source_changes_after_execution", [])
        ],
        "coverageNote": "The denominator includes every declaration in the four selected documentation catalogues, including framework, component and platform-extension entries. It is not limited to browser-compatible runtime APIs. It is not every historical or third-party API; AS3 post-Adobe HARMAN additions and documented source gaps remain outside this snapshot.",
        "sourceReferenceNote": "Rule evidence paths and lines refer to the audited source fingerprints. They are not verified line links into the recorded HEAD or the current commit.",
    }
    frozen = {"schemaVersion": 1, "catalogSha256": sha256(payload), "metadata": metadata,
              "summary": summary, "rules": reviewed_rules(evidence, entries)}
    validate_catalog({"schemaVersion": 1, "metadata": metadata, "entries": entries,
                      "summary": summary, "rules": frozen["rules"]})
    (root / CATALOG_PATH).parent.mkdir(parents=True, exist_ok=True)
    (root / CATALOG_PATH).write_bytes(payload)
    (root / AUDIT_PATH).write_text(json.dumps(frozen, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return load_catalog(root)


@lru_cache(maxsize=8)
def _blob(root, object_id):
    # Different days often share the same catalogue blob; avoid repeated reads.
    return subprocess.check_output(["git", "-C", root, "cat-file", "blob", object_id])


def read_bytes(root, path, revision=None):
    root = Path(root)
    if revision is None:
        return (root / path).read_bytes()
    object_id = subprocess.check_output(["git", "-C", str(root), "rev-parse", "--verify", f"{revision}:{path}"], text=True).strip()
    return _blob(str(root), object_id)


def validate_catalog(data):
    if data.get("schemaVersion") != 1:
        raise ValueError("Unsupported normalized ActionScript catalogue schema")
    seen = set()
    for entry in data["entries"]:
        if set(entry) != set(ENTRY_FIELDS):
            raise ValueError("Unexpected normalized catalogue entry fields")
        identifier = entry["catalog_id"]
        if not isinstance(identifier, str) or not identifier or identifier in seen:
            raise ValueError(f"Missing or duplicate catalogue ID: {identifier}")
        seen.add(identifier)
        if entry["language"] not in ("AS2", "AS3") or entry["status"] not in STATUSES:
            raise ValueError(f"Invalid language/status for {identifier}")
        for field in ("name", "kind", "scope", "rule_id", "evidence_kind"):
            if not isinstance(entry[field], str) or not entry[field].strip():
                raise ValueError(f"Missing {field} for {identifier}")
        if entry["owner"] is not None and not isinstance(entry["owner"], str):
            raise ValueError(f"Invalid owner for {identifier}")
        if not isinstance(entry["source_url"], str):
            raise ValueError(f"Invalid source URL for {identifier}")
        url = urlsplit(entry["source_url"])
        if url.scheme not in ("http", "https") or not url.hostname or url.username or url.password:
            raise ValueError(f"Invalid source URL for {identifier}")
    if aggregate(data["entries"]) != data["summary"]:
        raise ValueError("Catalogue summary differs from entry classifications")
    rules = data["rules"]
    if len({rule["id"] for rule in rules}) != len(rules):
        raise ValueError("Duplicate audit rule IDs")
    if {entry["rule_id"] for entry in data["entries"]} != {rule["id"] for rule in rules}:
        raise ValueError("Rule explanations do not match the classified entries")
    metadata = data["metadata"]
    if metadata.get("currentSourceVerified") is not False:
        raise ValueError("Frozen imported evidence must not claim current implementation verification")
    if not re.fullmatch(r"[a-f0-9]{40}", metadata["baselineRevision"]):
        raise ValueError("Invalid recorded baseline revision")
    for rule in rules:
        for ref in rule["evidence"]:
            safe_path(ref["path"])
    for path in metadata["implementationSourceHashes"]:
        safe_path(path)


def load_catalog(root=ROOT, revision=None):
    """Return metadata, all entries, exact counts and rules from working tree/Git.

    revision is an optional committed Git revision. Imported status decisions stay
    attached to their original audit date and source fingerprints in either case.
    """
    raw = read_bytes(root, CATALOG_PATH, revision)
    frozen = json.loads(read_bytes(root, AUDIT_PATH, revision))
    if frozen.get("schemaVersion") != 1:
        raise ValueError("Unsupported normalized ActionScript audit schema")
    checked_hash(raw, frozen["catalogSha256"], CATALOG_PATH)
    data = json.loads(raw)
    data.update({key: frozen[key] for key in ("metadata", "summary", "rules")})
    validate_catalog(data)
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, help="import from an original docs/actionscript-reference directory")
    parser.add_argument("--revision", help="read a committed normalized snapshot instead of working-tree files")
    parser.add_argument("--summary", action="store_true", help="show exact status counts (for all entries or the selected entries)")
    parser.add_argument("--language", choices=("AS2", "AS3"))
    parser.add_argument("--scope", help="filter by exact documentation scope")
    parser.add_argument("--status", choices=STATUSES)
    parser.add_argument("--owner", help="filter by exact declaration owner")
    parser.add_argument("--id", dest="catalog_id", help="look up one original catalogue ID")
    parser.add_argument("--query", help="case-insensitive search in ID, owner and name")
    parser.add_argument("--json", action="store_true", help="print matching entries as JSON")
    args = parser.parse_args()
    if args.source and args.revision:
        parser.error("--source and --revision cannot be combined")
    data = import_catalog(args.source) if args.source else load_catalog(ROOT, args.revision)
    filters = {key: value for key, value in (("language", args.language), ("scope", args.scope),
               ("status", args.status), ("owner", args.owner), ("catalog_id", args.catalog_id)) if value is not None}
    entries = [row for row in data["entries"] if all(row[key] == value for key, value in filters.items())
               and (not args.query or args.query.casefold() in " ".join(str(row[key] or "")
                    for key in ("catalog_id", "owner", "name")).casefold())]
    if args.summary or not (filters or args.query or args.json):
        print(json.dumps({"auditId": data["metadata"]["auditId"], "currentSourceVerified": False,
                          "summary": aggregate(entries)}, indent=2))
    elif args.json:
        print(json.dumps({"metadata": data["metadata"], "entries": entries, "summary": aggregate(entries)}, ensure_ascii=False, indent=2))
    else:
        for row in entries:
            print(f"{row['status']:20} {row['catalog_id']}\n  {row['source_url']}\n  Rule: {row['rule_id']}; evidence: {row['evidence_kind']}")
        print(f"{len(entries)} matching declarations. Audit: {data['metadata']['auditId']}. Current implementation was not re-audited by this query.")


if __name__ == "__main__":
    main()
