# Progress and implementation coverage

The [coverage page](https://jazzymiles.github.io/NeoFlash/progress.html), [daily history](https://jazzymiles.github.io/NeoFlash/history.html) and repository README provide a compact answer to “where are we?”. All public copy and generated reports use English.

## ActionScript: complete declaration catalogue

AS2/AS3 counts come from **46,047 documented declarations**, joined by exact catalogue ID to the existing independent implementation audit. AS2 has 2,307 entries across the language reference and two component inventories. AS3 has 43,740 entries across the selected Adobe Flash Platform reference, including AIR, Flex and components. These are methods, properties, syntax, classes, packages and other declaration kinds—not 46,047 functions or equal-sized engineering tasks.

- **Implemented subset:** the exact linked rule's bounded behavior is implemented; this is not full API or language conformance.
- **Partial:** an executable path exists with incomplete behavior or known differences.
- **Missing:** required implementation is absent. Most entries use architectural absence inference, not an individual negative execution test.
- **Needs review:** evidence is insufficient for an individual decision. This is neither missing nor implemented, and remains in the total.

No overall AS compatibility percentage is displayed. Core + language + runtime subtotals (AS2: 1,238; AS3: 5,750) are separate from full-reference totals; even this narrower AS3 view includes some AIR declarations. All original catalogue entries are retained in [actionscript-catalog.json](actionscript-catalog.json), with safe rules, source hashes, input hashes and reference limitations in [actionscript-audit.json](actionscript-audit.json). See the generated [audit report](ACTIONSCRIPT.md) for scope breakdowns and evidence.

The importer verifies all four original catalogue hashes and the audit evidence hash, exact one-to-one ID coverage and status totals. Normalization preserves every declaration's ID, name, owner, kind, scope, documentation URL, decision and evidence rule. It excludes raw local artifact paths and logs. It does not rediscover implementations or run conformance tests.

```sh
python3 scripts/progress_catalog.py --summary
python3 scripts/progress_catalog.py --language AS3 --scope runtime --query MovieClip
```

Independent follow-up runs can override explicitly reviewed declaration IDs while preserving their own source manifest, recorded HEAD, browser boundary and per-entry scope. The normalized rules retain these identities; raw local fixtures and browser logs stay in the research evidence. Product/import acceptance without AS declarations does not increase AS counts. The generated report lists these runs separately from the original baseline.

The report is a dated audit, not a fresh test run. The page compares its published implementation with the baseline source fingerprints, showing when that implementation has changed. Documentation-only differences are separate. Original Flash differential testing was not performed. Later unshipped working-tree reviews in the original audit are not counted as shipped behavior or assigned baseline test passes.

## AS3 adaptation priorities

The existing [declaration report](ACTIONSCRIPT.md#as3-adaptation-priorities), README summary and coverage page separate the AS3 catalogue into Flash Player foundations, subsequent local media, deferred Player services/advanced features, AIR-only APIs, external libraries and unresolved availability. The [roadmap](../ROADMAP.md#as3-flash-player-foundations-first) defines the remaining work: mathematics, objects/language, MovieClip, display/events, graphics and text, before media, cameras and networking.

[as3-priorities.json](as3-priorities.json) preserves the classification and its reference provenance independently of implementation decisions. Player availability is read at declaration level, including AIR-only members of mixed classes. A `flash.*` name or the old `runtime` scope alone does not establish Player availability. Conversely, a library's Flash runtime label does not make its classes built in: Flash components, Flex, TLF and other libraries stay separate. Flash Builder is an IDE; JSFL is authoring automation outside this AS3 catalogue. Unresolved metadata stays visible rather than being silently assigned to Player or AIR. Player support can have version, sandbox, debug/standalone or operation-specific restrictions.

Every AS3 declaration belongs to exactly one priority group; Player declarations additionally belong to one ordered stage. Both partitions preserve the four audited statuses and reconcile to the unchanged complete AS3 total. **Audit missing** is the dated implementation decision, **partial** means remaining behavior on an existing path, and **needs review** is an evidence gap. None is a count of equal-sized engineering tasks. Existing implementations are not scheduled again solely because an older audit says missing. Priority classification does not promote statuses, certify the current source or change the language coverage denominator.

The classifier and report checks validate exact ID coverage, metadata provenance and balanced counts. Public builds read the classification at the same Git revision as the audit and page; revisions predating this classification retain their original reporting. Raw reference downloads are not required for a normal report build. To revise the priority assignment, update its manifest, preserve provenance and regenerate the existing reports; semantic implementation decisions still use the separate review workflow below.

## Product capability groups

`product.json` tracks 54 bounded capability groups in editor, animation, HTML5, imports and platform. The main presentation is **Closed / Partial / Planned**. **Closed** is the display label for the existing `implemented` status: the group's stated scope is implemented. **Partial** means a working subset exists with explicit remaining limits; **planned** means the group remains future work.

The secondary **Closed capability groups** percentage remains implemented / total × 100, displayed to one decimal place. Partial groups contribute zero to this numerator. Adding working tools inside a partial group, or improving a group that is already closed, can leave the percentage unchanged. Groups differ in size; this is not a percentage of working features, effort, overall product completion or test line coverage. No weights or extra groups are introduced to make the number rise.

The existing coverage page shows working partial scopes and remaining limits, with links to the full registry. Keep those descriptions current and record user-visible improvements in the existing changelog. Keep each group's completion boundary stable; promote its status only when its defined scope and validation support that decision. Presentation labels do not rewrite stored statuses, denominators or historical counts.

`as2.json` and `as3.json` remain supplementary implementation notes. Their 35/36 groups are **not** the AS coverage denominator. The [detailed notes](DETAILS.md) explain scopes and limitations without substituting those groups for the complete declaration catalogue.

The product notes and supplementary AS descriptions include the workspace and real-FLA increments reviewed in `d6a41a1` and `ac19fb7`. Those descriptive updates do not automatically change the frozen declaration audit; its source freshness remains separate.

## Maintaining the data

With implementation changes, review affected declaration IDs, evidence rules and source fingerprints, plus affected product capability IDs. Do not promote an AS entry because parsing succeeds or a JavaScript counterpart exists. An inspection API is not editable import. Keep scopes stable and preserve review gaps. Update the frozen source audit through its review workflow, then re-import it using:

```sh
python3 scripts/progress_catalog.py --source /path/to/docs/actionscript-reference
python3 scripts/progress.py --write-reports
python3 scripts/progress.py --check
python3 -m unittest discover -s scripts/tests -p 'test_progress*.py'
```

Use the existing catalogue's real directory for `--source`; never copy a different implementation into a nested version tree. Re-importing validates the audit inputs and deterministically regenerates normalized data. The exact catalogue/audit IDs and hashes preserve provenance. Run relevant model/runtime tests and browser checks before claiming behavior. Commit the implementation, reviewed data and generated reports together. CI validates structural integrity and report consistency; it cannot replace semantic review.

## History and publishing

Methodology **v2** corrects the earlier AS2/AS3 denominator. The original 35/36 family percentages did not answer declaration coverage; they are excluded from AS declaration history rather than relabelled. Product history remains available. A change in counting methodology breaks chart lines.

`python3 scripts/progress.py` builds committed HEAD into ignored `dist/progress/`; `--revision <commit>` selects an immutable source revision. Full Git history is required. The builder chooses the last first-parent commit per UTC committer date and reads that revision's data. There is one point per day with commits, beginning when the respective metric exists; no earlier zeroes are invented. The current day's point is replaced by later commits. Git history rewrites may change the series.

Public `progress.json` contains the compact product data, AS aggregates and audit freshness; `history.json` contains daily counts. The 46,047-row list stays in the repository for detailed local work. The `Progress Pages` workflow validates on pull requests and builds/publishes on pushes to `main`. GitHub Pages uses the **GitHub Actions** source setting.

The presentation is inspired by [AnyPS5 coverage](https://boykopovar.github.io/AnyPS5/progress.html) and [history](https://boykopovar.github.io/AnyPS5/history.html). NeoFlash preserves distinctions between declaration status, product capability groups and full Flash conformance.


### Scoped reviews against the normalized catalogue

Completed runtime batches can append explicit per-entry decisions without re-importing an unrelated or newer local research snapshot. Author a reviewed record with the exact current catalogue SHA-256, source fingerprints, bounded scope and browser evidence, then run `python3 scripts/progress_catalog.py --review docs/progress/reviews/2026-10-28-runtime-batches.json`. Apply each record once; unknown IDs, mismatched input hashes and replayed review IDs are rejected. The importer preserves previous decisions and superseded evidence. Coordinator reviews are labelled separately from independent acceptance. Regenerate reports with `python3 scripts/progress.py --write-reports`; counts remain complete-catalogue counts, not full Flash compatibility.
