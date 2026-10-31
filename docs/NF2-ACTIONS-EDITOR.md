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
| Gutter markers, compiler messages, problem count | **working (existing)** | Now refreshed by a silent live check 0.6 s after typing stops. Explicit **Check** still writes to Output. |
| Toolbar › Debug options, Collapse between braces, Expand all; Script assist | disabled, "Not available yet" | Need debugger/source maps, code folding, structured editing (plan §4). |
| Error peek + "Quick fix ⌘.", overview ruler, warning markers, pinned script tabs | not shown | Need diagnostic severity/codes/fixes from the compiler and NF2 components (plan §2–3). |

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
crates/neoflash-editor/assets/actionscript-api.json   (versioned, ~340 KB, embedded with include_str!)
            │
            ▼  crates/neoflash-editor/src/script_api.rs  (parsed once, OnceLock)
toolbox tree · code hints · class support tooltips
            +
Project::sources  ──scan_class() on every request──►  "Project classes" folder + hints (no build step)
```

**Built-in API.** When a declaration's status changes in the catalogue, the editor picks it up on the next index
build. Nothing in the editor code lists class or member names.

- `python3 scripts/progress.py --write-reports` now also runs `scripts/editor-api-index.py` when the local reference
  snapshot exists. Codex already runs this command after every implementation change (AGENTS.md), so new support
  flows into the toolbox and hints with no extra step.
- Without the reference snapshot, `progress.py` and `scripts/check.sh` run `editor-api-index.py --check`, which
  compares the catalogue SHA-256 recorded in the asset with the current catalogue and fails with the command to run.
- The generator fails loudly if a ready catalogue entry has no reference signature (currently 0 of 2,842).

**Project classes.** `script_api::scan_class` reads `package`, `class`/`interface`, `extends`, and public/internal
`function`, `get`/`set`, `var`, `const` declarations (comments stripped; `private` skipped). A class written in
Project files appears immediately under "Project classes", resolves in `var h:Hero`, `Hero.` (statics) and inherits
built-in members through `extends`.

### Asset schema (`schemaVersion` 1)

```json
{
  "catalogSha256": "…", "generatedBy": "scripts/editor-api-index.py", "schemaVersion": 1,
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
| `crates/neoflash-editor/assets/actionscript-api.json` | Generated asset. Never edit by hand. |
| `crates/neoflash-editor/src/script_api.rs` | Index loading; `HintContext` (receiver resolution, inheritance, hints); project-class scan; toolbox folders, own members, search; AS3 `missing_import`. Unit tests at the bottom. |
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
   right: `this`, `super`, a `name:Type` declaration earlier in the script, a stage instance, a class name
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

### 5.4 Live check

`live_check` records the time of the last text change per editor id and, 0.6 s later, runs the same compile as Check
(`check_action_source` / `check_project_sources`) with `announce = false`, so Output is not flooded. Results are
cached by the existing fingerprint (`code_feedback`), which already includes all project sources and linkage.

## 6. Tests

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

- Hints are lexical: no inference through function return values of user functions, array elements, casts or
  `with`; `a[i].` and `f().` give no hints. A compiler language service removes these limits.
- `scan_class` is line-based: multi-line member declarations and nested/inner classes are not scanned.
- Code hints delay and the on/off switch from Preferences › Code editor are not implemented (no Preferences dialog);
  hints open immediately.
- Auto-format does not wrap, align or reflow, and keeps lines inside multi-line comments, strings and XML literals.
- Live check compiles the whole project; on very large projects it may need to move off the UI thread.
