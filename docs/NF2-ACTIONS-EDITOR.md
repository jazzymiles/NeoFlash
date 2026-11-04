# Actions editor — toolbox, Script navigator, code hints and code commands

This page explains to implementation agents (Codex) and maintainers how the Actions script editor uses the
ActionScript support that already exists, how new runtime support reaches the editor automatically, and which parts
are still waiting for the engine. Design rules: [DESIGN-SYSTEM.md](DESIGN-SYSTEM.md) and AGENTS.md › "Design system
(NF2) — locked". Remaining engine work: [CODEX-ACTIONSCRIPT-PLAN.md](CODEX-ACTIONSCRIPT-PLAN.md).

## 1. What the user gets

| NF2 element (Figma `panel-actions-floating`, `editor-actions-tab`) | State | Behaviour |
| --- | --- | --- |
| Sidebar › Search toolbox + toolbox tree | **working** | Top level, Language elements, Project classes, then packages A–Z → classes → members (type on the right). Search is a flat list of classes and members (max 200). Hover: signature, owner, kind; on a class, its support level. Double-click inserts at the caret. |
| Sidebar › Script navigator | **working** | Current selection; main timeline (root symbol name, scene icon); Symbol definitions; Project files. Click opens the script: enters the timeline, moves to the frame, selects the object, switches the Actions target. |
| Toolbar › Auto-format | **working** | Re-indents the whole script (4 spaces, braces/brackets/`case` depth), strips trailing spaces. Never moves a token. |
| Toolbar › Show code hint, ⌃Space, typing `.` | **working** | Code hint popover under the caret. ↑/↓ select, Return/Tab insert, Esc closes, click inserts. ActionScript only. |
| Toolbar › Apply block comment / Apply line comment / Remove comment | **working** | `/* … */` around the selection (or the caret line); `// ` at the block's indentation; removal of either. |
| Toolbar › Show toolbox sidebar | **working** | Toggles the sidebar. The sidebar also hides itself when the panel is narrower than 640 pt. |
| Gutter markers, compiler messages, problem count | **working** | Errors and warnings retain severity, code, Unicode source range and source path. Silent live check runs 0.6 s after editing; explicit **Check** writes to Output. Project diagnostics navigate to their own file. |
| Toolbar › Debug options, Collapse between braces, Expand all; Script assist | disabled, "Not available yet" | Need debugger/source maps, code folding, structured editing (plan §4). |
| Quick fix ⌘. | **partial** | Applies one unambiguous safe fix at the caret line through normal Undo; multiple choices are reported in Output until the NF2 selector is available. |
| Error peek, range squiggles, overview ruler, pinned script tabs | not shown | Await NF2 components (R-033). |

## 2. Principle: offer only what runs

The toolbox and hints never advertise an API that NeoFlash does not run. They are built from the declaration audit:
only entries whose status is `implemented_subset` or `partial` are included. `missing` and `needs_review` entries
never appear. The compiler stays the only authority on what a script means: hints are assistance, not a type checker,
and an unknown receiver simply produces no hints.

## 3. Data pipeline — how new objects, functions and classes reach the editor

```
docs/progress/actionscript-catalog.json      statuses (versioned; rewritten by progress tooling)
docs/actionscript-reference/{as2,as3,…}.json  signatures, types, inheritance (local snapshot, not versioned)
            │
            ▼  python3 scripts/editor-api-index.py
crates/neoflash-script/assets/actionscript-api.json   (versioned, ~340 KB, embedded with include_str!)
            │
            ▼  crates/neoflash-script/src/language_service.rs  (parsed once, OnceLock)
toolbox tree · code hints · class support tooltips
            +
Project::sources  ──scan_class() on every request──►  "Project classes" folder + hints (no build step)
```

**Built-in API.** Partial declarations carry their current audited limitation text; warnings only use it when
a built-in receiver is unambiguously resolved. Both catalogue and audit hashes are checked. When a declaration's status changes in the catalogue, the editor picks it up on the next index
build. Nothing in the editor code lists class or member names.

