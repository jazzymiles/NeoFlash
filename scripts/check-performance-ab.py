#!/usr/bin/env python3
"""Compare HTML5 runtime performance with a blessed source revision on one host.

Hosted runners are intentionally heterogeneous, so absolute CPU milliseconds are
not a portable CI verdict. This script builds the blessed and current revisions
once, drives both with the current test harness in an ABBA-balanced sequence, and
checks the ratio of every measured suite. Raw reports and command logs stay under
``target/artifacts/perf-ab``.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
from typing import Any


# Remote technical-branch equivalent of local product commit 11136c5. Only the
# later performance harness changed before this baseline was blessed.
BASELINE_REVISION = "5fac0290d20ffa0970541882cacf6eee6661fb3c"
EXPECTED_NODE = "v22.23.5"
PERF_TEST = "exported_games_and_stress_suites_meet_html5_frame_time_budgets"
RATIO_LIMIT = 1.05
GEOMETRIC_STDDEV_LIMIT = 1.12
RATIO_SPREAD_LIMIT = 1.35
EXPECTED_SUITES = frozenset(
    {
        "arkanoid-as2",
        "arkanoid-as3",
        "snake-as2",
        "snake-as3",
        "stress-collision-as2",
        "stress-collision-as3",
        "stress-graphics-as2",
        "stress-graphics-as3",
        "stress-hud-physics-as2",
        "stress-hud-physics-as3",
        "stress-particles-as2",
        "stress-particles-as3",
        "tetris-as2",
        "tetris-as3",
    }
)
PAIR_SEQUENCE = (
    ("current", 0),
    ("baseline", 0),
    ("baseline", 1),
    ("current", 1),
    ("current", 2),
    ("baseline", 2),
    ("baseline", 3),
    ("current", 3),
    ("current", 4),
    ("baseline", 4),
)


class PerformanceGateError(RuntimeError):
    """The benchmark or its measurement integrity failed closed."""


def _run(
    command: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    log_path: Path,
    timeout_seconds: int = 1200,
) -> None:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w", encoding="utf-8") as log:
        log.write("command: " + " ".join(command) + "\n")
        log.flush()
        try:
            result = subprocess.run(
                command,
                cwd=cwd,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                text=True,
                check=False,
                timeout=timeout_seconds,
            )
        except subprocess.TimeoutExpired as error:
            raise PerformanceGateError(
                f"command exceeded {timeout_seconds}s; see {log_path}"
            ) from error
    if result.returncode != 0:
        raise PerformanceGateError(
            f"command failed with exit code {result.returncode}; see {log_path}"
        )


def _tool(name: str) -> str:
    found = shutil.which(name)
    if found is None:
        raise PerformanceGateError(f"required tool is unavailable: {name}")
    return found


def _load_report(path: Path) -> dict[str, Any]:
    try:
        report = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise PerformanceGateError(f"invalid performance report {path}: {error}") from error
    if report.get("schema") != 1:
        raise PerformanceGateError(f"unsupported performance report schema in {path}")
    if report.get("mode") != "report":
        raise PerformanceGateError(f"{path} was not produced in report mode")
    if report.get("node") != EXPECTED_NODE:
        raise PerformanceGateError(
            f"{path} used {report.get('node')!r}; expected exact Node {EXPECTED_NODE}"
        )
    if report.get("sample_count") != 1 or report.get("frames_per_sample") != 240:
        raise PerformanceGateError(f"{path} did not contain one 240-frame measurement")
    if report.get("warmup_frames") != 240:
        raise PerformanceGateError(f"{path} did not contain the 240-frame warmup")
    if not isinstance(report.get("platform"), str) or not report["platform"]:
        raise PerformanceGateError(f"{path} has no platform identity")
    if not isinstance(report.get("arch"), str) or not report["arch"]:
        raise PerformanceGateError(f"{path} has no architecture identity")
    results = report.get("results")
    if not isinstance(results, list) or not results:
        raise PerformanceGateError(f"{path} has no suite results")
    return report


def _suite_map(report: dict[str, Any], path: Path) -> dict[str, dict[str, Any]]:
    suites: dict[str, dict[str, Any]] = {}
    for result in report["results"]:
        if not isinstance(result, dict) or not isinstance(result.get("suite"), str):
            raise PerformanceGateError(f"{path} contains an invalid suite row")
        name = result["suite"]
        if name in suites:
            raise PerformanceGateError(f"{path} repeats suite {name}")
        value = result.get("rawCpuMsPerFrame")
        if not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0:
            raise PerformanceGateError(f"{path} has invalid CPU time for {name}: {value!r}")
        if result.get("frames") != 240:
            raise PerformanceGateError(f"{path} has an invalid frame count for {name}")
        snapshots = result.get("snapshotsPerFrame")
        draws = result.get("draws")
        if (
            not isinstance(snapshots, (int, float))
            or not math.isfinite(snapshots)
            or snapshots < 0
            or snapshots > 1.05
        ):
            raise PerformanceGateError(f"{path} has invalid snapshot coverage for {name}")
        if not isinstance(draws, int) or draws <= 0:
            raise PerformanceGateError(f"{path} has invalid draw coverage for {name}")
        if result.get("node") != report["node"]:
            raise PerformanceGateError(f"{path} has inconsistent Node identity for {name}")
        if result.get("platform") != report["platform"]:
            raise PerformanceGateError(f"{path} has inconsistent platform identity for {name}")
        if result.get("arch") != report["arch"]:
            raise PerformanceGateError(f"{path} has inconsistent architecture identity for {name}")
        suites[name] = result
    return suites


def compare_reports(
    current_paths: list[Path], baseline_paths: list[Path]
) -> dict[str, Any]:
    if len(current_paths) != 5 or len(baseline_paths) != 5:
        raise PerformanceGateError("the hosted gate requires exactly five A/B pairs")
    current_reports = [_load_report(path) for path in current_paths]
    baseline_reports = [_load_report(path) for path in baseline_paths]

    host = (
        current_reports[0].get("platform"),
        current_reports[0].get("arch"),
    )
    for report, path in zip(current_reports + baseline_reports, current_paths + baseline_paths):
        if (report.get("platform"), report.get("arch")) != host:
            raise PerformanceGateError(f"host identity changed within A/B run: {path}")

    current_suites = [
        _suite_map(report, path) for report, path in zip(current_reports, current_paths)
    ]
    baseline_suites = [
        _suite_map(report, path)
        for report, path in zip(baseline_reports, baseline_paths)
    ]
    expected_names = set(current_suites[0])
    if expected_names != EXPECTED_SUITES:
        missing = sorted(EXPECTED_SUITES - expected_names)
        unexpected = sorted(expected_names - EXPECTED_SUITES)
        raise PerformanceGateError(
            "performance suite inventory differs from the required 14 suites: "
            f"missing={missing}, unexpected={unexpected}"
        )
    for suites, path in zip(current_suites + baseline_suites, current_paths + baseline_paths):
        if set(suites) != expected_names:
            raise PerformanceGateError(f"suite inventory changed within A/B run: {path}")

    rows = []
    failures = []
    for name in sorted(expected_names):
        for index in range(5):
            current_result = current_suites[index][name]
            baseline_result = baseline_suites[index][name]
            if current_result["draws"] != baseline_result["draws"]:
                raise PerformanceGateError(
                    f"{name}: draw coverage changed in pair {index + 1} "
                    f"({current_result['draws']} != {baseline_result['draws']})"
                )
            if not math.isclose(
                current_result["snapshotsPerFrame"],
                baseline_result["snapshotsPerFrame"],
                rel_tol=0.0,
                abs_tol=1e-12,
            ):
                raise PerformanceGateError(
                    f"{name}: snapshot coverage changed in pair {index + 1}"
                )
        ratios = [
            current_suites[index][name]["rawCpuMsPerFrame"]
            / baseline_suites[index][name]["rawCpuMsPerFrame"]
            for index in range(5)
        ]
        logs = [math.log(ratio) for ratio in ratios]
        mean_log = sum(logs) / len(logs)
        geometric_mean = math.exp(mean_log)
        geometric_stddev = math.exp(
            math.sqrt(sum((value - mean_log) ** 2 for value in logs) / len(logs))
        )
        spread = max(ratios) / min(ratios)
        row = {
            "suite": name,
            "ratios": ratios,
            "geometric_mean": geometric_mean,
            "geometric_stddev": geometric_stddev,
            "spread": spread,
        }
        rows.append(row)
        if geometric_stddev > GEOMETRIC_STDDEV_LIMIT or spread > RATIO_SPREAD_LIMIT:
            failures.append(
                f"{name}: inconclusive dispersion (gstd={geometric_stddev:.3f}, "
                f"spread={spread:.3f}, ratios={','.join(f'{ratio:.3f}' for ratio in ratios)})"
            )
        elif geometric_mean > RATIO_LIMIT:
            failures.append(
                f"{name}: sustained regression {geometric_mean:.3f} > {RATIO_LIMIT:.3f} "
                f"(ratios={','.join(f'{ratio:.3f}' for ratio in ratios)})"
            )

    summary = {
        "schema": 1,
        "baseline_revision": BASELINE_REVISION,
        "node": EXPECTED_NODE,
        "platform": host[0],
        "arch": host[1],
        "pair_count": 5,
        "ratio_limit": RATIO_LIMIT,
        "geometric_stddev_limit": GEOMETRIC_STDDEV_LIMIT,
        "ratio_spread_limit": RATIO_SPREAD_LIMIT,
        "suites": rows,
        "failures": failures,
    }
    if failures:
        raise PerformanceGateError("performance A/B gate failed:\n" + "\n".join(failures))
    return summary


def _prepare_runner(
    source_root: Path,
    fixture_root: Path,
    prepared_dir: Path,
    *,
    env: dict[str, str],
    artifact_dir: Path,
    label: str,
) -> Path:
    prepared_dir.mkdir(parents=True, exist_ok=False)
    command_env = env.copy()
    command_env.update(
        {
            "NEOFLASH_PERF_MODE": "prepare",
            "NEOFLASH_PERF_PREPARE_DIR": str(prepared_dir),
            "NEOFLASH_PERF_FIXTURE_ROOT": str(fixture_root),
        }
    )
    _run(
        [
            _tool("cargo"),
            "test",
            "--locked",
            "-p",
            "neoflash-export",
            "--test",
            "games_script_export",
            PERF_TEST,
            "--",
            "--exact",
            "--nocapture",
        ],
        cwd=source_root,
        env=command_env,
        log_path=artifact_dir / f"prepare-{label}.log",
    )
    runner = prepared_dir / "run-perf-suite.mjs"
    if not runner.is_file():
        raise PerformanceGateError(f"{label} did not prepare {runner}")
    return runner


def _build_baseline_web_runtime(
    baseline_root: Path,
    baseline_target: Path,
    *,
    env: dict[str, str],
    artifact_dir: Path,
) -> None:
    _run(
        [
            _tool("cargo"),
            "build",
            "--locked",
            "--release",
            "-p",
            "neoflash-web",
            "--target",
            "wasm32-unknown-unknown",
        ],
        cwd=baseline_root,
        env=env,
        log_path=artifact_dir / "build-baseline-web.log",
    )
    wasm = baseline_target / "wasm32-unknown-unknown" / "release" / "neoflash_web.wasm"
    out_dir = baseline_root / "target" / "web-runtime"
    out_dir.mkdir(parents=True, exist_ok=True)
    _run(
        [
            _tool("wasm-bindgen"),
            "--target",
            "no-modules",
            "--no-modules-global",
            "NeoFlashWasm",
            "--out-name",
            "neoflash_web",
            "--out-dir",
            str(out_dir),
            str(wasm),
        ],
        cwd=baseline_root,
        env=env,
        log_path=artifact_dir / "bindgen-baseline-web.log",
    )


def run_gate(root: Path) -> dict[str, Any]:
    node = _tool("node")
    node_version = subprocess.check_output(
        [node, "--version"], text=True, timeout=30
    ).strip()
    if node_version != EXPECTED_NODE:
        raise PerformanceGateError(f"expected exact Node {EXPECTED_NODE}, got {node_version}")

    git = _tool("git")
    work_root = root / "target" / "perf-ab"
    baseline_root = work_root / "baseline-source"
    baseline_target = work_root / "baseline-target"
    prepared_root = work_root / "prepared"
    artifact_dir = root / "target" / "artifacts" / "perf-ab"
    if work_root.exists():
        raise PerformanceGateError(
            "target/perf-ab already exists; remove the stale explicit A/B workspace first"
        )
    artifact_dir.mkdir(parents=True, exist_ok=True)

    current_env = os.environ.copy()
    current_env.update(
        {
            "CARGO_INCREMENTAL": "0",
            "CARGO_TARGET_DIR": str(root / "target"),
        }
    )
    baseline_env = current_env.copy()
    baseline_env["CARGO_TARGET_DIR"] = str(baseline_target)
    _run(
        [git, "cat-file", "-e", f"{BASELINE_REVISION}^{{commit}}"],
        cwd=root,
        env=current_env,
        log_path=artifact_dir / "verify-baseline.log",
    )
    _run(
        [git, "worktree", "add", "--detach", str(baseline_root), BASELINE_REVISION],
        cwd=root,
        env=current_env,
        log_path=artifact_dir / "create-baseline-worktree.log",
    )

    try:
        harness = root / "crates" / "neoflash-export" / "tests" / "games_script_export.rs"
        baseline_harness = (
            baseline_root / "crates" / "neoflash-export" / "tests" / "games_script_export.rs"
        )
        shutil.copy2(harness, baseline_harness)
        _build_baseline_web_runtime(
            baseline_root,
            baseline_target,
            env=baseline_env,
            artifact_dir=artifact_dir,
        )
        current_runner = _prepare_runner(
            root,
            root,
            prepared_root / "current",
            env=current_env,
            artifact_dir=artifact_dir,
            label="current",
        )
        baseline_runner = _prepare_runner(
            baseline_root,
            root,
            prepared_root / "baseline",
            env=baseline_env,
            artifact_dir=artifact_dir,
            label="baseline",
        )

        report_paths: dict[str, list[Path]] = {"current": [], "baseline": []}
        for sequence_index, (label, pair_index) in enumerate(PAIR_SEQUENCE):
            report_path = artifact_dir / f"{sequence_index:02d}-{label}-pair-{pair_index + 1}.json"
            runner = current_runner if label == "current" else baseline_runner
            run_env = current_env.copy()
            run_env.update(
                {
                    "NEOFLASH_PERF_MODE": "report",
                    "NEOFLASH_PERF_REPORT": str(report_path),
                    "NEOFLASH_PERF_LABEL": f"{label}-pair-{pair_index + 1}",
                    "NEOFLASH_PERF_SAMPLE_COUNT": "1",
                    "NEOFLASH_PERF_FRAMES": "240",
                    "NEOFLASH_PERF_WARMUP_FRAMES": "240",
                }
            )
            _run(
                [node, "--expose-gc", str(runner)],
                cwd=root if label == "current" else baseline_root,
                env=run_env,
                log_path=artifact_dir / f"{sequence_index:02d}-{label}-pair-{pair_index + 1}.log",
            )
            report_paths[label].append(report_path)

        summary = compare_reports(report_paths["current"], report_paths["baseline"])
        current_revision = subprocess.check_output(
            [git, "rev-parse", "HEAD"],
            cwd=root,
            env=current_env,
            text=True,
            timeout=30,
        ).strip()
        summary["current_revision"] = current_revision
        summary["harness_sha256"] = hashlib.sha256(harness.read_bytes()).hexdigest()
        (artifact_dir / "summary.json").write_text(
            json.dumps(summary, indent=2) + "\n", encoding="utf-8"
        )
        for row in summary["suites"]:
            print(
                f"{row['suite']}: {row['geometric_mean']:.3f}x "
                f"(gstd {row['geometric_stddev']:.3f}, spread {row['spread']:.3f})"
            )
        return summary
    finally:
        removed = subprocess.run(
            [git, "worktree", "remove", "--force", str(baseline_root)],
            cwd=root,
            env=current_env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if prepared_root.exists():
            shutil.rmtree(prepared_root)
        if baseline_target.exists():
            shutil.rmtree(baseline_target)
        if removed.returncode == 0:
            try:
                work_root.rmdir()
            except OSError:
                # Preserve any unexpected entry for inspection instead of deleting it.
                pass


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--project-root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
    )
    parser.add_argument(
        "--compare-only",
        nargs=10,
        type=Path,
        metavar="REPORT",
        help="compare five current reports followed by five baseline reports",
    )
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    try:
        if args.compare_only:
            summary = compare_reports(args.compare_only[:5], args.compare_only[5:])
            print(json.dumps(summary, indent=2))
        else:
            run_gate(args.project_root.resolve())
    except PerformanceGateError as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
