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

    def inventory(self, statuses=("implemented", "partial", "planned"), method=1):
        for name, status in zip(("as2", "as3", "product"), statuses):
            category = {"id": name, "title": name.upper(), "description": "A bounded feature group."}
            feature = {
                "id": name + "-example", "title": "Example", "status": status,
                "scope": "Specified bounded behavior.", "limitations": "No wider parity claim.",
                "evidence": ["src/evidence.txt"],
            }
            if name == "product":
                feature["category"] = name
                data = {"schemaVersion": 1, "categories": [category], "features": [feature]}
            else:
                data = {"schemaVersion": 1, "category": category, "features": [feature]}
            self.write_json(f"docs/progress/{name}.json", data)
        self.write_json("docs/progress/methodology.json", {"version": method})
        if method >= 2:
            write_catalog_fixture(self.root)
        self.write("README.md", "Before\n<!-- progress:start -->\n<!-- progress:end -->\nAfter\n")
        for name in ("index.html", "history.html", "style.css", "app.js"):
            self.write(f"web/progress/{name}", f"committed {name}\n")
        return progress.read_inventory(self.root)

    def test_partial_never_contributes_to_completed_numerator(self):
        categories, features = self.inventory()
        count = progress.counts(features)
        self.assertEqual(count, {"implemented": 1, "partial": 1, "planned": 1, "total": 3})
        combined = [{"id": "all", "title": "All", "description": "All capabilities."}]
        combined_features = [{**feature, "category": "all"} for feature in features]
        self.assertIn("| All | 1/3 | 1 | 1 | 33.3% |", progress.summary_table(combined, combined_features))

    def test_invalid_ids_status_and_evidence_are_rejected(self):
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
        }
        for label, mutate in mutations.items():
            with self.subTest(label=label):
                changed_categories, changed_features = copy.deepcopy((categories, features))
                mutate(changed_categories, changed_features)
                with self.assertRaises(ValueError):
                    progress.validate_inventory(self.root, changed_categories, changed_features)

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
        self.write("web/progress/index.html", "uncommitted page")
        (self.root / "src/evidence.txt").unlink()
        output = self.root / "dist/site"
        data = progress.build(self.root, output, revision)
        self.assertEqual(data["revision"], revision)
        self.assertEqual(data["generatedAt"], "2026-10-27T12:00:00Z")
        self.assertEqual(data["history"][0]["total"]["implemented"], 1)
        self.assertEqual((output / "index.html").read_text(), "committed index.html\n")
        self.assertEqual((output / "progress.html").read_bytes(), (output / "index.html").read_bytes())
        self.assertEqual(json.loads((output / "progress.json").read_text()), data)
        self.assertEqual(json.loads((output / "history.json").read_text()), data["history"])
        self.assertTrue((output / ".nojekyll").is_file())

    def test_history_preserves_methodology_versions_per_revision(self):
        self.inventory(method=1)
        old = self.commit("method one", "2026-10-26T12:00:00+00:00")
        self.inventory(method=2)
        new = self.commit("method two", "2026-10-27T12:00:00+00:00")
        data = progress.build(self.root, self.root / "dist/site", new)
        self.assertEqual(data["methodology"]["version"], 2)
        self.assertEqual(data["schemaVersion"], 2)
        self.assertEqual([(row["revision"], row["methodologyVersion"]) for row in data["history"]], [(old, 1), (new, 2)])
        self.assertEqual(set(data["history"][0]["categories"]), {"product"})
        self.assertEqual(data["history"][0]["categories"]["product"]["planned"], 1)
        self.assertEqual(data["history"][1]["categories"]["as2"], {
            "implemented": 1, "partial": 1, "planned": 1, "missing": 1,
            "needs_review": 1, "total": 4, "metric": "catalogue",
        })
        self.assertEqual(data["history"][1]["total"]["total"], 1)

    def test_family_counts_are_not_relabelled_as_declaration_history(self):
        self.inventory(method=1)
        for language, count in (("as2", 35), ("as3", 36)):
            path = self.root / f"docs/progress/{language}.json"
            inventory = json.loads(path.read_text())
            prototype = inventory["features"][0]
            inventory["features"] = [{**prototype, "id": f"{language}-family-{index}"} for index in range(count)]
            self.write_json(path.relative_to(self.root), inventory)
        original = self.commit("old family denominators", "2026-10-26T12:00:00+00:00")
        self.inventory(method=2)
        corrected = self.commit("declaration catalogue", "2026-10-27T12:00:00+00:00")
        data = progress.build(self.root, self.root / "dist/site", corrected)
        old, current = data["history"]
        self.assertEqual(old["revision"], original)
        self.assertNotIn("as2", old["categories"])
        self.assertNotIn("as3", old["categories"])
        self.assertIn("product", old["categories"])
        self.assertEqual(current["categories"]["as2"]["total"], 4)
        self.assertEqual(current["categories"]["as3"]["total"], 4)
        self.assertEqual({feature["category"] for feature in data["features"]}, {"product"})
        self.assertEqual(data["actionscript"]["counts"]["AS3"]["needs_review"], 1)
        self.assertNotIn("entries", data["actionscript"])

    def test_catalogue_build_ignores_dirty_catalogue_and_preserves_review_count(self):
        self.inventory(method=2)
        revision = self.commit("immutable declaration audit")
        self.write("docs/progress/actionscript-catalog.json", "invalid uncommitted catalogue")
        self.write("docs/progress/actionscript-audit.json", "invalid uncommitted audit")
        data = progress.build(self.root, self.root / "dist/site", revision)
        self.assertEqual(data["actionscript"]["counts"]["AS2"], {
            "implemented_subset": 1, "partial": 1, "missing": 1, "needs_review": 1, "total": 4,
        })
        self.assertEqual(data["history"][0]["categories"]["as2"]["total"], 4)
        self.assertEqual(data["history"][0]["categories"]["as2"]["needs_review"], 1)

    def test_catalogue_reports_include_every_status_and_detect_drift(self):
        categories, features = self.inventory(method=2)
        progress.reports(self.root, categories, features, write=True)
        progress.reports(self.root, categories, features)
        readme = (self.root / "README.md").read_text()
        self.assertIn("| AS2 | 4 | 1 | 1 | 1 | 1 |", readme)
        self.assertIn("Needs review", readme)
        self.write("docs/progress/ACTIONSCRIPT.md", "Outdated audit report.\n")
        with self.assertRaisesRegex(ValueError, "stale"):
            progress.reports(self.root, categories, features)

    def test_generated_report_drift_is_rejected(self):
        categories, features = self.inventory()
        progress.reports(self.root, categories, features, write=True)
        progress.reports(self.root, categories, features)
        self.assertTrue((self.root / "README.md").read_text().startswith("Before\n"))
        self.assertTrue((self.root / "README.md").read_text().endswith("\nAfter\n"))
        features[0]["status"] = "partial"
        with self.assertRaisesRegex(ValueError, "stale"):
            progress.reports(self.root, categories, features)
        progress.reports(self.root, categories, features, write=True)
        self.write("docs/progress/DETAILS.md", "Manually changed generated report.\n")
        with self.assertRaisesRegex(ValueError, "stale"):
            progress.reports(self.root, categories, features)

    def test_incomplete_historical_registry_fails_instead_of_publishing_zeroes(self):
        self.inventory()
        (self.root / "docs/progress/as3.json").unlink()
        self.commit("incomplete measurement")
        with self.assertRaisesRegex(ValueError, "Incomplete historical inventory"):
            progress.history(self.root, "HEAD")


if __name__ == "__main__":
    unittest.main()
