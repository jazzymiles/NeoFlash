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
    def write_report(
        self,
        root: Path,
        name: str,
        values: dict[str, float],
        *,
        platform: str = "linux",
    ) -> Path:
        path = root / f"{name}.json"
        results = [
            {
                "suite": suite,
                "frames": MODULE.EXPECTED_FRAMES,
                "rawCpuMsPerFrame": value,
                "snapshotsPerFrame": 1.0,
                "draws": 500,
                "node": MODULE.EXPECTED_NODE,
                "platform": platform,
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
                    "platform": platform,
                    "arch": "x64",
                    "sample_count": MODULE.EXPECTED_SAMPLE_COUNT,
                    "frames_per_sample": MODULE.EXPECTED_FRAMES,
                    "warmup_frames": MODULE.EXPECTED_WARMUP_FRAMES,
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

    def test_six_balanced_pairs_pass_below_five_percent(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(
                Path(temp), [1.01, 1.02, 1.00, 1.03, 1.01, 1.00]
            )
            summary = MODULE.compare_reports(current, baseline)
        self.assertEqual(summary["pair_count"], MODULE.PAIR_COUNT)
        self.assertFalse(summary["failures"])
        self.assertLess(summary["suites"][0]["geometric_mean"], 1.05)

    def test_sustained_regression_fails_without_dropping_a_pair(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(Path(temp), [1.08] * MODULE.PAIR_COUNT)
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "sustained regression"):
                MODULE.compare_reports(current, baseline)

    def test_excessive_ratio_dispersion_is_inconclusive_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(
                Path(temp), [0.8, 1.2, 0.8, 1.2, 1.0, 1.0]
            )
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "inconclusive dispersion"):
                MODULE.compare_reports(current, baseline)

    def test_suite_inventory_mismatch_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            current, baseline = self.reports(root, [1.0] * MODULE.PAIR_COUNT)
            mismatched = json.loads(current[2].read_text(encoding="utf-8"))
            mismatched["results"].pop()
            current[2].write_text(json.dumps(mismatched), encoding="utf-8")
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "suite inventory changed"):
                MODULE.compare_reports(current, baseline)

    def test_shared_suite_omission_still_fails_required_inventory(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(Path(temp), [1.0] * MODULE.PAIR_COUNT)
            for path in current + baseline:
                report = json.loads(path.read_text(encoding="utf-8"))
                report["results"].pop()
                path.write_text(json.dumps(report), encoding="utf-8")
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "required 14 suites"):
                MODULE.compare_reports(current, baseline)

    def test_report_mode_mismatch_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(Path(temp), [1.0] * MODULE.PAIR_COUNT)
            mismatched = json.loads(current[0].read_text(encoding="utf-8"))
            mismatched["mode"] = "functional"
            current[0].write_text(json.dumps(mismatched), encoding="utf-8")
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "report mode"):
                MODULE.compare_reports(current, baseline)

    def test_functional_counter_mismatch_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            current, baseline = self.reports(Path(temp), [1.0] * MODULE.PAIR_COUNT)
            mismatched = json.loads(current[1].read_text(encoding="utf-8"))
            mismatched["results"][0]["draws"] += 1
            current[1].write_text(json.dumps(mismatched), encoding="utf-8")
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "draw coverage changed"):
                MODULE.compare_reports(current, baseline)

    def test_pair_sequence_is_symmetric_and_complete(self):
        self.assertEqual(MODULE.PAIR_COUNT % 2, 0)
        self.assertEqual(len(MODULE.PAIR_SEQUENCE), 2 * MODULE.PAIR_COUNT)
        first_slots = {"current": 0, "baseline": 0}
        seen = set()
        for pair_index in range(MODULE.PAIR_COUNT):
            pair_entries = [
                label for label, idx in MODULE.PAIR_SEQUENCE if idx == pair_index
            ]
            self.assertEqual(sorted(pair_entries), ["baseline", "current"])
            first_slots[pair_entries[0]] += 1
            for label in pair_entries:
                seen.add((label, pair_index))
        self.assertEqual(len(seen), 2 * MODULE.PAIR_COUNT)
        self.assertEqual(first_slots["current"], MODULE.PAIR_COUNT // 2)
        self.assertEqual(first_slots["baseline"], MODULE.PAIR_COUNT // 2)

    def test_single_suite_validation_and_pair_assembly_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            per_suite = []
            for suite in sorted(MODULE.EXPECTED_SUITES):
                p = self.write_report(root, f"one-{suite}", {suite: 1.5})
                per_suite.append(
                    MODULE._load_report(p, expected_suites=frozenset({suite}))
                )
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "suite inventory"):
                MODULE._load_report(
                    root / "one-snake-as2.json",
                    expected_suites=frozenset({"tetris-as2"}),
                )
            assembled = root / "assembled.json"
            MODULE._write_assembled_pair_report(
                assembled, label="current-pair-1", suite_reports=per_suite
            )
            loaded = MODULE._load_report(assembled)
            self.assertEqual(set(MODULE._suite_map(loaded, assembled)), MODULE.EXPECTED_SUITES)

            mismatch_p = self.write_report(
                root, "mismatch", {"tetris-as2": 1.5}, platform="darwin"
            )
            bad_reports = per_suite[:-1] + [
                MODULE._load_report(mismatch_p, expected_suites=frozenset({"tetris-as2"}))
            ]
            with self.assertRaisesRegex(MODULE.PerformanceGateError, "host identity changed"):
                MODULE._write_assembled_pair_report(
                    root / "bad.json", label="current-pair-1", suite_reports=bad_reports
                )

    def test_dispersion_predicate_distinguishes_noisy_host_from_sustained_regression(self):
        # Clean sustained regression (e.g. 1.08x across all 6 pairs) is NOT inconclusive dispersion
        # and therefore never triggers a dispersion re-run.
        gmean, gstd, spread, _ = MODULE._suite_dispersion_stats(
            [{"rawCpuMsPerFrame": 1.08}] * MODULE.PAIR_COUNT,
            [{"rawCpuMsPerFrame": 1.00}] * MODULE.PAIR_COUNT,
        )
        self.assertAlmostEqual(gmean, 1.08)
        self.assertFalse(MODULE._is_inconclusive_dispersion(gstd, spread))

        # Single hosted-VM co-tenant / E-core spike (1.42x on one pair) IS inconclusive dispersion.
        _, noisy_gstd, noisy_spread, _ = MODULE._suite_dispersion_stats(
            [{"rawCpuMsPerFrame": v} for v in (1.00, 0.99, 1.42, 1.01, 1.00, 1.00)],
            [{"rawCpuMsPerFrame": 1.00}] * MODULE.PAIR_COUNT,
        )
        self.assertTrue(MODULE._is_inconclusive_dispersion(noisy_gstd, noisy_spread))

    def test_pair_first_labels_preserve_abba_balance_and_slice_divisibility(self):
        first_labels = MODULE._pair_first_labels()
        self.assertEqual(len(first_labels), MODULE.PAIR_COUNT)
        self.assertEqual(first_labels.count("current"), MODULE.PAIR_COUNT // 2)
        self.assertEqual(first_labels.count("baseline"), MODULE.PAIR_COUNT // 2)
        self.assertEqual(MODULE.EXPECTED_FRAMES % MODULE.INTERLEAVE_SLICE_FRAMES, 0)
        self.assertEqual(MODULE.EXPECTED_WARMUP_FRAMES % MODULE.INTERLEAVE_SLICE_FRAMES, 0)


if __name__ == "__main__":
    unittest.main()
