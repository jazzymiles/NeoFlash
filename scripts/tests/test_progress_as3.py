"""Check platform boundaries, preserved status counts and frozen priority data."""

import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import progress_as3
import progress_catalog


def fixture():
    definitions = [
        ("flash.net.URLRequest", "URLRequest", "class", "runtime", "Flash", "missing"),
        ("flash.net.URLRequest", "data", "property", "runtime", "Flash", "partial"),
        ("flash.net.URLRequest", "authenticate", "property", "runtime", "AIR", "missing"),
        ("flash.net.URLRequest", "unlabeled", "property", "runtime", None, "needs_review"),
        ("fl.controls.Button", "Button", "class", "component", "Flash", "missing"),
        ("mx.controls.Button", "Button", "class", "framework", "Flash", "missing"),
        ("<language>", "if", "statement", "language", None, "implemented_subset"),
        ("flash.desktop", "flash.desktop", "package", "runtime", None, "needs_review"),
        ("flash.desktop.Clipboard", "Clipboard", "constructor", "runtime", "Flash", "missing"),
        ("flash.utils.ByteArray", "readInt", "method", "runtime", "Flash", "partial"),
        ("flash.utils.ByteArray", "shareable", "property", "runtime", "Flash", "missing"),
        ("flash.system.System", "pause", "method", "runtime", "Flash", "missing"),
        ("Math", "sqrt", "method", "core", "Flash", "partial"),
    ]
    entries, originals = [], []
    for index, (owner, name, kind, scope, runtime, status) in enumerate(definitions):
        identifier = f"AS3:fixture:{index}"
        if owner == "flash.desktop.Clipboard":
            identifier = "AS3:constructor:flash/desktop/Clipboard.html#Clipboard()"
        identity = {"language": "AS3", "owner": owner, "name": name, "kind": kind, "scope": scope,
                    "source_url": "https://help.adobe.com/reference/" + str(index)}
        entries.append({**identity, "catalog_id": identifier, "status": status,
                        "rule_id": "fixture", "evidence_kind": "fixture_evidence"})
        originals.append({**identity, "id": identifier, "availability": [{"runtime": runtime, "version": "1"}] if runtime else []})
    # A shared class is not authority for an AIR-only member. Detail wins over a
    # conflicting index label, as happens with metadata differences in Adobe.
    originals[2]["availability"] = [{"runtime": "Flash", "version": "9"}]
    originals[2]["detail_availability"] = [{"runtime": "AIR", "version": "1"}]
    reference = {"entries": originals, "source_baseline": {"reference_generation_date": "2018-12-06"}}
    sha = hashlib.sha256(json.dumps(reference).encode()).hexdigest()
    catalog = {"entries": entries, "metadata": {"sourceFiles": [{"file": "as3.json", "sha256": sha}]}}
    return catalog, progress_as3.build_manifest(reference, catalog, sha)


