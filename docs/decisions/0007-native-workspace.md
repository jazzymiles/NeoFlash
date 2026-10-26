# 0007 — Workspace preferences and native panels

Status: accepted, 2026-10-27.

## Context

The authoring workflow needs readable Light/Dark appearances, compact timelines and tools, and Tools/Timeline/Actions that can move outside the main window. Workspace changes must preserve authored content, active tools, selections and script cursor state. Hiding a panel must not strand its controls.

## Decision

Keep one Editor and one Project. Render shared panel content functions either in their normal docks or in egui immediate native viewports. Give content, scrolling and the target-specific Actions TextEdit stable IDs across reparenting. Closing a child hides its panel; Window/F9 can reveal it; Dock and Restore Workspace return it to the main window. Deferred text clipboard commands retain both their authored target and their originating viewport.

Persist only bounded, versioned workspace preferences through eframe Storage: theme, panel visibility, active dock, dock sizes and three detached-window geometries. Do not serialize arbitrary egui memory or script buffers. Unknown/malformed preference versions fall back safely. Theme/placement changes never enter Project history. Restore resets layout, preserving theme and document state.

Use semantic palette tokens for ordinary widgets and manual timeline/stage painting. Strong text must be tested through egui's actual Visuals API because it derives its color from active widgets. Use original vector icons and fixed logical-point tween arrowheads clipped to their row. A shared timeline scroll body keeps layer labels aligned with keys; compact toolbars must remain reachable by wrapping or scrolling.

Store child geometry in OS logical coordinates. On macOS, safe NSScreen enumeration supplies all display work areas; coordinate conversion uses the primary screen, not the currently focused screen. Recover inaccessible title bars on creation or a display-topology change, without continuously constraining normal user movement. Do not overwrite recovered positions with the stale input snapshot from the same repaint.

Keep narrowly documented, removable patches to the pinned native integration crates. The egui-winit patch preserves paste-key modifiers. The eframe wgpu patch initializes the existing AccessKit adapter for every child window; upstream already routes its events and tree updates by window ID. Both retain upstream licenses and provenance under vendor/.

## Consequences and boundaries

Native windows can move beyond the root window and retain the shared authoring state. Only three panels currently detach. Drag-to-dock, arbitrary splits/tabs, named workspaces and multiple documents remain later work. Full VoiceOver/IME and physical multi-monitor hot-unplug acceptance are separate from headless state/geometry tests.

Verification combines layout and immediate-viewport input tests, real macOS interaction, application restart and preference inspection. Rendering child widgets is insufficient evidence of accessibility: their native tree and actions must also be checked.
