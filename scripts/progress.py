#!/usr/bin/env python3
"""Build the public capability ledger from reviewed manifests and Git history."""

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
MANIFESTS = ("as2.json", "as3.json", "product.json")
STATUSES = ("implemented", "partial", "planned")
REPOSITORY = "https://github.com/jazzymiles/NeoFlash"
REPORT_START = "<!-- progress:start -->"
REPORT_END = "<!-- progress:end -->"


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def read_inventory(root, revision=None):
    categories, features = [], []
    for name in MANIFESTS:
        path = f"docs/progress/{name}"
        content = git(root, "show", f"{revision}:{path}") if revision else (root / path).read_text()
        data = json.loads(content)
        if data.get("schemaVersion") != 1:
            raise ValueError(f"{path}: unsupported schemaVersion")
        if "category" in data:
            category = data["category"]
            categories.append(category)
            features.extend({**f, "category": category["id"]} for f in data["features"])
        else:
            categories.extend(data["categories"])
            features.extend(data["features"])
    validate_inventory(root, categories, features, revision)
    return categories, features


def validate_inventory(root, categories, features, revision=None):
    ids, feature_ids, paths = set(), set(), set()
    for category in categories:
        cid = category["id"]
        if not re.fullmatch(r"[a-z][a-z0-9-]*", cid) or cid in ids:
            raise ValueError(f"Invalid/duplicate category: {cid}")
        ids.add(cid)
        for field in ("title", "description"):
            if not isinstance(category.get(field), str) or not category[field].strip():
                raise ValueError(f"{cid}: missing {field}")
    for feature in features:
        fid = feature["id"]
        if not re.fullmatch(r"[a-z][a-z0-9-]*", fid) or fid in feature_ids:
            raise ValueError(f"Invalid/duplicate feature: {fid}")
        feature_ids.add(fid)
        if feature.get("category") not in ids or feature.get("status") not in STATUSES:
            raise ValueError(f"{fid}: invalid category/status")
        for field in ("title", "scope", "limitations"):
            if not isinstance(feature.get(field), str) or not feature[field].strip():
                raise ValueError(f"{fid}: missing {field}")
        if not isinstance(feature.get("evidence"), list) or not feature["evidence"]:
            raise ValueError(f"{fid}: evidence is required, including a roadmap for planned work")
        for path in feature["evidence"]:
            if not isinstance(path, str) or path.startswith("/") or ".." in Path(path).parts or ":" in path:
                raise ValueError(f"{fid}: unsafe evidence path")
            paths.add(path)
    if not features or ids != {f["category"] for f in features}:
        raise ValueError("Every category must contain at least one capability")
    for path in sorted(paths):
        if revision:
            if git(root, "cat-file", "-t", f"{revision}:{path}") != "blob":
                raise ValueError(f"Evidence must be a file: {path}")
        elif not (root / path).is_file():
            raise ValueError(f"Missing evidence: {path}")


def counts(features):
    result = dict.fromkeys(STATUSES, 0)
    for feature in features:
        result[feature["status"]] += 1
    return {**result, "total": len(features)}


def summary_table(categories, features):
    lines = ["| Направление | Готово | Частично | В плане | Выполнено по реестру |", "| --- | ---: | ---: | ---: | ---: |"]
    for category in categories:
        count = counts([f for f in features if f["category"] == category["id"]])
        percentage = 100 * count["implemented"] / count["total"]
        lines.append(f"| {category['title']} | {count['implemented']}/{count['total']} | {count['partial']} | {count['planned']} | {percentage:.1f}% |")
    return "\n".join(lines)


def reports(root, categories, features, write=False):
    block = "\n".join([
        REPORT_START,
        "## Где мы сейчас",
        "",
        "Работает основа редактора: вложенные таймлайны, вектор, скрипты и автономный HTML5. Полные AS2/AS3, импорт FLA/SWF, переносимые медиа и DOM-приложения ещё в работе.",
        "",
        summary_table(categories, features),
        "",
        "Готово = выполнена описанная граница пункта; частичные пункты не включены в процент. Это покрытие нашего реестра возможностей, **не процент совместимости с Flash и не покрытие строк кода тестами**. Размер пунктов различается; единого процента готовности продукта нет.",
        "",
        "[Текущее покрытие](https://jazzymiles.github.io/NeoFlash/progress.html) · [История по дням](https://jazzymiles.github.io/NeoFlash/history.html) · [Подробный реестр](docs/progress/DETAILS.md) · [Правила подсчёта](docs/progress/README.md)",
        "",
        "Сводка генерируется из `docs/progress/*.json`; GitHub Pages пересобирается при каждом push в `main`.",
        REPORT_END,
    ])
    readme_path = root / "README.md"
    readme = readme_path.read_text()
    if REPORT_START not in readme or REPORT_END not in readme:
        raise ValueError("README needs progress:start and progress:end markers")
    before, rest = readme.split(REPORT_START, 1)
    _, after = rest.split(REPORT_END, 1)
    updated = before + block + after
    labels = {"implemented": "Реализовано", "partial": "Частично", "planned": "В плане"}
    detail = ["# Подробный реестр возможностей NeoFlash", "", "Generated by `python3 scripts/progress.py --write-reports`. Edit the JSON inventories, not this file.", "", "[Методика](README.md). Статусы — аудит реализации в явно указанной границе; ссылки на тесты не означают их запуск при сборке страницы. Сводка не измеряет полную совместимость Flash.", "", summary_table(categories, features)]
    for category in categories:
        detail.extend(["", f"## {category['title']}", "", category["description"]])
        for feature in [f for f in features if f["category"] == category["id"]]:
            detail.extend(["", f"### {feature['title']}", "", f"`{feature['id']}` · **{labels[feature['status']]}**", "", f"**Граница:** {feature['scope']}", "", f"**Ограничения:** {feature['limitations']}", "", "**Доказательства:** " + ", ".join(f"[{p}](../../{p})" for p in feature["evidence"])])
    details = "\n".join(detail) + "\n"
    detail_path = root / "docs/progress/DETAILS.md"
    if write:
        readme_path.write_text(updated)
        detail_path.write_text(details)
    elif readme != updated or not detail_path.exists() or detail_path.read_text() != details:
        raise ValueError("Progress reports are stale: python3 scripts/progress.py --write-reports")


