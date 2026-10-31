# Codex plan — ActionScript tooling and cost-efficient API coverage

Instructions for Codex. Part A lists engine work that the Actions editor is waiting for (the editor side is done; see
[NF2-ACTIONS-EDITOR.md](NF2-ACTIONS-EDITOR.md)). Part B replaces the current way of closing unsupported AS2/AS3
declarations with a cheaper pipeline. AGENTS.md still applies; the NF2 design lock is unchanged.

## Current state (2026-11-01)

- **Compiler** (`crates/neoflash-script`): own lexer, recursive-descent + Pratt parser, AST, scope analysis and JS
  emitter for AS1/AS2/AS3; AS2 and AS3 project-class profiles (packages, imports, inheritance, accessors,
  interfaces, linkage); structured control flow, exceptions, closures, typed defaults/coercions, `is`/`as`, E4X
  literals. Public API: `compile`, `compile_project`, `compile_with_program`, `Diagnostic { language, line, column, message }`.
- **Runtime**: shared Rust kernels in `crates/neoflash-web/src/{as2_*,as3_*,as_*}.rs` (WASM) with browser adapters in
  `web/*.js`, plus `crates/neoflash-script/src/actionscript/*.js` intrinsics.
- **Coverage** (declaration audit): AS2 2,307 entries — 15 subset, 1,518 partial, 737 missing, 37 needs review.
  AS3 43,740 — 15 / 1,294 / 42,281 / 150; of these, 37,990 are far-future libraries and 1,448 AIR-only. AS3
  foundation stages 1–6: 1,939 entries, 1,033 missing.
- **Editor**: everything the compiler and audit already provide is wired (toolbox, navigator, hints, diagnostics,
  live check, format, comments). Remaining editor features depend on Part A.

---

## Part A — Engine work for the Actions editor (in this order)

Each item names what the editor will consume. Do not change Actions UI; when an item lands, enable the matching
control (allowed by the lock) and update NF2-ACTIONS-EDITOR.md §1.

### A1. Diagnostic severity, code and range

Extend `neoflash_script::Diagnostic` without breaking existing fields:

```rust
pub enum Severity { Error, Warning, Info }
pub struct Diagnostic {
    pub language: ScriptLanguage,
    pub line: usize, pub column: usize,          // unchanged, 1-based chars
    pub end_line: usize, pub end_column: usize,  // new: exclusive end of the offending span
    pub severity: Severity,                      // new; existing errors stay Error
    pub code: Option<u16>,                       // new: Flash-compatible number where one exists (1120, 1046, 1180…)
    pub message: String,
}
```

- Return *all* diagnostics of a unit, not the first one (`compile` for AS currently maps one error into a Vec).
- Warnings: unused import, deprecated AS2 API, an API whose catalogue status is `partial` (message names the
  limit), unreachable code after `return`/`throw` (completion analysis already exists).
- Editor consumption: `sources.rs::record_code_check` maps severity to `nf::code::Severity` (Warning markers and the
  warning counter already exist in `nf::code::script_status`). Squiggles under `end_*` spans are a design request.
- Tests: extend the existing compiler tests; one family scenario, not one test per code.

### A2. Language service (replaces editor heuristics)

Add a read-only API next to `compile_project`, using the real parser and scope analysis:

```rust
pub struct Analysis { /* opaque, cached per (project fingerprint, script) */ }
pub fn analyze(project: &ProjectProgram, script: &Script) -> Analysis;
impl Analysis {
    pub fn diagnostics(&self) -> &[Diagnostic];
    pub fn completions(&self, offset_chars: usize) -> Vec<Completion>;   // label, detail(type), insert, caret, kind
    pub fn hover(&self, offset_chars: usize) -> Option<String>;          // signature + doc line
    pub fn definition(&self, offset_chars: usize) -> Option<SourceRef>;  // project unit id + range
    pub fn symbols(&self) -> Vec<OutlineItem>;                            // functions/vars/classes for navigator
}
```

- Built-in API knowledge must come from one place. Move `crates/neoflash-editor/assets/actionscript-api.json` and its
  generator output path to `crates/neoflash-script/assets/` and load it there; the editor then calls `analyze` and
  deletes `HintContext` resolution (keep `toolbox`/`search`, they only need the index). Make the compiler's own
  `BUILTINS` / `is_host_method` tables consistent with the index, or generate them from it.
