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

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
#[cfg(not(test))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Maximum `public.file-url` paths read from a single native drag operation.
pub const MAX_NATIVE_DRAG_FILE_URLS: usize = 128;
/// Maximum queued native `public.file-url` drop batches retained before oldest eviction.
const MAX_QUEUED_NATIVE_DROP_BATCHES: usize = 16;

/// Destination window for a fallback `public.file-url` drag operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeDragWindowTarget {
    Root,
    FloatingTitle(String),
}

impl NativeDragWindowTarget {
    fn matches(&self, floating_title: Option<&str>) -> bool {
        match (self, floating_title) {
            (Self::Root, None) => true,
            (Self::FloatingTitle(expected), Some(actual)) => {
                expected == actual
                    || actual
                        .strip_suffix(" — NeoFlash")
                        .is_some_and(|stem| stem == expected)
                    || expected
                        .strip_suffix(" — NeoFlash")
                        .is_some_and(|stem| stem == actual)
            }
            _ => false,
        }
    }

    pub fn same_window(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Root, Self::Root) => true,
            (Self::FloatingTitle(_), Self::FloatingTitle(b)) => self.matches(Some(b)),
            _ => false,
        }
    }
}

/// Bounded queue bridging `public.file-url`-only macOS drags into `egui::RawInput`.
#[derive(Default, Debug)]
pub struct NativeWindowFileDragQueue {
    hovered: Option<(NativeDragWindowTarget, Vec<PathBuf>)>,
    dropped_batches: VecDeque<(NativeDragWindowTarget, Vec<PathBuf>)>,
}

impl NativeWindowFileDragQueue {
    pub fn set_hovered(&mut self, target: NativeDragWindowTarget, paths: Vec<PathBuf>) {
        let paths = Self::bounded_paths(paths);
        if paths.is_empty() {
            self.clear_hovered_for(&target);
        } else {
            self.hovered = Some((target, paths));
        }
    }

    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    pub fn has_hovered_for(&self, target: &NativeDragWindowTarget) -> bool {
        self.hovered
            .as_ref()
            .is_some_and(|(t, _)| t.same_window(target))
    }

    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    pub fn clear_hovered_for(&mut self, target: &NativeDragWindowTarget) {
        if self
            .hovered
            .as_ref()
            .is_some_and(|(t, _)| t.same_window(target))
        {
            self.hovered = None;
        }
    }

    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    pub fn clear_hovered(&mut self) {
        self.hovered = None;
    }

    pub fn push_dropped(&mut self, target: NativeDragWindowTarget, paths: Vec<PathBuf>) {
        self.clear_hovered_for(&target);
        let paths = Self::bounded_paths(paths);
        if paths.is_empty() {
            return;
        }
        while self.dropped_batches.len() >= MAX_QUEUED_NATIVE_DROP_BATCHES {
            self.dropped_batches.pop_front();
        }
        self.dropped_batches.push_back((target, paths));
    }

    fn bounded_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
        paths
            .into_iter()
            .filter(|path| !path.as_os_str().is_empty())
            .take(MAX_NATIVE_DRAG_FILE_URLS)
            .collect()
    }

    /// Inject fallback `public.file-url` hover/drop files into `ctx` only when `winit` did not
    /// already populate `NSFilenamesPboardType` events on `RawInput`.
    pub fn drain_into_context(
        &mut self,
        ctx: &eframe::egui::Context,
        floating_title: Option<&str>,
    ) -> bool {
        let matching_hover = self
            .hovered
            .as_ref()
            .filter(|(target, _)| target.matches(floating_title))
            .map(|(_, paths)| paths.clone());

        let has_matching_drop = self
            .dropped_batches
            .iter()
            .any(|(target, _)| target.matches(floating_title));

        if matching_hover.is_none() && !has_matching_drop {
            return false;
        }

        let mut injected = false;
        let mut retry_drop = false;
        ctx.input_mut(|input| {
            if let Some(hover_paths) = matching_hover
                && input.raw.hovered_files.is_empty()
            {
                input.raw.hovered_files = hover_paths
                    .into_iter()
                    .map(|path| eframe::egui::HoveredFile {
                        path: Some(path),
                        ..Default::default()
                    })
                    .collect();
                injected = !input.raw.hovered_files.is_empty();
            }
            if !has_matching_drop {
                return;
            }
            if input.raw.dropped_files.is_empty() {
                let mut matching_drops = Vec::new();
                self.dropped_batches.retain_mut(|(target, paths)| {
                    if !target.matches(floating_title) {
                        return true;
                    }
                    matching_drops.append(paths);
                    false
                });
                input.raw.dropped_files = matching_drops
                    .into_iter()
                    .map(|path| eframe::egui::DroppedFile {
                        path: Some(path),
                        ..Default::default()
                    })
                    .collect();
                injected = true;
            } else {
                // Keep a different pending drop for a later frame, but discard files already
                // delivered by winit so the same drop cannot reappear after RawInput clears.
                let delivered: HashSet<_> = input
                    .raw
                    .dropped_files
                    .iter()
                    .filter_map(|file| file.path.as_deref())
                    .collect();
                self.dropped_batches.retain_mut(|(target, paths)| {
                    if !target.matches(floating_title) {
                        return true;
                    }
                    paths.retain(|path| !delivered.contains(path.as_path()));
                    retry_drop |= !paths.is_empty();
                    !paths.is_empty()
                });
            }
        });
        if retry_drop {
            ctx.request_repaint_of(eframe::egui::ViewportId::ROOT);
        }
        injected
    }
}

