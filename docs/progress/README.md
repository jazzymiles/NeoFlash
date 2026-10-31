# How progress is counted (methodology v3)

The [coverage page](https://jazzymiles.github.io/NeoFlash/progress.html), its [daily history](https://jazzymiles.github.io/NeoFlash/history.html) and the README summary answer one question: **what works today, by function**. All three are generated from this directory by `scripts/progress.py`. Nothing here is a Flash compatibility score.

There are exactly two measures:

1. **ActionScript**: documented declarations in eight Flash Player areas.
2. **Product**: user-visible functions listed inside each capability group.

## ActionScript: eight Flash Player areas

Every AS2 and AS3 declaration from the Adobe reference is in [actionscript-catalog.json](actionscript-catalog.json) (46,047 entries) with one status:

| Status | Meaning | Required evidence |
| --- | --- | --- |
| **Working** | The main behavior described in the Adobe reference runs. Only environment limits (sandbox, profile, browser) remain. | A test in the shared suite that exercises it. |
| **Partial** | A runtime path exists and is useful; named behavior is still missing. | A test for the working part and a note naming what is missing. |
| **Missing** | No runtime path, or the compiler reports it as unsupported. | — |
| **Needs review** | Not yet classified. | A note on what is unclear. |

The stored key for Working is `implemented_subset`. Review files use `working`.

The **target** is what NeoFlash adapts. It is split into the same eight areas for both languages:

1. Mathematics and 2D geometry
2. Objects, language and collections
3. MovieClip and timeline
4. Display, events and basic input
5. Graphics, bitmaps and filters
6. Text and fonts
7. File audio and video playback
8. Services and advanced Player APIs

Areas 1–6 are the current priority ("Now"). AS3 routing comes from [as3-priorities.json](as3-priorities.json), which reads Player availability per declaration. AS2 routing (language, core and runtime scopes) is in `scripts/progress_targets.py`. An unknown AS2 class fails the check rather than falling into a default.

For each area the summary shows entry counts per status, then **Working %** and **Working or partial %**. **Outside the target** covers AIR-only APIs, Flex, components, TLF, other libraries and platform-unresolved entries. It is reported as one line of totals and never enters a percentage. The full-reference tables in [ACTIONSCRIPT.md](ACTIONSCRIPT.md) are kept for provenance, not as progress.

### Changing a status

Statuses change only through a **test-backed review**, applied with:

```sh
python3 scripts/progress_catalog.py --review docs/progress/reviews/2026-11-01-as3-movieclip.json
python3 scripts/progress.py --write-reports
```

```json
{
  "schemaVersion": 2,
  "baseCatalogSha256": "<sha256 of actionscript-catalog.json before the review>",
  "reviews": [{
    "id": "AS3-MOVIECLIP-20261101",
    "date": "2026-11-01",
    "commit": "<full SHA the tests ran on>",
    "title": "MovieClip frame navigation and labels",
    "summary": "One to three sentences: what now works and how it is proven. At most 400 characters.",
    "tests": ["crates/neoflash-script/tests/project-execute.mjs"],
    "entries": [
      {"catalogId": "AS3:method:flash/display/MovieClip.html#gotoAndPlay()", "status": "working"},
      {"catalogId": "AS3:method:flash/display/MovieClip.html#nextScene()", "status": "partial", "note": "Single-scene projects only; multi-scene navigation is missing."}
    ]
  }]
}
```

The importer enforces:

- File name `YYYY-MM-DD-<area>.json`, one per delivery.
- One language per review.
- Existing test files.
- A note for every non-working status.
- Only changed entries.
- No reused review IDs.

Promotions and demotions are both allowed. The previous decision is kept in the rule for audit. Reviews applied before v3 (schema 1, partial-only) remain in the history unchanged.

## Product: functions inside capability groups

[product.json](product.json) has capability groups in five areas: editor, animation, HTML5, imports and platform. Each group lists its user-visible functions in `items`, each `working`, `partial` or `planned`.

The group status is derived from its items and checked:

- **Closed** (`implemented`): all items are working.
- **Planned**: all items are planned.
- **Partial**: everything else.

The summary counts functions per area ("31 of 58 functions work") and shows the group split beside it. A new or finished function changes the counts the day it lands. Adding a function the product needs is honest even when it lowers the share. `scope` and `limitations` remain the prose description, and every item must be backed by that text and its evidence.

## History

The builder takes the last first-parent commit per UTC day on `main` and reads that revision's data. No earlier zeroes are invented.

- **ActionScript lines** are recounted from each day's committed catalogue with the current area rules, so they are continuous.
- **Product lines** count whole groups before methodology v3 and functions from v3 on. The line breaks where the unit changes.

Supplementary AS family notes (`as2.json`/`as3.json`, removed in v3) were never part of either measure.

## Checks and publishing

```sh
python3 scripts/progress.py --check
python3 -m unittest discover -s scripts/tests -p 'test_progress*.py'
```

`--check` fails on stale generated reports, on group statuses that contradict their items, and on unknown evidence paths. The `Progress Pages` workflow runs the same checks on pull requests and publishes `dist/progress/` from `main` through GitHub Actions. The public `progress.json` contains the target views and product registry. The per-declaration list stays in the repository.
