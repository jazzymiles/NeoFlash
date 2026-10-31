"""Regression checks for counts, immutable publication and Git-backed history."""

import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


MODULE_PATH = Path(__file__).resolve().parents[1] / "progress.py"
sys.path.insert(0, str(MODULE_PATH.parent))
from test_progress_catalog import write_catalog_fixture
from progress_as3 import build_manifest

SPEC = importlib.util.spec_from_file_location("progress", MODULE_PATH)
progress = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(progress)


class ProgressTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="neoflash-progress-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.git("init", "-b", "main")
        self.git("config", "user.name", "Progress fixture")
        self.git("config", "user.email", "progress@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.write("src/evidence.txt", "Original implementation evidence.\n")

    def git(self, *args, env=None):
        result = subprocess.run(
            ["git", "-C", str(self.root), *args],
            check=True, capture_output=True, text=True, env=env,
        )
        return result.stdout.strip()

    def write(self, path, text):
        path = self.root / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def write_json(self, path, value):
        self.write(path, json.dumps(value) + "\n")

    def commit(self, message, date="2026-10-27T12:00:00+00:00", author_date=None):
        self.git("add", "--all")
        env = {**os.environ, "GIT_COMMITTER_DATE": date, "GIT_AUTHOR_DATE": author_date or date}
        self.git("commit", "--allow-empty", "-m", message, env=env)
        return self.git("rev-parse", "HEAD")

    def inventory(self, statuses=("implemented", "partial", "planned"), method=3):
        items = {"implemented": ["working"], "partial": ["working", "planned"], "planned": ["planned"]}
        categories, features = [], []
        for index, status in enumerate(statuses):
            name = f"area{index}"
            categories.append({"id": name, "title": name.upper(), "description": "A bounded feature group."})
            features.append({
                "id": name + "-example", "title": "Example", "status": status, "category": name,
                "scope": "Specified bounded behavior.", "limitations": "No wider parity claim.",
                "items": [{"id": f"function-{n}", "title": f"Function {n}", "status": item} for n, item in enumerate(items[status])],
                "evidence": ["src/evidence.txt"],
            })
        self.write_json("docs/progress/product.json", {"schemaVersion": 1, "categories": categories, "features": features})
        self.write_json("docs/progress/methodology.json", {"version": method})
        if method >= 2:
            write_catalog_fixture(self.root)
        self.write("README.md", "Before\n<!-- progress:start -->\n<!-- progress:end -->\nAfter\n")
        for name in ("index.html", "history.html", "style.css", "app.js"):
            self.write(f"web/progress/{name}", f"committed {name}\n")
        return progress.read_inventory(self.root)

    def priority_manifest(self):
        catalog = progress.load_catalog(self.root)
        source = next(item for item in catalog["metadata"]["sourceFiles"] if item["file"] == "as3.json")
        reference = {
            "source_baseline": {"reference_generation_date": source["referenceGeneration"]},
            "entries": [{**row, "id": row["catalog_id"], "availability": [{"runtime": "Flash", "version": "9"}],
                         "detail_availability": None}
                        for row in catalog["entries"] if row["language"] == "AS3"],
        }
        self.write_json("docs/progress/as3-priorities.json", build_manifest(reference, catalog, source["sha256"]))

    def test_functions_are_counted_inside_groups(self):
        categories, features = self.inventory()
        self.assertEqual(progress.counts(features), {"implemented": 1, "partial": 1, "planned": 1, "total": 3})
        self.assertEqual(progress.item_counts(features), {"working": 2, "partial": 0, "planned": 2, "total": 4})
        table = progress.summary_table(categories, features)
        self.assertIn("| AREA1 | 1 of 2 | 0 | 1 | 0 / 1 / 0 |", table)
        self.assertIn("| **All areas** | 2 of 4 | 0 | 2 | 1 / 1 / 1 |", table)

    def test_invalid_ids_status_items_and_evidence_are_rejected(self):
        categories, features = self.inventory()
        mutations = {
            "duplicate category": lambda c, f: c.append(copy.deepcopy(c[0])),
            "duplicate capability": lambda c, f: f.append(copy.deepcopy(f[0])),
            "invalid status": lambda c, f: f[0].update(status="mostly-done"),
            "unknown category": lambda c, f: f[0].update(category="missing"),
            "no evidence": lambda c, f: f[0].update(evidence=[]),
            "missing file": lambda c, f: f[0].update(evidence=["does-not-exist.txt"]),
            "parent escape": lambda c, f: f[0].update(evidence=["../outside.txt"]),
            "absolute path": lambda c, f: f[0].update(evidence=["/tmp/outside.txt"]),
            "URL evidence": lambda c, f: f[0].update(evidence=["https://example.invalid/evidence"]),
            "directory evidence": lambda c, f: f[0].update(evidence=["src"]),
            "no items": lambda c, f: f[0].update(items=[]),
            "closed group with planned item": lambda c, f: f[0]["items"].append({"id": "later", "title": "Later", "status": "planned"}),
            "partial group with only working items": lambda c, f: f[1].update(items=[{"id": "done", "title": "Done", "status": "working"}]),
            "unknown item status": lambda c, f: f[0]["items"][0].update(status="implemented"),
            "duplicate item": lambda c, f: f[1]["items"].append(copy.deepcopy(f[1]["items"][0])),
        }
        for label, mutate in mutations.items():
            with self.subTest(label=label):
                changed_categories, changed_features = copy.deepcopy((categories, features))
                mutate(changed_categories, changed_features)
                with self.assertRaises(ValueError):
                    progress.validate_inventory(self.root, changed_categories, changed_features, require_items=True)

    def test_daily_history_uses_utc_committer_dates_and_last_ancestry_commit(self):
        first = self.commit("first", "2026-10-26T23:30:00-02:00", "2020-01-01T00:00:00+00:00")
        last = self.commit("last", "2026-10-27T00:30:00+00:00", "2021-01-01T00:00:00+00:00")
        self.assertNotEqual(first, last)
        self.assertEqual(progress.daily_revisions(self.root, "HEAD"), [("2026-10-27", last)])

    def test_daily_history_excludes_side_branch_commits(self):
        base = self.commit("base", "2026-10-25T12:00:00+00:00")
        self.git("checkout", "-b", "side")
        self.write("side.txt", "side branch only\n")
        side = self.commit("side", "2026-10-26T12:00:00+00:00")
        self.git("checkout", "main")
        main = self.commit("main", "2026-10-27T12:00:00+00:00")
        env = {**os.environ, "GIT_COMMITTER_DATE": "2026-10-28T12:00:00+00:00", "GIT_AUTHOR_DATE": "2026-10-28T12:00:00+00:00"}
        self.git("merge", "--no-ff", "side", "-m", "merge side", env=env)
        merged = self.git("rev-parse", "HEAD")
        revisions = progress.daily_revisions(self.root, "HEAD")
        self.assertEqual(revisions, [("2026-10-25", base), ("2026-10-27", main), ("2026-10-28", merged)])
        self.assertNotIn(side, [sha for _, sha in revisions])

    def test_history_skips_days_without_docs_or_without_registry(self):
        self.commit("before docs", "2026-10-24T12:00:00+00:00")
        self.write("docs/ROADMAP.md", "Future work.\n")
        self.commit("docs without registry", "2026-10-25T12:00:00+00:00")
        self.inventory()
        measured = self.commit("first measurement", "2026-10-27T12:00:00+00:00")
        history = progress.history(self.root, "HEAD")
        self.assertEqual([(row["date"], row["revision"]) for row in history], [("2026-10-27", measured)])
        self.assertEqual(history[0]["total"]["total"], 3)

    def test_later_commit_replaces_the_same_day_snapshot(self):
        self.inventory()
        morning = self.commit("morning", "2026-10-27T09:00:00+00:00")
        self.inventory(("implemented", "implemented", "planned"))
        evening = self.commit("evening", "2026-10-27T19:00:00+00:00")
        records = progress.history(self.root, "HEAD")
        self.assertEqual(len(records), 1)
        self.assertEqual(records[0]["revision"], evening)
        self.assertEqual(records[0]["total"]["implemented"], 2)
        self.assertEqual(progress.history(self.root, morning)[0]["total"]["implemented"], 1)

    def test_build_uses_committed_inventory_and_assets_even_with_dirty_files(self):
        self.inventory()
        revision = self.commit("published measurement")
        self.write("docs/progress/product.json", "uncommitted invalid JSON")
        self.write("docs/progress/actionscript-catalog.json", "invalid uncommitted catalogue")
        self.write("web/progress/index.html", "uncommitted page")
        (self.root / "src/evidence.txt").unlink()
        output = self.root / "dist/site"
        data = progress.build(self.root, output, revision)
        self.assertEqual(data["revision"], revision)
        self.assertEqual(data["schemaVersion"], 3)
        self.assertEqual(data["generatedAt"], "2026-10-27T12:00:00Z")
        self.assertEqual(data["history"][0]["total"]["implemented"], 1)
        self.assertEqual(data["actionscript"]["targets"]["AS2"]["target"]["total"], 2)
        self.assertNotIn("entries", data["actionscript"])
        self.assertEqual((output / "index.html").read_text(), "committed index.html\n")
        self.assertEqual((output / "progress.html").read_bytes(), (output / "index.html").read_bytes())
        self.assertEqual(json.loads((output / "progress.json").read_text()), data)
        self.assertEqual(json.loads((output / "history.json").read_text()), data["history"])
        self.assertTrue((output / ".nojekyll").is_file())

    def test_history_switches_product_unit_and_recounts_actionscript_targets(self):
        self.inventory(method=1)
        old = self.commit("method one", "2026-10-25T12:00:00+00:00")
        self.inventory(method=2)
        audit = self.commit("method two", "2026-10-26T12:00:00+00:00")
        self.inventory(method=3)
        new = self.commit("method three", "2026-10-27T12:00:00+00:00")
        data = progress.build(self.root, self.root / "dist/site", new)
        first, second, third = data["history"]
        self.assertEqual([(row["revision"], row["methodologyVersion"]) for row in data["history"]], [(old, 1), (audit, 2), (new, 3)])
        self.assertNotIn("as2", first["categories"])
        self.assertEqual(second["categories"]["area1"], {"implemented": 0, "partial": 1, "planned": 0, "total": 1})
        self.assertEqual(third["categories"]["area1"]["metric"], "functions")
        self.assertEqual(third["categories"]["area1"]["total"], 2)
        # Both catalogue days are recounted with the same target rules.
        self.assertEqual(second["categories"]["as2"], third["categories"]["as2"])
        self.assertEqual(third["categories"]["as2"]["metric"], "flash-player-target")
        self.assertEqual(third["categories"]["as2"]["working"], 1)
        self.assertEqual(third["categories"]["as2"]["total"], 2)
        self.assertEqual({category["id"] for category in data["categories"]}, {"area0", "area1", "area2", "as2"})

    def test_reports_show_flash_player_targets_and_detect_drift(self):
        categories, features = self.inventory()
        progress.reports(self.root, categories, features, write=True)
        progress.reports(self.root, categories, features)
        readme = (self.root / "README.md").read_text()
        self.assertIn("| 1. Mathematics and 2D geometry | 2 | 1 | 1 | 0 | 0 | 50.0% | 100.0% |", readme)
        self.assertIn("Outside the target and never in a percentage: AS2 UI components 2", readme)
        self.assertNotIn("ActionScript 3 — Flash Player", readme)
        report = (self.root / "docs/progress/ACTIONSCRIPT.md").read_text()
        self.assertIn("## Full reference catalogue", report)
        self.write("docs/progress/ACTIONSCRIPT.md", "Outdated audit report.\n")
        with self.assertRaisesRegex(ValueError, "stale"):
            progress.reports(self.root, categories, features)

    def test_as3_target_reads_the_selected_revision_manifest(self):
        self.inventory()
        earlier = self.commit("audit before priority plan", "2026-10-26T12:00:00+00:00")
        self.priority_manifest()
        current = self.commit("priority plan", "2026-10-27T12:00:00+00:00")
        self.write("docs/progress/as3-priorities.json", "invalid uncommitted priority plan")
        data = progress.build(self.root, self.root / "dist/current", current)
        as3 = data["actionscript"]["targets"]["AS3"]
        self.assertEqual(as3["total"]["total"], 4)
        self.assertEqual(as3["target"]["total"], 3)  # The framework entry is a library, outside the target.
        self.assertEqual([item["counts"]["total"] for item in as3["outside"]], [0, 1, 0])
        self.assertNotIn("catalog_id", json.dumps(data["actionscript"]["targets"]))
        self.assertEqual([("as3" in row["categories"]) for row in data["history"]], [False, True])
        historical = progress.build(self.root, self.root / "dist/earlier", earlier)
        self.assertNotIn("AS3", historical["actionscript"]["targets"])

    def test_generated_report_drift_is_rejected(self):
        categories, features = self.inventory()
        progress.reports(self.root, categories, features, write=True)
        progress.reports(self.root, categories, features)
        self.assertTrue((self.root / "README.md").read_text().startswith("Before\n"))
        self.assertTrue((self.root / "README.md").read_text().endswith("\nAfter\n"))
        features[0]["items"].append({"id": "new-function", "title": "New function", "status": "working"})
        with self.assertRaisesRegex(ValueError, "stale"):
            progress.reports(self.root, categories, features)
        progress.reports(self.root, categories, features, write=True)
        self.write("docs/progress/DETAILS.md", "Manually changed generated report.\n")
        with self.assertRaisesRegex(ValueError, "stale"):
            progress.reports(self.root, categories, features)

    def test_incomplete_historical_registry_fails_instead_of_publishing_zeroes(self):
        self.inventory()
        (self.root / "docs/progress/product.json").unlink()
        self.commit("incomplete measurement")
        with self.assertRaisesRegex(ValueError, "Incomplete historical inventory"):
            progress.history(self.root, "HEAD")


if __name__ == "__main__":
    unittest.main()
