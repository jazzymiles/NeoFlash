import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "check-performance-ab.py"
SPEC = importlib.util.spec_from_file_location("check_performance_ab", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PerformanceAbTests(unittest.TestCase):
    def write_report(self, root: Path, name: str, values: dict[str, float]) -> Path:
        path = root / f"{name}.json"
        results = [
            {
                "suite": suite,
                "frames": 240,
                "rawCpuMsPerFrame": value,
                "snapshotsPerFrame": 1.0,
                "draws": 500,
                "node": MODULE.EXPECTED_NODE,
                "platform": "linux",
                "arch": "x64",
            }
            for suite, value in sorted(values.items())
        ]
        path.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "mode": "report",
                    "node": MODULE.EXPECTED_NODE,
                    "platform": "linux",
                    "arch": "x64",
                    "sample_count": 1,
                    "frames_per_sample": 240,
                    "warmup_frames": 240,
                    "results": results,
                }
            ),
            encoding="utf-8",
        )
        return path

    def reports(self, root: Path, ratios: list[float]):
        baseline = []
        current = []
        baseline_values = {
            suite: 1.0 + index / 10.0
            for index, suite in enumerate(sorted(MODULE.EXPECTED_SUITES))
        }
        for index, ratio in enumerate(ratios):
            baseline.append(
                self.write_report(root, f"baseline-{index}", baseline_values)
            )
            current.append(
                self.write_report(
                    root,
                    f"current-{index}",
                    {suite: value * ratio for suite, value in baseline_values.items()},
                )
            )
        return current, baseline

    def test_five_balanced_pairs_pass_below_five_percent(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(Path(temp), [1.01, 1.02, 1.00, 1.03, 1.01])
            summary = MODULE.compare_reports(current, baseline)
        self.assertEqual(summary["pair_count"], 5)
        self.assertFalse(summary["failures"])
        self.assertLess(summary["suites"][0]["geometric_mean"], 1.05)

    def test_sustained_regression_fails_without_dropping_a_pair(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(Path(temp), [1.08] * 5)
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "sustained regression"):
                MODULE.compare_reports(current, baseline)

    def test_excessive_ratio_dispersion_is_inconclusive_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(Path(temp), [0.8, 1.2, 0.8, 1.2, 1.0])
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "inconclusive dispersion"):
                MODULE.compare_reports(current, baseline)

    def test_suite_inventory_mismatch_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            current, baseline = self.reports(root, [1.0] * 5)
            mismatched = json.loads(current[2].read_text(encoding="utf-8"))
            mismatched["results"].pop()
            current[2].write_text(json.dumps(mismatched), encoding="utf-8")
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "suite inventory changed"):
                MODULE.compare_reports(current, baseline)

    def test_shared_suite_omission_still_fails_required_inventory(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(Path(temp), [1.0] * 5)
            for path in current + baseline:
                report = json.loads(path.read_text(encoding="utf-8"))
                report["results"].pop()
                path.write_text(json.dumps(report), encoding="utf-8")
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "required 14 suites"):
                MODULE.compare_reports(current, baseline)

    def test_report_mode_mismatch_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(Path(temp), [1.0] * 5)
            mismatched = json.loads(current[0].read_text(encoding="utf-8"))
            mismatched["mode"] = "functional"
            current[0].write_text(json.dumps(mismatched), encoding="utf-8")
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "report mode"):
                MODULE.compare_reports(current, baseline)

    def test_functional_counter_mismatch_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(Path(temp), [1.0] * 5)
            mismatched = json.loads(current[1].read_text(encoding="utf-8"))
            mismatched["results"][0]["draws"] += 1
            current[1].write_text(json.dumps(mismatched), encoding="utf-8")
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "draw coverage changed"):
                MODULE.compare_reports(current, baseline)


if __name__ == "__main__":
    unittest.main()
