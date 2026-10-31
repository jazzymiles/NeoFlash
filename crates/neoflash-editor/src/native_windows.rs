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
    use objc2_app_kit::{NSApplication, NSWindow, NSWindowCollectionBehavior, NSWindowTabbingMode};
    use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

    static ROOT: AtomicIsize = AtomicIsize::new(0);
    static SEEN: AtomicUsize = AtomicUsize::new(0);

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

    /// Configure windows created since the last call. Cheap when nothing changed.
    pub fn configure_panels() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        let windows = app.windows();
        let count = windows.len();
        if SEEN.swap(count, Ordering::Relaxed) == count {
            return;
        }
        let root = ROOT.load(Ordering::Relaxed);
        for window in windows.iter() {
            if window.windowNumber() == root {
                continue;
            }
            window.setTabbingMode(NSWindowTabbingMode::Disallowed);
            let behavior = (window.collectionBehavior()
                - NSWindowCollectionBehavior::FullScreenPrimary)
                | NSWindowCollectionBehavior::FullScreenAuxiliary;
            window.setCollectionBehavior(behavior);
        }
    }
}

#[cfg(all(target_os = "macos", not(test)))]
pub use imp::{configure_panels, install};

#[cfg(any(not(target_os = "macos"), test))]
pub fn install() {}

#[cfg(any(not(target_os = "macos"), test))]
pub fn configure_panels() {}