- `python3 scripts/progress.py --write-reports` now also runs `scripts/editor-api-index.py` when the local reference
  snapshot exists. Codex already runs this command after every implementation change (AGENTS.md), so new support
  flows into the toolbox and hints with no extra step.
- Without the reference snapshot, a catalogue-hash-verified existing index can refresh audit limitation metadata.
  Changed declaration signatures/statuses still require the reference snapshot; `--check` verifies both hashes.
- The generator fails loudly if a ready catalogue entry has no reference signature (currently 0 of 2,842).

**Project classes.** `language_service::scan_class` uses the ActionScript compiler token stream to read `package`, `class`/`interface`, `extends`, and public/internal
`function`, `get`/`set`, `var`, `const` declarations (comments stripped; `private` skipped). A class written in
Project files appears immediately under "Project classes", resolves in `var h:Hero`, `Hero.` (statics) and inherits
built-in members through `extends`.

### Asset schema (`schemaVersion` 2)

```json
{
  "catalogSha256": "…", "auditSha256": "…", "generatedBy": "scripts/editor-api-index.py", "schemaVersion": 2,
  "statuses": ["implemented_subset", "partial"],
  "languages": {
    "AS3": {
      "classes": { "flash.display.MovieClip": { "kind": "class", "parents": ["flash.display.Sprite", "…", "Object"], "status": "partial" } },
      "members": [ { "owner": "flash.display.MovieClip", "name": "gotoAndPlay", "kind": "method",
                     "type": "void", "params": "frame:Object, scene:String = null", "status": "partial" } ]
    },
    "AS2": { "…": "…" }
  }
}
```

- `classes[*].status`: `subset`, `partial`, or `members` — the class declaration itself is not ready, but some of
  its members are (for example `flash.display.Sprite`); such owners are still listed with their superclass chain so
  inherited hints work.
- `members[*]`: `owner` is the qualified class or `""` for globals and language elements (`<global>` in the AS3
  reference is normalized to `""`); `static` and `params` are present only when true/callable; `group: "language"`
  marks operators, statements and directives (Language elements folder).
- AS2 superclass chains are derived from the reference's inherited-member pages; AS3 uses its `inheritance` list.

## 4. Code map

| File | Responsibility |
| --- | --- |
| `scripts/editor-api-index.py` | Join catalogue statuses with reference signatures → asset. `--check` staleness test. |
| `scripts/progress.py` (`main`) | Calls the generator after `--write-reports`, or `--check` otherwise. |
| `scripts/check.sh` | Runs `editor-api-index.py --check` after the design lock. |
| `crates/neoflash-script/assets/actionscript-api.json` | Generated asset. Never edit by hand. |
| `crates/neoflash-script/src/language_service.rs` (re-exported by editor `script_api.rs`) | Index loading; `HintContext` (receiver resolution, inheritance, hints); project-class scan; toolbox folders, own members, search; AS3 `missing_import`. Unit tests at the bottom. |
| `crates/neoflash-editor/src/code_tools.rs` | `CodeCommand` and pure text commands: `apply`, `line_comment`, `block_comment`, `uncomment`, `auto_format`. Unit tests at the bottom. |
| `crates/neoflash-editor/src/panels/actions_sidebar.rs` | `ActionsUi` (non-document panel state), sidebar drawing, toolbox rows, Script navigator, `ScriptLocation` + `open_script_location`, `toolbox_insertion`. |
| `crates/neoflash-editor/src/panels/actions.rs` | Toolbar: `CODE_TOOLS` table (command or `None` = planned), sidebar toggle; sidebar/main split in `actions_body`. |
| `crates/neoflash-editor/src/sources.rs` | `script_workspace` applies queued commands (`apply_code_command`), builds the `HintContext` (`hint_context`), runs `live_check`; `code_editor` handles hint keys, the popover (`hint_popover`) and insertion (`accept_hint`). |
| `crates/neoflash-ui/src/components/tree.rs` | `TreeItem::icon()` — folder icon override (package, class, scene, target), matching the Figma instance overrides. Design-owned. |

