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
//! * Detached studio panels and palettes (`Actions`, `Properties`, `Library`, `Output`, `Tools`,
//!   `Color`, `Swatches`, `Align`, `Transform`, `Info`, `Brushes`, etc.) use
//!   `NSFloatingWindowLevel` with `hidesOnDeactivate = true` so clicking the Stage or Timeline
//!   never buries them behind the main window, while switching to another app (e.g. Safari) hides
//!   them cleanly; ALL detached windows temporarily drop to `NSNormalWindowLevel` (and order below
//!   the root window for root-viewport modals/Help/menus) whenever `Help`, a modal dialog, or a
//!   menu/popup is open.

use std::collections::HashSet;
#[cfg(not(test))]
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct PanelWindowLevelsState {
    lower_all: bool,
    #[cfg_attr(any(not(target_os = "macos"), test), allow(dead_code))]
    lower_below_root: bool,
    #[cfg_attr(any(not(target_os = "macos"), test), allow(dead_code))]
    was_lowered_below_root: bool,
    floating_panel_titles: HashSet<String>,
}

#[cfg(not(test))]
static NATIVE_MENU_TRACKING: AtomicBool = AtomicBool::new(false);
#[cfg(not(test))]
static PANEL_WINDOW_LEVELS: OnceLock<Mutex<PanelWindowLevelsState>> = OnceLock::new();

#[cfg(not(test))]
fn panel_window_levels() -> &'static Mutex<PanelWindowLevelsState> {
    PANEL_WINDOW_LEVELS.get_or_init(|| Mutex::new(PanelWindowLevelsState::default()))
}

#[cfg(test)]
thread_local! {
    static TEST_NATIVE_MENU_TRACKING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TEST_PANEL_WINDOW_LEVELS: std::cell::RefCell<PanelWindowLevelsState> =
        std::cell::RefCell::new(PanelWindowLevelsState::default());
}

/// Returns `true` while a native macOS `NSMenu` bar tracking session is open.
pub fn is_native_menu_tracking() -> bool {
    #[cfg(not(test))]
    {
        NATIVE_MENU_TRACKING.load(Ordering::Acquire)
    }
    #[cfg(test)]
    {
        TEST_NATIVE_MENU_TRACKING.with(|c| c.get())
    }
}

#[cfg(test)]
pub fn set_native_menu_tracking_for_test(tracking: bool) {
    TEST_NATIVE_MENU_TRACKING.with(|c| c.set(tracking));
}

#[cfg(test)]
pub fn should_order_below_root_for_test() -> bool {
    TEST_PANEL_WINDOW_LEVELS.with(|cell| cell.borrow().lower_below_root)
}

/// Record which detached window titles belong to active studio panels/palettes that float above
/// the Stage/Timeline when no modal, `Help`, or menu/popup is open, plus whether floating windows
/// must be lowered (and ordered below the root window when a root-window modal/Help/menu is active).
pub fn set_panel_window_levels(
    lower_all: bool,
    lower_below_root: bool,
    floating_panel_titles: HashSet<String>,
) {
    #[cfg(not(test))]
    if let Ok(mut state) = panel_window_levels().lock() {
        state.lower_all = lower_all;
        state.lower_below_root = lower_below_root;
        state.floating_panel_titles = floating_panel_titles;
    }
    #[cfg(test)]
    TEST_PANEL_WINDOW_LEVELS.with(|cell| {
        let mut state = cell.borrow_mut();
        state.lower_all = lower_all;
        state.lower_below_root = lower_below_root;
        state.floating_panel_titles = floating_panel_titles;
    });
}

/// Pure check used by `configure_panels` (and unit tests) to decide whether a detached window with
/// `title` should use `NSFloatingWindowLevel` (`true`) or `NSNormalWindowLevel` (`false`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn window_should_float_for_title(title: Option<&str>) -> bool {
    if is_native_menu_tracking() {
        return false;
    }
    let Some(title) = title else {
        return false;
    };
    #[cfg(not(test))]
    {
        panel_window_levels()
            .lock()
            .ok()
            .is_some_and(|state| !state.lower_all && state.floating_panel_titles.contains(title))
    }
    #[cfg(test)]
    {
        TEST_PANEL_WINDOW_LEVELS.with(|cell| {
            let state = cell.borrow();
            !state.lower_all && state.floating_panel_titles.contains(title)
        })
    }
}

