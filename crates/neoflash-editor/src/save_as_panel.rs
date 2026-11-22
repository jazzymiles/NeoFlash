//! Illustrator-style `Save As` dialog with native macOS `NSSavePanel` `Format` and `Script`
//! conversion dropdowns (`T-455`).
//!
//! Pure option construction and `(converted)` filename transition logic live in cross-platform
//! functions so unit tests cover every language/format transition without opening modal OS sheets.

use neoflash_core::ScriptLanguage;
use neoflash_script::ConversionTarget;
use std::path::{Path, PathBuf};

/// File container selected in the `Save As` `Format` dropdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveAsFormat {
    NeoFlash,
    FlashFla,
    UncompressedXfl,
}

#[cfg_attr(any(not(target_os = "macos"), test), allow(dead_code))]
impl SaveAsFormat {
    pub const ALL: [Self; 3] = [Self::NeoFlash, Self::FlashFla, Self::UncompressedXfl];

    pub const fn label(self) -> &'static str {
        match self {
            Self::NeoFlash => "NeoFlash Project (.nflash)",
            Self::FlashFla => "Flash FLA (Animate CC / CS6–CS4 / Flash 8) (.fla)",
            Self::UncompressedXfl => "Uncompressed XFL Directory (.xfl)",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::NeoFlash => "nflash",
            Self::FlashFla => "fla",
            Self::UncompressedXfl => "xfl",
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::NeoFlash)
    }
}

/// Script conversion choice selected in the `Save As` `Script` dropdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveAsScriptOption {
    KeepCurrent,
    ConvertTo(ConversionTarget),
}

#[cfg_attr(any(not(target_os = "macos"), test), allow(dead_code))]
impl SaveAsScriptOption {
    pub const fn conversion_target(self) -> Option<ConversionTarget> {
        match self {
            Self::KeepCurrent => None,
            Self::ConvertTo(target) => Some(target),
        }
    }

    pub const fn is_converting(self) -> bool {
        matches!(self, Self::ConvertTo(_))
    }
}

/// User selection returned from the `Save As` dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveAsDialogResult {
    pub path: PathBuf,
    pub format: SaveAsFormat,
    pub conversion: Option<ConversionTarget>,
}

/// Returns the valid `Script:` dropdown options for a document authored in `project_lang`.
///
/// Only ActionScript sources (`AS1`, `AS2`, `AS3`) offer cross-language conversion
/// (`AS2 -> AS3`, `AS3 -> AS2`, `AS2 -> JS`, `AS3 -> JS`, plus `AS1 -> AS2/AS3/JS`);
/// `JS/TS -> AS*` is intentionally omitted.
pub fn available_script_options(project_lang: ScriptLanguage) -> Vec<(SaveAsScriptOption, String)> {
    let keep = (
        SaveAsScriptOption::KeepCurrent,
        format!("Keep current ({})", project_lang.display_label()),
    );
    match project_lang {
        ScriptLanguage::ActionScript2 => vec![
            keep,
            (
                SaveAsScriptOption::ConvertTo(ConversionTarget::ActionScript3),
                "Convert scripts to ActionScript 3.0".to_owned(),
            ),
            (
                SaveAsScriptOption::ConvertTo(ConversionTarget::JavaScript),
                "Convert scripts to JavaScript".to_owned(),
            ),
        ],
        ScriptLanguage::ActionScript1 => vec![
            keep,
            (
                SaveAsScriptOption::ConvertTo(ConversionTarget::ActionScript2),
                "Convert scripts to ActionScript 2.0".to_owned(),
            ),
            (
                SaveAsScriptOption::ConvertTo(ConversionTarget::ActionScript3),
                "Convert scripts to ActionScript 3.0".to_owned(),
            ),
            (
                SaveAsScriptOption::ConvertTo(ConversionTarget::JavaScript),
                "Convert scripts to JavaScript".to_owned(),
            ),
        ],
        ScriptLanguage::ActionScript3 => vec![
            keep,
            (
                SaveAsScriptOption::ConvertTo(ConversionTarget::ActionScript2),
                "Convert scripts to ActionScript 2.0".to_owned(),
            ),
            (
                SaveAsScriptOption::ConvertTo(ConversionTarget::JavaScript),
                "Convert scripts to JavaScript".to_owned(),
            ),
        ],
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => vec![keep],
    }
}

