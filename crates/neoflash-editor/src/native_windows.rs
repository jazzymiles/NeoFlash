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
    floating_panel_titles: HashSet<String>,
    lowered_below_root_titles: HashSet<String>,
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
#[cfg_attr(not(test), allow(dead_code))]
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
    TEST_PANEL_WINDOW_LEVELS.with(|cell| {
        let state = cell.borrow();
        state.lower_below_root || !state.lowered_below_root_titles.is_empty()
    })
}

/// Record which detached window titles belong to active studio panels/palettes that float above
/// the Stage/Timeline when no modal, `Help`, or overlapping root popup is open, plus whether all or
/// specific overlapping floating windows must be lowered below the root window (`T-390`).
pub fn set_panel_window_levels(
    lower_all: bool,
    lower_below_root: bool,
    floating_panel_titles: HashSet<String>,
    lowered_below_root_titles: HashSet<String>,
) {
    #[cfg(not(test))]
    if let Ok(mut state) = panel_window_levels().lock() {
        state.lower_all = lower_all;
        state.lower_below_root = lower_below_root;
        state.floating_panel_titles = floating_panel_titles;
        state.lowered_below_root_titles = lowered_below_root_titles;
    }
    #[cfg(test)]
    TEST_PANEL_WINDOW_LEVELS.with(|cell| {
        let mut state = cell.borrow_mut();
        state.lower_all = lower_all;
        state.lower_below_root = lower_below_root;
        state.floating_panel_titles = floating_panel_titles;
        state.lowered_below_root_titles = lowered_below_root_titles;
    });
}

/// Pure check used by `configure_panels` (and unit tests) to decide whether a detached window with
/// `title` should use `NSFloatingWindowLevel` (`true`) or `NSNormalWindowLevel` (`false`).
/// Native macOS `NSMenu` renders at `NSPopUpMenuWindowLevel` (`101`), already far above
/// `NSFloatingWindowLevel` (`3`), so menu tracking never lowers floating panels (`T-390`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn window_should_float_for_title(title: Option<&str>) -> bool {
    let Some(title) = title else {
        return false;
    };
    #[cfg(not(test))]
    {
        panel_window_levels().lock().ok().is_some_and(|state| {
            !state.lower_all
                && !state.lowered_below_root_titles.contains(title)
                && state.floating_panel_titles.contains(title)
        })
    }
    #[cfg(test)]
    {
        TEST_PANEL_WINDOW_LEVELS.with(|cell| {
            let state = cell.borrow();
            !state.lower_all
                && !state.lowered_below_root_titles.contains(title)
                && state.floating_panel_titles.contains(title)
        })
    }
}

#[cfg(all(target_os = "macos", not(test)))]
mod imp {
    use super::{NATIVE_MENU_TRACKING, panel_window_levels};
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
    const NS_WINDOW_ABOVE: isize = 1;

    struct ConfiguredPanelWindow {
        title: String,
        chrome_configured: bool,
        applied_floating: bool,
        ordered_below_root: bool,
    }

    static ROOT: AtomicIsize = AtomicIsize::new(0);
    static CONFIGURED_WINDOWS: OnceLock<Mutex<HashMap<isize, ConfiguredPanelWindow>>> =
        OnceLock::new();

    fn configured_windows() -> &'static Mutex<HashMap<isize, ConfiguredPanelWindow>> {
        CONFIGURED_WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn root_window_number() -> isize {
        ROOT.load(Ordering::Relaxed)
    }

    pub fn floating_window_title_for_number(number: isize) -> Option<String> {
        configured_windows()
            .lock()
            .ok()
            .and_then(|windows| windows.get(&number).map(|w| w.title.clone()))
            .filter(|t| !t.is_empty())
    }

    type MenuNotificationFn =
        unsafe extern "C-unwind" fn(&AnyObject, objc2::runtime::Sel, Option<&AnyObject>);