#[cfg(all(target_os = "macos", not(test)))]
mod imp {
    use super::{NATIVE_MENU_TRACKING, is_native_menu_tracking, panel_window_levels};
    use objc2::{
        MainThreadMarker, ffi, msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, Imp},
        sel,
    };
    use objc2_app_kit::{
        NSApplication, NSFloatingWindowLevel, NSNormalWindowLevel, NSWindow,
        NSWindowCollectionBehavior, NSWindowStyleMask, NSWindowTabbingMode,
        NSWindowTitleVisibility,
    };
    use std::{
        collections::{HashMap, HashSet},
        ffi::{CString, c_char},
        sync::{
            Mutex, OnceLock,
            atomic::{AtomicIsize, Ordering},
        },
    };

    const NS_WINDOW_BELOW: isize = -1;

    static ROOT: AtomicIsize = AtomicIsize::new(0);
    static FLOATING_TITLES: OnceLock<Mutex<HashMap<isize, String>>> = OnceLock::new();

    fn floating_titles() -> &'static Mutex<HashMap<isize, String>> {
        FLOATING_TITLES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn root_window_number() -> isize {
        ROOT.load(Ordering::Relaxed)
    }

    pub fn floating_window_title_for_number(number: isize) -> Option<String> {
        floating_titles()
            .lock()
            .ok()
            .and_then(|titles| titles.get(&number).cloned())
    }

    type MenuNotificationFn =
        unsafe extern "C-unwind" fn(&AnyObject, objc2::runtime::Sel, Option<&AnyObject>);

    unsafe extern "C-unwind" fn menu_did_begin_tracking(
        _observer: &AnyObject,
        _sel: objc2::runtime::Sel,
        _notification: Option<&AnyObject>,
    ) {
        NATIVE_MENU_TRACKING.store(true, Ordering::Release);
        configure_panels();
    }

    unsafe extern "C-unwind" fn menu_did_end_tracking(
        _observer: &AnyObject,
        _sel: objc2::runtime::Sel,
        _notification: Option<&AnyObject>,
    ) {
        NATIVE_MENU_TRACKING.store(false, Ordering::Release);
        configure_panels();
    }

    fn ns_string(text: &str) -> Option<Retained<AnyObject>> {
        let cls = AnyClass::get(c"NSString")?;
        let cstr = CString::new(text).ok()?;
        let ptr: *const c_char = cstr.as_ptr();
        unsafe {
            let raw: *mut AnyObject = msg_send![cls, alloc];
            if raw.is_null() {
                return None;
            }
            let initialized: *mut AnyObject = msg_send![raw, initWithUTF8String: ptr];
            Retained::from_raw(initialized)
        }
    }

    fn ensure_menu_tracking_observer_class() -> Option<&'static AnyClass> {
        static OBSERVER_CLASS: OnceLock<Option<&'static AnyClass>> = OnceLock::new();
        *OBSERVER_CLASS.get_or_init(|| {
            if let Some(existing) = AnyClass::get(c"NeoFlashMenuTrackingObserver") {
                return Some(existing);
            }
            let superclass = AnyClass::get(c"NSObject")?;
            let cls = unsafe {
                ffi::objc_allocateClassPair(superclass, c"NeoFlashMenuTrackingObserver".as_ptr(), 0)
            };
            if cls.is_null() {
                return None;
            }
            let begin_imp =
                unsafe { std::mem::transmute::<MenuNotificationFn, Imp>(menu_did_begin_tracking) };
            let end_imp =
                unsafe { std::mem::transmute::<MenuNotificationFn, Imp>(menu_did_end_tracking) };
            let added_begin = unsafe {
                ffi::class_addMethod(
                    cls,
                    sel!(menuDidBeginTracking:),
                    begin_imp,
                    c"v@:@".as_ptr(),
                )
            };
            let added_end = unsafe {
                ffi::class_addMethod(cls, sel!(menuDidEndTracking:), end_imp, c"v@:@".as_ptr())
            };
            if !added_begin.as_bool() || !added_end.as_bool() {
                return None;
            }
            unsafe { ffi::objc_registerClassPair(cls) };
            AnyClass::get(c"NeoFlashMenuTrackingObserver")
        })
    }

    fn install_menu_tracking_observer() {
        static INSTALLED: OnceLock<()> = OnceLock::new();
        INSTALLED.get_or_init(|| {
            let Some(obs_cls) = ensure_menu_tracking_observer_class() else {
                return;
            };
            let Some(begin_name) = ns_string("NSMenuDidBeginTrackingNotification") else {
                return;
            };
            let Some(end_name) = ns_string("NSMenuDidEndTrackingNotification") else {
                return;
            };
            let Some(nc_cls) = AnyClass::get(c"NSNotificationCenter") else {
                return;
            };
            unsafe {
                let raw_obs: *mut AnyObject = msg_send![obs_cls, new];
                let Some(observer) = Retained::from_raw(raw_obs) else {
                    return;
                };
                let center: *const AnyObject = msg_send![nc_cls, defaultCenter];
                if !center.is_null() {
                    let nil_obj: Option<&AnyObject> = None;
                    let _: () = msg_send![
                        &*center,
                        addObserver: &*observer,
                        selector: sel!(menuDidBeginTracking:),
                        name: &*begin_name,
                        object: nil_obj
                    ];
                    let _: () = msg_send![
                        &*center,
                        addObserver: &*observer,
                        selector: sel!(menuDidEndTracking:),
                        name: &*end_name,
                        object: nil_obj
                    ];
                    std::mem::forget(observer);
                }
            }
        });
    }

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
        install_menu_tracking_observer();
    }

    /// Configure detached panel windows so they have no native OS title bar or traffic-light
    /// buttons, hide on app deactivation, never merge into full-screen tabs, keep workspace/code
    /// panels (`Actions`, `Output`, `Library`, etc.) at `NSNormalWindowLevel`, and lower even
    /// compact utility palettes (`Tools`, `Color`, etc.) to `NSNormalWindowLevel` below the main
    /// window whenever `Help`, a modal dialog, or a menu/popup is open.
    pub fn configure_panels() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        let windows = app.windows();
        let mut root = ROOT.load(Ordering::Relaxed);
        if root <= 0
            && let Some(candidate) = windows
                .iter()
                .find(|w| {
                    w.styleMask().contains(NSWindowStyleMask::Titled)
                        && w.level() != NSFloatingWindowLevel
                })
                .or_else(|| windows.iter().find(|w| w.level() != NSFloatingWindowLevel))
                .or_else(|| windows.iter().next())
        {
            root = candidate.windowNumber();
            ROOT.store(root, Ordering::Relaxed);
        }
        let native_menu_open = is_native_menu_tracking();
        let (lower_all, order_below_root, transitioned_below_root, floating_titles_set) =
            if let Ok(mut state) = panel_window_levels().lock() {
                // Native macOS `NSMenu` already renders at `NSPopUpMenuWindowLevel` (101); keep
                // `lower_all` true so floating panels drop to `NSNormalWindowLevel`, while only
                // root-window modals, Help, and in-window root menus reorder windows below `root`.
                let order_below = state.lower_below_root;
                let transitioned = order_below && !state.was_lowered_below_root;
                state.was_lowered_below_root = order_below;
                (
                    state.lower_all || native_menu_open,
                    order_below,
                    transitioned,
                    state.floating_panel_titles.clone(),
                )
            } else {
                (native_menu_open, false, false, HashSet::new())
            };
        const NS_WINDOW_ABOVE: isize = 1;
        let app_active = app.isActive();
        let mut live_floating = HashSet::new();
        for window in windows.iter() {
            let number = window.windowNumber();
            if number == root {
                continue;
            }
            live_floating.insert(number);
            // Removing `NSWindowStyleMask::Titled` clears `NSWindow.title()` in AppKit; preserve
            // any non-empty viewport title first so native hit-testing and z-order policy can
            // identify the window.
            let raw_title = window.title().to_string();
            let resolved_title = if let Ok(mut titles) = floating_titles().lock() {
                if !raw_title.is_empty() {
                    titles.insert(number, raw_title.clone());
                    Some(raw_title)
                } else {
                    titles.get(&number).cloned()
                }
            } else {
                (!raw_title.is_empty()).then_some(raw_title)
            };
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
            let should_float = !lower_all
                && resolved_title
                    .as_deref()
                    .is_some_and(|title| floating_titles_set.contains(title));
            let desired_level = if should_float {
                NSFloatingWindowLevel
            } else {
                NSNormalWindowLevel
            };
            let was_floating = window.level() == NSFloatingWindowLevel;
            if window.level() != desired_level {
                window.setLevel(desired_level);
            }
            if order_below_root && root > 0 && (transitioned_below_root || was_floating) {
                unsafe {
                    let _: () = msg_send![&*window, orderWindow: NS_WINDOW_BELOW, relativeTo: root];
                }
            } else if should_float && !was_floating && root > 0 && app_active && window.isVisible()
            {
                unsafe {
                    let _: () = msg_send![&*window, orderWindow: NS_WINDOW_ABOVE, relativeTo: root];
                }
            }
            let behavior = (window.collectionBehavior()
                - NSWindowCollectionBehavior::FullScreenPrimary)
                | NSWindowCollectionBehavior::FullScreenAuxiliary;
            if window.collectionBehavior() != behavior {
                window.setCollectionBehavior(behavior);
            }
        }
        if let Ok(mut titles) = floating_titles().lock() {
            titles.retain(|number, _| live_floating.contains(number));
        }
    }
}

#[cfg(all(target_os = "macos", not(test)))]
pub use imp::{configure_panels, floating_window_title_for_number, install, root_window_number};

#[cfg(any(not(target_os = "macos"), test))]
pub fn install() {}

#[cfg(any(not(target_os = "macos"), test))]
pub fn configure_panels() {}

#[cfg(any(not(target_os = "macos"), test))]
#[allow(dead_code)]
pub fn root_window_number() -> isize {
    0
}

#[cfg(any(not(target_os = "macos"), test))]
#[allow(dead_code)]
pub fn floating_window_title_for_number(_number: isize) -> Option<String> {
    None
}