def methodology_version(root, revision):
    version = json.loads(git(root, "show", f"{revision}:docs/progress/methodology.json"))["version"]
    if type(version) is not int or version < 1:
        raise ValueError("Methodology version must be a positive integer")
    return version


def snapshot(categories, features, date, revision, methodology_version=1):
    return {
        "date": date,
        "revision": revision,
        "methodologyVersion": methodology_version,
        "categories": {c["id"]: counts([f for f in features if f["category"] == c["id"]]) for c in categories},
        "total": counts(features),
    }


def daily_revisions(root, revision):
    # Keep the last commit in first-parent ancestry order on each UTC date.
    # Author timestamps may be rewritten or retain an older timezone/date.
    days = {}
    for line in git(root, "log", "--first-parent", "--reverse", "--format=%H %ct", revision).splitlines():
        sha, timestamp = line.split()
        day = datetime.fromtimestamp(int(timestamp), timezone.utc).date().isoformat()
        days[day] = sha
    return sorted(days.items())


def history(root, revision):
    records = []
    for day, sha in daily_revisions(root, revision):
        files = set(git(root, "ls-tree", "-r", "--name-only", sha, "--", "docs/progress/").splitlines())
        if not files:
            continue  # No measurements existed yet; do not invent zeroes.
        required = {f"docs/progress/{name}" for name in (*MANIFESTS, "methodology.json")}
        if not required.issubset(files):
            raise ValueError(f"Incomplete historical inventory at {sha}")
        categories, features = read_inventory(root, sha)
        records.append(snapshot(categories, features, day, sha, methodology_version(root, sha)))
    return records


def build(root, output, revision):
    if git(root, "rev-parse", "--is-shallow-repository") == "true":
        raise ValueError("History needs a full checkout: git fetch --unshallow")
    revision = git(root, "rev-parse", "--verify", f"{revision}^{{commit}}")
    categories, features = read_inventory(root, revision)
    timestamp = git(root, "show", "-s", "--format=%ct", revision)
    data = {
        "schemaVersion": 1,
        "generatedAt": datetime.fromtimestamp(int(timestamp), timezone.utc).isoformat().replace("+00:00", "Z"),
        "revision": revision,
        "repository": REPOSITORY,
        "methodology": {
            "version": methodology_version(root, revision),
            "description": "Доля полностью выполненных пунктов проверяемого реестра; не процент совместимости с Flash. Частичные пункты не входят в числитель. История — последний commit main за каждый день UTC; до начала измерений данных нет. Изменения состава реестра меняют знаменатель.",
        },
        "categories": categories,
        "features": features,
        "history": history(root, revision),
    }
    output.mkdir(parents=True, exist_ok=True)
    for name in ("index.html", "history.html", "style.css", "app.js"):
        # Source and evidence links always describe the same immutable revision.
        content = git(root, "show", f"{revision}:web/progress/{name}")
        (output / name).write_text(content + "\n")
    (output / "progress.html").write_text((output / "index.html").read_text())
    (output / ".nojekyll").write_text("")
    (output / "progress.json").write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
    (output / "history.json").write_text(json.dumps(data["history"], ensure_ascii=False, indent=2) + "\n")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="validate the working-tree inventory without publishing")
    parser.add_argument("--write-reports", action="store_true", help="regenerate the README summary and local detailed ledger")
    parser.add_argument("--revision", default="HEAD", help="committed revision to build (default HEAD)")
    parser.add_argument("--output", type=Path, default=ROOT / "dist/progress")
    args = parser.parse_args()
    if args.check or args.write_reports:
        version = json.loads((ROOT / "docs/progress/methodology.json").read_text())["version"]
        if type(version) is not int or version < 1:
            raise ValueError("Methodology version must be a positive integer")
        categories, features = read_inventory(ROOT)
        reports(ROOT, categories, features, write=args.write_reports)
        print(f"Valid inventory: {len(categories)} categories, {len(features)} capabilities; {counts(features)}")
    else:
        data = build(ROOT, args.output, args.revision)
        print(f"Built {args.output}: {len(data['features'])} capabilities, {len(data['history'])} daily snapshots ({data['revision'][:7]})")


if __name__ == "__main__":
    main()