## 5. How each feature works

### 5.1 Commands (toolbar and toolbox)

Toolbar buttons and toolbox double-clicks never touch text directly. They store a `CodeCommand` in
`Editor::actions_ui.pending`. On its next frame the visible code editor (`script_workspace`) takes the command,
reads its own selection from `egui::TextEditState`, calls `code_tools::apply`, writes the new text and selection and
focuses itself. The caller then commits the changed text through the existing `transact` path, so every command is
one normal Undo step and works the same for frame scripts, object scripts and project files.

`CodeCommand::Insert { text, caret, import }`: replaces the selection; `caret` is the char offset inside `text`
(between the parentheses of a method with parameters). `import` (AS3 only, qualified class) adds
`import a.b.C;` after the last leading import, or at the top, unless an exact or wildcard import exists.

### 5.2 Code hints

`HintContext` is built lazily (only while hints are open or being triggered) from the document:

- `this`: `MovieClip` for timeline scripts; for object scripts the selected element (`ThisKind::of_element`:
  instance of a Button symbol → `SimpleButton`/`Button`, text → `TextField`, bitmap → `Bitmap`, shapes → `Shape`);
  in Project files, the class declared by the open file.
- Named stage instances of the current timeline (all keyframes), with their kinds.
- Project classes of the same dialect family (AS3, or AS2/AS1).

`hints(source, caret, explicit)`:

1. Skip inside comments and strings (`in_comment_or_string`, also unterminated ones).
2. Take the identifier prefix before the caret. If it follows `.`, resolve the receiver chain (`a.b.c.`) left to
   right: `this`, `super`, a declaration visible in the containing function, a stage instance, a class name
   (→ static members), or a member of `this`; each further segment follows the member's documented type.
3. Instance members walk the superclass chain (project `extends` first, then the reference chain); statics are not
   inherited. Constructors are hidden; AS3 `event` entries are hidden (they are strings for `addEventListener`).
4. Without a receiver (⌃Space, toolbar, or a typed prefix in an open popover): members of `this`, globals, class
   names and stage instances; after `:` only class names.
5. Case-insensitive prefix filter, exact-case matches first, then alphabetical; at most 200.

Popover: `egui::Area` under the caret line, `nf::popover_frame`, `NF2/Menu Item` rows with the type as the
right-hand text, 220 pt content width (Figma "Code hint" 230 incl. frame), up to 8 visible rows, keyboard selection
painted with the Hover fill. The state (`HintState`) lives in egui temp memory keyed by the editor id; moving to
another word, losing focus (unless the pointer is over the popover) or Esc closes it. Keys are consumed *before* the
`TextEdit` runs so Return/Tab/arrows do not also edit the text.

### 5.3 Script navigator

`navigator_entries(symbol)` lists non-empty frame scripts ("Name : Frame n") and object scripts
("instance : Event", deduplicated per element/event). `open_script_location` leaves to the root
(`leave_to_level(0)`), enters the symbol when needed, pauses playback, sets the frame, selects the object and the
Actions target/event. The current entry is highlighted by comparing `current_script_location()`.

### 5.4 Diagnostics and quick fixes

Both Actions and Compiler errors consume structured diagnostics. Source-file errors keep their path instead of being
attached to the selected file. Only diagnostics belonging to the open script create gutter markers. Warnings use
the existing amber NF2 marker and count; codes remain visible in compiler messages.

⌘. checks the current source fingerprint and applies a unique available fix on the caret line. The existing command
path preserves Unicode positions, focus and one Undo for frame/object actions and project files. Unsupported or
ambiguous fixes leave the text intact. Lints never offer deletion that can discard initializer side effects, or
legacy operator replacement that changes coercion. Error peek and a multiple-fix selector await R-033.

### 5.5 Live check

