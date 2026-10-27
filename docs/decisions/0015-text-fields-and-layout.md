# Shared text fields, uniform formatting and layout

Status: accepted for the first boxed text increment, 2026-10-27.

## Document and runtime ownership

Existing authored Text elements preserve their unboxed appearance and serialization. TextBox adds explicit width/height, multiline/wrapping flags and a uniform TextFormat. The Text tool creates wrapped multiline boxes; Properties can explicitly convert old Text, preserving its text, font, color and opacity. The unknown element variant causes old readers to reject unsupported documents rather than discard the field. No machine font path is serialized.

Dynamic TextField objects use the existing reference graph, event routing and publication lifetime. Their current format and insertion defaults are separate Rust state. Setting a new whole text value applies the defaults; a whole-field formatting update affects current content. Getters return copies. A partial insertion that would require mixed formatting fails atomically until character runs are implemented. AS2 uses createTextField and get/setNewTextFormat; its depth follows the existing append-only profile.

TextField width/height setters change the logical layout box independently of display scale. Existing transformed dimension getters and getBounds remain observable. A box is not an automatically responsive DOM element; explicit responsive authoring remains a separate part of the product.

## Shared line calculation, host font measurement

Rust owns word/space breaks, fallback splitting of overwide segments, UTF-16 line ranges, line positions and alignment. It requests at most three batches of measurements: segments, needed scalar fallback widths and final rendered lines. Native egui and browser Canvas supply widths using their actual selected fonts. Final line widths drive center/right alignment and text metrics. There is no per-prefix quadratic measurement loop or separate editable browser text model.

The bounded profile supports left/center/right alignment, font family/size/color, bold/italic, leading, multiline text and wrapping. New script-created fields begin at 100×100 with wrapping and multiline disabled. Boxed fields have two pixels of inset and clip to their box. Native glyph triangles clip before the affine transform, including rotated fields; Canvas uses its transformed clip. Native and browser layout caches are bounded to 8 MiB. Text is capped at 1 MiB per field, with explicit layout-work limits and a session override budget.

Native font-kit selection uses installed regular/bold/italic faces. Browser publication references CSS font families and retains its fallback warning. The shared algorithm does not make different font files or engines produce identical pixels or line breaks. Font embedding/subsetting, full shaping and historical Flash line metrics remain separate work.

## Evidence and remaining scope

One original text-layout publication combines authored boxed text, two independent AS3 fields, AS2 creation/defaults, width/alignment changes, visible metrics and Restart. One core family scenario and extensions to existing native workflows cover state ownership, layout reuse, reflow, rotated clipping and Undo. A shared regression pass and one browser workflow cover the delivery; no per-method acceptance loops.

HTML/CSS text, mixed-format runs, input/selection/IME, justified layout, autosizing, advanced paragraph formatting and font embedding remain explicit follow-ups. The framework keeps those requirements in the product goal; uniform fields do not establish full TextField/TextFormat compatibility. Live native interaction must be recorded separately from headless rendering evidence.