- Editor hook: `sources.rs::hint_context` and `script_api::HintContext::hints` → `Analysis::completions`. Keep the
  `Hint` struct shape.

### A3. Quick fixes

`Diagnostic` gets `fixes: Vec<Fix { title, edits: Vec<(range, text)> }>`. First fixes: add missing `import`, declare a
missing `var`, correct the case of a known member, replace an AS2 global with the AS3 equivalent. The NF2 "Error
peek" with "Quick fix ⌘." is in Figma but not in `neoflash-ui`: add an entry to `docs/NF2-REQUESTS.md` when A3 is
ready; do not build the peek yourself.

### A4. Source maps, then debugging

1. Emit a line map from generated JS back to `(script id, line, column)` in `compile_with_program`; use it in the
   browser error surface and in `trace()` output so runtime errors in Test Movie point to the authored line.
2. A debug channel between the Test Movie page and the editor (local WebSocket or the existing preview server):
   breakpoints by authored line, pause/continue/step, call stack, locals. Only after 1 works.
3. Then request the Debug UI (breakpoint markers, call stack, variables panels exist as planned previews) via
   NF2-REQUESTS; enable "Debug options" when the protocol works.

### A5. Later

- AST pretty printer (`format(script) -> String`) to replace re-indent-only Auto-format; keep it idempotent and
  token-preserving, with "Format on save" as a later Preferences item.
- Script assist (structured statement insertion) — after A2.
- Folding and pinned script tabs are editor/design work, not Codex.

---

## Part B — Developing unsupported AS2/AS3 functions at a fraction of the cost

### B1. Why the current process burns limits

Measured in this repository:

| Cost driver | Evidence | Effect |
| --- | --- | --- |
| Giant files re-read every task | `actionscript.rs` 164 KB, `project.rs` 82 KB, `scope.rs` 47 KB, `web/runtime.js` 92 KB, `web/display.js` 78 KB | Tens of thousands of tokens before the first edit, repeated per session and per retry. |
| Huge data files in reach | `actionscript-catalog.json` 20 MB, `actionscript-audit.json` 3 MB, `COMPATIBILITY.md` 134 KB with 900-character lines | One careless `cat`/`grep`/diff fills the context. |
| Per-batch ceremony | 32 `docs/AS2-*-BATCH.md` / `*-HOUR*.md` reports (140 KB), provenance pages, 18 dated "independent HTML5 acceptance" runs, per-entry `rule_id`/`evidence_kind` rewrites | More tokens on reporting than on runtime code. |
| Browser acceptance via screenshots | in-app browser sessions per family | Screenshot tokens per step, repeated per family. |
| Very large commits | recent commits: 58–124 files, 4k–18k inserted lines | Every review/fix cycle re-reads large diffs. |
| One reasoning level for everything | mechanical constants and signatures handled like compiler semantics | Highest-cost reasoning on trivial work. |
| Denominator-driven choice of work | 43,740 AS3 declarations, mostly libraries/AIR | Effort spread over entries real projects rarely use. |

### B2. Rules for every ActionScript coverage session

1. **Never open** `docs/progress/actionscript-catalog.json`, `actionscript-audit.json` or
   `docs/actionscript-reference/*.json`. Query them: `python3 scripts/as-gap.py <class|package> [--missing]`,
   `--classes`, `--stage N`, `--lang AS2` (one line per declaration, signatures included).
2. **Read by symbol, not by file.** Locate with `rg -n "fn name|name:" path` and read ±60 lines. Do not read a file
   above 40 KB whole.
3. **No new report pages.** No `*-BATCH.md`, `*-HOUR.md`, timeboxes or run diaries in `docs/`. One CHANGELOG line per
   batch; capability limits go into the existing COMPATIBILITY section of that family; raw notes into `target/`.
4. **Browser only once per delivery batch**, and prefer the headless harness:
   `NEOFLASH_WORKFLOWS=<family> node crates/neoflash-script/tests/project-execute.mjs`. No screenshots unless the
   change is visual (display, text, filters).
5. **Small sessions.** One family per session; stop at ≈ 1,500 changed lines. Commit, then start the next session
   from a fresh context instead of continuing a long one.
