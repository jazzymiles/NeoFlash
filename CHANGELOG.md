# Changelog

## Unreleased

- Added a licensed public FLA regression corpus: 15 unchanged external source files with explicit refusal diagnostics, plus three original XFL/FLA projects with exact native imports and actual-WASM behavior checks.

- Added persistent Light/Dark workspace themes, panel visibility, F9 reveal and layout reset; Tools, Timeline and Actions can detach into native windows and dock again without copying the document. Window recovery accounts for connected macOS displays.
- Replaced missing-glyph controls with accessible vector icons, bounded tween arrowheads to their own rows, synchronized layer/frame vertical scrolling, and made compact panels scroll or wrap.
- Kept Actions cursor state through panel relocation and bound deferred text clipboard commands to their originating viewport.

- Added structured ActionScript switch/do control flow, AS3 labels, synchronous exceptions and selected typed error support, with explicit per-language boundaries and executable cleanup/scope regressions.
- Fixed HTML5 failure reporting for arbitrary thrown values; added an editable AS2/AS3 recovery example and integration checks through the actual compiler, browser host and WASM Player.
- Preserved initiating paste-key modifiers in a pinned egui-winit adapter, removing the Shift/V release-order ambiguity while retaining one text paste event.
- Added an opt-in Flash 3 solid-contour adapter with exact endpoint assembly, affine path coordinates, bounded topology certification and provenance. General binary FLA document conversion remains blocked.

- Added object Copy/Cut/Paste and Paste in Place across frames/documents, preserving sampled placements, shared nested symbols, timeline scripts and accessibility metadata. Complete candidate validation precedes each paste; text-focused commands retain their recipient and pause playback.
- Added opt-in typed binary FLA timelines with archive identity, explicit child ownership, partial record fields and shape associations. Added bounded Flash 3 publish/compatibility suffix reading to exact EOF and proven empty historical-action envelopes. Editable binary conversion remains blocked.
- Added cross-layer multi-selection, Shift selection, marquee, Select All, duplicate/delete and grouped Undo; V remains the selection tool and Q provides Free Transform.
- Added rotated/reflected resize handles, center/proportional resizing, continuous rotation and uniform multi-object transforms. Gestures preserve sampled keyframe poses and avoid creating keys or Undo entries for unchanged results.
- Fixed final-release keyframe movement, fast pointer batches, selection/drawing lifecycle across frame/tool/view changes, Undo menu availability for pending Pen paths, and native rectangle/ellipse strokes under nested transforms.
- Added bounded binary FLA Contents descriptors, shared MFC identities and raw document settings, preserving unread bytes through an opt-in API. The private corpus reaches settings in all 19 accepted containers; editable binary conversion remains blocked.
- Added ActionScript function declarations and expressions, lexical locals, recursion, closures and selected AS3 argument/return coercions, with explicit diagnostics for unsupported function semantics.
- Added optional authored names, descriptions, Button/Image roles and focus order to placements, an Accessibility panel and lifetime-checked activation in the shared runtime.
- Added semantic HTML5 controls, persistent keyboard focus, visible script trace output, and Fit/100%/zoom/pan preview controls. The original keyboard example exercises independent AS3 instance counters.
- Added bounded preservation of binary FLA shape matrices, fills, strokes, edge deltas and auxiliary curves. Inspection still does not claim editable document conversion.
- Added shared line/quadratic/cubic vector paths, compound contours, fill rules, geometry validation, analytic bounds and bounded hit testing.
- Added desktop pen, pencil and point editing with grouped undo, plus native fill/stroke tessellation and HTML5 Path2D rendering.
- Added interpolation for paths with matching command topology and a shareable vector example. Arbitrary Flash shape tweening remains unsupported.
- Added strict binary FLA container/stream inspection, save-target revision detection and bounded MFC timeline structure reading. All binary FLA editable conversions remain explicitly blocked until their contents can be preserved.

## 0.1.0 — development preview, 2026-10-26

- Started the open-source Rust workspace for macOS 15+ with a classic Flash-inspired editor.
- Added versioned documents, library symbols, instance transforms, nested timelines, keyframes, easing and deterministic authored evaluation.
- Added undo/redo, JSON save/open, basic shapes/text and installed-font discovery.
- Added Actions, JS validation, TS transformation and an explicitly bounded AS2/AS3 parser.
- Added shared Rust/WASM playback and standalone HTML5 publication with script diagnostics.
- Added initial legacy format inspection and a strict documented XFL import subset.
- Added unit/integration tests, build/check/package scripts, CI configuration and product documentation.

This preview does not provide full Flash UI/tool parity, complete AS2/AS3, general FLA/SWF import, font embedding, DOM layout or production packaging.
