import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "compare-fla-corpus.py"
SPEC = importlib.util.spec_from_file_location("compare_fla_corpus", SCRIPT)
corpus = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(corpus)


class CaptureProvenanceTests(unittest.TestCase):
    def test_rejects_adobe_receipt_from_another_input(self):
        with tempfile.TemporaryDirectory() as temp:
            case = Path(temp)
            staged = case / "input.fla"
            staged.write_bytes(b"first revision")
            source = {"id": "case", "staged": staged.name, "source_sha256": corpus.digest(staged),
                      "capture_token": "case:new-input:recipe"}
            (case / "complete.txt").write_text("case:old-input:recipe")
            with patch.object(corpus, "helper", return_value={"scenes": []}):
                with self.assertRaisesRegex(ValueError, "different source"):
                    corpus.compare_case(case, source, Path("unused"), "helper hash")
            staged.write_bytes(b"changed after capture")
            with self.assertRaisesRegex(ValueError, "Staged source has changed"):
                corpus.compare_case(case, source, Path("unused"), "helper hash")

    def test_rejects_helper_replaced_during_comparison(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            tool = root / "helper"
            tool.write_text("first build")
            recipe = root / "corpus.jsfl"
            recipe.write_text("capture command")
            source = {"id": "case", "source_sha256": "source", "capture_token": "case:source:" + corpus.digest(recipe)}
            (root / "inputs.json").write_text(json.dumps([source]))

            def replace_helper(*_args):
                tool.write_text("second build")
                return {"id": "case"}

            with patch.object(corpus, "compare_case", side_effect=replace_helper):
                with self.assertRaisesRegex(ValueError, "changed during measurement"):
                    corpus.compare(root, tool)
            self.assertFalse((root / "comparison.json").exists())
            recipe.write_text("changed capture command")
            with self.assertRaisesRegex(ValueError, "capture command changed"):
                corpus.compare(root, tool)
