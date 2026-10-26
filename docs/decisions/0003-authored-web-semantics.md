# 0003 — authored web semantics and keyboard activation

Status: accepted and implemented for the documented initial roles, 2026-10-26. Native visual and assistive-technology checks remain as recorded in VERIFICATION.md.

The editor's object model must support interactive web applications as well as visual animation. A Canvas display list alone does not describe individual actions to assistive technology. Each authored element can therefore carry optional accessibility metadata: name, description, role, reading/focus order and disabled state. The first roles are Button and Image. This is a foundation for semantic web components, not a substitute for the planned DOM text, input and responsive-layout model.

Metadata belongs to a placement, including symbol instances, and can vary at keyframes. Existing projects omit the field and retain their rendering behavior. Names and descriptions are bounded and names must not be blank. The editor offers a dedicated Accessibility panel. Converting an object to a symbol keeps one semantic target on the new instance rather than copying it into the symbol's content as well.

The shared Rust runtime exposes active semantic targets, ordered by authored order with stable display-order ties. Targets with zero effective opacity or a singular world transform are excluded. Disabled Button ancestors propagate disabled state. Every exposed target carries its placement lifetime, so a removed object and a replacement at the same path are distinguishable.

A dedicated activation API checks current role, visibility, disabled state and lifetime, then queues the same Click event used by pointer input. The browser host represents authored buttons with native DOM buttons; native Enter/Space activation should produce one event, without a second keyboard handler synthesizing a duplicate. Images expose descriptive semantics without joining keyboard focus order. Stable buttons should keep focus while their geometry moves. Focus bounds come from the current display list; pointer hit tests continue to use the actual rendered geometry.

Preview zoom changes viewing scale only. Fit, 100%, scroll/pan and playback controls must stay usable on a narrow viewport without modifying authored coordinates or silently treating a fixed stage as a responsive layout.

Verification must include actual keyboard interaction in a browser, two independently named controls, visible focus, transformed/nested geometry, disabled and removed targets, and a narrow viewport. Automated tests of serialization or generated markup alone do not prove usability with assistive technology. Screen-reader behavior, more semantic roles, rich controls and responsive authoring remain separate work.
