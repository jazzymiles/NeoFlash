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
            source = {
                "id": "case",
                "staged": staged.name,
                "source_sha256": corpus.digest(staged),
                "capture_token": "case:new-input:recipe",
            }
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
            source = {
                "id": "case",
                "source_sha256": "source",
                "capture_token": "case:source:" + corpus.digest(recipe),
            }
            (root / "inputs.json").write_text(json.dumps([source]))

            def replace_helper(*_args, **_kwargs):
                tool.write_text("second build")
                return {"id": "case"}

            with patch.object(corpus, "compare_case", side_effect=replace_helper):
                with self.assertRaisesRegex(ValueError, "changed during measurement"):
                    corpus.compare(root, tool)
            self.assertFalse((root / "comparison.json").exists())
            recipe.write_text("changed capture command")
            with self.assertRaisesRegex(ValueError, "capture command changed"):
                corpus.compare(root, tool)

    def test_separates_editable_stage_from_scripted_playback_divergence(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            tool = root / "helper"
            tool.write_text("stable helper binary")
            recipe = root / "corpus.jsfl"
            recipe.write_text("t.currentFrame = f; doc.exportPNG(out, true, true);")

            case = root / "nested-mc-case"
            (case / "source").mkdir(parents=True)
            staged = case / "source/input.fla"
            staged.write_bytes(b"synthetic fla payload")
            src_sha = corpus.digest(staged)
            token = f"nested-mc-case:{src_sha}:{corpus.digest(recipe)}"

            (case / "complete.txt").write_text(token)
            (case / "host.txt").write_text("Adobe Animate 24.0")
            (case / "stage.tsv").write_text("320\t240\t24\t#112233\n")
            (case / "scenes.tsv").write_text("48\t2\t3\t0\t1\n")
            for f in (0, 23, 47):
                (case / f"adobe-s0-f{f}.png").write_bytes(f"adobe-{f}".encode())

            source = {
                "id": "nested-mc-case",
                "staged": "source/input.fla",
                "source_sha256": src_sha,
                "capture_token": token,
            }
            (root / "inputs.json").write_text(json.dumps([source]))

            authored_payload = {
                "evaluation_mode": "editable_stage",
                "stage": [320.0, 240.0, 24.0],
                "background": "#112233",
                "scenes": [[48, 2, 3, 0, 1]],
                "symbols": 3,
                "render_errors": [],
                "scripted_playback": {
                    "engine": "AuthoredPlayback",
                    "has_scripts": True,
                    "has_independent_movie_clips": True,
                    "divergent_frames": 2,
                    "traces": ["tick:1", "stopped"],
                    "diagnostics": [],
                    "scenes": [
                        {
                            "scene": 0,
                            "frames": [
                                {
                                    "step": 0,
                                    "playhead_frame": 0,
                                    "root_halted_by_script": False,
                                    "advancing_clip_count": 0,
                                    "diverges_from_editable_stage": False,
                                    "different_rgba_pixels_vs_editable": 0,
                                    "different_visible_pixels_vs_editable": 0,
                                    "divergence_causes": [],
                                },
                                {
                                    "step": 23,
                                    "playhead_frame": 5,
                                    "root_halted_by_script": True,
                                    "advancing_clip_count": 1,
                                    "diverges_from_editable_stage": True,
                                    "different_rgba_pixels_vs_editable": 1400,
                                    "different_visible_pixels_vs_editable": 1400,
                                    "divergence_causes": [
                                        "playhead_redirected_by_script",
                                        "independent_movie_clip_timeline",
                                    ],
                                },
                                {
                                    "step": 47,
                                    "playhead_frame": 5,
                                    "root_halted_by_script": True,
                                    "advancing_clip_count": 1,
                                    "diverges_from_editable_stage": True,
                                    "different_rgba_pixels_vs_editable": 1400,
                                    "different_visible_pixels_vs_editable": 1400,
                                    "divergence_causes": [
                                        "playhead_redirected_by_script",
                                        "independent_movie_clip_timeline",
                                    ],
                                },
                            ],
                        }
                    ],
                },
            }

            def fake_helper(_cli, mode, *rest):
                if mode == "authored":
                    out_dir = Path(rest[1])
                    for f in (0, 23, 47):
                        (out_dir / f"native-s0-f{f}.png").write_bytes(f"native-{f}".encode())
                    return authored_payload
                if mode == "pixels":
                    return {
                        "pixels": 76800,
                        "different_rgba_pixels": 0,
                        "different_visible_pixels": 0,
                        "rgba_mae": 0.0,
                    }
                raise AssertionError(f"unexpected helper mode {mode}")

            with patch.object(corpus, "helper", side_effect=fake_helper):
                rc = corpus.compare(root, tool)
            self.assertEqual(rc, 0)

            report = json.loads((root / "comparison.json").read_text())
            self.assertEqual(report["frame_mode"], "sampled")
            self.assertEqual(report["summary"]["editable_stage"], {"exact": 1})
            self.assertEqual(
                report["summary"]["scripted_playback"],
                {"diverges_from_editable_stage": 1},
            )
            self.assertEqual(
                report["summary"]["behavioral_parity"],
                {"authored_scripted_only": 1},
            )
            self.assertEqual(report["counts"]["editable_stage_exact_documents"], 1)
            self.assertEqual(report["counts"]["scripted_playback_divergent_documents"], 1)
            self.assertEqual(report["counts"]["scripted_playback_divergent_frames"], 2)

            case_row = report["cases"][0]
            self.assertEqual(case_row["editable_stage"]["status"], "exact")
            self.assertFalse(case_row["editable_stage"]["frames"][0]["scripted_playback_diverges"])
            self.assertTrue(case_row["editable_stage"]["frames"][1]["scripted_playback_diverges"])
            self.assertEqual(
                case_row["editable_stage"]["frames"][1]["scripted_playhead_frame"],
                5,
            )
            self.assertEqual(
                case_row["scripted_playback"]["status"],
                "diverges_from_editable_stage",
            )
            self.assertTrue(case_row["scripted_playback"]["explains_editable_vs_runtime_gap"])

    def test_companion_swf_trace_oracle_populates_scripted_playback_status(self):
        with tempfile.TemporaryDirectory() as temp:
            case = Path(temp)
            (case / "source").mkdir()
            staged = case / "source/test.fla"
            staged.write_bytes(b"pinned fla")
            movie = case / "test.swf"
            movie.write_bytes(b"FWS\x08")
            expected = case / "output.txt"
            expected.write_text("frame0\nframe1\n")
            config = case / "test.toml"
            config.write_text("num_frames = 2\n")

            source = {
                "id": "pinned-trace-case",
                "staged": "source/test.fla",
                "source_sha256": corpus.digest(staged),
                "capture_token": "pinned-trace-case:sha:recipe",
                "trace_oracle": {
                    "movie": "test.swf",
                    "movie_sha256": corpus.digest(movie),
                    "expected_sha256": corpus.digest(expected),
                    "config_sha256": corpus.digest(config),
                },
            }
            (case / "complete.txt").write_text("Adobe capture missing")

            def fake_helper(_cli, mode, *rest):
                if mode == "playback":
                    self.assertEqual(rest[1], 2)
                    return {"traces": ["frame0", "frame1"], "diagnostics": []}
                if mode == "authored":
                    return {
                        "evaluation_mode": "editable_stage",
                        "stage": [550.0, 400.0, 24.0],
                        "background": "#ffffff",
                        "scenes": [[2, 1, 2, 0, 2]],
                        "symbols": 1,
                        "render_errors": [],
                        "scripted_playback": {
                            "engine": "AuthoredPlayback",
                            "has_scripts": True,
                            "has_independent_movie_clips": False,
                            "divergent_frames": 1,
                            "traces": ["frame0", "frame1"],
                            "diagnostics": [],
                            "scenes": [],
                        },
                    }
                raise AssertionError(f"unexpected mode {mode}")

            with patch.object(corpus, "helper", side_effect=fake_helper):
                row = corpus.compare_case(case, source, Path("unused"), "helper-sha")

            self.assertEqual(row["behavior"]["status"], "match")
            self.assertEqual(row["editable_stage"]["status"], "native only")
            self.assertEqual(row["scripted_playback"]["status"], "trace_match")
            self.assertTrue(row["scripted_playback"]["explains_editable_vs_runtime_gap"])
            self.assertEqual(row["behavioral_parity"]["status"], "parity_match")

    def test_keyframes_and_midpoints_sweep_planner_and_bounding(self):
        # Default sampled mode keeps exact [0, last // 2, last] deduplicated compatibility.
        self.assertEqual(corpus.plan_scene_frames(1, frame_mode="sampled"), [0])
        self.assertEqual(corpus.plan_scene_frames(2, frame_mode="sampled"), [0, 1])
        self.assertEqual(corpus.plan_scene_frames(48, frame_mode="sampled"), [0, 23, 47])

        # Keyframes-and-midpoints collects unique keyframe starts, tween midpoints, scripts, and last.
        swept = corpus.plan_scene_frames(
            41,
            keyframes=(0, 10, 30),
            tween_spans=((0, 10), (10, 30)),
            frame_scripts=(35,),
            frame_mode="keyframes-and-midpoints",
            max_frames_per_scene=64,
        )
        self.assertEqual(swept, [0, 5, 10, 20, 30, 35, 40])

        # Dense timelines are deterministically subsampled to max_frames_per_scene while keeping 0 and last.
        dense = corpus.plan_scene_frames(
            100,
            keyframes=range(100),
            frame_mode="keyframes-and-midpoints",
            max_frames_per_scene=6,
        )
        self.assertEqual(len(dense), 6)
        self.assertEqual(dense[0], 0)
        self.assertEqual(dense[-1], 99)
        self.assertEqual(dense, sorted(set(dense)))

    def test_compare_keyframes_and_midpoints_and_companion_behavioral_parity(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            tool = root / "helper"
            tool.write_text("stable helper v3")
            recipe = root / "corpus.jsfl"
            recipe.write_text("t.currentFrame = f; doc.exportPNG(out, true, true);")

            case = root / "motion-arc-case"
            (case / "source").mkdir(parents=True)
            staged = case / "source/input.fla"
            staged.write_bytes(b"synthetic motion arc fla")
            companion = case / "companion.swf"
            companion.write_bytes(b"FWS\x0a\x10\x00\x00\x00")
            src_sha = corpus.digest(staged)
            swf_sha = corpus.digest(companion)
            token = f"motion-arc-case:{src_sha}:{corpus.digest(recipe)}"

            (case / "complete.txt").write_text(token)
            (case / "host.txt").write_text("Adobe Animate 24.0")
            (case / "stage.tsv").write_text("400\t300\t24\t#ffffff\n")
            (case / "scenes.tsv").write_text("21\t1\t3\t2\t1\n")
            for f in (0, 10, 20):
                (case / f"adobe-s0-f{f}.png").write_bytes(f"adobe-{f}".encode())

            source = {
                "id": "motion-arc-case",
                "staged": "source/input.fla",
                "source_sha256": src_sha,
                "capture_token": token,
                "companion_swf": {
                    "movie": "companion.swf",
                    "movie_sha256": swf_sha,
                },
            }
            (root / "inputs.json").write_text(json.dumps([source]))

            planned = [0, 5, 10, 15, 20]
            recorded_authored_args = []

            def fake_helper(_cli, mode, *rest):
                if mode == "authored":
                    recorded_authored_args.append(list(map(str, rest)))
                    out_dir = Path(rest[1])
                    for f in planned:
                        (out_dir / f"native-s0-f{f}.png").write_bytes(f"native-{f}".encode())
                    return {
                        "evaluation_mode": "editable_stage",
                        "frame_mode": "keyframes-and-midpoints",
                        "max_frames_per_scene": 8,
                        "planned_scene_frames": [planned],
                        "stage": [400.0, 300.0, 24.0],
                        "background": "#ffffff",
                        "scenes": [[21, 1, 3, 2, 1]],
                        "symbols": 2,
                        "render_errors": [],
                        "scripted_playback": {
                            "engine": "AuthoredPlayback",
                            "has_scripts": True,
                            "has_independent_movie_clips": False,
                            "divergent_frames": 0,
                            "traces": ["intro", "outro"],
                            "diagnostics": [],
                            "scenes": [
                                {
                                    "scene": 0,
                                    "planned_frames": planned,
                                    "frames": [
                                        {
                                            "step": f,
                                            "playhead_frame": f,
                                            "label": "start" if f == 0 else ("end" if f == 20 else None),
                                            "display_node_count": 3,
                                            "root_halted_by_script": False,
                                            "advancing_clip_count": 0,
                                            "diverges_from_editable_stage": False,
                                            "different_rgba_pixels_vs_editable": 0,
                                            "different_visible_pixels_vs_editable": 0,
                                            "divergence_causes": [],
                                        }
                                        for f in planned
                                    ],
                                }
                            ],
                        },
                        "behavioral_parity": {
                            "status": "parity_match",
                            "has_companion_swf": True,
                            "has_scripts": True,
                            "frame_count_equal": True,
                            "labels_equal": True,
                            "display_presence_equal": True,
                            "display_node_counts_equal": True,
                            "trace_equal": True,
                            "authored": {
                                "scene_count": 1,
                                "total_frames": 21,
                                "labels": [
                                    {"scene": 0, "scene_frame": 0, "global_frame": 0, "label": "start"},
                                    {"scene": 0, "scene_frame": 20, "global_frame": 20, "label": "end"},
                                ],
                                "traces": ["intro", "outro"],
                                "diagnostics": [],
                            },
                            "swf": {
                                "swf_version": 10,
                                "vm_kind": "avm2",
                                "total_frames": 21,
                                "labels": [
                                    {"frame": 0, "label": "start"},
                                    {"frame": 20, "label": "end"},
                                ],
                                "traces": ["intro", "outro"],
                                "diagnostics": [],
                            },
                        },
                    }
                if mode == "pixels":
                    return {
                        "pixels": 120000,
                        "different_rgba_pixels": 0,
                        "different_visible_pixels": 0,
                        "rgba_mae": 0.0,
                    }
                raise AssertionError(f"unexpected helper mode {mode}")

            with patch.object(corpus, "helper", side_effect=fake_helper):
                rc = corpus.compare(
                    root,
                    tool,
                    frame_mode="keyframes-and-midpoints",
                    max_frames_per_scene=8,
                )
            self.assertEqual(rc, 0)
            self.assertEqual(len(recorded_authored_args), 1)
            self.assertIn("--frame-mode", recorded_authored_args[0])
            self.assertIn("keyframes-and-midpoints", recorded_authored_args[0])
            self.assertIn("--max-frames-per-scene", recorded_authored_args[0])
            self.assertIn("8", recorded_authored_args[0])
            self.assertIn("--companion-swf", recorded_authored_args[0])

            report = json.loads((root / "comparison.json").read_text())
            self.assertEqual(report["frame_mode"], "keyframes-and-midpoints")
            self.assertEqual(report["max_frames_per_scene"], 8)
            self.assertEqual(report["summary"]["editable_stage"], {"exact": 1})
            self.assertEqual(report["summary"]["behavioral_parity"], {"parity_match": 1})
            self.assertEqual(report["counts"]["sampled_frames"], 5)
            self.assertEqual(report["counts"]["compared_frames"], 3)
            self.assertEqual(report["counts"]["exact_frames"], 3)
            self.assertEqual(report["counts"]["behavioral_parity_compared"], 1)
            self.assertEqual(report["counts"]["behavioral_parity_matches"], 1)

            case_row = report["cases"][0]
            self.assertEqual(
                [f["frame"] for f in case_row["editable_stage"]["frames"]],
                planned,
            )
            self.assertEqual(
                case_row["editable_stage"]["frames"][1]["status"],
                "native_sweep_only",
            )
            self.assertEqual(
                case_row["editable_stage"]["frames"][1]["display_node_count"],
                3,
            )
            self.assertEqual(case_row["behavioral_parity"]["status"], "parity_match")
            self.assertTrue(case_row["behavioral_parity"]["trace_equal"])
            self.assertTrue(case_row["behavioral_parity"]["labels_equal"])


if __name__ == "__main__":
    unittest.main()