class AS3PriorityTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="neoflash-as3-priority-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.catalog, self.manifest = fixture()
        self.path = self.root / progress_as3.MANIFEST_PATH
        self.path.parent.mkdir(parents=True)
        self.save()

    def save(self):
        self.path.write_text(json.dumps(self.manifest) + "\n")

    def test_player_class_air_member_unknown_member_and_library_are_separate(self):
        rows = progress_as3.classify_entries(self.manifest, self.catalog)
        by_name = {(row["owner"], row["name"]): row for row in rows}
        self.assertEqual(by_name["flash.net.URLRequest", "URLRequest"]["bucket"], "player")
        self.assertEqual(by_name["flash.net.URLRequest", "authenticate"]["bucket"], "air")
        self.assertEqual(by_name["flash.net.URLRequest", "unlabeled"]["bucket"], "unresolved")
        self.assertEqual(by_name["flash.desktop", "flash.desktop"]["bucket"], "unresolved")
        self.assertEqual(by_name["<language>", "if"]["bucket"], "player")
        for owner in ("fl.controls.Button", "mx.controls.Button"):
            self.assertEqual(by_name[owner, "Button"]["bucket"], "library")

    def test_existing_status_and_evidence_are_unchanged_and_partitions_conserve_counts(self):
        original = copy.deepcopy(self.catalog)
        result = progress_as3.load_priorities(self.root, self.catalog)
        self.assertEqual(self.catalog, original)
        self.assertEqual(result["total"], progress_catalog.counter(self.catalog["entries"]))
        for collection in ("buckets", "groups"):
            for status in (*progress_catalog.STATUSES, "total"):
                self.assertEqual(sum(row["counts"][status] for row in result[collection]), result["total"][status])
        player = next(row["counts"] for row in result["buckets"] if row["id"] == "player")
        for status in (*progress_catalog.STATUSES, "total"):
            self.assertEqual(sum(row["counts"][status] for row in result["stages"]), player[status])
        self.assertFalse(result["currentSourceVerified"])

    def test_specialized_members_and_restricted_constructor_are_not_foundation(self):
        rows = progress_as3.classify_entries(self.manifest, self.catalog)
        by_name = {(row["owner"], row["name"]): row for row in rows}
        self.assertEqual(by_name["flash.utils.ByteArray", "readInt"]["stage"], "objects-language")
        for key in (("flash.utils.ByteArray", "shareable"), ("flash.system.System", "pause"), ("flash.desktop.Clipboard", "Clipboard")):
            self.assertEqual(by_name[key]["stage"], "services-advanced")
        self.assertIn("AS3:constructor:flash/desktop/Clipboard.html#Clipboard()", self.manifest["profileRestrictions"])

    def test_hash_identity_and_unknown_override_fail_closed(self):
        changed = copy.deepcopy(self.manifest)
        changed["source"]["referenceSha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "reference hash"):
            progress_as3.classify_entries(changed, self.catalog)
        changed = copy.deepcopy(self.catalog)
        changed["entries"][0]["owner"] = "OtherClass"
        with self.assertRaisesRegex(ValueError, "catalogue identities"):
            progress_as3.classify_entries(self.manifest, changed)
        changed = copy.deepcopy(self.manifest)
        changed["entryExceptions"]["AS3:unknown"] = ["player", "math-geometry"]
        with self.assertRaisesRegex(ValueError, "unknown catalogue IDs"):
            progress_as3.classify_entries(changed, self.catalog)

    def test_definitions_and_routes_are_read_from_manifest_not_current_policy(self):
        stage = self.manifest["stages"][0]
        old_id = stage["id"]
        stage.update(id="historical-math", title="Frozen historical stage", order=20)
        for route in [*self.manifest["ownerDefaults"].values(), *self.manifest["entryExceptions"].values()]:
            if route[1] == old_id:
                route[1] = stage["id"]
        self.save()
        result = progress_as3.load_priorities(self.root, self.catalog)
        self.assertEqual(result["stages"][-1]["title"], "Frozen historical stage")
        self.assertEqual(result["stages"][-1]["counts"]["partial"], 1)

    def test_historical_build_ignores_dirty_manifest_and_never_reads_raw_reference(self):
        def git(*args):
            return subprocess.check_output(["git", "-C", str(self.root), *args], stderr=subprocess.DEVNULL).decode().strip()
        git("init", "-q")
        git("config", "user.name", "Fixture")
        git("config", "user.email", "fixture@example.invalid")
        git("add", ".")
        git("commit", "-qm", "priority fixture")
        revision = git("rev-parse", "HEAD")
        expected = progress_as3.load_priorities(self.root, self.catalog, revision)
        self.manifest["stages"][0]["purpose"] = "Dirty local purpose"
        self.save()
        self.assertEqual(progress_as3.load_priorities(self.root, self.catalog, revision), expected)
        self.assertNotEqual(progress_as3.load_priorities(self.root, self.catalog), expected)
        self.path.unlink()
        self.assertIsNone(progress_as3.load_priorities(self.root, self.catalog))
        self.assertEqual(progress_as3.load_priorities(self.root, self.catalog, revision), expected)

    def test_repository_manifest_covers_exact_existing_as3_catalogue(self):
        root = Path(__file__).resolve().parents[2]
        catalog = progress_catalog.load_catalog(root)
        result = progress_as3.load_priorities(root, catalog)
        self.assertEqual(result["total"], catalog["summary"]["byLanguage"]["AS3"])
        self.assertEqual(result["total"]["total"], 43740)


if __name__ == "__main__":
    unittest.main()
