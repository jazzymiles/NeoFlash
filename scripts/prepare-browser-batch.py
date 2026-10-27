#!/usr/bin/env python3
"""Export a set of original projects into one browser review dashboard; never builds code.

Usage: prepare-browser-batch.py --exporter target/debug/neoflash-export examples/*.nflash
JSON manifests accept a list of paths, or {"cases":[{"id":"events","path":"examples/events-stage.nflash",
"title":"Events", "checks":["Click the blue pad", "Use arrow keys"]}]}. Manifest paths are repository-relative.
Exit 1 means at least one case failed export; its dashboard and other cases are still prepared.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
from pathlib import Path
import re
import shlex
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[1]
TEMPLATE = ROOT / "web/tests/batch-browser.html"
MARKER = ".neoflash-browser-batch.json"


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def cases_from(args):
    entries = []
    if args.manifest:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        entries = manifest.get("cases") if isinstance(manifest, dict) else manifest
        if not isinstance(entries, list):
            raise ValueError("Manifest must be a path list or an object with a cases list.")
        entries = [(item, ROOT) for item in entries]
    entries += [(str(path), Path.cwd()) for path in args.sources]
    if not entries:
        raise ValueError("Supply project paths or a nonempty --manifest.")
    cases, ids = [], set()
    for index, (item, base) in enumerate(entries, 1):
        if isinstance(item, str):
            item = {"path": item}
        if not isinstance(item, dict) or not isinstance(item.get("path"), str) or not item["path"]:
            raise ValueError(f"Case {index}: expected a nonempty path.")
        unknown = set(item) - {"id", "path", "title", "checks"}
        if unknown:
            raise ValueError(f"Case {index}: unknown fields {sorted(unknown)}.")
        path = (base / item["path"]).resolve()
        slug = re.sub(r"[^a-z0-9_-]+", "-", path.stem.lower()).strip("-")[:48] or "project"
        case_id = item.get("id", f"{index:02}-{slug}")
        if not isinstance(case_id, str) or not re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,79}", case_id) or case_id in ids:
            raise ValueError(f"Case {index}: id must be unique, 1–80 lowercase letters/digits/_/-, starting with a letter/digit.")
        ids.add(case_id)
        title = item.get("title", path.stem)
        checks = item.get("checks", [])
        if not isinstance(title, str) or not isinstance(checks, list) or any(not isinstance(check, str) for check in checks):
            raise ValueError(f"Case {case_id}: title and checks must contain text.")
        cases.append({"id": case_id, "path": path, "title": title, "checks": checks})
    return cases


def tail(path, limit=16384):
    with path.open("rb") as source:
        source.seek(max(0, path.stat().st_size - limit))
        return source.read(limit).decode("utf-8", errors="replace")


def export_case(case, args, output, run):
    directory = run / case["id"]
    directory.mkdir()
    log = directory / "export.log"
    source = case["path"]
    entry = {"id": case["id"], "title": case["title"], "checks": case["checks"],
             "source": source.relative_to(ROOT).as_posix() if source.is_relative_to(ROOT) else str(source),
             "export": {"status": "failed", "log_url": log.relative_to(output).as_posix()},
             "runtime": {"status": "not_observed"}, "manual": {"status": "not_checked", "notes": ""}}
    result = entry["export"]
    try:
        entry["source_sha256"] = sha256(source)
        command = [str(args.exporter), str(source), "--output", str(directory / "project")]
        if args.runtime:
            command += ["--runtime", str(args.runtime)]
        with log.open("wb") as stream:
            try:
                process = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT,
                                         timeout=args.timeout, check=False)
                result["exit_code"] = process.returncode
            except subprocess.TimeoutExpired:
                result.update(status="timed_out", message=f"Exporter exceeded {args.timeout:g} seconds; no automatic retry.")
                process = None
        result["message"] = result.get("message") or tail(log).strip()
        if process is not None:
            index = directory / "project/index.html"
            if process.returncode == 0 and index.is_file() and not index.is_symlink():
                result["status"] = "exported"
                entry["url"] = index.relative_to(output).as_posix()
                result["files"] = {}
                for file in sorted((directory / "project").rglob("*")):
                    if file.is_symlink():
                        raise ValueError("Exporter produced a symlink; this batch requires ordinary local files.")
                    if file.is_file():
                        result["files"][file.relative_to(directory / "project").as_posix()] = sha256(file)
            elif process.returncode == 0:
                result.update(status="failed", message="Exporter returned success without an index.html file.")
            else:
                with log.open("rb") as stream:
                    prefix = stream.read(2048).decode("utf-8", errors="replace")
                result["status"] = "compile_failed" if "Script compilation failed:" in prefix + result["message"] else "failed"
    except (OSError, ValueError) as error:
        result.update(status="failed", message=str(error))
        entry.pop("url", None)
        if not log.exists():
            log.write_text(str(error) + "\n", encoding="utf-8")
    return entry


def prepare(args):
    cases = cases_from(args)
    template = TEMPLATE.read_text(encoding="utf-8")
    args.exporter = args.exporter.resolve()
    if not args.exporter.is_file():
        raise ValueError("--exporter must name an already built exporter binary. This tool never builds it.")
    if args.runtime:
        args.runtime = args.runtime.resolve()
    output = args.output.resolve()
    if output.exists() and any(output.iterdir()):
        try:
            marker = json.loads((output / MARKER).read_text(encoding="utf-8"))
        except (OSError, ValueError):
            marker = {}
        if marker.get("generator") != "NeoFlash browser batch":
            raise ValueError("Output is not an empty directory or a previous NeoFlash browser batch; no files were changed.")
    output.mkdir(parents=True, exist_ok=True)
    if (output / "runs").is_symlink():
        raise ValueError("Batch runs directory must not be a symlink.")
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ-") + uuid.uuid4().hex[:8]
    run = output / "runs" / run_id
    run.mkdir(parents=True)
    batch = {"schema_version": 1, "generator": "NeoFlash browser batch", "run_id": run_id,
             "created_utc": datetime.now(timezone.utc).isoformat(), "exporter_sha256": sha256(args.exporter),
             "runtime_timeout_ms": round(args.runtime_timeout * 1000), "cases": []}
    for case in cases:
        entry = export_case(case, args, output, run)
        batch["cases"].append(entry)
        print(f"{entry['id']}: {entry['export']['status']}", flush=True)
    encoded = json.dumps(batch, ensure_ascii=False, indent=2)
    (run / "batch.json").write_text(encoded + "\n", encoding="utf-8")
    (output / "batch.json").write_text(encoded + "\n", encoding="utf-8")
    # JSON is data, including source filenames and exporter diagnostics. Protect the HTML script boundary.
    safe_json = encoded.replace("<", "\\u003c").replace(">", "\\u003e").replace("&", "\\u0026")
    html = template.replace("__BATCH_DATA__", safe_json)
    (output / "index.html").write_text(html, encoding="utf-8")
    (output / MARKER).write_text(json.dumps({"generator": batch["generator"], "schema_version": 1}) + "\n", encoding="utf-8")
    failures = sum(case["export"]["status"] != "exported" for case in batch["cases"])
    print(f"Prepared {len(cases)} cases; {failures} export failures. Browser/manual checks have not run.\nDashboard: {output / 'index.html'}")
    print(f"For same-origin iframe observations, serve this directory locally, for example:\n  python3 -m http.server 8000 --directory {shlex.quote(str(output))}")
    return 1 if failures else 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("sources", nargs="*", type=Path, help="project paths relative to the current directory")
    parser.add_argument("--manifest", type=Path, help="JSON cases; source paths relative to repository root")
    parser.add_argument("--exporter", required=True, type=Path, help="already built exporter binary")
    parser.add_argument("--runtime", type=Path, help="existing WASM runtime directory passed to exporter")
    parser.add_argument("--output", type=Path, default=ROOT / "dist/acceptance")
    parser.add_argument("--timeout", type=float, default=60, help="seconds per export (default: 60)")
    parser.add_argument("--runtime-timeout", type=float, default=15, help="one browser readiness deadline per iframe (default: 15 seconds)")
    args = parser.parse_args(argv)
    if any(not math.isfinite(value) or value <= 0 for value in [args.timeout, args.runtime_timeout]):
        parser.error("Timeouts must be positive finite seconds.")
    try:
        return prepare(args)
    except (OSError, ValueError) as error:
        print(f"Cannot prepare batch: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
