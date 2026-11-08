//! macOS behaviour of detached panel windows.
//!
//! Detached panels are auxiliary windows: they keep their own size and position and never become
//! full-screen or a tab of the main window. Without this, macOS opens every new window as a tab of
//! a full-screen main window ("Prefer tabs when opening documents: In Full Screen"), so an undocked
//! panel filled the whole screen.
//!
//! * Automatic window tabbing is disabled for the whole app before any panel window exists.
//! * Every window except the main one gets `tabbingMode = Disallowed` and the collection behaviour
//!   `FullScreenAuxiliary` (may appear over a full-screen main window) instead of `FullScreenPrimary`
//!   (the green button no longer turns a panel into a full-screen space).

#[cfg(all(target_os = "macos", not(test)))]
mod imp {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSApplication, NSFloatingWindowLevel, NSWindow, NSWindowCollectionBehavior,
        NSWindowStyleMask, NSWindowTabbingMode, NSWindowTitleVisibility,
    };
    use std::sync::atomic::{AtomicIsize, Ordering};

    static ROOT: AtomicIsize = AtomicIsize::new(0);

    /// Call once while only the main window exists (from `Editor::new`).
    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
        let app = NSApplication::sharedApplication(mtm);
        if let Some(root) = app.windows().iter().next() {
            ROOT.store(root.windowNumber(), Ordering::Relaxed);
        }
    }

    /// Configure detached panel windows so they have no native OS title bar or traffic-light
    /// buttons, stay above the main Stage window while NeoFlash is active, hide on app
    /// deactivation, and never merge into full-screen tabs.
    pub fn configure_panels() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        let windows = app.windows();
        let root = ROOT.load(Ordering::Relaxed);
        for window in windows.iter() {
            if window.windowNumber() == root {
                continue;
            }
            let mask = (window.styleMask()
                - NSWindowStyleMask::Titled
                - NSWindowStyleMask::Closable
                - NSWindowStyleMask::Miniaturizable
                - NSWindowStyleMask::FullSizeContentView)
                | NSWindowStyleMask::Borderless
                | NSWindowStyleMask::Resizable;
            if window.styleMask() != mask {
                window.setStyleMask(mask);
            }
            if window.titleVisibility() != NSWindowTitleVisibility::Hidden {
                window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
            }
            if !window.titlebarAppearsTransparent() {
                window.setTitlebarAppearsTransparent(true);
            }
            if !window.hasShadow() {
                window.setHasShadow(true);
            }
            if window.tabbingMode() != NSWindowTabbingMode::Disallowed {
                window.setTabbingMode(NSWindowTabbingMode::Disallowed);
            }
            if !window.hidesOnDeactivate() {
                window.setHidesOnDeactivate(true);
            }
            if window.level() != NSFloatingWindowLevel {
                window.setLevel(NSFloatingWindowLevel);
            }
            let behavior = (window.collectionBehavior()
                - NSWindowCollectionBehavior::FullScreenPrimary)
                | NSWindowCollectionBehavior::FullScreenAuxiliary;
            if window.collectionBehavior() != behavior {
                window.setCollectionBehavior(behavior);
            }
        }
    }
}

#[cfg(all(target_os = "macos", not(test)))]
pub use imp::{configure_panels, install};

#[cfg(any(not(target_os = "macos"), test))]
pub fn install() {}

#[cfg(any(not(target_os = "macos"), test))]
pub fn configure_panels() {}
