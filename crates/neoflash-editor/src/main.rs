mod app;
mod authoring_modify;
mod authoring_symbols;
mod authoring_tools;
mod bitmaps;
mod clipboard;
mod code_tools;
mod detached;
mod dock_grid;
mod fonts;
mod graphic_import;
mod layer_controls;
mod native_screens;
mod native_windows;
mod paint_controls;
mod panels;
pub(crate) use panels::library;
mod preview;
mod resources;
mod script_api;
mod selection;
mod shell;
mod snapping;
mod sources;
mod stage;
mod stage_clipping;
mod stage_primitives;
mod stage_selection;
mod stage_view;
mod svg_graphics;
mod symbol_dialog;
mod text_boxes;
mod theme;
mod timeline_commands;
mod timeline_edit;
mod timeline_motion;
mod timeline_paint;
mod timeline_preview;
mod timeline_sound;
mod transform_geometry;
mod ui_icons;
mod vector;
mod workspace;

#[cfg(test)]
mod clipboard_tests;
#[cfg(test)]
mod detached_tests;
#[cfg(test)]
mod panel_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod stage_view_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod timeline_commands_tests;
#[cfg(test)]
mod timeline_motion_tests;
#[cfg(test)]
mod timeline_panels_tests;
#[cfg(test)]
mod timeline_sound_tests;
#[cfg(test)]
mod transform_gesture_tests;
#[cfg(test)]
mod vector_gesture_tests;
#[cfg(test)]
mod workspace_editor_tests;

#[test]
#[cfg(target_os = "macos")]
fn native_build_includes_metal_backend() {
    assert!(wgpu::Instance::enabled_backend_features().contains(wgpu::Backends::METAL));
}

fn main() -> eframe::Result {
    // `NEOFLASH_WINDOW=1920x1080` fixes the window size (screenshots, design comparison) and stops
    // eframe from restoring the last window geometry.
    let fixed = shell::fixed_window_size();
    eframe::run_native(
        "NeoFlash",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_title("NeoFlash — Classic workspace")
                .with_inner_size(fixed.unwrap_or([1440.0, 960.0]))
                .with_min_inner_size([1024.0, 720.0]),
            persist_window: fixed.is_none(),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::Editor::new(cc)))),
    )
}
