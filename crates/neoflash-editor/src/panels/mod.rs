//! One file per editor panel. Every panel is drawn only with `neoflash_ui` (NF2) components and the
//! editor roles in `crate::theme`; see docs/DESIGN-SYSTEM.md › "Panel files".
//!
//! | File | Panel(s) |
//! |---|---|
//! | `menu.rs` | Application menu bar inside the top bar |
//! | `timeline.rs` | Timeline (layer column, ruler, frames, playhead) |
//! | `timeline_controls.rs` | Timeline controls bar, onion-skin popover, frame view menu |
//! | `tools.rs` | Tools |
//! | `library.rs` | Library |
//! | `properties.rs` | Properties (inspector) |
//! | `accessibility.rs` | Accessibility |
//! | `actions.rs` | Actions (script editor), Output, bottom-panel dispatch |
//! | `actions_sidebar.rs` | Actions sidebar: toolbox and Script navigator |
//! | `view_settings.rs` | View settings popover (rulers, guides & grid) |
//! | `planned.rs` | NF2 panels the engine does not support yet (read-only previews; bodies in `planned_*.rs`) |
pub(crate) mod accessibility;
mod actions;
pub(crate) mod actions_sidebar;
pub(crate) mod library;
mod menu;
pub(crate) mod planned;
mod planned_color;
mod planned_dialogs;
mod planned_documents;
mod planned_drawing;
mod planned_motion;
mod planned_panels;
pub(crate) mod properties;
mod timeline;
mod timeline_controls;
mod tools;
mod view_settings;
