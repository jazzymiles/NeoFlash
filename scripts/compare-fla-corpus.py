#!/usr/bin/env python3
"""Compare editable FLA/XFL imports against Adobe Animate authoring frames.

prepare DIRECTORY creates fresh source copies. Run DIRECTORY/corpus.jsfl in
licensed Animate, then compare DIRECTORY. Without --folder the input is the
pinned corpus, including its independently recorded companion SWF trace oracles.
With --folder, stage individual FLA files only and use anonymous case IDs.
inventory indexes an additional source/reference collection without executing it.
compare writes every outcome to comparison.json. Its exit status reports
measurement errors; visual or playback differences are reported per case.
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
            # Pinned external AS3 classes are source companions of these FLAs.
            if pinned:
                for script in source.parent.glob("*.as"):
                    shutil.copyfile(script, staged.parent / script.name)
                oracle = source.parent / "output.txt"
                movie = source.with_suffix(".swf")
                config = source.parent / "test.toml"
                if oracle.is_file() and movie.is_file() and config.is_file():
                    for companion in (oracle, movie, config):
                        shutil.copyfile(companion, case / companion.name)
                    row["trace_oracle"] = {
                        "movie": movie.name, "movie_sha256": digest(movie),
                        "expected_sha256": digest(oracle), "config_sha256": digest(config),
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


def compare_case(case, source, cli, helper_hash):
    staged = case / source["staged"]
    if source_digest(staged) != source["source_sha256"]:
        raise ValueError("Staged source has changed; recapture the original")
    row = {"id": source["id"], "source_sha256": source["source_sha256"]}
    row["behavior"] = behavior(case, source, cli)
    if staged.is_file() and not staged.stat().st_size:
        row["editable"] = {"status": "empty input"}
        row["visual"] = {"status": "unavailable", "reason": "empty input"}
        return row
    try:
        cached = case / "native-result.json"
        cache = json.loads(cached.read_text()) if cached.is_file() else {}
        signature = [helper_hash, source["source_sha256"]]
        if cache.get("signature") == signature and all(
                (case / f"native-s{idx}-f{frame}.png").is_file()
                for idx, scene in enumerate(cache["result"]["scenes"])
                for frame in {0, (scene[0] - 1) // 2, scene[0] - 1}):
            native = cache["result"]
        else:
            native = helper(cli, "authored", staged, case)
            save(cached, {"signature": signature, "result": native})
    except (ValueError, subprocess.TimeoutExpired) as error:
        row["editable"] = {"status": "import or render failed", "reason": str(error)}
        row["visual"] = {"status": "unavailable"}
        return row
    mark = case / "complete.txt"
    adobe = mark.read_text().strip() if mark.is_file() else "Adobe capture missing"
    if adobe != source["capture_token"]:
        if adobe not in ("Adobe capture missing", "empty input", "in progress") and not adobe.startswith("error:"):
            raise ValueError("Adobe capture belongs to a different source or capture command")
        row["editable"] = {"status": "native only", "native": native, "reason": adobe}
        row["visual"] = {"status": "unavailable", "reason": adobe}
        return row
    stage_fields = (case / "stage.tsv").read_text().strip().split("\t")
    stage = list(map(float, stage_fields[:3]))
    background = stage_fields[3].lower()
    adobe_scenes = [list(map(int, line.split("\t"))) for line in (case / "scenes.tsv").read_text().splitlines()]
    same_lengths = [scene[0] for scene in native["scenes"]] == [scene[0] for scene in adobe_scenes]
    row["adobe_host"] = (case / "host.txt").read_text().strip()
    row["editable"] = {"status": "stage match" if native["stage"] == stage and same_lengths else "stage difference",
                       "adobe_stage": stage, "adobe_background": background,
                       "background_equal": background == native["background"], "native": native,
                       "adobe_scenes": adobe_scenes, "layout_equal": native["scenes"] == adobe_scenes}
    frames = []
    failed = {(error["scene"], error["frame"]) for error in native["render_errors"]}
    for scene, layout in enumerate(adobe_scenes):
        last = layout[0] - 1
        for frame in sorted({0, last // 2, last}):
            image = case / f"native-s{scene}-f{frame}.png"
            reference = case / f"adobe-s{scene}-f{frame}.png"
            pixels = helper(cli, "pixels", image, reference) if (scene, frame) not in failed and image.is_file() and reference.is_file() else {
                "status": "missing frame"}
            frames.append({"scene": scene, "frame": frame, **pixels})
            if reference.is_file():
                frames[-1]["adobe_sha256"] = digest(reference)
            if image.is_file() and (scene, frame) not in failed:
                frames[-1]["native_sha256"] = digest(image)
    row["visual"] = {"status": "exact" if all(f.get("different_rgba_pixels") == 0 for f in frames)
                     else "difference", "frames": frames}
    return row


def compare(destination, cli, jobs=1):
    sources = json.loads((destination / "inputs.json").read_text())
    recipe_hash = digest(destination / "corpus.jsfl")
    if any(row["capture_token"] != ":".join((row["id"], row["source_sha256"], recipe_hash)) for row in sources):
        raise ValueError("Prepared input or Adobe capture command changed; prepare a fresh evidence directory")
    host = destination / "host.txt"
    report = {"adobe_host": host.read_text().strip() if host.is_file() else None,
              "frame_basis": "first, middle and last authored frame of each scene, zero-based, transparent stage",
              "scene_fields": ["frames", "layers", "keys", "labels", "frame scripts"],
              "helper_sha256": digest(cli), "capture_recipe_sha256": recipe_hash, "cases": []}
    def measure(source):
        try:
            return compare_case(destination / source["id"], source, cli, report["helper_sha256"])
        except (ValueError, OSError, subprocess.TimeoutExpired) as error:
            return {"id": source["id"], "error": str(error)}

    with ThreadPoolExecutor(max_workers=jobs) as pool:
        report["cases"] = list(pool.map(measure, sources))
    summary = {key: dict(collections.Counter(c.get(key, {}).get("status", "error") for c in report["cases"]))
               for key in ("editable", "visual", "behavior")}
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
        return compare(args.directory.resolve(strict=True), args.helper.resolve(strict=True), args.jobs)
    if args.adobe:
        args.importer = args.importer.resolve(strict=True)
        args.adobe = args.adobe.resolve(strict=True)
    return inventory(args.folder.resolve(strict=True), args.output_json, args.importer, args.adobe)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