    unsafe extern "C-unwind" fn menu_did_begin_tracking(
        _observer: &AnyObject,
        _sel: objc2::runtime::Sel,
        _notification: Option<&AnyObject>,
    ) {
        // Native `NSMenu` renders at `NSPopUpMenuWindowLevel` (101), already far above
        // `NSFloatingWindowLevel` (3); record tracking state without WindowServer level thrashing.
        NATIVE_MENU_TRACKING.store(true, Ordering::Release);
    }

    unsafe extern "C-unwind" fn menu_did_end_tracking(
        _observer: &AnyObject,
        _sel: objc2::runtime::Sel,
        _notification: Option<&AnyObject>,
    ) {
        NATIVE_MENU_TRACKING.store(false, Ordering::Release);
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
        if windows.len() <= 1 {
            if let Ok(mut cached) = configured_windows().lock() {
                cached.clear();
            }
            return;
        }
        let (lower_all, order_below_root, floating_titles_set, lowered_below_root_titles) =
            if let Ok(state) = panel_window_levels().lock() {
                (
                    state.lower_all,
                    state.lower_below_root,
                    state.floating_panel_titles.clone(),
                    state.lowered_below_root_titles.clone(),
                )
            } else {
                (false, false, HashSet::new(), HashSet::new())
            };
        let app_active = app.isActive();
        let mut live_floating = HashSet::new();
        let Ok(mut cached_map) = configured_windows().lock() else {
            return;
        };
        for window in windows.iter() {
            let number = window.windowNumber();
            if number == root {
                continue;
            }
            live_floating.insert(number);

            // Reuse cached window title whenever it already matches a known active/lowered panel
            // title, avoiding per-frame `NSString -> String` allocations. Removing
            // `NSWindowStyleMask::Titled` clears `NSWindow.title()` in AppKit, so any non-empty
            // viewport title observed before/after style-mask setup is preserved here.
            let was_seen = cached_map.contains_key(&number);
            let entry = cached_map
                .entry(number)
                .or_insert_with(|| ConfiguredPanelWindow {
                    title: String::new(),
                    chrome_configured: false,
                    applied_floating: false,
                    ordered_below_root: false,
                });
            let title_already_matched = !entry.title.is_empty()
                && (floating_titles_set.contains(&entry.title)
                    || lowered_below_root_titles.contains(&entry.title));
            if !title_already_matched {
                let raw_title = window.title().to_string();
                if !raw_title.is_empty() {
                    entry.title = raw_title;
                }
            }

            if !entry.chrome_configured
                || !window.styleMask().contains(NSWindowStyleMask::Borderless)
            {
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
                let behavior = (window.collectionBehavior()
                    - NSWindowCollectionBehavior::FullScreenPrimary)
                    | NSWindowCollectionBehavior::FullScreenAuxiliary;
                if window.collectionBehavior() != behavior {
                    window.setCollectionBehavior(behavior);
                }
                entry.chrome_configured = true;
            }

            let order_this_below_root = order_below_root
                || (!entry.title.is_empty() && lowered_below_root_titles.contains(&entry.title));
            let should_float = !lower_all
                && !order_this_below_root
                && !entry.title.is_empty()
                && floating_titles_set.contains(&entry.title);
            let desired_level = if should_float {
                NSFloatingWindowLevel
            } else {
                NSNormalWindowLevel
            };

            if (!was_seen || entry.applied_floating != should_float)
                && window.level() != desired_level
            {
                window.setLevel(desired_level);
            }
            if order_this_below_root && !entry.ordered_below_root && root > 0 {
                unsafe {
                    let _: () = msg_send![&*window, orderWindow: NS_WINDOW_BELOW, relativeTo: root];
                }
            } else if should_float
                && (!was_seen || !entry.applied_floating || entry.ordered_below_root)
                && root > 0
                && app_active
                && window.isVisible()
            {
                unsafe {
                    let _: () = msg_send![&*window, orderWindow: NS_WINDOW_ABOVE, relativeTo: root];
                }
            }
            entry.applied_floating = should_float;
            entry.ordered_below_root = order_this_below_root;
        }
        cached_map.retain(|number, _| live_floating.contains(number));
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
