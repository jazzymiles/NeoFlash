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

/// Only remove a suffix that this dialog added. A document may already be named
/// `Scene (converted).nflash` before the dialog opens.
#[derive(Default)]
pub(crate) struct SaveAsFilename {
    added_suffix: bool,
}

impl SaveAsFilename {
    pub(crate) fn update(
        &mut self,
        current_filename: &str,
        format: SaveAsFormat,
        converting: bool,
    ) -> String {
        let name = current_filename.trim();
        let stem = [".nflash", ".fla", ".xfl", ".json"]
            .iter()
            .find_map(|ext| strip_ascii_suffix(name, ext))
            .unwrap_or(name);
        let stem = if stem.is_empty() { "Untitled" } else { stem };
        const SUFFIX: &str = " (converted)";
        let unsuffixed = strip_ascii_suffix(stem, SUFFIX);
        if converting && unsuffixed.is_none() {
            self.added_suffix = true;
            format!("{stem}{SUFFIX}.{}", format.extension())
        } else {
            let stem = if !converting && std::mem::take(&mut self.added_suffix) {
                unsuffixed.unwrap_or(stem)
            } else {
                stem
            };
            format!("{stem}.{}", format.extension())
        }
    }
}

fn strip_ascii_suffix<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let (stem, tail) = text.split_at_checked(text.len().checked_sub(suffix.len())?)?;
    (!stem.is_empty() && tail.eq_ignore_ascii_case(suffix)).then_some(stem)
}

