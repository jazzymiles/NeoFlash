mod accessibility;
mod app;
mod fonts;
mod panels;
mod selection;
mod stage;
mod stage_primitives;
mod stage_selection;
mod transform_geometry;
mod vector;

#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transform_gesture_tests;
#[cfg(test)]
mod vector_gesture_tests;

#[test]
#[cfg(target_os = "macos")]
fn native_build_includes_metal_backend() {
    assert!(wgpu::Instance::enabled_backend_features().contains(wgpu::Backends::METAL));
}

fn main() -> eframe::Result {
    eframe::run_native(
        "NeoFlash",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_title("NeoFlash — Classic workspace")
                .with_inner_size([1440.0, 960.0])
                .with_min_inner_size([1024.0, 720.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::Editor::new(cc)))),
    )
}
