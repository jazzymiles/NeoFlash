# Backlog

Priority is based on the stated product goal, with explicit evidence required for completion.

## P0 — first working foundation

- [x] Complete recorded desktop and browser smoke checks; fix discovered defects.
- [ ] Publish the tested initial source tree to the authorized public GitHub repository.
- [x] Inspect the user's local legacy corpus and record aggregate format/compatibility results.

## P1 — essential authoring quality

- Multi-selection, marquee, transform handles and registration points; cut/copy/paste across frames and documents.
- Drag/copy/reverse frame spans, keyframe movement, multi-frame edit and layer folders/reordering.
- Bezier paths, pen/brush/pencil, fill/stroke editing, shape splitting/merging, gradients, masks and guides.
- Autosave/recovery, file-change detection, unsaved project protection and multiple documents.
- Display and edit custom easing curves visually, including overshoot; property-level motion curves.
- Timeline virtualization and resource-aware undo for large documents.
- System fonts: weight/style/variable axes, shaping/fallback and refresh after font activation.
- Font embedding/subsetting and outline export with explicit font policies.

## P1 — universal HTML5 and scripting

- DOM layer with semantic text, inputs, keyboard focus and responsive layout.
- JS/TS module imports, type checking, code completion, diagnostics navigation and debugger.
- Full AS2/AS3 parser/type/coercion conformance, classes/prototypes, E4X/namespaces and API mappings.
- Sound/video/image import; synchronization, browser input and lifecycle.
- Runtime event conformance for nested clips, construction, re-entry, bubbling, scope and removed objects.

## P1 — legacy

- Extend XFL geometry, library assets, masks, tweens, color transforms and text formatting with fixtures.
- Binary FLA reader; identify versions from actual source corpus.
- Evaluate Ruffle as optional SWF playback/compatibility module. Keep new HTML5 publication independent of mandatory SWF.
- Reproducible private-corpus tests: per-file diagnostics, rendered frames, input scenarios, comparison provenance.

## P2 — product finish

- Native macOS menus/document integration, shortcuts, accessibility tree, IME, trackpad conventions.
- Localization, workspace layouts, panels/docking, performance profiling and extension API.
- Signed/notarized releases, Intel build verification, browser support matrix and updater policy.