/// The returned URL is the destination for which AppKit checked overwrite consent.
fn confirmed_save_as_path(path: PathBuf, format: SaveAsFormat) -> Option<PathBuf> {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(format.extension()))
        .then_some(path)
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
    use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};
    use std::cell::RefCell;
    use std::ffi::{CStr, CString, c_char};
    use std::sync::OnceLock;

    const NS_MODAL_RESPONSE_OK: isize = 1;
    const NS_TEXT_ALIGNMENT_RIGHT: isize = 2;

    struct PanelRename {
        name: String,
        directory: Option<Retained<AnyObject>>,
        tags: Option<Retained<AnyObject>>,
    }

    struct ActivePanelRefs {
        panel: *mut AnyObject,
        format_popup: *mut AnyObject,
        script_popup: *mut AnyObject,
        script_options: Vec<SaveAsScriptOption>,
        filename: SaveAsFilename,
        rename: Option<PanelRename>,
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
        sender: Option<&AnyObject>,
    ) {
        ACTIVE_PANEL.with(|cell| {
            let mut borrow = cell.borrow_mut();
            let Some(refs) = borrow.as_mut() else {
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
            let next_name = refs
                .filename
                .update(&current_name, format, script_opt.is_converting());
            if sender.is_some_and(|popup| std::ptr::eq(popup, refs.format_popup)) {
                unsafe { set_allowed_format(&*refs.panel, format) };
            }
            // The file-type filter updates the extension live. AppKit only accepts a new
            // nameFieldStringValue during configuration, before runModal.
            if next_name != current_name
                && Path::new(&current_name).with_extension(format.extension())
                    != Path::new(&next_name)
            {
                let directory: *mut AnyObject = unsafe { msg_send![refs.panel, directoryURL] };
                let tags: *mut AnyObject = unsafe { msg_send![refs.panel, tagNames] };
                refs.rename = Some(PanelRename {
                    name: next_name,
                    directory: unsafe { Retained::retain(directory) },
                    tags: unsafe { Retained::retain(tags) },
                });
                let _: () = unsafe { msg_send![refs.panel, cancel: refs.panel] };
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

    unsafe fn set_allowed_format(panel: &AnyObject, format: SaveAsFormat) {
        let extensions = NSArray::from_retained_slice(&[NSString::from_str(format.extension())]);
        let _: () = unsafe { msg_send![panel, setAllowedFileTypes: &*extensions] };
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

        let default_directory = default_dir.and_then(|dir| {
            let path = ns_string(dir.to_str()?)?;
            let class = AnyClass::get(c"NSURL")?;
            let url = unsafe { msg_send![class, fileURLWithPath: &*path, isDirectory: Bool::YES] };
            unsafe { Retained::retain(url) }
        });

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
        }

        let _guard = ActivePanelGuard;
        ACTIVE_PANEL.with(|cell| {
            *cell.borrow_mut() = Some(ActivePanelRefs {
                panel: std::ptr::null_mut(),
                format_popup: Retained::as_ptr(&fmt_popup) as *mut AnyObject,
                script_popup: Retained::as_ptr(&scr_popup) as *mut AnyObject,
                script_options: script_opts.clone(),
                filename: SaveAsFilename::default(),
                rename: None,
            });
        });

        let mut configuration = PanelRename {
            name: default_name.to_owned(),
            directory: default_directory,
            tags: None,
        };
        let panel = loop {
            let raw: *mut AnyObject = unsafe { msg_send![panel_cls, savePanel] };
            let panel = unsafe { Retained::retain(raw)? };
            let title = ns_string("Save As")?;
            let name = ns_string(&configuration.name)?;
            let fmt_idx: isize = unsafe { msg_send![&*fmt_popup, indexOfSelectedItem] };
            unsafe {
                let _: () = msg_send![&*panel, setTitle: &*title];
                let _: () = msg_send![&*panel, setCanCreateDirectories: Bool::YES];
                let _: () = msg_send![&*panel, setExtensionHidden: Bool::NO];
                set_allowed_format(&panel, SaveAsFormat::from_index(fmt_idx.max(0) as usize));
                let _: () = msg_send![&*panel, setAllowsOtherFileTypes: Bool::NO];
                let _: () = msg_send![&*panel, setNameFieldStringValue: &*name];
                if let Some(dir) = &configuration.directory {
                    let _: () = msg_send![&*panel, setDirectoryURL: &**dir];
                }
                if let Some(tags) = &configuration.tags {
                    let _: () = msg_send![&*panel, setTagNames: &**tags];
                }
                let _: () = msg_send![&*panel, setAccessoryView: &*accessory];
            }
            ACTIVE_PANEL.with(|cell| {
                cell.borrow_mut().as_mut().unwrap().panel = raw;
            });
            let response: isize = unsafe { msg_send![&*panel, runModal] };
            let rename = ACTIVE_PANEL.with(|cell| cell.borrow_mut().as_mut()?.rename.take());
            if let Some(rename) = rename {
                configuration = rename;
                let _: () =
                    unsafe { msg_send![&*panel, setAccessoryView: std::ptr::null::<AnyObject>()] };
            } else if response == NS_MODAL_RESPONSE_OK {
                break panel;
            } else {
                return Some(None);
            }
        };

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
        Some(
            confirmed_save_as_path(PathBuf::from(raw_path), format).map(|path| {
                SaveAsDialogResult {
                    path,
                    format,
                    conversion,
                }
            }),
        )
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
        let mut name = SaveAsFilename::default();
        assert_eq!(
            name.update("ёжик.fla", SaveAsFormat::NeoFlash, true),
            "ёжик (converted).nflash"
        );
        assert_eq!(
            name.update("🌅🌅🌅🌅.nflash", SaveAsFormat::FlashFla, false),
            "🌅🌅🌅🌅.fla"
        );
        let f1 = name.update("Arcade.nflash", SaveAsFormat::NeoFlash, true);
        assert_eq!(f1, "Arcade (converted).nflash");

        // Switching to another conversion target must not double-append ` (converted)`.
        let f2 = name.update(&f1, SaveAsFormat::FlashFla, true);
        assert_eq!(f2, "Arcade (converted).fla");

        // Switching back to `Keep current` removes ` (converted)` while keeping chosen format.
        let f3 = name.update(&f2, SaveAsFormat::FlashFla, false);
        assert_eq!(f3, "Arcade.fla");

        let f4 = name.update(&f3, SaveAsFormat::UncompressedXfl, false);
        assert_eq!(f4, "Arcade.xfl");
    }

    #[test]
    fn save_as_only_returns_the_filename_confirmed_by_the_system() {
        let confirmed = PathBuf::from("Scene.FLA");
        assert_eq!(
            confirmed_save_as_path(confirmed.clone(), SaveAsFormat::FlashFla),
            Some(confirmed.clone())
        );
        assert!(confirmed_save_as_path(confirmed, SaveAsFormat::NeoFlash).is_none());
        assert!(confirmed_save_as_path(PathBuf::from("Scene"), SaveAsFormat::NeoFlash).is_none());

        let mut name = SaveAsFilename::default();
        let existing = "Scene (converted).nflash";
        assert_eq!(
            name.update(existing, SaveAsFormat::NeoFlash, true),
            existing
        );
        assert_eq!(
            name.update(existing, SaveAsFormat::FlashFla, false),
            "Scene (converted).fla"
        );
    }
}