6. **Reasoning effort by task type** (Codex `model_reasoning_effort`): `low`/`medium` for generated tables,
   constants, signatures, facades and status updates; `high` only for compiler semantics (scope, coercion, class
   initialization, control flow) and runtime ordering (events, frame lifecycle).
7. **Status updates are generated, not hand-written.** Change statuses for whole owners or member lists in one
   override, then `python3 scripts/progress.py --write-reports` (this also refreshes the editor toolbox and hints).

### B3. Order of work: cheapest coverage first, then real usage

AS3 foundation (stages 1–6) still has 1,133 missing declarations. By kind: 444 constants, 264 properties,
168 methods, 93 class declarations, 86 events, 37 constructors, the rest language entries. About **55 % is
mechanical** (constants, event-name constants, class facades):

- `python3 scripts/as-gap.py --stage 4 --classes` → `flash.ui.Keyboard` alone is 284 missing constants.

**Wave 1 — generated surface (one session, low effort).** Write `scripts/as-codegen.py` that reads the reference
snapshot and emits, per selected class: static constants with documented values (`public static const X:T = v`),
event-type string constants, class facades with constructor signatures and default arguments, and an explicit
"not supported yet" runtime error for every documented method that has no implementation (named, with the class).
The compiler then accepts the API surface, the editor shows it, and unsupported calls fail loudly instead of silently.
Mark constants/facades `implemented_subset`; leave unimplemented methods `missing`.

**Wave 2 — usage-ranked behaviour.** Rank remaining members by real use, not by catalogue order:
`scripts/as-usage.py` counts `Class.member` / `instance.member` references in the local FLA/XFL corpus
(`target/fla-corpus`, `scripts/check-fla-corpus.py`) and in permissively licensed public AS2/AS3 sources placed under
`target/`. Implement the top of that list per family. Stop a family when its remaining members have no usage.

**Wave 3 — oracle conformance from existing tests.** Ruffle (MIT/Apache-2.0, already a pinned provenance source)
ships AVM1/AVM2 regression tests with ActionScript sources and recorded Flash Player `output.txt` traces
(`tests/tests/swfs/avm1|avm2/<name>/`). Copy the relevant source + expected output for a family into `target/`,
convert the ones whose sources are available into `project-execute.mjs` workflows, and compare traces. This is
original-player evidence without running Flash, and it replaces hand-written expectations. Keep only the tests you
adapt, with attribution in THIRD_PARTY_NOTICES.

**Algorithms**: keep adapting from Ruffle (Rust, same licence family; already pinned in AS3-REUSE-PROVENANCE.md)
before writing from scratch; OpenFL/Royale remain behavioural references only. No complete engine embedding.

### B4. Task card (give Codex exactly this per session)

```markdown
## Family: flash.geom (AS3) — session N
Goal: close missing members listed by `python3 scripts/as-gap.py flash.geom --missing`.
Read only: crates/neoflash-web/src/as3_geom.rs, web/<adapter>.js (symbols: …), crates/neoflash-script/src/actionscript/project.rs (BUILTINS, is_host_method).
Reuse: Ruffle <path @ pinned rev> for <member>.
Tests: extend workflow `geometry-timer` in crates/neoflash-script/tests/project-execute.mjs (+ adapted Ruffle traces).
Verify: cargo test -p neoflash-web -p neoflash-script; NEOFLASH_WORKFLOWS=geometry-timer node crates/neoflash-script/tests/project-execute.mjs
Done: members implemented or explicitly unsupported with a diagnostic; statuses updated through one override;
`python3 scripts/progress.py --write-reports`; one CHANGELOG line. No new docs pages, no browser screenshots.
Effort: medium.
```

### B5. Delivery batch (once per several sessions)

1. `scripts/check.sh` once (it includes the design lock and the editor index check).
2. One browser pass over the families touched in the batch.
3. Update COMPATIBILITY (existing sections), ROADMAP, CHANGELOG. Commit.

### B6. Expected effect

- Wave 1 closes roughly half of the remaining foundation entries in one low-effort session.
- Usage ranking prevents work on members that no project calls.
- Reading budgets and the absence of report pages cut per-session context by an order of magnitude compared with
  sessions that load the compiler, runtime and progress files whole.
