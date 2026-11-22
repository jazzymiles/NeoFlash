#!/usr/bin/env python3
"""Compare editable FLA/XFL imports against Adobe Animate authoring frames.

prepare DIRECTORY creates fresh source copies. Run DIRECTORY/corpus.jsfl in
licensed Animate, then compare DIRECTORY. Without --folder the input is the
pinned corpus, including its independently recorded companion SWF trace oracles.
With --folder, stage individual FLA files only and use anonymous case IDs.
inventory indexes an additional source/reference collection without executing it.
compare writes every outcome to comparison.json, separating static editable_stage
composition (matching Adobe's t.currentFrame = f; exportPNG(..., true, true))
from scripted_playback (runtime MovieClip clocks, frame/instance scripts, and
companion SWF trace oracles) and behavioral_parity (AuthoredPlayback vs
SwfBytecodePlayer frame counts, labels, display-node counts, and trace() output).
Its exit status reports measurement errors; visual or playback differences are
reported per case.
"""
import argparse
import collections
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "examples/fla-corpus"
JSFL = ROOT / "scripts/fixtures/animate-oracle/corpus.jsfl"
DEFAULT_HELPER = ROOT / "target/release/examples/compare_fla_corpus"
CACHE_SCHEMA = "v2-editable-vs-scripted"
DEFAULT_MAX_FRAMES_PER_SCENE = 64
FRAME_MODES = ("sampled", "keyframes-and-midpoints")

EDITABLE_STAGE_BASIS = (
    "static authoring stage (t.currentFrame = f; exportPNG(..., true, true); "
    "MovieClip instances at frame 0, Graphic instances synchronized)"
)
SCRIPTED_PLAYBACK_BASIS = (
    "runtime playback (AuthoredPlayback advancing MovieClip timelines and "
    "frame/instance scripts, plus SwfBytecodePlayer companion trace oracle when present)"
)
BEHAVIORAL_PARITY_BASIS = (
    "behavioral parity (AuthoredPlayback vs SwfBytecodePlayer frame counts, "
    "labels, display-node counts, and trace() output across scenes)"
)


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def source_digest(path):
    if path.is_file():
        return digest(path)
    # Include shared symbol XML and embedded assets, not just the XFL marker.
    parts = [(p.relative_to(path).as_posix(), digest(p)) for p in sorted(path.rglob("*")) if p.is_file()]
    return hashlib.sha256(json.dumps(parts, ensure_ascii=False).encode()).hexdigest()