/// Computes the updated filename in the `Save As` text field when the user switches `Format` or
/// `Script` (appending/removing `" (converted)"` like After Effects / Illustrator without losing
/// custom stem edits).
pub fn update_save_as_filename(
    current_filename: &str,
    format: SaveAsFormat,
    converting: bool,
) -> String {
    let trimmed = current_filename.trim();
    let raw_stem = strip_save_as_extension(trimmed);
    let base_stem = if raw_stem.is_empty() {
        "Untitled"
    } else {
        raw_stem
    };

    let clean_stem = strip_converted_suffix(base_stem);
    let next_stem = if converting {
        format!("{clean_stem} (converted)")
    } else {
        clean_stem.to_owned()
    };

    format!("{next_stem}.{}", format.extension())
}

/// Ensures `path` carries the extension matching `format` if no supported extension is present.
pub fn normalize_save_as_result_path(path: PathBuf, format: SaveAsFormat) -> PathBuf {
    let matches_known = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("nflash")
                || ext.eq_ignore_ascii_case("fla")
                || ext.eq_ignore_ascii_case("xfl")
        });
    if matches_known {
        let current_ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if current_ext.eq_ignore_ascii_case(format.extension()) {
            path
        } else {
            path.with_extension(format.extension())
        }
    } else {
        path.with_extension(format.extension())
    }
}

fn strip_save_as_extension(name: &str) -> &str {
    for ext in [".nflash", ".fla", ".xfl", ".json"] {
        if name.len() > ext.len() && name[name.len() - ext.len()..].eq_ignore_ascii_case(ext) {
            return &name[..name.len() - ext.len()];
        }
    }
    name
}

fn strip_converted_suffix(stem: &str) -> &str {
    const SUFFIX: &str = " (converted)";
    if stem.len() > SUFFIX.len() && stem[stem.len() - SUFFIX.len()..].eq_ignore_ascii_case(SUFFIX) {
        &stem[..stem.len() - SUFFIX.len()]
    } else {
        stem
    }
}

/// Opens the platform `Save As` dialog with `Format` and `Script` selectors.
pub fn prompt_save_as_dialog(
    default_dir: Option<&Path>,
    default_name: &str,
    #[cfg_attr(any(not(target_os = "macos"), test), allow(unused_variables))]
    project_lang: ScriptLanguage,
) -> Option<SaveAsDialogResult> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        if let Some(res) = macos::run_native_save_panel(default_dir, default_name, project_lang) {
            return res;
        }
    }
    fallback_rfd_save_dialog(default_dir, default_name)
}

fn fallback_rfd_save_dialog(
    default_dir: Option<&Path>,
    default_name: &str,
) -> Option<SaveAsDialogResult> {
    let mut dialog = rfd::FileDialog::new()
        .add_filter("NeoFlash project", &["nflash"])
        .add_filter("Flash FLA (Animate CC / CS6–CS4 / Flash 8)", &["fla"])
        .add_filter("Uncompressed XFL directory", &["xfl"])
        .set_file_name(default_name);
    if let Some(dir) = default_dir {
        dialog = dialog.set_directory(dir);
    }
    let path = dialog.save_file()?;
    let format = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("fla") => SaveAsFormat::FlashFla,
        Some(ext) if ext.eq_ignore_ascii_case("xfl") => SaveAsFormat::UncompressedXfl,
        _ => SaveAsFormat::NeoFlash,
    };
    Some(SaveAsDialogResult {
        path,
        format,
        conversion: None,
    })
}

#[cfg(all(target_os = "macos", not(test)))]
mod macos {
    use super::*;
    use objc2::{
        MainThreadMarker, ffi, msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, Bool, Imp, Sel},
        sel,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use std::cell::RefCell;
    use std::ffi::{CStr, CString, c_char};
    use std::sync::OnceLock;

    const NS_MODAL_RESPONSE_OK: isize = 1;
    const NS_TEXT_ALIGNMENT_RIGHT: isize = 2;

    struct ActivePanelRefs {
        panel: *mut AnyObject,
        format_popup: *mut AnyObject,
        script_popup: *mut AnyObject,
        script_options: Vec<SaveAsScriptOption>,
    }

    thread_local! {
        static ACTIVE_PANEL: RefCell<Option<ActivePanelRefs>> = const { RefCell::new(None) };
    }

