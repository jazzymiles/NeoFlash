# Progress and implementation coverage

The public [coverage page](https://jazzymiles.github.io/NeoFlash/progress.html) and [daily history](https://jazzymiles.github.io/NeoFlash/history.html) answer “where are we?” without displaying a long API dump. The repository front page repeats the compact category table. The [detailed ledger](DETAILS.md) retains every scope, limitation and evidence path for local work and review.

## Counting rules, version 1

The source of truth is `as2.json`, `as3.json` and `product.json` in this directory. Stable feature IDs describe **reviewed capability families**, not equal-sized engineering tasks or an exhaustive list of every AS/Flash API member. AS2 and AS3 are separate language views; a shared compiler feature can appear in both. Categories must not be added together to claim a global product completion percentage.

- **implemented:** the explicit bounded scope is implemented. This does not extend to everything suggested by a short feature title.
- **partial:** part of the scope exists, or behavior still uses approximations. This contributes zero to the completed numerator; it is shown separately.
- **planned:** the capability is missing or is only planned. An inspection API does not count as editable import, parsing does not establish runtime compatibility, and a JavaScript counterpart does not establish Flash semantics.
- **percentage:** `implemented / (implemented + partial + planned) × 100`, displayed to one decimal place. It is neither Flash compatibility nor test line coverage. There is no 50% weight for “partial.”

Evidence points to code, tests or the documented backlog. The generator validates paths and schema; it **does not infer semantic support from names, run conformance tests or automatically promote statuses**. The initial source review uses published implementation `cc9e7c90c2f661dc3b3d2a85ed28da7bbc753075`. Uncommitted work in other checkouts is excluded. The underlying documentation and tests describe more detail than a category percentage can.

## Maintaining the ledger

With every implementation change, review the affected capability IDs, scopes, status, limitations and evidence in the JSON files. Keep meaningful acceptance boundaries stable: do not split completed tasks merely to increase percentages. Add missing scope openly; the denominator may grow and the displayed percentage may fall. Remove entries only when scope is deliberately retired and explain it in the changelog. A change to the counting method itself requires a new methodology version and a separately labelled series.

```sh
python3 scripts/progress.py --write-reports  # updates README and DETAILS.md
python3 scripts/progress.py --check         # rejects stale reports/broken evidence
python3 -m unittest discover -s scripts/tests -p 'test_progress.py'
```

Run the relevant model/runtime tests and browser checks when claiming new behavior. Preserve explicit limitations when those checks do not prove full Flash parity. Commit the implementation, ledger and generated reports together. `AGENTS.md` records this maintenance requirement; CI validates it structurally but cannot replace a semantic review.

## Daily history and publication

`python3 scripts/progress.py` builds **committed HEAD**, including the static UI, into ignored `dist/progress/`. It rejects shallow clones. `--revision <commit>` reproduces an earlier snapshot with immutable code links. Output includes `progress.json` and `history.json`; generated timestamps identify the source commit, not the time someone opened the page.

The builder walks first-parent Git history and selects the last commit in ancestry order on each **UTC commit date**, using committer timestamps. It reads that commit's reviewed registry. There is one point per day with commits, beginning with the first registry; earlier dates are absent, never fabricated zeroes. The current day's point is replaced as new commits arrive. Historical values retain their original registry and denominator; the dashed total line exposes scope changes. History is reconstructed from Git, without a bot committing measurements or a second source tree. Rewriting Git history may change the series.

The `Progress Pages` workflow validates the manifests/reports and generator tests on pull requests. Each push to `main` builds and publishes the compact overview and history through GitHub Pages. Source setting: **GitHub Actions**. No site registration, external analytics, npm dependencies or separate hosting service is required.

The format is inspired by [AnyPS5 coverage](https://boykopovar.github.io/AnyPS5/progress.html) and [history](https://boykopovar.github.io/AnyPS5/history.html). NeoFlash's metric is a reviewed capability register because symbol counts alone cannot prove AS semantics or complete authoring workflows.
