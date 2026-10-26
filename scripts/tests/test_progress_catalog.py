"""Check exact declaration joins and integrity boundaries using tiny audit files."""

from collections import Counter
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import progress_catalog


STATUSES = ("implemented_subset", "partial", "missing", "needs_review")


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, sort_keys=True) + "\n")


def fingerprint(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def expected_counts(rows):
    values = Counter(row["status"] for row in rows)
    return {status: values[status] for status in STATUSES}


def write_original_fixture(source):
    """Four catalogues with eight distinct declarations and all four statuses."""
    definitions = (
        ("as2.json", "AS2", "language", "implemented_subset"),
        ("as2.json", "AS2", "core", "partial"),
        ("as2-components.json", "AS2", "component", "missing"),
        ("as2-components-supplement.json", "AS2", "component", "needs_review"),
        ("as3.json", "AS3", "language", "implemented_subset"),
        ("as3.json", "AS3", "core", "partial"),
        ("as3.json", "AS3", "framework", "missing"),
        ("as3.json", "AS3", "runtime", "needs_review"),
    )
    originals = {name: [] for name in progress_catalog.SOURCES}
    decisions, classified = [], []
    for index, (file, language, scope, status) in enumerate(definitions):
        identifier = f"{language}:fixture:{index}"
        originals[file].append({
            "id": identifier, "language": language, "scope": scope,
            "owner": "Fixture", "name": f"declaration{index}", "kind": "method",
            "source_url": f"https://example.invalid/reference#{index}",
        })
        decisions.append({
            "catalog_id": identifier, "status": status, "rule_id": status,
            "evidence_kind": "fixture_source_review",
            "full_flash_compatibility_verified": False,
        })
        classified.append({"language": language, "scope": scope, "status": status})
    for name, entries in originals.items():
        write_json(source / name, {"schema_version": 1, "entries": entries})
    evidence = {
        "schema_version": 1, "audit_id": "synthetic-test-audit", "head": "a" * 40,
        "created_at_utc": "2026-10-27T10:00:00Z", "source_files": {},
        "working_tree_note": "Original synthetic test inputs.",
        "original_flash_differential": "not_run", "browser_matrix": "not_run",
        "late_review_execution_policy": "No execution is claimed.",
        "baseline_evidence": {"id": "B01", "reason": "Synthetic baseline."},
        "compiler": {"findings": []}, "html5": {"capabilities": [], "findings": []},
        "manual_rules": [{"id": status, "reason": f"Synthetic {status} decision."} for status in STATUSES],
        "ignored_private_probe": "/Users/__private__/target/local-only.log",
    }
    write_json(source / "implementation-evidence.json", evidence)
    audit = {
        "schema_version": 1, "audit_id": evidence["audit_id"], "head": evidence["head"],
        "evidence_sha256": fingerprint(source / "implementation-evidence.json"),
        "catalog_sha256": {name: fingerprint(source / name) for name in originals},
        "classification_note": "Bounded decisions, not full compatibility.",
        "version_note": "Synthetic historical audit.", "total_entries": len(decisions),
        "entries": decisions, "counts": expected_counts(classified),
        "by_language": {
            language: expected_counts([row for row in classified if row["language"] == language])
            for language in ("AS2", "AS3")
        },
        "by_scope": {
            scope: expected_counts([row for row in classified if row["scope"] == scope])
            for scope in {row["scope"] for row in classified}
        },
    }
    write_json(source / "implementation-audit.json", audit)
    return audit


def write_catalog_fixture(root):
    """Supply a valid normalized catalogue to progress.py integration fixtures."""
    with tempfile.TemporaryDirectory(prefix="neoflash-original-audit-") as directory:
        source = Path(directory)
        write_original_fixture(source)
        return progress_catalog.import_catalog(source, root)


class CatalogTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="neoflash-catalog-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "repo"
        self.source = Path(self.temporary.name) / "original"
        self.audit = write_original_fixture(self.source)

    def import_catalog(self):
        return progress_catalog.import_catalog(self.source, self.root)

    def save_audit(self):
        write_json(self.source / "implementation-audit.json", self.audit)

    def test_join_preserves_every_id_and_all_statuses_in_the_denominator(self):
        data = self.import_catalog()
        self.assertEqual(len(data["entries"]), 8)
        self.assertEqual({row["catalog_id"] for row in data["entries"]}, {row["catalog_id"] for row in self.audit["entries"]})
        self.assertEqual(data["summary"]["total"], {"implemented_subset": 2, "partial": 2, "missing": 2, "needs_review": 2, "total": 8})
        for language in ("AS2", "AS3"):
            self.assertEqual(data["summary"]["byLanguage"][language], {"implemented_subset": 1, "partial": 1, "missing": 1, "needs_review": 1, "total": 4})
        self.assertEqual(data["summary"]["byLanguageScope"]["AS2"]["component"]["total"], 2)
        self.assertEqual(data["summary"]["byLanguageScope"]["AS3"]["runtime"]["needs_review"], 1)
        self.assertFalse(data["metadata"]["currentSourceVerified"])
        self.assertNotIn("__private__", json.dumps(data))

    def test_changed_source_bytes_fail_even_if_the_json_still_parses(self):
        path = self.source / "as3.json"
        path.write_text(path.read_text() + "\n")
        with self.assertRaisesRegex(ValueError, "hash differs"):
            self.import_catalog()

    def test_changed_evidence_bytes_fail(self):
        path = self.source / "implementation-evidence.json"
        path.write_text(path.read_text() + "\n")
        with self.assertRaisesRegex(ValueError, "hash differs"):
            self.import_catalog()

    def test_duplicate_source_id_fails_after_hash_verification(self):
        path = self.source / "as2.json"
        catalog = json.loads(path.read_text())
        catalog["entries"].append(dict(catalog["entries"][0]))
        write_json(path, catalog)
        self.audit["catalog_sha256"]["as2.json"] = fingerprint(path)
        self.save_audit()
        with self.assertRaisesRegex(ValueError, "Duplicate source catalogue ID"):
            self.import_catalog()

    def test_missing_or_duplicate_audit_mapping_fails(self):
        original_entries = list(self.audit["entries"])
        for mapping in (original_entries[:-1], original_entries + [original_entries[0]]):
            with self.subTest(count=len(mapping)):
                self.audit["entries"] = mapping
                self.save_audit()
                with self.assertRaisesRegex(ValueError, "every unique catalogue ID exactly once"):
                    self.import_catalog()

    def test_source_summary_must_match_joined_decisions(self):
        self.audit["counts"]["needs_review"] = 0
        self.save_audit()
        with self.assertRaisesRegex(ValueError, "Status totals differ"):
            self.import_catalog()

    def test_normalized_catalogue_bytes_are_bound_to_the_audit_hash(self):
        self.import_catalog()
        path = self.root / progress_catalog.CATALOG_PATH
        path.write_text(path.read_text() + "\n")
        with self.assertRaisesRegex(ValueError, "hash differs"):
            progress_catalog.load_catalog(self.root)

    def test_normalized_validation_rejects_duplicate_ids_and_missing_rules(self):
        data = self.import_catalog()
        data["entries"].append(dict(data["entries"][0]))
        with self.assertRaisesRegex(ValueError, "duplicate catalogue ID"):
            progress_catalog.validate_catalog(data)
        data = self.import_catalog()
        data["rules"].pop()
        with self.assertRaisesRegex(ValueError, "Rule explanations"):
            progress_catalog.validate_catalog(data)

    def test_frozen_evidence_cannot_claim_current_source_verification(self):
        data = self.import_catalog()
        data["metadata"]["currentSourceVerified"] = True
        with self.assertRaisesRegex(ValueError, "must not claim current implementation"):
            progress_catalog.validate_catalog(data)

    def test_source_references_cannot_escape_the_repository(self):
        path = self.source / "implementation-evidence.json"
        evidence = json.loads(path.read_text())
        evidence["source_files"] = {"../private/source.rs": "b" * 64}
        write_json(path, evidence)
        self.audit["evidence_sha256"] = fingerprint(path)
        self.save_audit()
        with self.assertRaisesRegex(ValueError, "repository-relative evidence path"):
            self.import_catalog()


if __name__ == "__main__":
    unittest.main()
