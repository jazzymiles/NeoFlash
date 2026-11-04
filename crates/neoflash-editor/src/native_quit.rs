//! Route native application termination through the editor's document decisions.
//!
//! Winit's macOS Quit menu calls `terminate:` directly, bypassing a window's
//! CloseRequested event. Keep its delegate and all its existing callbacks, adding
//! only the missing `applicationShouldTerminate:` veto. The callback queues work;
//! document mutation and dialogs still run from the ordinary editor update.
use eframe::egui;

#[cfg(target_os = "macos")]
mod native {
    use super::egui;
    use objc2::{
        Encode, MainThreadMarker, ffi,
        runtime::{AnyClass, AnyObject, Imp, Sel},
        sel,
    };
    use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
    use std::{
        ffi::CString,
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    static REQUESTED: AtomicBool = AtomicBool::new(false);
    static CONTEXT: Mutex<Option<egui::Context>> = Mutex::new(None);

    type ShouldTerminate =
        unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSApplication) -> NSApplicationTerminateReply;

    unsafe extern "C-unwind" fn should_terminate(
        _delegate: &AnyObject,
        _selector: Sel,
        _application: &NSApplication,
    ) -> NSApplicationTerminateReply {
        REQUESTED.store(true, Ordering::Release);
        // Clone before requesting repaint: the wake callback may re-enter native
        // event dispatch, and must never run while this mutex is held.
        let ctx = CONTEXT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(ctx) = ctx {
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        // Every Quit entry point (keyboard, menu, Dock) is cancelled here. Once
        // all documents are approved, the editor closes the root viewport and
        // eframe exits normally; it does not call terminate: again.
        NSApplicationTerminateReply::TerminateCancel
    }

    pub(super) fn install(ctx: &egui::Context) -> Result<(), String> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "Native Quit bridge must be installed on the main thread.".to_owned())?;
        let app = NSApplication::sharedApplication(mtm);
        let delegate = app.delegate().ok_or_else(|| {
            "Native Quit bridge could not find the application delegate.".to_owned()
        })?;
        let object: &AnyObject = (*delegate).as_ref();
        let class = object.class();
        let selector = sel!(applicationShouldTerminate:);
        // SAFETY: This function has the delegate method's exact Objective-C ABI:
        // object receiver, selector, NSApplication argument, NSUInteger reply.
        let implementation =
            unsafe { std::mem::transmute::<ShouldTerminate, Imp>(should_terminate) };
        if let Some(method) = class.instance_method(selector) {
            if !std::ptr::fn_addr_eq(method.implementation(), implementation) {
                return Err(format!(
                    "Native Quit bridge cannot replace the existing applicationShouldTerminate: handler on {}.",
                    class.name().to_string_lossy()
                ));
            }
        } else {
            let encoding = CString::new(format!("{}@:@", NSApplicationTerminateReply::ENCODING))
                .map_err(|_| {
                    "Native Quit bridge could not encode its method signature.".to_owned()
                })?;
            // SAFETY: Installation runs on the main thread after winit creates
            // its delegate. No ivars, delegate identity, superclass or existing
            // methods change. The runtime copies the type encoding. The callback
            // uses only process-lifetime state and remains valid for class lifetime.
            let added = unsafe {
                ffi::class_addMethod(
                    class as *const AnyClass as *mut AnyClass,
                    selector,
                    implementation,
                    encoding.as_ptr(),
                )
            };
            if !added.as_bool() {
                return Err(
                    "Native Quit bridge could not install applicationShouldTerminate:.".into(),
                );
            }
        }
        *CONTEXT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ctx.clone());
        REQUESTED.store(false, Ordering::Release);
        Ok(())
    }

    pub(super) fn take_requested() -> bool {
        REQUESTED.swap(false, Ordering::AcqRel)
    }
}

/// Install once from `Editor::new`; never install from worker-thread tests.
pub(crate) fn install(ctx: &egui::Context) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        native::install(ctx)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ctx;
        Ok(())
    }
}

/// Consume native Quit requests from the normal root-viewport update.
pub(crate) fn take_requested() -> bool {
    #[cfg(target_os = "macos")]
    {
        native::take_requested()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
