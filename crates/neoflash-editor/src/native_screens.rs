//! Native monitor enumeration; no window mutation or document access.
use crate::workspace::ScreenRect;

/// Empty means the caller must use its known root-window rectangle as fallback.
/// Cocoa screens use global OS logical points, regardless of Retina scale.
#[cfg(target_os = "macos")]
pub fn available_work_areas() -> Vec<ScreenRect> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    let Some(main_thread) = MainThreadMarker::new() else {
        return Vec::new();
    };
    let screens = NSScreen::screens(main_thread);
    // NSScreen.screens[0] contains the menu bar and defines the desktop origin.
    // NSScreen.mainScreen instead follows the active window: never use it here.
    let Some(primary) = screens.iter().next() else {
        return Vec::new();
    };
    let frame = primary.frame();
    let primary_top = frame.origin.y + frame.size.height;
    if !primary_top.is_finite() {
        return Vec::new();
    }
    screens
        .iter()
        .take(64)
        .filter_map(|screen| {
            let visible = screen.visibleFrame();
            cocoa_work_area(
                primary_top,
                [visible.origin.x, visible.origin.y],
                [visible.size.width, visible.size.height],
            )
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub fn available_work_areas() -> Vec<ScreenRect> {
    Vec::new()
}

#[cfg(any(target_os = "macos", test))]
fn cocoa_work_area(primary_top: f64, origin: [f64; 2], size: [f64; 2]) -> Option<ScreenRect> {
    let area = ScreenRect {
        origin: [origin[0] as f32, (primary_top - origin[1] - size[1]) as f32],
        size: [size[0] as f32, size[1] as f32],
    };
    area.is_valid().then_some(area)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cocoa_conversion_uses_primary_top_and_keeps_negative_monitor_coordinates() {
        assert_eq!(
            cocoa_work_area(900., [0., 50.], [1440., 826.]),
            Some(ScreenRect {
                origin: [0., 24.],
                size: [1440., 826.]
            })
        );
        assert_eq!(
            cocoa_work_area(900., [-1920., 0.], [1920., 1080.]),
            Some(ScreenRect {
                origin: [-1920., -180.],
                size: [1920., 1080.]
            })
        );
        assert_eq!(
            cocoa_work_area(900., [0., 900.], [1440., 900.]),
            Some(ScreenRect {
                origin: [0., -900.],
                size: [1440., 900.]
            })
        );
        assert!(cocoa_work_area(f64::NAN, [0., 0.], [1440., 900.]).is_none());
        assert!(cocoa_work_area(900., [f64::MAX, 0.], [1440., 900.]).is_none());
    }
}
