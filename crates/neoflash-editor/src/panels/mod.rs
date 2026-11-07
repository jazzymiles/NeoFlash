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
//! | `properties_sound.rs` | Properties ▸ Sound section of a keyframe with a frame sound |
//! | `accessibility.rs` | Accessibility |
//! | `actions.rs` | Actions (script editor), Output, bottom-panel dispatch |
//! | `actions_sidebar.rs` | Actions sidebar: toolbox and Script navigator |
//! | `view_settings.rs` | View settings popover (rulers, guides & grid) |
//! | `ease.rs` | Ease (tween curve of the span under the playhead) |
//! | `scene.rs` | Scene (list, add, duplicate, delete, rename, reorder) |
//! | `motion_tween.rs` | Properties — Motion tween sections and motion-span editing helpers |
//! | `motion_editor.rs` | Motion editor (property tracks of a motion tween) |
//! | `history.rs` | History (undo/redo step list and clear) |
//! | `swatches.rs` | Swatches (recent, document, palette and gradient swatches) |
//! | `info.rs` | Info (selection bounds and live stage cursor / RGBA readout) |
//! | `project.rs` | Project (source/asset file tree, search, create/delete/publish) |
//! | `filters.rs` | Filters (display visibility, blend mode, bitmap caching and display filters) |
//! | `compiler_errors.rs` | Compiler errors (`Window ▸ Compiler errors`, `⌥F2`) |
//! | `code_snippets.rs` | Code snippets (`Window ▸ Code snippets`) |
//! | `movie_explorer.rs` | Movie Explorer (`Window ▸ Movie Explorer`) |
//! | `find_replace.rs` | Find and replace modal dialog (`Edit ▸ Find and replace`, `⌘F`) |
//! | `document_settings.rs` | Document settings modal dialog (`Modify ▸ Document…`, `⌘J`) |
//! | `export_audio.rs` | Export audio modal dialog (`File ▸ Export ▸ Export audio…`, `⌥⇧⌘A`) |
//! | `export_lottie.rs` | Export Lottie modal dialog (`File ▸ Export ▸ Export Lottie…`, `⌥⇧⌘L`) |
//! | `export_video.rs` | Export video modal dialog (`File ▸ Export ▸ Export video…`, `⌥⇧⌘V`) |
//! | `edit_envelope.rs` | Edit envelope modal dialog (`Properties ▸ Sound ▸ Envelope…`, `Library menu ▸ Edit envelope…`) |
//! | `layer_properties.rs` | Layer properties modal dialog (`Modify ▸ Timeline ▸ Layer properties…`) |
//! | `sound_properties.rs` | Sound properties modal dialog (`Properties ▸ Sound`, `Library menu`) |
//! | `symbols_popover.rs` | Breadcrumb `▾` `Symbols & scenes` popover |
//! | `planned.rs` | NF2 panels the engine does not support yet (read-only previews; bodies in `planned_*.rs`) |
pub(crate) mod accessibility;
mod actions;
pub(crate) mod actions_sidebar;
pub(crate) mod code_snippets;
pub(crate) mod compiler_errors;
mod debugger;
pub(crate) mod document_settings;
pub(crate) mod ease;
pub(crate) mod edit_envelope;
pub(crate) mod export_audio;
pub(crate) mod export_image;
pub(crate) mod export_lottie;
pub(crate) mod export_video;
pub(crate) mod filters;
pub(crate) mod find_replace;
pub(crate) mod font_embedding;
mod history;
pub(crate) mod import_video;
mod info;
pub(crate) mod layer_properties;
pub(crate) mod library;
pub(crate) mod live_planned;
mod menu;
mod motion_editor;
mod motion_tween;
pub(crate) mod movie_explorer;
mod paste_motion;
pub(crate) mod planned;
mod planned_color;
mod planned_dialogs;
mod planned_documents;
mod planned_drawing;
mod planned_panels;
pub(crate) mod preferences;
pub(crate) mod project;
pub(crate) mod properties;
mod properties_sound;
pub(crate) mod publish_settings;
pub(crate) mod scene;
pub(crate) mod sound_properties;
pub(crate) mod stroke_style;
mod swatches;
pub(crate) mod symbols_popover;
pub(crate) mod timeline;
mod timeline_controls;
mod tools;
mod view_settings;

mod brushes;
mod customize_tools;
mod trace_bitmap;
