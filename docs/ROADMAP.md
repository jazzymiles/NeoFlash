# Roadmap

NeoFlash aims to provide a complete Flash-inspired environment for websites, applications, games and animation, with HTML5 as the primary publishing target. It is currently a development preview. The priority is useful support across the main subsystems, followed by deeper conformance.

## Authoring

Timeline authoring includes compact controls, inline layer rename, frame zoom, folders, guides and vector masks. The workspace has four-sided docks with resizable tab groups; the library has preview, folders, search and linkage. SVG/PNG/JPEG graphics can be imported from files or dropped onto the stage. Next authoring work includes frame-range selection/clipboard, independent layer lengths, richer easing controls, button states, advanced mask sources and motion-path guides.

Further work includes snapping, richer drawing tools, general shape morphing, custom transform pivots, autosave and crash recovery. See [workspace behavior](WORKSPACE.md) for the current controls.

## Remaining product work

| Area | Next capabilities and limits to resolve |
| --- | --- |
| Text and media | Embedded/subset fonts, portable layout, mixed formatting, complete IME behavior and native audio/video timeline synchronization. Scripted file playback does not provide timeline media authoring. |
| Interactive web | Responsive DOM layouts, accessible components/forms, breakpoints, JS/TS project modules and broader browser integration. Existing Canvas and scripted text-input profiles remain bounded. |
| AS2 and AS3 | Deeper language/coercion/prototype conformance, class initialization and reflection, remaining XML/E4X operations, display/event lifetime semantics and unsupported platform APIs. Partial support remains distinct from full compatibility. |
| Legacy projects | Broader XFL and binary FLA conversion, SWF AVM1/AVM2 import/playback strategy and repeatable per-file diagnostics. The single accepted external CS6 example is not general FLA import. |
| Platform and release quality | Performance and memory budgets, accessibility, Intel macOS verification, document recovery/migration, signed and notarized distribution, and extension interfaces. |

Individual compatible algorithms may be adapted into the shared Rust core with their licenses and provenance preserved. Complete third-party ActionScript engines are not embedded. Unsupported content must produce actionable diagnostics rather than being silently dropped.

## Coverage and validation

The [compatibility matrix](COMPATIBILITY.md) describes current behavior. The existing [capability registry](progress/DETAILS.md) and [complete ActionScript declaration audit](progress/ACTIONSCRIPT.md) track scope and evidence; no overall Flash-completion percentage is implied.

Representative editor and HTML5 workflows establish user-visible behavior; compiler tests and successful exports alone do not establish visual acceptance. [Verification scope](VERIFICATION.md) records observed boundaries, and the [backlog](BACKLOG.md) tracks concrete unresolved issues. These work areas can progress together; no completion date is promised.
