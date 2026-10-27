"""Focused batch-tool checks with a local fake exporter; no Rust builds or browsers."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("browser_batch", ROOT / "scripts/prepare-browser-batch.py")
batch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(batch)


class BrowserBatchTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.exporter = self.root / "exporter.py"
        self.exporter.write_text('''#!/usr/bin/env python3
import json, pathlib, sys, time
source = pathlib.Path(sys.argv[1])
if source.name.startswith("slow"):
    time.sleep(2)
if source.name.startswith("bad"):
    print("Script compilation failed: unsupported source", file=sys.stderr)
    raise SystemExit(1)
output = pathlib.Path(sys.argv[sys.argv.index("--output") + 1])
output.mkdir(parents=True)
(output / "index.html").write_text("<html><body>fixture</body></html>")
(output / "argv.json").write_text(json.dumps(sys.argv[1:]))
print("Exported fixture")
''')
        self.exporter.chmod(0o755)
        self.output = self.root / "batch"

    def source(self, name):
        path = self.root / name
        path.write_text('{"original":true}\n')
        return path

    def run_batch(self, *arguments):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return batch.main(["--exporter", str(self.exporter), "--output", str(self.output), *map(str, arguments)])

    def report(self):
        return json.loads((self.output / "batch.json").read_text())

    def test_independent_failures_continue_and_never_claim_browser_success(self):
        good = self.source("good with spaces;$(never-run).nflash")
        bad = self.source("bad.nflash")
        last = self.source("last.nflash")
        self.assertEqual(self.run_batch(good, bad, last), 1)
        report = self.report()
        self.assertEqual([case["export"]["status"] for case in report["cases"]], ["exported", "compile_failed", "exported"])
        for case in report["cases"]:
            self.assertEqual(case["runtime"]["status"], "not_observed")
            self.assertEqual(case["manual"]["status"], "not_checked")
        first = report["cases"][0]
        self.assertEqual(first["source_sha256"], hashlib.sha256(good.read_bytes()).hexdigest())
        exported = self.output / first["url"]
        self.assertEqual(json.loads(exported.with_name("argv.json").read_text())[0], str(good.resolve()))
        self.assertEqual(first["export"]["files"]["index.html"], hashlib.sha256(exported.read_bytes()).hexdigest())

    def test_fresh_runs_do_not_reuse_a_previous_successful_bundle(self):
        good = self.source("good.nflash")
        self.assertEqual(self.run_batch(good), 0)
        first = self.report()["cases"][0]["url"]
        self.assertEqual(self.run_batch(good), 0)
        second = self.report()["cases"][0]["url"]
        self.assertNotEqual(first, second)
        self.assertTrue((self.output / first).exists())
        self.assertTrue((self.output / second).exists())

    def test_manifest_text_is_html_data_and_case_ids_cannot_escape_output(self):
        project = self.source("good.nflash")
        manifest = self.root / "manifest.json"
        manifest.write_text(json.dumps({"cases": [{"id": "case", "path": str(project),
            "title": "</script><script>alert('injection')</script>", "checks": ["Check artwork"]}]}))
        self.assertEqual(self.run_batch("--manifest", manifest), 0)
        html = (self.output / "index.html").read_text()
        self.assertNotIn("<script>alert('injection')</script>", html)
        self.assertIn("\\u003c/script\\u003e", html)
        manifest.write_text(json.dumps({"cases": [{"id": "../escape", "path": str(project)}]}))
        self.assertEqual(self.run_batch("--manifest", manifest), 2)
        self.assertFalse((self.root / "escape").exists())

    def test_timeout_and_missing_source_do_not_abort_later_exports(self):
        self.assertEqual(self.run_batch("--timeout", "0.15", self.source("slow.nflash"),
            self.root / "missing.nflash", self.source("good.nflash")), 1)
        self.assertEqual([case["export"]["status"] for case in self.report()["cases"]], ["timed_out", "failed", "exported"])

    def test_unowned_destination_is_preserved(self):
        self.output.mkdir()
        original = self.output / "index.html"
        original.write_text("existing unrelated work")
        self.assertEqual(self.run_batch(self.source("good.nflash")), 2)
        self.assertEqual(original.read_text(), "existing unrelated work")
        self.assertFalse((self.output / "runs").exists())


if __name__ == "__main__":
    unittest.main()
