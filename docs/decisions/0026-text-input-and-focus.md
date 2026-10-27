# Text input, focus and selection

Status: accepted for the bounded HTML5 input profile, 2026-10-27.

## Shared committed state, native editing

TextField keeps its input/dynamic type, selectability, password flag, maxChars/restrict and UTF-16 selection in Rust. Programmatic text changes and selection replacements are atomic patches. A separate user-edit transaction checks the expected revision before applying insertion restrictions and length limits. Script assignment/replacement bypasses user-only filters. Limits and complete Unicode boundaries remain explicit; there is no second persistent browser text model.

The HTML5 host uses a native single-line input or multiline textarea while a field is focused. The browser handles transient composition, paste and native selection; committed edits pass through the Rust transaction. Canvas resumes drawing when editing ends and suppresses the active field's duplicate glyphs. The overlay follows the display transform and shared clip geometry. DOM font shaping and scroll behavior are browser-specific and do not establish exact Flash text metrics.

AS3 exposes TextFieldType, TextEvent and FocusEvent alongside the existing EventDispatcher. Cancellable textInput precedes insertion, change follows actual user mutation, and programmatic changes do not invent input events. Stage.focus and the AS2 Selection singleton share one focus owner and selection. Legacy onChanged/onSetFocus/onKillFocus adapters receive the same transitions. Restart removes transient controls and invalidates old display references.

## Bounded first delivery

The profile targets ordinary text forms. It includes common range/exclusion/escape restrict syntax (without automatic legacy case substitution), user length limits, selection replacement, password masking and named dynamic fields. It does not claim mixed formatting, HTML/CSS text, original IME event fidelity, embedded fonts, every grapheme/locale rule, or full native-editor input authoring. Native password editing requires a single-line field. Foreground Canvas artwork cannot occlude an active DOM editor; this layering boundary must remain visible in compatibility notes.

One two-field form combines typing, cancellation, filtering, focus/selection, both language adapters and local saved state. Existing compiler and text scenarios are extended; a single real-WASM form workflow and one browser acceptance cover the family. Original-player differential and broader browser/IME matrices remain separate evidence.

References: [TextField](https://airsdk.dev/reference/actionscript/3.0/flash/text/TextField.html), [TextEvent](https://airsdk.dev/reference/actionscript/3.0/flash/events/TextEvent.html), [FocusEvent](https://airsdk.dev/reference/actionscript/3.0/flash/events/FocusEvent.html), [AS2 Selection](https://open-flash.github.io/mirrors/as2-language-reference/Selection.html).
