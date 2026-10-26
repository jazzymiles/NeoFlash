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

The report is a dated audit, not a fresh test run. The page compares its published implementation with the baseline source fingerprints, showing when that implementation has changed. Documentation-only differences are separate. Original Flash differential testing was not performed. Later unshipped working-tree reviews in the original audit are not counted as shipped behavior or assigned baseline test passes.

## Product capability groups

`product.json` tracks bounded capabilities in editor, animation, HTML5, imports and platform. **Implemented** means the explicit scope is implemented; **partial** contributes zero to the completed numerator; **planned** remains in the denominator. Percentage = implemented / total × 100, displayed to one decimal place. This is not test line coverage or overall product completion.

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
