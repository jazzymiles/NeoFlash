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
            "owner": "Math" if language == "AS2" else "Fixture", "name": f"declaration{index}", "kind": "method",
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

    def test_later_verification_keeps_its_snapshot_scope_and_omits_private_logs(self):
        path = self.source / "implementation-evidence.json"
        evidence = json.loads(path.read_text())
        row = self.audit["entries"][0]
        row["rule_id"] = "independent-visibility"
        evidence["source_files"] = {"web/runtime.js": "b" * 64}
        evidence["independent_verifications"] = [{
            "id": row["rule_id"], "head": "c" * 40,
            "captured_at": "2026-10-28T10:00:00Z", "verified_at": "2026-10-28T11:00:00Z",
            "source_manifest_sha256": "d" * 64, "source_files": {"web/runtime.js": "e" * 64},
            "browser": {"name": "IAB", "engine": "unspecified", "original_flash_comparison": "documented_only"},
            "decision": "bounded_visibility_pass", "limitations": ["Boolean authored placements only."],
            "catalog_overrides": [{"catalog_id": row["catalog_id"], "status": row["status"],
                                   "evidence_kind": "browser", "scope_note": "Boolean true/false only."}],
            "private_log": "/Users/__private__/target/browser.log",
        }]
        write_json(path, evidence)
        self.audit["evidence_sha256"] = fingerprint(path)
        self.save_audit()
        data = self.import_catalog()
        rule = next(rule for rule in data["rules"] if rule["id"] == row["rule_id"])
        self.assertEqual(rule["evidence"][0]["sha256"], "e" * 64)
        self.assertEqual(rule["verification"]["recordedHead"], "c" * 40)
        self.assertEqual(rule["reviewedEntries"][0]["scopeNote"], "Boolean true/false only.")
        self.assertEqual(data["metadata"]["baselineRevision"], "a" * 40)
        self.assertFalse(data["metadata"]["independentVerifications"][0]["currentSourceVerified"])
        self.assertNotIn("__private__", json.dumps(data))

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

    def test_normalized_followup_preserves_history_and_rejects_invalid_or_replayed_reviews(self):
        original = self.import_catalog()
        catalog_path = self.root / progress_catalog.CATALOG_PATH
        audit_path = self.root / progress_catalog.AUDIT_PATH
        original_catalog, original_audit = catalog_path.read_bytes(), audit_path.read_bytes()
        targets = [row for row in original["entries"] if row["status"] == "missing"]
        review_path = self.root / "docs/progress/reviews/family.json"
        verification = {
            "id": "coordinator-family", "recordedHead": "b" * 40,
            "sourceCapturedAt": "2026-10-28T07:30:00Z", "verifiedAt": "2026-10-28T07:35:00Z",
            "sourceManifestSha256": "c" * 64, "browser": "IAB", "engine": "Chromium",
            "originalFlashComparison": "not_run", "decision": "bounded_family_pass",
            "currentSourceVerified": False, "reviewKind": "coordinator",
        }
        document = {"schemaVersion": 1, "baseCatalogSha256": fingerprint(catalog_path), "reviews": [{
            "id": verification["id"], "title": "Coordinator family review", "reason": "Bounded package passed.",
            "limitations": "Partial behavior only.",
            # The recorded source is historical and need not exist in this working tree.
            "evidence": [{"path": "web/historical-family.js", "sha256": "d" * 64}],
            "verification": verification,
            "reviewedEntries": [{"catalogId": row["catalog_id"], "status": "partial",
                                 "evidenceKind": "coordinator_browser", "scopeNote": "Authored family scenario."}
                                for row in targets],
        }]}
        invalid = json.loads(json.dumps(document))
        invalid["baseCatalogSha256"] = "0" * 64
        write_json(review_path, invalid)
        with self.assertRaisesRegex(ValueError, "hash differs"):
            progress_catalog.apply_review(review_path, self.root)
        invalid = json.loads(json.dumps(document))
        invalid["reviews"][0]["reviewedEntries"][0]["catalogId"] = "AS3:unknown"
        write_json(review_path, invalid)
        with self.assertRaisesRegex(ValueError, "Unknown catalogue ID"):
            progress_catalog.apply_review(review_path, self.root)
        invalid = json.loads(json.dumps(document))
        invalid["reviews"][0]["reviewedEntries"].append(invalid["reviews"][0]["reviewedEntries"][0])
        write_json(review_path, invalid)
        with self.assertRaisesRegex(ValueError, "Duplicate reviewed catalogue ID"):
            progress_catalog.apply_review(review_path, self.root)
        invalid = json.loads(json.dumps(document))
        invalid["reviews"][0]["reviewedEntries"][0]["catalogId"] = next(row["catalog_id"] for row in original["entries"] if row["status"] == "implemented_subset")
        write_json(review_path, invalid)
        with self.assertRaisesRegex(ValueError, "Do not override implemented_subset"):
            progress_catalog.apply_review(review_path, self.root)
        self.assertEqual(catalog_path.read_bytes(), original_catalog)
        self.assertEqual(audit_path.read_bytes(), original_audit)
        write_json(review_path, document)
        result = progress_catalog.apply_review(review_path, self.root)
        self.assertEqual(result["summary"]["total"], {"implemented_subset": 2, "partial": 4, "missing": 0, "needs_review": 2, "total": 8})
        for key in ("baselineRevision", "inputHashes", "implementationSourceHashes", "sourceFiles"):
            self.assertEqual(result["metadata"][key], original["metadata"][key])
        unchanged = {row["catalog_id"]: row for row in original["entries"] if row not in targets}
        self.assertEqual({row["catalog_id"]: row for row in result["entries"] if row["catalog_id"] in unchanged}, unchanged)
        self.assertEqual(result["metadata"]["independentVerifications"][-1], verification)
        self.assertEqual(result["metadata"]["normalizedReviewInputs"][-1]["sha256"], fingerprint(review_path))
        self.assertEqual(result["metadata"]["supersededRules"][0]["id"], "missing")
        rule = next(rule for rule in result["rules"] if rule["id"] == verification["id"])
        self.assertEqual([item["status"] for item in rule["previousEntries"]], ["missing", "missing"])
        self.assertEqual(rule["evidence"], document["reviews"][0]["evidence"])
        updated_catalog, updated_audit = catalog_path.read_bytes(), audit_path.read_bytes()
        with self.assertRaisesRegex(ValueError, "already-used review ID"):
            progress_catalog.apply_review(review_path, self.root)
        self.assertEqual(catalog_path.read_bytes(), updated_catalog)
        self.assertEqual(audit_path.read_bytes(), updated_audit)

    def test_status_review_promotes_and_demotes_with_tests_and_notes(self):
        original = self.import_catalog()
        catalog_path = self.root / progress_catalog.CATALOG_PATH
        (self.root / "tests").mkdir()
        (self.root / "tests/as3.mjs").write_text("// fixture test\n")
        rows = {row["status"]: row["catalog_id"] for row in original["entries"] if row["language"] == "AS3"}
        review_path = self.root / "docs/progress/reviews/2026-11-01-as3-core.json"
        document = {"schemaVersion": 2, "baseCatalogSha256": fingerprint(catalog_path), "reviews": [{
            "id": "AS3-CORE-20261101", "date": "2026-11-01", "commit": "d" * 40,
            "title": "Core promotion", "summary": "Core behavior now passes the shared suite.", "tests": ["tests/as3.mjs"],
            "entries": [{"catalogId": rows["partial"], "status": "working"},
                        {"catalogId": rows["implemented_subset"], "status": "partial", "note": "Edge cases regressed."},
                        {"catalogId": rows["missing"], "status": "partial", "note": "Only the default branch runs."}],
        }]}
        invalid_cases = {
            "already working": lambda d: d["reviews"][0]["entries"][0].update(catalogId=rows["implemented_subset"], status="working"),
            "note needed": lambda d: d["reviews"][0]["entries"][1].pop("note"),
            "unknown test": lambda d: d["reviews"][0].update(tests=["tests/absent.mjs"]),
            "wrong language": lambda d: d["reviews"][0].update(id="AS2-CORE-20261101"),
            "long summary": lambda d: d["reviews"][0].update(summary="x" * 401),
            "legacy status name": lambda d: d["reviews"][0]["entries"][0].update(status="implemented_subset"),
        }
        for label, mutate in invalid_cases.items():
            with self.subTest(label=label):
                invalid = json.loads(json.dumps(document))
                mutate(invalid)
                write_json(review_path, invalid)
                with self.assertRaises(ValueError):
                    progress_catalog.apply_review(review_path, self.root)
        diary = self.root / "docs/progress/reviews/2026-11-01-as3-hour-2.json"
        write_json(diary, document)
        with self.assertRaisesRegex(ValueError, "one per delivery"):
            progress_catalog.apply_review(diary, self.root)
        write_json(review_path, document)
        result = progress_catalog.apply_review(review_path, self.root)
        self.assertEqual(result["summary"]["byLanguage"]["AS3"], {"implemented_subset": 1, "partial": 2, "missing": 0, "needs_review": 1, "total": 4})
        review = result["metadata"]["statusReviews"][-1]
        self.assertEqual(review["changes"], {"partial": 2, "working": 1})
        rule = next(rule for rule in result["rules"] if rule["id"] == "AS3-CORE-20261101")
        self.assertEqual([item["status"] for item in rule["previousEntries"]], ["partial", "implemented_subset", "missing"])
        self.assertEqual(rule["evidence"][0]["path"], "tests/as3.mjs")
        with self.assertRaisesRegex(ValueError, "already-used review ID|baseCatalogSha256"):
            progress_catalog.apply_review(review_path, self.root)

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