    struct ActivePanelGuard;
    impl Drop for ActivePanelGuard {
        fn drop(&mut self) {
            ACTIVE_PANEL.with(|cell| {
                *cell.borrow_mut() = None;
            });
        }
    }

    type PopupChangedFn = unsafe extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>);

    unsafe extern "C-unwind" fn popup_changed(
        _target: &AnyObject,
        _sel: Sel,
        _sender: Option<&AnyObject>,
    ) {
        ACTIVE_PANEL.with(|cell| {
            let borrow = cell.borrow();
            let Some(refs) = borrow.as_ref() else {
                return;
            };
            if refs.panel.is_null() || refs.format_popup.is_null() || refs.script_popup.is_null() {
                return;
            }
            let fmt_idx: isize = unsafe { msg_send![refs.format_popup, indexOfSelectedItem] };
            let scr_idx: isize = unsafe { msg_send![refs.script_popup, indexOfSelectedItem] };
            let format = SaveAsFormat::from_index(fmt_idx.max(0) as usize);
            let script_opt = refs
                .script_options
                .get(scr_idx.max(0) as usize)
                .copied()
                .unwrap_or(SaveAsScriptOption::KeepCurrent);

            let current_ns: *const AnyObject =
                unsafe { msg_send![refs.panel, nameFieldStringValue] };
            let current_name = if current_ns.is_null() {
                String::new()
            } else {
                unsafe { ns_string_to_rust(&*current_ns) }.unwrap_or_default()
            };
            let next_name =
                update_save_as_filename(&current_name, format, script_opt.is_converting());
            if let Some(ns_next) = ns_string(&next_name) {
                let _: () = unsafe { msg_send![refs.panel, setNameFieldStringValue: &*ns_next] };
            }
        });
    }

    fn ensure_accessory_target_class() -> Option<&'static AnyClass> {
        static CLASS: OnceLock<Option<&'static AnyClass>> = OnceLock::new();
        *CLASS.get_or_init(|| {
            if let Some(existing) = AnyClass::get(c"NeoFlashSavePanelAccessoryTarget") {
                return Some(existing);
            }
            let superclass = AnyClass::get(c"NSObject")?;
            let cls = unsafe {
                ffi::objc_allocateClassPair(
                    superclass,
                    c"NeoFlashSavePanelAccessoryTarget".as_ptr(),
                    0,
                )
            };
            if cls.is_null() {
                return None;
            }
            let imp = unsafe { std::mem::transmute::<PopupChangedFn, Imp>(popup_changed) };
            let added =
                unsafe { ffi::class_addMethod(cls, sel!(popupChanged:), imp, c"v@:@".as_ptr()) };
            if !added.as_bool() {
                return None;
            }
            unsafe { ffi::objc_registerClassPair(cls) };
            AnyClass::get(c"NeoFlashSavePanelAccessoryTarget")
        })
    }

    fn ns_string(text: &str) -> Option<Retained<AnyObject>> {
        let cls = AnyClass::get(c"NSString")?;
        let clean = text.replace('\0', "");
        let cstr = CString::new(clean).ok()?;
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

    fn make_label(text: &str, frame: NSRect) -> Option<Retained<AnyObject>> {
        let cls = AnyClass::get(c"NSTextField")?;
        let ns_text = ns_string(text)?;
        unsafe {
            let raw: *mut AnyObject = msg_send![cls, labelWithString: &*ns_text];
            let label = Retained::retain(raw)?;
            let _: () = msg_send![&*label, setFrame: frame];
            let _: () = msg_send![&*label, setAlignment: NS_TEXT_ALIGNMENT_RIGHT];
            Some(label)
        }
    }

    fn make_popup(
        items: &[String],
        frame: NSRect,
        target: &AnyObject,
        enabled: bool,
    ) -> Option<Retained<AnyObject>> {
        let cls = AnyClass::get(c"NSPopUpButton")?;
        unsafe {
            let raw: *mut AnyObject = msg_send![cls, alloc];
            if raw.is_null() {
                return None;
            }
            let initialized: *mut AnyObject =
                msg_send![raw, initWithFrame: frame, pullsDown: Bool::NO];
            let popup = Retained::from_raw(initialized)?;
            for title in items {
                if let Some(ns_title) = ns_string(title) {
                    let _: () = msg_send![&*popup, addItemWithTitle: &*ns_title];
                }
            }
            let _: () = msg_send![&*popup, setTarget: target];
            let _: () = msg_send![&*popup, setAction: sel!(popupChanged:)];
            let _: () = msg_send![&*popup, setEnabled: Bool::new(enabled)];
            Some(popup)
        }
    }

    /// Runs `NSSavePanel` with an Illustrator-style `Format` + `Script` accessory view.
    /// Returns `None` only if AppKit classes could not be initialized on the main thread;
    /// returns `Some(None)` when the user cancels the modal Save sheet.
    pub fn run_native_save_panel(
        default_dir: Option<&Path>,
        default_name: &str,
        project_lang: ScriptLanguage,
    ) -> Option<Option<SaveAsDialogResult>> {
        let _mtm = MainThreadMarker::new()?;
        let panel_cls = AnyClass::get(c"NSSavePanel")?;
        let view_cls = AnyClass::get(c"NSView")?;
        let target_cls = ensure_accessory_target_class()?;

        let target: Retained<AnyObject> = unsafe {
            let raw: *mut AnyObject = msg_send![target_cls, new];
            Retained::from_raw(raw)?
        };

        let panel: Retained<AnyObject> = unsafe {
            let raw: *mut AnyObject = msg_send![panel_cls, savePanel];
            Retained::retain(raw)?
        };

        if let Some(ns_title) = ns_string("Save As") {
            let _: () = unsafe { msg_send![&*panel, setTitle: &*ns_title] };
        }
        let _: () = unsafe { msg_send![&*panel, setCanCreateDirectories: Bool::YES] };
        let _: () = unsafe { msg_send![&*panel, setExtensionHidden: Bool::NO] };

        let initial_name = update_save_as_filename(default_name, SaveAsFormat::NeoFlash, false);
        if let Some(ns_name) = ns_string(&initial_name) {
            let _: () = unsafe { msg_send![&*panel, setNameFieldStringValue: &*ns_name] };
        }

        if let Some(dir) = default_dir
            && let Some(dir_str) = dir.to_str()
            && let Some(ns_dir) = ns_string(dir_str)
            && let Some(url_cls) = AnyClass::get(c"NSURL")
        {
            let dir_url: *mut AnyObject =
                unsafe { msg_send![url_cls, fileURLWithPath: &*ns_dir, isDirectory: Bool::YES] };
            if !dir_url.is_null() {
                let _: () = unsafe { msg_send![&*panel, setDirectoryURL: &*dir_url] };
            }
        }

        let accessory: Retained<AnyObject> = unsafe {
            let raw: *mut AnyObject = msg_send![view_cls, alloc];
            if raw.is_null() {
                return None;
            }
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(468.0, 68.0));
            let init: *mut AnyObject = msg_send![raw, initWithFrame: frame];
            Retained::from_raw(init)?
        };

        let format_labels: Vec<String> = SaveAsFormat::ALL
            .iter()
            .map(|f| f.label().to_owned())
            .collect();
        let script_pairs = available_script_options(project_lang);
        let script_opts: Vec<SaveAsScriptOption> =
            script_pairs.iter().map(|(opt, _)| *opt).collect();
        let script_labels: Vec<String> = script_pairs.into_iter().map(|(_, label)| label).collect();

        let fmt_label = make_label(
            "Format:",
            NSRect::new(NSPoint::new(8.0, 39.0), NSSize::new(72.0, 20.0)),
        )?;
        let fmt_popup = make_popup(
            &format_labels,
            NSRect::new(NSPoint::new(88.0, 35.0), NSSize::new(368.0, 26.0)),
            &target,
            true,
        )?;
        let scr_label = make_label(
            "Script:",
            NSRect::new(NSPoint::new(8.0, 9.0), NSSize::new(72.0, 20.0)),
        )?;
        let scr_popup = make_popup(
            &script_labels,
            NSRect::new(NSPoint::new(88.0, 5.0), NSSize::new(368.0, 26.0)),
            &target,
            script_opts.len() > 1,
        )?;

        unsafe {
            let _: () = msg_send![&*accessory, addSubview: &*fmt_label];
            let _: () = msg_send![&*accessory, addSubview: &*fmt_popup];
            let _: () = msg_send![&*accessory, addSubview: &*scr_label];
            let _: () = msg_send![&*accessory, addSubview: &*scr_popup];
            let _: () = msg_send![&*panel, setAccessoryView: &*accessory];
        }

        let _guard = ActivePanelGuard;
        ACTIVE_PANEL.with(|cell| {
            *cell.borrow_mut() = Some(ActivePanelRefs {
                panel: Retained::as_ptr(&panel) as *mut AnyObject,
                format_popup: Retained::as_ptr(&fmt_popup) as *mut AnyObject,
                script_popup: Retained::as_ptr(&scr_popup) as *mut AnyObject,
                script_options: script_opts.clone(),
            });
        });

        let response: isize = unsafe { msg_send![&*panel, runModal] };
        if response != NS_MODAL_RESPONSE_OK {
            return Some(None);
        }

        let fmt_idx: isize = unsafe { msg_send![&*fmt_popup, indexOfSelectedItem] };
        let scr_idx: isize = unsafe { msg_send![&*scr_popup, indexOfSelectedItem] };
        let format = SaveAsFormat::from_index(fmt_idx.max(0) as usize);
        let conversion = script_opts
            .get(scr_idx.max(0) as usize)
            .copied()
            .and_then(SaveAsScriptOption::conversion_target);

        let url_ptr: *const AnyObject = unsafe { msg_send![&*panel, URL] };
        if url_ptr.is_null() {
            return Some(None);
        }
        let path_ns: *const AnyObject = unsafe { msg_send![&*url_ptr, path] };
        if path_ns.is_null() {
            return Some(None);
        }
        let raw_path = unsafe { ns_string_to_rust(&*path_ns) }?;
        if raw_path.is_empty() {
            return Some(None);
        }
        let normalized = normalize_save_as_result_path(PathBuf::from(raw_path), format);
        Some(Some(SaveAsDialogResult {
            path: normalized,
            format,
            conversion,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_options_skip_js_to_as_and_offer_as2_as3_js_matrix() {
        let as2 = available_script_options(ScriptLanguage::ActionScript2);
        assert_eq!(
            as2.iter().map(|(o, _)| *o).collect::<Vec<_>>(),
            vec![
                SaveAsScriptOption::KeepCurrent,
                SaveAsScriptOption::ConvertTo(ConversionTarget::ActionScript3),
                SaveAsScriptOption::ConvertTo(ConversionTarget::JavaScript),
            ]
        );

        let as3 = available_script_options(ScriptLanguage::ActionScript3);
        assert_eq!(
            as3.iter().map(|(o, _)| *o).collect::<Vec<_>>(),
            vec![
                SaveAsScriptOption::KeepCurrent,
                SaveAsScriptOption::ConvertTo(ConversionTarget::ActionScript2),
                SaveAsScriptOption::ConvertTo(ConversionTarget::JavaScript),
            ]
        );

        let js = available_script_options(ScriptLanguage::JavaScript);
        assert_eq!(js.len(), 1);
        assert_eq!(js[0].0, SaveAsScriptOption::KeepCurrent);

        let ts = available_script_options(ScriptLanguage::TypeScript);
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].0, SaveAsScriptOption::KeepCurrent);
    }

    #[test]
    fn filename_transitions_append_and_strip_converted_and_switch_extensions() {
        let f1 = update_save_as_filename("Arcade.nflash", SaveAsFormat::NeoFlash, true);
        assert_eq!(f1, "Arcade (converted).nflash");

        // Switching to another conversion target must not double-append ` (converted)`.
        let f2 = update_save_as_filename(&f1, SaveAsFormat::FlashFla, true);
        assert_eq!(f2, "Arcade (converted).fla");

        // Switching back to `Keep current` removes ` (converted)` while keeping chosen format.
        let f3 = update_save_as_filename(&f2, SaveAsFormat::FlashFla, false);
        assert_eq!(f3, "Arcade.fla");

        let f4 = update_save_as_filename(&f3, SaveAsFormat::UncompressedXfl, false);
        assert_eq!(f4, "Arcade.xfl");
    }

    #[test]
    fn normalize_save_as_result_path_enforces_selected_format_extension() {
        assert_eq!(
            normalize_save_as_result_path(
                PathBuf::from("/tmp/Demo (converted)"),
                SaveAsFormat::NeoFlash
            ),
            PathBuf::from("/tmp/Demo (converted).nflash")
        );
        assert_eq!(
            normalize_save_as_result_path(
                PathBuf::from("/tmp/Demo (converted).nflash"),
                SaveAsFormat::FlashFla
            ),
            PathBuf::from("/tmp/Demo (converted).fla")
        );
    }
}
