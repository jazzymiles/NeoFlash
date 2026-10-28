//! Read-only native screen/pointer coordinates; no window mutation or document access.
use crate::workspace::ScreenRect;

/// Continue an already captured panel gesture after the pointer leaves its
/// viewport. Winit may omit MouseInput release after CursorLeft clears its
/// local pointer position. AppKit supplies global logical points, not pixels.
#[cfg(all(target_os = "macos", not(test)))]
pub fn pointer_state() -> Option<(eframe::egui::Pos2, bool)> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen};
    let main_thread = MainThreadMarker::new()?;
    let screens = NSScreen::screens(main_thread);
    let primary = screens.iter().next()?;
    let primary_frame = primary.frame();
    let point = NSEvent::mouseLocation();
    let position = eframe::egui::pos2(
        point.x as f32,
        (primary_frame.origin.y + primary_frame.size.height - point.y) as f32,
    );
    position
        .is_finite()
        .then(|| (position, NSEvent::pressedMouseButtons() & 1 != 0))
}

#[cfg(any(not(target_os = "macos"), test))]
pub fn pointer_state() -> Option<(eframe::egui::Pos2, bool)> {
    None
}

/// Frontmost native content rectangle under a desktop point. `None` means the
/// platform cannot answer; `Some(None)` means desktop or another application.
/// This prevents a drop from going through a floating window into the root.
#[cfg(all(target_os = "macos", not(test)))]
pub fn front_window_at(position: eframe::egui::Pos2) -> Option<Option<eframe::egui::Rect>> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSScreen, NSWindow};
    let main_thread = MainThreadMarker::new()?;
    let screens = NSScreen::screens(main_thread);
    let primary = screens.iter().next()?.frame();
    let primary_top = primary.origin.y + primary.size.height;
    let mut cocoa_point = NSEvent::mouseLocation();
    cocoa_point.x = f64::from(position.x);
    cocoa_point.y = primary_top - f64::from(position.y);
    let number =
        NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(cocoa_point, 0, main_thread);
    let app = NSApplication::sharedApplication(main_thread);
    let Some(window) = app.windowWithWindowNumber(number) else {
        return Some(None);
    };
    if !window.isVisible() || window.isMiniaturized() {
        return Some(None);
    }
    let content = window.contentRectForFrameRect(window.frame());
    let area = cocoa_work_area(
        primary_top,
        [content.origin.x, content.origin.y],
        [content.size.width, content.size.height],
    )?;
    Some(Some(eframe::egui::Rect::from_min_size(
        eframe::egui::pos2(area.origin[0], area.origin[1]),
        eframe::egui::vec2(area.size[0], area.size[1]),
    )))
}

#[cfg(any(not(target_os = "macos"), test))]
pub fn front_window_at(_position: eframe::egui::Pos2) -> Option<Option<eframe::egui::Rect>> {
    None
}

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