static NATIVE_DRAG_QUEUE: OnceLock<Mutex<NativeWindowFileDragQueue>> = OnceLock::new();
static WAKEUP_CONTEXT: OnceLock<Mutex<Option<eframe::egui::Context>>> = OnceLock::new();

pub fn global_file_drag_queue() -> &'static Mutex<NativeWindowFileDragQueue> {
    NATIVE_DRAG_QUEUE.get_or_init(|| Mutex::new(NativeWindowFileDragQueue::default()))
}

pub fn register_wakeup_context(ctx: &eframe::egui::Context) {
    let slot = WAKEUP_CONTEXT.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(ctx.clone());
    }
}

#[cfg_attr(any(not(target_os = "macos"), test), allow(dead_code))]
fn request_drag_repaint() {
    if let Some(slot) = WAKEUP_CONTEXT.get()
        && let Ok(guard) = slot.lock()
        && let Some(ctx) = guard.as_ref()
    {
        ctx.request_repaint();
    }
}

pub fn drain_file_drag_into_context(
    ctx: &eframe::egui::Context,
    floating_title: Option<&str>,
) -> bool {
    if let Ok(mut queue) = global_file_drag_queue().lock() {
        queue.drain_into_context(ctx, floating_title)
    } else {
        false
    }
}

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
    use super::{
        MAX_NATIVE_DRAG_FILE_URLS, NATIVE_MENU_TRACKING, NativeDragWindowTarget,
        global_file_drag_queue, panel_window_levels, request_drag_repaint,
    };
    use objc2::{
        Encode, MainThreadMarker, ffi, msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, Bool, Imp, Method, Sel},
        sel,
    };
    use objc2_app_kit::{
        NSApplication, NSFloatingWindowLevel, NSNormalWindowLevel, NSWindow,
        NSWindowCollectionBehavior, NSWindowStyleMask, NSWindowTabbingMode,
        NSWindowTitleVisibility,
    };
    use std::{
        collections::{HashMap, HashSet},
        ffi::{CStr, CString, c_char},
        path::PathBuf,
        sync::{
            Mutex, OnceLock,
            atomic::{AtomicIsize, Ordering},
        },
    };

    const NS_WINDOW_BELOW: isize = -1;
    const NS_WINDOW_ABOVE: isize = 1;
    const NS_DRAG_OPERATION_NONE: usize = 0;
    const NS_DRAG_OPERATION_COPY: usize = 1;

    struct ConfiguredPanelWindow {
        title: String,
        chrome_configured: bool,
        drag_types_registered: bool,
        applied_floating: bool,
        ordered_below_root: bool,
    }

    static ROOT: AtomicIsize = AtomicIsize::new(0);
    static ROOT_DRAG_REGISTERED: AtomicIsize = AtomicIsize::new(0);
    static CONFIGURED_WINDOWS: OnceLock<Mutex<HashMap<isize, ConfiguredPanelWindow>>> =
        OnceLock::new();

    #[derive(Clone, Copy)]
    enum OrigDragOpFn {
        Bool(unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>) -> Bool),
        Usize(unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>) -> usize),
    }

    impl OrigDragOpFn {
        unsafe fn call(self, this: &AnyObject, sel: Sel, sender: Option<&AnyObject>) -> usize {
            match self {
                Self::Bool(f) => {
                    if (unsafe { f(this, sel, sender) }).as_bool() {
                        NS_DRAG_OPERATION_COPY
                    } else {
                        NS_DRAG_OPERATION_NONE
                    }
                }
                Self::Usize(f) => unsafe { f(this, sel, sender) },
            }
        }
    }

    type PerformDragFn = unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>) -> Bool;
    type DraggingUpdatedFn =
        unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>) -> usize;
    type DraggingExitedFn = unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>);

    struct DelegateDragHooks {
        orig_entered: Option<OrigDragOpFn>,
        orig_updated: Option<OrigDragOpFn>,
        orig_perform: Option<PerformDragFn>,
        orig_exited: Option<DraggingExitedFn>,
    }

    static DELEGATE_HOOKS: OnceLock<Mutex<HashMap<usize, DelegateDragHooks>>> = OnceLock::new();

    fn delegate_hooks_map() -> &'static Mutex<HashMap<usize, DelegateDragHooks>> {
        DELEGATE_HOOKS.get_or_init(|| Mutex::new(HashMap::new()))
    }

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

    unsafe fn ns_string_to_rust(obj: &AnyObject) -> Option<String> {
        let utf8: *const c_char = unsafe { msg_send![obj, UTF8String] };
        if utf8.is_null() {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(utf8) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn resolve_drag_window_target(sender: &AnyObject) -> NativeDragWindowTarget {
        let win_ptr: *const NSWindow = unsafe { msg_send![sender, draggingDestinationWindow] };
        if win_ptr.is_null() {
            return NativeDragWindowTarget::Root;
        }
        let win = unsafe { &*win_ptr };
        let number = win.windowNumber();
        let root = ROOT.load(Ordering::Relaxed);
        if root > 0 && number == root {
            return NativeDragWindowTarget::Root;
        }
        if let Some(title) = floating_window_title_for_number(number) {
            return NativeDragWindowTarget::FloatingTitle(title);
        }
        let raw_title = win.title().to_string();
        if !raw_title.is_empty() && root > 0 && number != root {
            NativeDragWindowTarget::FloatingTitle(raw_title)
        } else {
            NativeDragWindowTarget::Root
        }
    }

    /// Returns `true` when `NSFilenamesPboardType` carries a non-empty file array so `winit`'s
    /// native handler owns `HoveredFile`/`DroppedFile` emission without duplicate events.
    fn pasteboard_has_legacy_filenames(sender: &AnyObject) -> bool {
        let pb: *const AnyObject = unsafe { msg_send![sender, draggingPasteboard] };
        if pb.is_null() {
            return false;
        }
        let Some(pboard_type) = ns_string("NSFilenamesPboardType") else {
            return false;
        };
        let plist: *const AnyObject =
            unsafe { msg_send![&*pb, propertyListForType: &*pboard_type] };
        if plist.is_null() {
            return false;
        }
        let responds_count: Bool = unsafe { msg_send![&*plist, respondsToSelector: sel!(count)] };
        if !responds_count.as_bool() {
            return false;
        }
        let count: usize = unsafe { msg_send![&*plist, count] };
        count > 0
    }

    /// Extract local file paths from `public.file-url` (`NSURL`) items on `draggingPasteboard`.
    fn read_pasteboard_file_urls(sender: &AnyObject) -> Vec<PathBuf> {
        let pb: *const AnyObject = unsafe { msg_send![sender, draggingPasteboard] };
        if pb.is_null() {
            return Vec::new();
        }
        let Some(url_cls) = AnyClass::get(c"NSURL") else {
            return Vec::new();
        };
        let Some(arr_cls) = AnyClass::get(c"NSArray") else {
            return Vec::new();
        };
        let Some(dict_cls) = AnyClass::get(c"NSDictionary") else {
            return Vec::new();
        };
        let Some(num_cls) = AnyClass::get(c"NSNumber") else {
            return Vec::new();
        };
        let Some(file_only_key) = ns_string("NSPasteboardURLReadingFileURLsOnlyKey") else {
            return Vec::new();
        };
        unsafe {
            let cls_ptr: *const AnyObject = (url_cls as *const AnyClass).cast();
            let classes: *const AnyObject =
                msg_send![arr_cls, arrayWithObjects: &cls_ptr, count: 1usize];
            if classes.is_null() {
                return Vec::new();
            }
            let yes_num: *const AnyObject = msg_send![num_cls, numberWithBool: Bool::YES];
            if yes_num.is_null() {
                return Vec::new();
            }
            let options: *const AnyObject =
                msg_send![dict_cls, dictionaryWithObject: &*yes_num, forKey: &*file_only_key];
            if options.is_null() {
                return Vec::new();
            }
            let urls: *const AnyObject =
                msg_send![&*pb, readObjectsForClasses: &*classes, options: &*options];
            if urls.is_null() {
                return Vec::new();
            }
            let count: usize = msg_send![&*urls, count];
            let capped = count.min(MAX_NATIVE_DRAG_FILE_URLS);
            let mut out = Vec::with_capacity(capped);
            for idx in 0..capped {
                let url_obj: *const AnyObject = msg_send![&*urls, objectAtIndex: idx];
                if url_obj.is_null() {
                    continue;
                }
                let responds_file_path_url: Bool =
                    msg_send![&*url_obj, respondsToSelector: sel!(filePathURL)];
                let resolved_url: *const AnyObject = if responds_file_path_url.as_bool() {
                    let fp: *const AnyObject = msg_send![&*url_obj, filePathURL];
                    if fp.is_null() { url_obj } else { fp }
                } else {
                    url_obj
                };
                let path_ns: *const AnyObject = msg_send![&*resolved_url, path];
                if path_ns.is_null() {
                    continue;
                }
                if let Some(path_str) = ns_string_to_rust(&*path_ns)
                    && !path_str.is_empty()
                {
                    out.push(PathBuf::from(path_str));
                }
            }
            out
        }
    }

    fn lookup_delegate_hooks(this: &AnyObject) -> Option<DelegateDragHooks> {
        let mut cur: Option<&AnyClass> = Some(this.class());
        let map = delegate_hooks_map().lock().ok()?;
        while let Some(cls) = cur {
            let key = (cls as *const AnyClass) as usize;
            if let Some(hooks) = map.get(&key) {
                return Some(DelegateDragHooks {
                    orig_entered: hooks.orig_entered,
                    orig_updated: hooks.orig_updated,
                    orig_perform: hooks.orig_perform,
                    orig_exited: hooks.orig_exited,
                });
            }
            cur = cls.superclass();
        }
        None
    }

    unsafe extern "C-unwind" fn swizzled_dragging_entered(
        this: &AnyObject,
        sel: Sel,
        sender: Option<&AnyObject>,
    ) -> usize {
        if MainThreadMarker::new().is_none() {
            return NS_DRAG_OPERATION_NONE;
        }
        let Some(sender) = sender else {
            return NS_DRAG_OPERATION_NONE;
        };
        if pasteboard_has_legacy_filenames(sender) {
            if let Ok(mut queue) = global_file_drag_queue().lock() {
                queue.clear_hovered();
            }
            let op = lookup_delegate_hooks(this)
                .and_then(|h| h.orig_entered)
                .map_or(NS_DRAG_OPERATION_COPY, |orig| unsafe {
                    orig.call(this, sel, Some(sender))
                });
            request_drag_repaint();
            return op;
        }
        let paths = read_pasteboard_file_urls(sender);
        if paths.is_empty() {
            return NS_DRAG_OPERATION_NONE;
        }
        let target = resolve_drag_window_target(sender);
        if let Ok(mut queue) = global_file_drag_queue().lock() {
            queue.set_hovered(target, paths);
        }
        request_drag_repaint();
        NS_DRAG_OPERATION_COPY
    }

    unsafe extern "C-unwind" fn swizzled_dragging_updated(
        this: &AnyObject,
        sel: Sel,
        sender: Option<&AnyObject>,
    ) -> usize {
        if MainThreadMarker::new().is_none() {
            return NS_DRAG_OPERATION_NONE;
        }
        let Some(sender) = sender else {
            return NS_DRAG_OPERATION_NONE;
        };
        if pasteboard_has_legacy_filenames(sender) {
            let op = lookup_delegate_hooks(this)
                .and_then(|h| h.orig_updated)
                .map_or(NS_DRAG_OPERATION_COPY, |orig| unsafe {
                    orig.call(this, sel, Some(sender))
                });
            request_drag_repaint();
            return op;
        }
        let target = resolve_drag_window_target(sender);
        let has_cached_hover = global_file_drag_queue()
            .lock()
            .ok()
            .is_some_and(|q| q.has_hovered_for(&target));
        if has_cached_hover {
            request_drag_repaint();
            return NS_DRAG_OPERATION_COPY;
        }
        let paths = read_pasteboard_file_urls(sender);
        if paths.is_empty() {
            return NS_DRAG_OPERATION_NONE;
        }
        if let Ok(mut queue) = global_file_drag_queue().lock() {
            queue.set_hovered(target, paths);
        }
        request_drag_repaint();
        NS_DRAG_OPERATION_COPY
    }

    unsafe extern "C-unwind" fn swizzled_perform_drag_operation(
        this: &AnyObject,
        sel: Sel,
        sender: Option<&AnyObject>,
    ) -> Bool {
        if MainThreadMarker::new().is_none() {
            return Bool::NO;
        }
        let Some(sender) = sender else {
            return Bool::NO;
        };
        let target = resolve_drag_window_target(sender);
        let legacy = pasteboard_has_legacy_filenames(sender);
        let paths = if legacy {
            Vec::new()
        } else {
            read_pasteboard_file_urls(sender)
        };
        let accepted = !paths.is_empty();
        if let Ok(mut queue) = global_file_drag_queue().lock() {
            queue.push_dropped(target, paths);
        }
        let result = if legacy {
            lookup_delegate_hooks(this)
                .and_then(|hooks| hooks.orig_perform)
                .map_or(Bool::NO, |orig| unsafe { orig(this, sel, Some(sender)) })
        } else {
            Bool::new(accepted)
        };
        request_drag_repaint();
        result
    }

    unsafe extern "C-unwind" fn swizzled_dragging_exited(
        this: &AnyObject,
        sel: Sel,
        sender: Option<&AnyObject>,
    ) {
        if MainThreadMarker::new().is_none() {
            return;
        }
        let target_opt = sender.map(resolve_drag_window_target);
        if let Ok(mut queue) = global_file_drag_queue().lock() {
            match target_opt {
                Some(target) => queue.clear_hovered_for(&target),
                None => queue.clear_hovered(),
            }
        }
        if let Some(orig) = lookup_delegate_hooks(this).and_then(|h| h.orig_exited) {
            unsafe { orig(this, sel, sender) };
        }
        request_drag_repaint();
    }

    fn decode_orig_drag_op_imp(imp: Imp, is_usize_ret: bool) -> OrigDragOpFn {
        if is_usize_ret {
            OrigDragOpFn::Usize(unsafe {
                std::mem::transmute::<
                    Imp,
                    unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>) -> usize,
                >(imp)
            })
        } else {
            OrigDragOpFn::Bool(unsafe {
                std::mem::transmute::<
                    Imp,
                    unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>) -> Bool,
                >(imp)
            })
        }
    }

    fn install_or_replace_delegate_method(
        delegate_cls: &AnyClass,
        selector: Sel,
        new_imp: Imp,
        encoding: &CStr,
    ) -> Option<(Imp, bool)> {
        let existing = delegate_cls.instance_method(selector).map(|method| {
            let prev_imp = method.implementation();
            let enc_ptr = unsafe { ffi::method_getTypeEncoding(method as *const Method) };
            let is_usize_ret = if enc_ptr.is_null() {
                false
            } else {
                let bytes = unsafe { CStr::from_ptr(enc_ptr) }.to_bytes();
                let first = bytes
                    .iter()
                    .copied()
                    .find(|b| !matches!(b, b'r' | b'n' | b'N' | b'o' | b'O' | b'R' | b'V'));
                matches!(first, Some(b'Q' | b'L' | b'q' | b'l'))
            };
            (prev_imp, is_usize_ret)
        });
        let raw_cls = (delegate_cls as *const AnyClass).cast_mut();
        match existing {
            None => {
                let _ =
                    unsafe { ffi::class_addMethod(raw_cls, selector, new_imp, encoding.as_ptr()) };
                None
            }
            Some((prev_imp, _)) if std::ptr::fn_addr_eq(prev_imp, new_imp) => None,
            Some((prev_imp, is_usize_ret)) => {
                let added =
                    unsafe { ffi::class_addMethod(raw_cls, selector, new_imp, encoding.as_ptr()) };
                if !added.as_bool() {
                    let m = unsafe { ffi::class_getInstanceMethod(raw_cls, selector) };
                    if !m.is_null() {
                        let _ = unsafe { ffi::method_setImplementation(m.cast_mut(), new_imp) };
                    }
                }
                Some((prev_imp, is_usize_ret))
            }
        }
    }

    fn ensure_delegate_drag_hooks(delegate_cls: &AnyClass) {
        let cls_key = (delegate_cls as *const AnyClass) as usize;
        let Ok(mut map) = delegate_hooks_map().lock() else {
            return;
        };
        if map.contains_key(&cls_key) {
            return;
        }
        let Ok(drag_op_enc) = CString::new(format!("{}@:@", usize::ENCODING)) else {
            return;
        };
        let Ok(bool_enc) = CString::new(format!("{}@:@", Bool::ENCODING)) else {
            return;
        };

        let entered_imp =
            unsafe { std::mem::transmute::<DraggingUpdatedFn, Imp>(swizzled_dragging_entered) };
        let orig_entered = install_or_replace_delegate_method(
            delegate_cls,
            sel!(draggingEntered:),
            entered_imp,
            drag_op_enc.as_c_str(),
        )
        .map(|(prev, is_usize)| decode_orig_drag_op_imp(prev, is_usize));

        let updated_imp =
            unsafe { std::mem::transmute::<DraggingUpdatedFn, Imp>(swizzled_dragging_updated) };
        let orig_updated = install_or_replace_delegate_method(
            delegate_cls,
            sel!(draggingUpdated:),
            updated_imp,
            drag_op_enc.as_c_str(),
        )
        .map(|(prev, is_usize)| decode_orig_drag_op_imp(prev, is_usize));

        let perform_imp =
            unsafe { std::mem::transmute::<PerformDragFn, Imp>(swizzled_perform_drag_operation) };
        let orig_perform = install_or_replace_delegate_method(
            delegate_cls,
            sel!(performDragOperation:),
            perform_imp,
            bool_enc.as_c_str(),
        )
        .map(|(prev, _)| unsafe { std::mem::transmute::<Imp, PerformDragFn>(prev) });

        let exited_imp =
            unsafe { std::mem::transmute::<DraggingExitedFn, Imp>(swizzled_dragging_exited) };
        let orig_exited = install_or_replace_delegate_method(
            delegate_cls,
            sel!(draggingExited:),
            exited_imp,
            c"v@:@",
        )
        .map(|(prev, _)| unsafe { std::mem::transmute::<Imp, DraggingExitedFn>(prev) });

        map.insert(
            cls_key,
            DelegateDragHooks {
                orig_entered,
                orig_updated,
                orig_perform,
                orig_exited,
            },
        );
    }

    fn ensure_window_file_drag_support(window: &NSWindow) {
        let Some(arr_cls) = AnyClass::get(c"NSArray") else {
            return;
        };
        let Some(filenames_type) = ns_string("NSFilenamesPboardType") else {
            return;
        };
        let Some(file_url_type) = ns_string("public.file-url") else {
            return;
        };
        unsafe {
            let objs: [*const AnyObject; 2] = [&*filenames_type, &*file_url_type];
            let types_arr: *const AnyObject =
                msg_send![arr_cls, arrayWithObjects: objs.as_ptr(), count: 2usize];
            if !types_arr.is_null() {
                let _: () = msg_send![window, registerForDraggedTypes: &*types_arr];
            }
            let delegate: *const AnyObject = msg_send![window, delegate];
            if !delegate.is_null() {
                ensure_delegate_drag_hooks((&*delegate).class());
            }
        }
    }

    /// Call once while only the main window exists (from `Editor::new`).
    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
        let app = NSApplication::sharedApplication(mtm);
        if let Some(root) = app.windows().iter().next() {
            let num = root.windowNumber();
            ROOT.store(num, Ordering::Relaxed);
            ensure_window_file_drag_support(&root);
            ROOT_DRAG_REGISTERED.store(num, Ordering::Relaxed);
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
        if root > 0
            && ROOT_DRAG_REGISTERED.load(Ordering::Relaxed) != root
            && let Some(root_win) = windows.iter().find(|w| w.windowNumber() == root)
        {
            ensure_window_file_drag_support(&root_win);
            ROOT_DRAG_REGISTERED.store(root, Ordering::Relaxed);
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
                    drag_types_registered: false,
                    applied_floating: false,
                    ordered_below_root: false,
                });
            if !entry.drag_types_registered {
                ensure_window_file_drag_support(&window);
                entry.drag_types_registered = true;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t474_a05_overlapping_root_and_floating_hover_targets_never_starve_or_clobber_each_other() {
        let mut queue = NativeWindowFileDragQueue::default();
        let root_target = NativeDragWindowTarget::Root;
        let floating_tl = NativeDragWindowTarget::FloatingTitle("Timeline — NeoFlash".into());
        let floating_tl_short = NativeDragWindowTarget::FloatingTitle("Timeline".into());
        let floating_lib = NativeDragWindowTarget::FloatingTitle("Library — NeoFlash".into());
        let clip_path = PathBuf::from("/tmp/overlapping_drag.mp4");

        // 1. Entering Root caches hover only for Root, NOT for overlapping floating windows.
        queue.set_hovered(root_target.clone(), vec![clip_path.clone()]);
        assert!(queue.has_hovered_for(&root_target));
        assert!(!queue.has_hovered_for(&floating_tl));
        assert!(!queue.has_hovered_for(&floating_lib));

        // 2. Moving into an overlapping floating Timeline window updates hover to FloatingTitle
        //    even before Root's `draggingExited:` fires; a subsequent `[Root draggingExited:]`
        //    must NOT wipe the floating Timeline's active hover state!
        queue.set_hovered(floating_tl.clone(), vec![clip_path.clone()]);
        assert!(queue.has_hovered_for(&floating_tl));
        assert!(queue.has_hovered_for(&floating_tl_short));
        assert!(!queue.has_hovered_for(&root_target));

        queue.clear_hovered_for(&root_target);
        assert!(
            queue.has_hovered_for(&floating_tl),
            "Root draggingExited: must not wipe FloatingTitle hover state"
        );

        let ctx = eframe::egui::Context::default();
        let _ = ctx.run(eframe::egui::RawInput::default(), |c| {
            assert!(!queue.drain_into_context(c, None));
            assert!(c.input(|i| i.raw.hovered_files.is_empty()));
            assert!(queue.drain_into_context(c, Some("Timeline — NeoFlash")));
            assert_eq!(c.input(|i| i.raw.hovered_files.len()), 1);
        });

        // 3. Leaving a different floating window (`Library`) also leaves `Timeline` intact;
        //    leaving `Timeline` clears it cleanly.
        queue.clear_hovered_for(&floating_lib);
        assert!(queue.has_hovered_for(&floating_tl));
        queue.set_hovered(floating_lib.clone(), vec![]);
        queue.push_dropped(root_target, vec![clip_path]);
        assert!(queue.has_hovered_for(&floating_tl));
        queue.clear_hovered_for(&floating_tl);
        assert!(!queue.has_hovered_for(&floating_tl));
    }

    #[test]
    fn matching_winit_drop_is_not_replayed_and_other_pending_files_are_retained() {
        use eframe::egui::{Context, DroppedFile, RawInput};

        let duplicate = PathBuf::from("/tmp/shared.mp4");
        let other = PathBuf::from("/tmp/other.wav");
        let mut queue = NativeWindowFileDragQueue::default();
        queue.push_dropped(
            NativeDragWindowTarget::Root,
            vec![duplicate.clone(), other.clone()],
        );
        queue.push_dropped(
            NativeDragWindowTarget::FloatingTitle("Library — NeoFlash".into()),
            vec![duplicate.clone()],
        );

        let ctx = Context::default();
        let winit_input = RawInput {
            dropped_files: vec![DroppedFile {
                path: Some(duplicate.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let dropped_paths = |ctx: &Context| {
            ctx.input(|input| {
                input
                    .raw
                    .dropped_files
                    .iter()
                    .filter_map(|file| file.path.clone())
                    .collect::<Vec<_>>()
            })
        };
        let _ = ctx.run(winit_input, |ctx| {
            assert!(!queue.drain_into_context(ctx, None));
            assert_eq!(dropped_paths(ctx), std::slice::from_ref(&duplicate));
        });
        let _ = ctx.run(RawInput::default(), |ctx| {
            assert!(queue.drain_into_context(ctx, None));
            assert_eq!(dropped_paths(ctx), std::slice::from_ref(&other));
        });
        let _ = ctx.run(RawInput::default(), |ctx| {
            assert!(!queue.drain_into_context(ctx, None));
            assert!(dropped_paths(ctx).is_empty());
            assert!(queue.drain_into_context(ctx, Some("Library")));
            assert_eq!(dropped_paths(ctx), std::slice::from_ref(&duplicate));
        });
    }
}