`live_check` records the time of the last text change per editor id and, 0.6 s later, runs the same compile as Check
(`check_action_source` / `check_project_sources`) with `announce = false`, so Output is not flooded. Results are
cached by the existing fingerprint (`code_feedback`), which already includes all project sources and linkage.

### 5.6 Source maps and Test Movie

Published frame/object handlers and project class/module bundles carry Source Map v3 plus decoded runtime locations.
Test Movie errors and trace output show authored source positions, with browser wrapper offsets calibrated per engine.
AS maps identify the containing statement start (a multiline expression uses that first line); JS/TS maps use token/AST
positions. Include files retain their own source identity. Generated helper lines remain unmapped.

Identical handler text is compiled once as `Shared script N`, with every authored location stored in the bundle.
The dispatch packet does not yet identify the exact occurrence. Browser `neoflash-script-message` events expose the
mapped payload; there is no connection back to the native editor and no real pause/continue/step protocol. The older
debug instrumentation API records notifications and does not make the disabled Debug options control functional.

## 6. Tests

- `sources::integration_tests` — real Actions warning display, ⌘. for frame/object/project sources, one Undo,
  document validation and cross-file diagnostic navigation.
- Compiler `language_service` tests — scoped declarations, multiline project members, receiver hints and source refs.
- `typescript_toolchain` — structured diagnostic ranges and safe fixes; source maps and tooling boundaries.
- `source_mapping` plus exporter and `web/tests/runtime.test.cjs` families — repeated calls, nested functions,
  Unicode/includes, classes/modules, helper exclusion, shared origins, wrapper calibration and hostile errors.


- `script_api::tests` — index statuses; inherited `this` members; `var p:Point` and `MouseEvent.CL…` statics; AS2
  index and no hints in comments/strings; project classes (public members only, inheritance, statics, toolbox
  folder); import detection.
- `code_tools::tests` — line/block comment round trips, auto-format idempotence and token preservation, comments and
  XML literal lines kept, Insert adds an AS3 import once.
- `workspace_editor_tests::actions_code_hints_toolbox_and_commands_edit_the_open_script` — renders the real Actions
  panel: toolbox present, Show code hint lists `gotoAndPlay()`/`gotoAndStop()` for `this.goto`, hints do not edit,
  Apply line comment creates one Undo step, toolbox insertion adds the import.
- `workspace_editor_tests::nf2_actions_panel_reports_caret_planned_tools_and_jumps_to_diagnostics` — working tools
  enabled, planned tools disabled, sidebar present.
- Visual: `NEOFLASH_WINDOW=1920x1080 NEOFLASH_SCREENSHOT=target/ui-shots/actions.png NEOFLASH_SCREENSHOT_WORKSPACE=developer
  cargo run -p neoflash-editor -- <project with an AS3 frame script>` compared with `exports/interface/dark/png/editor-actions-tab.png`.

## 7. What Codex may and may not change here

Allowed (behaviour, no visual change): regenerate the asset; extend `scan_class`; improve receiver resolution in
`HintContext`; add `ThisKind` cases; replace heuristics with a compiler language service (plan §1); enable a planned
toolbar command by giving it a `CodeCommand` in `CODE_TOOLS` once its behaviour exists.

Not allowed (design-owned): the sidebar width/padding, row components, icons, labels, popover look, toolbar order,
adding Error peek / overview ruler / folding UI. Missing UI goes to `docs/NF2-REQUESTS.md`.

## 8. Known limits

- Analysis uses the compiler lexer for declarations, function scopes, multiline members, typed parameters/returns and
  bounded receiver inference. It does not provide full semantic/type-flow analysis, dynamic `with` lookup or complete
  E4X/cast/generic inference. Hover and definition data are read-only compiler APIs; no new editor UI is implied.
- The Preferences › Code editor switch and delay control automatic code hints; explicit hints remain available.
- Auto-format does not wrap, align or reflow, and keeps lines inside multi-line comments, strings and XML literals.
- Live check compiles the whole project; on very large projects it may need to move off the UI thread.