def plan_scene_frames(
    frame_count,
    keyframes=(),
    tween_spans=(),
    frame_scripts=(),
    frame_mode="sampled",
    max_frames_per_scene=DEFAULT_MAX_FRAMES_PER_SCENE,
):
    """Deterministically plan zero-based frame indices for a scene timeline."""
    if frame_mode not in FRAME_MODES:
        raise ValueError(f"Unsupported frame_mode {frame_mode!r}; expected one of {FRAME_MODES}")
    total = max(int(frame_count), 1)
    last = total - 1
    if frame_mode == "sampled":
        out = []
        for f in (0, last // 2, last):
            if f not in out:
                out.append(f)
        return out

    limit = max(int(max_frames_per_scene), 1)
    if limit == 1 or last == 0:
        return [0]

    candidates_set = {0, last // 2, last}
    for kf in keyframes:
        candidates_set.add(min(max(int(kf), 0), last))
    for sf in frame_scripts:
        candidates_set.add(min(max(int(sf), 0), last))
    for start, end in tween_spans:
        s = min(max(int(start), 0), last)
        e = min(max(int(end), 0), last)
        candidates_set.add(s)
        candidates_set.add(e)
        if e > s + 1:
            candidates_set.add((s + e) // 2)

    candidates = sorted(candidates_set)
    if len(candidates) <= limit:
        return candidates

    bounded = []
    max_idx = len(candidates) - 1
    for slot in range(limit):
        pick = candidates[(slot * max_idx) // (limit - 1)]
        if not bounded or bounded[-1] != pick:
            bounded.append(pick)
    if bounded[0] != 0:
        bounded.insert(0, 0)
    if bounded[-1] != last:
        if len(bounded) >= limit:
            bounded[-1] = last
        else:
            bounded.append(last)
    return bounded


def case_inputs(folder):
    if folder is None:
        for case in json.loads((CORPUS / "manifest.json").read_text())["cases"]:
            yield case["id"], CORPUS / case["path"], True
    else:
        for source in sorted(folder.rglob("*")):
            if source.is_file() and source.suffix.lower() == ".fla":
                rel = source.relative_to(folder).as_posix()
                yield "fla-" + hashlib.sha256(rel.encode()).hexdigest()[:20], source, False


def prepare(destination, folder):
    if destination.is_relative_to(folder or CORPUS):
        raise ValueError("Use an evidence directory outside the source collection")
    if folder is None:
        subprocess.run([sys.executable, str(ROOT / "scripts/check-fla-corpus.py"), "--integrity-only"], check=True)
    destination.mkdir(parents=True, exist_ok=False)
    rows = []
    inputs = []
    recipe_hash = digest(JSFL)
    for case_id, source, pinned in case_inputs(folder):
        case = destination / case_id
        case.mkdir()
        row = {"id": case_id, "source_sha256": source_digest(source)}
        if source.is_dir():
            staged = case / "source"
            shutil.copytree(source, staged)
            marker, = staged.glob("*.xfl")
        else:
            staged = case / "source" / (source.name if pinned else "input.fla")
            staged.parent.mkdir()
            shutil.copyfile(source, staged)
            movie = source.with_suffix(".swf")
            # Pinned external AS3 classes are source companions of these FLAs.
            if pinned:
                for script in source.parent.glob("*.as"):
                    shutil.copyfile(script, staged.parent / script.name)
                oracle = source.parent / "output.txt"
                config = source.parent / "test.toml"
                if oracle.is_file() and movie.is_file() and config.is_file():
                    for companion in (oracle, movie, config):
                        shutil.copyfile(companion, case / companion.name)
                    row["trace_oracle"] = {
                        "movie": movie.name, "movie_sha256": digest(movie),
                        "expected_sha256": digest(oracle), "config_sha256": digest(config),
                    }
                elif movie.is_file():
                    shutil.copyfile(movie, case / movie.name)
                    row["companion_swf"] = {
                        "movie": movie.name,
                        "movie_sha256": digest(movie),
                    }
            elif movie.is_file():
                staged_movie = case / "companion.swf"
                shutil.copyfile(movie, staged_movie)
                row["companion_swf"] = {
                    "movie": staged_movie.name,
                    "movie_sha256": digest(staged_movie),
                }
            marker = staged
        row["staged"] = staged.relative_to(case).as_posix()
        row["capture_token"] = ":".join((case_id, row["source_sha256"], recipe_hash))
        if marker.stat().st_size:
            inputs.append("\t".join((case_id, marker.relative_to(case).as_posix(), row["capture_token"])))
        else:
            (case / "complete.txt").write_text("empty input")
        rows.append(row)
    if not rows:
        raise ValueError("No FLA inputs found")
    (destination / "inputs.tsv").write_text("\n".join(inputs) + "\n")
    save(destination / "inputs.json", rows)
    shutil.copyfile(JSFL, destination / "corpus.jsfl")
    print(f"Prepared {len(rows)} cases. Run {destination / 'corpus.jsfl'} in Animate.")


def helper(cli, *args):
    run = subprocess.run([str(cli), *map(str, args)], capture_output=True, text=True, timeout=1200)
    if run.returncode:
        raise ValueError(run.stderr.strip()[-800:] or f"exit {run.returncode}")
    return json.loads(run.stdout)


def resolve_companion_swf(case, source):
    if "companion_swf" in source:
        meta = source["companion_swf"]
        movie = case / meta["movie"]
        if not movie.is_file() or digest(movie) != meta["movie_sha256"]:
            raise ValueError("Companion SWF missing or digest changed")
        return movie
    if "trace_oracle" in source:
        meta = source["trace_oracle"]
        movie = case / meta["movie"]
        if not movie.is_file() or digest(movie) != meta["movie_sha256"]:
            raise ValueError("Companion movie, trace or playback settings changed")
        return movie
    return None


def behavior(case, source, cli):
    if "trace_oracle" not in source:
        return {"status": "no independent trace"}
    oracle = source["trace_oracle"]
    movie = case / oracle["movie"]
    expected = case / "output.txt"
    config = case / "test.toml"
    if (digest(movie), digest(expected), digest(config)) != (
            oracle["movie_sha256"], oracle["expected_sha256"], oracle["config_sha256"]):
        raise ValueError("Companion movie, trace or playback settings changed")
    settings = tomllib.loads(config.read_text())
    ticks = settings.get("num_frames", settings.get("num_ticks"))
    result = helper(cli, "playback", movie, ticks)
    lines = expected.read_text().splitlines()
    match = result["traces"] == lines
    return {"status": "match" if match and not result["diagnostics"] else "difference",
            "subject": "original companion SWF", "reference": "pinned Flash Player trace",
            "ticks": ticks, "expected_lines": len(lines), "actual_lines": len(result["traces"]),
            "trace_equal": match, "diagnostics": result["diagnostics"]}


def build_scripted_playback(native, behavior_row):
    rt = native.get("scripted_playback") or {
        "engine": "AuthoredPlayback",
        "has_scripts": False,
        "has_independent_movie_clips": False,
        "divergent_frames": 0,
        "traces": [],
        "diagnostics": [],
        "scenes": [],
    }
    divergent = int(rt.get("divergent_frames", 0))
    b_status = behavior_row.get("status")
    if b_status == "match":
        status = "trace_match"
    elif b_status == "difference":
        status = "trace_difference"
    elif divergent > 0:
        status = "diverges_from_editable_stage"
    else:
        status = "matches_editable_stage"
    return {
        "status": status,
        "basis": SCRIPTED_PLAYBACK_BASIS,
        "explains_editable_vs_runtime_gap": divergent > 0,
        "divergent_frames": divergent,
        "runtime_timeline": rt,
        "trace_oracle": behavior_row,
    }


def build_behavioral_parity(native, behavior_row):
    raw = native.get("behavioral_parity")
    if isinstance(raw, dict):
        out = {"basis": BEHAVIORAL_PARITY_BASIS, **raw}
        if behavior_row.get("status") in ("match", "difference"):
            out["pinned_trace_oracle"] = behavior_row
        return out

    rt = native.get("scripted_playback") or {}
    scenes = native.get("scenes") or []
    total_frames = sum(int(s[0]) for s in scenes if s)
    has_scripts = bool(rt.get("has_scripts", False))
    traces = list(rt.get("traces", []))
    b_status = behavior_row.get("status")
    if b_status == "match":
        status = "parity_match"
    elif b_status == "difference":
        status = "parity_difference"
    elif has_scripts:
        status = "authored_scripted_only"
    else:
        status = "authored_static_only"
    return {
        "status": status,
        "basis": BEHAVIORAL_PARITY_BASIS,
        "has_companion_swf": b_status in ("match", "difference"),
        "has_scripts": has_scripts,
        "trace_equal": behavior_row.get("trace_equal") if b_status in ("match", "difference") else None,
        "authored": {
            "scene_count": len(scenes),
            "total_frames": total_frames,
            "traces": traces,
            "diagnostics": list(rt.get("diagnostics", [])),
        },
        "pinned_trace_oracle": behavior_row if b_status in ("match", "difference") else None,
    }


def index_scripted_frames(native):
    lookup = {}
    rt = native.get("scripted_playback") or {}
    for scene_entry in rt.get("scenes", []):
        scene_idx = scene_entry.get("scene")
        for f_entry in scene_entry.get("frames", []):
            lookup[(scene_idx, f_entry.get("step"))] = f_entry
    return lookup


def expected_cached_frames(result_json, frame_mode, max_frames_per_scene):
    planned = result_json.get("planned_scene_frames")
    if isinstance(planned, list) and len(planned) == len(result_json.get("scenes", [])):
        for idx, frames in enumerate(planned):
            for f in frames:
                yield idx, int(f)
        return
    for idx, scene in enumerate(result_json.get("scenes", [])):
        for f in plan_scene_frames(scene[0], frame_mode=frame_mode, max_frames_per_scene=max_frames_per_scene):
            yield idx, f


def compare_case(
    case,
    source,
    cli,
    helper_hash,
    frame_mode="sampled",
    max_frames_per_scene=DEFAULT_MAX_FRAMES_PER_SCENE,
):
    if frame_mode not in FRAME_MODES:
        raise ValueError(f"Unsupported frame_mode {frame_mode!r}")
    staged = case / source["staged"]
    if source_digest(staged) != source["source_sha256"]:
        raise ValueError("Staged source has changed; recapture the original")
    row = {"id": source["id"], "source_sha256": source["source_sha256"]}
    row["behavior"] = behavior(case, source, cli)
    companion_movie = resolve_companion_swf(case, source)
    if staged.is_file() and not staged.stat().st_size:
        row["editable"] = {"status": "empty input"}
        row["visual"] = {"status": "unavailable", "reason": "empty input"}
        row["editable_stage"] = {"status": "empty input", "basis": EDITABLE_STAGE_BASIS}
        row["scripted_playback"] = {
            "status": "empty input",
            "basis": SCRIPTED_PLAYBACK_BASIS,
            "explains_editable_vs_runtime_gap": False,
            "divergent_frames": 0,
            "trace_oracle": row["behavior"],
        }
        row["behavioral_parity"] = {
            "status": "empty input",
            "basis": BEHAVIORAL_PARITY_BASIS,
            "has_companion_swf": companion_movie is not None,
        }
        return row
    try:
        cached = case / (
            "native-result.json"
            if frame_mode == "sampled"
            else f"native-result-{frame_mode}-{max_frames_per_scene}.json"
        )
        cache = json.loads(cached.read_text()) if cached.is_file() else {}
        signature = [CACHE_SCHEMA, helper_hash, source["source_sha256"]]
        if frame_mode != "sampled":
            signature.extend([frame_mode, int(max_frames_per_scene)])
        if companion_movie is not None and "companion_swf" in source:
            signature.append(source["companion_swf"]["movie_sha256"])

        if cache.get("signature") == signature and all(
            (case / f"native-s{idx}-f{frame}.png").is_file()
            for idx, frame in expected_cached_frames(
                cache.get("result", {}), frame_mode, max_frames_per_scene
            )
        ):
            native = cache["result"]
        else:
            authored_args = ["authored", staged, case]
            if frame_mode != "sampled":
                authored_args.extend([
                    "--frame-mode",
                    frame_mode,
                    "--max-frames-per-scene",
                    int(max_frames_per_scene),
                ])
            if companion_movie is not None and "companion_swf" in source:
                authored_args.extend(["--companion-swf", companion_movie])
            native = helper(cli, *authored_args)
            save(cached, {"signature": signature, "result": native})
    except (ValueError, subprocess.TimeoutExpired) as error:
        row["editable"] = {"status": "import or render failed", "reason": str(error)}
        row["visual"] = {"status": "unavailable"}
        row["editable_stage"] = {
            "status": "import or render failed",
            "basis": EDITABLE_STAGE_BASIS,
            "reason": str(error),
        }
        row["scripted_playback"] = {
            "status": "unavailable",
            "basis": SCRIPTED_PLAYBACK_BASIS,
            "reason": str(error),
            "explains_editable_vs_runtime_gap": False,
            "divergent_frames": 0,
            "trace_oracle": row["behavior"],
        }
        row["behavioral_parity"] = {
            "status": "unavailable",
            "basis": BEHAVIORAL_PARITY_BASIS,
            "reason": str(error),
            "has_companion_swf": companion_movie is not None,
        }
        return row
    mark = case / "complete.txt"
    adobe = mark.read_text().strip() if mark.is_file() else "Adobe capture missing"
    if adobe != source["capture_token"]:
        if adobe not in ("Adobe capture missing", "empty input", "in progress") and not adobe.startswith("error:"):
            raise ValueError("Adobe capture belongs to a different source or capture command")
        row["editable"] = {"status": "native only", "native": native, "reason": adobe}
        row["visual"] = {"status": "unavailable", "reason": adobe}
        row["editable_stage"] = {
            "status": "native only",
            "basis": EDITABLE_STAGE_BASIS,
            "native": native,
            "reason": adobe,
        }
        row["scripted_playback"] = build_scripted_playback(native, row["behavior"])
        row["behavioral_parity"] = build_behavioral_parity(native, row["behavior"])
        return row
    stage_fields = (case / "stage.tsv").read_text().strip().split("\t")
    stage = list(map(float, stage_fields[:3]))
    background = stage_fields[3].lower()
    adobe_scenes = [list(map(int, line.split("\t"))) for line in (case / "scenes.tsv").read_text().splitlines()]
    same_lengths = [scene[0] for scene in native["scenes"]] == [scene[0] for scene in adobe_scenes]
    stage_equal = native["stage"] == stage and same_lengths
    row["adobe_host"] = (case / "host.txt").read_text().strip()
    row["editable"] = {"status": "stage match" if stage_equal else "stage difference",
                       "adobe_stage": stage, "adobe_background": background,
                       "background_equal": background == native["background"], "native": native,
                       "adobe_scenes": adobe_scenes, "layout_equal": native["scenes"] == adobe_scenes}
    frames = []
    editable_stage_frames = []
    pb_lookup = index_scripted_frames(native)
    planned_by_scene = native.get("planned_scene_frames") or []
    failed = {(error["scene"], error["frame"]) for error in native.get("render_errors", [])}
    for scene, layout in enumerate(adobe_scenes):
        if frame_mode != "sampled" and scene < len(planned_by_scene):
            sweep_frames = sorted({int(f) for f in planned_by_scene[scene]})
        else:
            sweep_frames = plan_scene_frames(
                layout[0], frame_mode=frame_mode, max_frames_per_scene=max_frames_per_scene
            )
        for frame in sweep_frames:
            image = case / f"native-s{scene}-f{frame}.png"
            reference = case / f"adobe-s{scene}-f{frame}.png"
            if (scene, frame) not in failed and image.is_file() and reference.is_file():
                pixels = helper(cli, "pixels", image, reference)
            elif frame_mode != "sampled" and (scene, frame) not in failed and image.is_file() and not reference.is_file():
                pixels = {"status": "native_sweep_only"}
            else:
                pixels = {"status": "missing frame"}
            frame_row = {"scene": scene, "frame": frame, **pixels}
            if reference.is_file():
                frame_row["adobe_sha256"] = digest(reference)
            if image.is_file() and (scene, frame) not in failed:
                frame_row["native_sha256"] = digest(image)
            frames.append(frame_row)
            pb_info = pb_lookup.get((scene, frame), {})
            editable_stage_frames.append({
                **frame_row,
                "scripted_playback_diverges": bool(pb_info.get("diverges_from_editable_stage", False)),
                "scripted_playback_causes": list(pb_info.get("divergence_causes", [])),
                "scripted_playhead_frame": pb_info.get("playhead_frame", frame),
                "display_node_count": pb_info.get("display_node_count"),
                "different_rgba_pixels_vs_scripted": pb_info.get("different_rgba_pixels_vs_editable", 0),
            })
    compared_with_adobe = [f for f in frames if f.get("status") != "native_sweep_only"]
    visual_exact = bool(compared_with_adobe) and all(
        f.get("different_rgba_pixels") == 0 for f in compared_with_adobe
    )
    row["visual"] = {"status": "exact" if visual_exact else "difference", "frames": frames}
    row["editable_stage"] = {
        "status": "exact" if (stage_equal and visual_exact) else ("stage difference" if not stage_equal else "difference"),
        "basis": EDITABLE_STAGE_BASIS,
        "frame_mode": frame_mode,
        "stage_equal": stage_equal,
        "background_equal": background == native["background"],
        "layout_equal": native["scenes"] == adobe_scenes,
        "adobe_stage": stage,
        "adobe_background": background,
        "adobe_scenes": adobe_scenes,
        "frames": editable_stage_frames,
    }
    row["scripted_playback"] = build_scripted_playback(native, row["behavior"])
    row["behavioral_parity"] = build_behavioral_parity(native, row["behavior"])
    return row


def compare(
    destination,
    cli,
    jobs=1,
    frame_mode="sampled",
    max_frames_per_scene=DEFAULT_MAX_FRAMES_PER_SCENE,
):
    if frame_mode not in FRAME_MODES:
        raise ValueError(f"Unsupported frame_mode {frame_mode!r}; expected one of {FRAME_MODES}")
    sources = json.loads((destination / "inputs.json").read_text())
    recipe_hash = digest(destination / "corpus.jsfl")
    if any(row["capture_token"] != ":".join((row["id"], row["source_sha256"], recipe_hash)) for row in sources):
        raise ValueError("Prepared input or Adobe capture command changed; prepare a fresh evidence directory")
    host = destination / "host.txt"
    frame_basis = (
        "first, middle and last authored frame of each scene, zero-based, transparent stage"
        if frame_mode == "sampled"
        else f"unique keyframe starts, tween midpoints, middle and last frame per scene (bounded to {max_frames_per_scene}), zero-based, transparent stage"
    )
    report = {"adobe_host": host.read_text().strip() if host.is_file() else None,
              "frame_mode": frame_mode,
              "max_frames_per_scene": int(max_frames_per_scene),
              "frame_basis": frame_basis,
              "editable_stage_basis": EDITABLE_STAGE_BASIS,
              "scripted_playback_basis": SCRIPTED_PLAYBACK_BASIS,
              "behavioral_parity_basis": BEHAVIORAL_PARITY_BASIS,
              "scene_fields": ["frames", "layers", "keys", "labels", "frame scripts"],
              "helper_sha256": digest(cli), "capture_recipe_sha256": recipe_hash, "cases": []}

    def measure(source):
        try:
            return compare_case(
                destination / source["id"],
                source,
                cli,
                report["helper_sha256"],
                frame_mode=frame_mode,
                max_frames_per_scene=max_frames_per_scene,
            )
        except (ValueError, OSError, subprocess.TimeoutExpired) as error:
            return {"id": source["id"], "error": str(error)}

    with ThreadPoolExecutor(max_workers=jobs) as pool:
        report["cases"] = list(pool.map(measure, sources))
    summary = {
        key: dict(collections.Counter(c.get(key, {}).get("status", "error") for c in report["cases"]))
        for key in ("editable", "visual", "behavior", "editable_stage", "scripted_playback", "behavioral_parity")
    }
    report["summary"] = summary
    report["adobe_hosts"] = sorted({row["adobe_host"] for row in report["cases"] if "adobe_host" in row})
    if not report["adobe_host"] and len(report["adobe_hosts"]) == 1:
        report["adobe_host"] = report["adobe_hosts"][0]
    report["counts"] = {
        "documents": len(sources),
        "adobe_stages": sum("adobe_stage" in row.get("editable", {}) for row in report["cases"]),
        "exact_backgrounds": sum(row.get("editable", {}).get("background_equal", False) for row in report["cases"]),
        "exact_scene_layouts": sum(row.get("editable", {}).get("layout_equal", False) for row in report["cases"]),
        "adobe_scenes": sum(len(row.get("editable", {}).get("adobe_scenes", [])) for row in report["cases"]),
        "sampled_frames": sum(len(row.get("visual", {}).get("frames", [])) for row in report["cases"]),
        "compared_frames": sum("different_rgba_pixels" in frame for row in report["cases"]
                               for frame in row.get("visual", {}).get("frames", [])),
        "exact_frames": sum(frame.get("different_rgba_pixels") == 0 for row in report["cases"]
                            for frame in row.get("visual", {}).get("frames", [])),
        "editable_stage_exact_documents": sum(
            row.get("editable_stage", {}).get("status") == "exact" for row in report["cases"]
        ),
        "scripted_playback_divergent_documents": sum(
            row.get("scripted_playback", {}).get("explains_editable_vs_runtime_gap", False)
            for row in report["cases"]
        ),
        "scripted_playback_divergent_frames": sum(
            int(row.get("scripted_playback", {}).get("divergent_frames", 0))
            for row in report["cases"]
        ),
        "trace_oracle_matches": sum(
            row.get("behavior", {}).get("status") == "match" for row in report["cases"]
        ),
        "behavioral_parity_compared": sum(
            bool(row.get("behavioral_parity", {}).get("has_companion_swf", False))
            for row in report["cases"]
        ),
        "behavioral_parity_matches": sum(
            row.get("behavioral_parity", {}).get("status") == "parity_match"
            for row in report["cases"]
        ),
    }
    if digest(cli) != report["helper_sha256"]:
        raise ValueError("Comparison helper changed during measurement; rerun with a stable binary")
    save(destination / "comparison.json", report)
    print(json.dumps(summary))
    return int(any("error" in c for c in report["cases"]))


def companion_stage(movie, fla, directory, capture, importer):
    stage_file = directory / "stage.tsv"
    scene_file = directory / "scenes.tsv"
    receipt = directory / "complete.txt"
    if not stage_file.is_file() or not scene_file.is_file():
        return {"status": "Adobe capture unavailable"}
    if (not capture or not receipt.is_file() or receipt.read_text().strip() != capture["capture_token"]
            or digest(fla) != capture["source_sha256"]):
        return {"status": "stale or incomplete Adobe capture"}
    stage_fields = stage_file.read_text().strip().split("\t")
    total_frames = sum(int(line.split("\t")[0]) for line in scene_file.read_text().splitlines())
    adobe_stage = [*map(float, stage_fields[:3]), total_frames]
    try:
        # Inspection does not execute the original movie's bytecode.
        result = subprocess.run([str(importer), "inspect", str(movie)], text=True,
                                capture_output=True, timeout=90, check=True)
        swf = json.loads(result.stdout)["swf"]
        stage = [swf[k] for k in ("width_px", "height_px", "frame_rate", "frame_count")]
    except (KeyError, TypeError, ValueError, subprocess.SubprocessError):
        return {"status": "SWF header unavailable"}
    return {"status": "match" if stage == adobe_stage else "difference", "swf": stage, "adobe": adobe_stage}


def inventory(folder, destination, importer=None, adobe=None):
    sources = {}
    captures = {row["id"]: row for row in json.loads((adobe / "inputs.json").read_text())} if adobe else {}
    for p in folder.rglob("*"):
        if p.is_file() and p.suffix.lower() == ".fla":
            sources.setdefault((p.parent, p.stem.casefold()), []).append(p)
    rows = []
    for p in sorted(folder.rglob("*")):
        if not p.is_file() or not (p.suffix.lower() in {".swf", ".fla", ".xfl"} or p.name == "DOMDocument.xml"):
            continue
        if p.name == "DOMDocument.xml" and any(p.parent.glob("*.xfl")):
            continue
        rel = p.relative_to(folder).as_posix()
        row = {"id": hashlib.sha256(rel.encode()).hexdigest()[:20], "sha256": digest(p),
               "kind": "xfl" if p.name == "DOMDocument.xml" else p.suffix[1:].lower(),
               "size": p.stat().st_size, "status": "inventoried"}
        if p.suffix.lower() == ".swf" and p.name == "test.swf":
            row["expected_trace"] = (p.parent / "output.txt").is_file()
            row["expected_images"] = sum(1 for image in p.parent.glob("*.expected.png"))
        if p.suffix.lower() == ".swf":
            siblings = sources.get((p.parent, p.stem.casefold()), [])
            if len(siblings) == 1:
                rel_fla = siblings[0].relative_to(folder).as_posix()
                case_id = "fla-" + hashlib.sha256(rel_fla.encode()).hexdigest()[:20]
                row["same_stem_fla"] = case_id
                if adobe:
                    row["companion_stage"] = companion_stage(p, siblings[0], adobe / case_id, captures.get(case_id), importer)
        rows.append(row)
    save(destination, {"cases": rows, "files_by_format": dict(collections.Counter(r["kind"] for r in rows))})
    print(f"Inventoried {len(rows)} source files without execution.")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="mode", required=True)
    prep = sub.add_parser("prepare")
    prep.add_argument("directory", type=Path)
    prep.add_argument("--folder", type=Path, help="additional local FLA collection instead of the pinned corpus")
    comp = sub.add_parser("compare")
    comp.add_argument("--jobs", type=int, default=1, help="Number of documents to render concurrently (default: 1)")
    comp.add_argument(
        "--frame-mode",
        choices=FRAME_MODES,
        default="sampled",
        help="Frame sweep mode: sampled (first/middle/last) or keyframes-and-midpoints",
    )
    comp.add_argument(
        "--max-frames-per-scene",
        type=int,
        default=DEFAULT_MAX_FRAMES_PER_SCENE,
        help=f"Maximum swept frames per scene in keyframes-and-midpoints mode (default: {DEFAULT_MAX_FRAMES_PER_SCENE})",
    )
    comp.add_argument("directory", type=Path)
    comp.add_argument("--helper", type=Path, default=DEFAULT_HELPER,
                      help="cargo build --release -p neoflash-export --example compare_fla_corpus")
    inv = sub.add_parser("inventory")
    inv.add_argument("folder", type=Path)
    inv.add_argument("output_json", type=Path)
    inv.add_argument("--adobe", type=Path, help="captured --folder directory to compare same-name SWF stage headers")
    inv.add_argument("--importer", type=Path, default=ROOT / "target/release/neoflash-import")
    args = parser.parse_args()
    if args.mode == "prepare":
        return prepare(args.directory.resolve(), args.folder.resolve(strict=True) if args.folder else None)
    if args.mode == "compare":
        return compare(
            args.directory.resolve(strict=True),
            args.helper.resolve(strict=True),
            args.jobs,
            frame_mode=args.frame_mode,
            max_frames_per_scene=args.max_frames_per_scene,
        )
    if args.adobe:
        args.importer = args.importer.resolve(strict=True)
        args.adobe = args.adobe.resolve(strict=True)
    return inventory(args.folder.resolve(strict=True), args.output_json, args.importer, args.adobe)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
