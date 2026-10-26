# Backlog

Priority is based on the stated product goal, with explicit evidence required for completion.

## P0 — first working foundation

- [x] Complete recorded desktop and browser smoke checks; fix discovered defects.
- [x] Publish the tested initial source tree to the authorized public GitHub repository.
- [x] Inspect the user's local legacy corpus and record aggregate format/compatibility results.

## P1 — essential authoring quality

- [x] Cross-layer multi-selection, Shift/marquee/Select All, whole-selection duplicate/delete and V/Q object editing, with automated native-input regression coverage. Live native visual checks of this increment remain pending.
- Custom registration points/pivots, skew, nonuniform multi-object transforms, groups and cut/copy/paste across frames and documents.
- Drag/copy/reverse frame spans, keyframe movement, multi-frame edit and layer folders/reordering.
- [x] Bezier paths, pen/pencil, direct point editing, compound fills and HTML5 geometry.
- Brush/pressure, spline fitting, point insertion/deletion, tangent constraints, shape splitting/merging, gradients, masks and guides.
- [x] Tessellate native rectangle/ellipse fills and strokes before applying their full affine transforms, matching the Canvas geometry convention. Tests cover scaled, reflected and nested transforms; live native visual comparison remains pending.
- Autosave/recovery, file-change detection, unsaved project protection and multiple documents.
- Display and edit custom easing curves visually, including overshoot; property-level motion curves.
- Timeline virtualization and resource-aware undo for large documents.
- System fonts: weight/style/variable axes, shaping/fallback and refresh after font activation.
- Font embedding/subsetting and outline export with explicit font policies.

## P1 — universal HTML5 and scripting

- DOM layer with semantic text, inputs, keyboard focus and responsive layout.
- [x] D-001 HTML5 criteria: authored Button/Image metadata, independent semantic targets, Tab focus and Enter/Space activation exactly once, verified in Chrome and independently in the in-app browser. Live native-panel authoring and VoiceOver remain pending; this does not close general web accessibility.
- [x] D-002 preview criteria: Fit/100%/zoom/pan at 390×844, authored geometry preserved and playback controls reachable; verified with the keyboard example. Touch-device and Safari checks remain pending.
- JS/TS module imports, type checking, code completion, diagnostics navigation and debugger.
- Full AS2/AS3 parser/type/coercion conformance, classes/prototypes, E4X/namespaces and API mappings.
- [x] Function declarations/expressions, lexical locals, recursion and closures in the documented ActionScript subset; executable coercion/evaluation-order regressions.
- Sound/video/image import; synchronization, browser input and lifecycle.
- Runtime event conformance for nested clips, construction, re-entry, bubbling, scope and removed objects.

## P1 — legacy

- Extend XFL geometry, library assets, masks, tweens, color transforms and text formatting with fixtures.
- [x] Binary FLA CFB validation, stream inventory, save-target detection and bounded MFC structure inspection against the private corpus.
- [x] Preserve bounded binary shape/style/edge records with exact source bytes and aggregate-only inspection; verify the private corpus without publishing artwork.
- [x] Preserve bounded Contents prefixes, scene/library/media descriptors, MFC identities and raw settings; resolve all 289 descriptors in the 19 accepted local containers while retaining unread suffix bytes.
- Binary FLA lossless document conversion: Contents extensions/suffix, interpreted linkage, contour assembly, object/media payloads and historical actions. Investigate one corpus container rejected by strict directory validation.
- Evaluate Ruffle as optional SWF playback/compatibility module. Keep new HTML5 publication independent of mandatory SWF.
- Reproducible private-corpus tests: per-file diagnostics, rendered frames, input scenarios, comparison provenance.

## P2 — product finish

- Native macOS menus/document integration, shortcuts, accessibility tree, IME, trackpad conventions.
- Localization, workspace layouts, panels/docking, performance profiling and extension API.
- Signed/notarized releases, Intel build verification, browser support matrix and updater policy.
