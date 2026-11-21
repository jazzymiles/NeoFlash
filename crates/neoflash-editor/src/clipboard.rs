use crate::{
    app::{BottomTab, Editor, ScriptTarget},
    selection::{Selection, SelectionTarget},
};
use eframe::egui::{self, Event, Key};
use neoflash_core::{ClipboardPayload, Id, ObjectEvent, evaluate, new_id};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "clipboard_native.rs"]
mod native;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StageClipboardFlavors {
    pub(crate) generation: Option<u64>,
    pub(crate) neoflash: Option<String>,
    pub(crate) plain_text: Option<String>,
    pub(crate) html: Option<String>,
    pub(crate) pdf: Option<Vec<u8>>,
    pub(crate) eps: Option<Vec<u8>>,
    pub(crate) svg: Option<String>,
    pub(crate) png: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub(crate) struct StageClipboardBundle {
    pub(crate) neoflash_text: String,
    pub(crate) pdf_bytes: Option<Vec<u8>>,
    pub(crate) eps_bytes: Option<Vec<u8>>,
    pub(crate) svg_text: Option<String>,
}

#[cfg(test)]
pub(crate) fn set_test_stage_clipboard_flavors(
    ctx: &egui::Context,
    flavors: Option<StageClipboardFlavors>,
) {
    ctx.data_mut(|data| {
        let key = egui::Id::new("test-stage-clipboard-flavors");
        if let Some(f) = flavors {
            data.insert_temp(key, f);
        } else {
            data.remove::<StageClipboardFlavors>(key);
        }
    });
}

fn read_stage_flavors_for_paste_event(
    ctx: &egui::Context,
    generation: Option<u64>,
) -> Option<StageClipboardFlavors> {
    #[cfg(test)]
    {
        ctx.data(|data| {
            data.get_temp::<StageClipboardFlavors>(egui::Id::new("test-stage-clipboard-flavors"))
                .filter(|f| generation.is_none() || f.generation == generation)
        })
    }
    #[cfg(not(test))]
    {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if native_rich_clipboard_enabled(ctx) {
            return native::read_stage_flavors(generation);
        }
        let _ = (ctx, generation);
        let mut cb = arboard::Clipboard::new().ok()?;
        let html = cb
            .get()
            .html()
            .ok()
            .filter(|h| !h.is_empty() && neoflash_import::contains_figma_clipboard_marker(h))?;
        let png = encode_arboard_image_png(&mut cb);
        Some(StageClipboardFlavors {
            generation,
            neoflash: None,
            plain_text: None,
            html: Some(html),
            pdf: None,
            eps: None,
            svg: None,
            png,
        })
    }
}

#[cfg(not(test))]
fn encode_arboard_image_png(cb: &mut arboard::Clipboard) -> Option<Vec<u8>> {
    let img = cb.get_image().ok()?;
    let rgba =
        image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())?;
    let mut cursor = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .ok()?;
    Some(cursor.into_inner())
}

#[cfg(not(test))]
fn read_os_stage_clipboard_flavors(ctx: &egui::Context) -> Result<StageClipboardFlavors, String> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if native_rich_clipboard_enabled(ctx)
        && let Some(mut flavors) = native::read_stage_flavors(None)
    {
        if flavors.png.is_none()
            && (flavors
                .html
                .as_deref()
                .is_some_and(neoflash_import::contains_figma_clipboard_marker)
                || (flavors.neoflash.is_none()
                    && flavors.pdf.is_none()
                    && flavors.eps.is_none()
                    && flavors.svg.is_none()
                    && flavors
                        .plain_text
                        .as_deref()
                        .is_none_or(|t| t.trim().is_empty())))
            && let Ok(mut cb) = arboard::Clipboard::new()
        {
            flavors.png = encode_arboard_image_png(&mut cb);
        }
        return Ok(flavors);
    }
    let _ = ctx;
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let plain_text = cb.get_text().ok().filter(|t| !t.is_empty());
    let html = cb.get().html().ok().filter(|h| !h.is_empty());
    let wants_png = html
        .as_deref()
        .is_some_and(neoflash_import::contains_figma_clipboard_marker)
        || plain_text.as_deref().is_none_or(|t| t.trim().is_empty());
    let png = wants_png
        .then(|| encode_arboard_image_png(&mut cb))
        .flatten();
    Ok(StageClipboardFlavors {
        generation: clipboard_generation(ctx),
        neoflash: None,
        plain_text,
        html,
        pdf: None,
        eps: None,
        svg: None,
        png,
    })
}

// This belongs to the GUI context, not a document tab. Headless editors retain
// their injected clipboard path and never access the user's native clipboard.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn enable_native_rich_clipboard(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("native-rich-clipboard"), true));
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn native_rich_clipboard_enabled(ctx: &egui::Context) -> bool {
    ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::new("native-rich-clipboard"))
            .unwrap_or(false)
    })
}

fn clipboard_generation(_ctx: &egui::Context) -> Option<u64> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if native_rich_clipboard_enabled(_ctx) {
        return native::generation();
    }
    Some(0)
}

pub(crate) fn paste_generation(ctx: &egui::Context) -> Option<u64> {
    ctx.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, Event::Paste(_)))
    })
    .then(|| clipboard_generation(ctx))
    .flatten()
}

fn copy_rich_text(ctx: &egui::Context, text: &str) -> Option<u64> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if native_rich_clipboard_enabled(ctx) {
        return copy_native_rich_text(ctx, text, || native::write_text(text));
    }
    ctx.copy_text(text.to_owned());
    Some(0)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn copy_native_rich_text(
    ctx: &egui::Context,
    text: &str,
    write: impl FnOnce() -> native::WriteResult,
) -> Option<u64> {
    // Retire earlier deferred writes before the new synchronous Copy. They
    // must not overwrite it (or a newer external owner) at frame end.
    ctx.output_mut(|output| {
        output.commands.retain(|command| {
            !matches!(
                command,
                egui::OutputCommand::CopyText(_) | egui::OutputCommand::CopyImage(_)
            )
        });
    });
    match write() {
        native::WriteResult::Written(generation) => Some(generation),
        native::WriteResult::Unavailable => {
            // Keep the existing plain-text fallback, without trusting an
            // unconfirmed rich-cache generation.
            ctx.copy_text(text.to_owned());
            None
        }
        native::WriteResult::Superseded => None,
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn copy_native_stage_clipboard(
    ctx: &egui::Context,
    fallback_text: &str,
    write: impl FnOnce() -> native::WriteResult,
) -> Option<u64> {
    // Retire any deferred egui CopyText / CopyImage commands so end-of-frame platform
    // output NEVER overwrites our multi-flavor NSPasteboard generation!
    ctx.output_mut(|output| {
        output.commands.retain(|command| {
            !matches!(
                command,
                egui::OutputCommand::CopyText(_) | egui::OutputCommand::CopyImage(_)
            )
        });
    });
    match write() {
        native::WriteResult::Written(generation) => Some(generation),
        native::WriteResult::Unavailable => {
            ctx.copy_text(fallback_text.to_owned());
            None
        }
        native::WriteResult::Superseded => None,
    }
}

/// The pinned native adapter puts the initiating key immediately before its
/// Paste payload. Never infer intent from final frame modifiers or a release.
fn paste_shortcut_mode(editor: &Editor, event: &Event) -> Option<bool> {
    use crate::commands::CommandId;
    if editor.command_matches_event(CommandId::EditPasteInPlace, event) {
        return Some(true);
    }
    if editor.command_matches_event(CommandId::EditPaste, event) {
        return Some(false);
    }
    match event {
        Event::Key {
            key: Key::Paste,
            pressed: true,
            ..
        } => Some(false),
        Event::Key {
            key: Key::Insert,
            pressed: true,
            modifiers,
            ..
        } if cfg!(target_os = "windows") && modifiers.shift => Some(false),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardCommand {
    Copy,
    Cut,
    Paste,
    PasteInPlace,
}

#[derive(Clone)]
struct Scope {
    session: String,
    edit_revision: u64,
    symbol: Id,
    layer: Id,
    frame: u32,
}
impl Scope {
    fn new(editor: &Editor) -> Self {
        Self {
            session: editor.clipboard.session.clone(),
            edit_revision: editor.edit_session.revision,
            symbol: editor.symbol_id().to_owned(),
            layer: editor.symbol().timeline.layers[editor.layer].id.clone(),
            frame: editor.frame,
        }
    }
    fn matches(&self, editor: &Editor) -> bool {
        self.session == editor.clipboard.session
            && self.edit_revision == editor.edit_session.revision
            && self.symbol == editor.symbol_id()
            && self.frame == editor.frame
            && editor
                .symbol()
                .timeline
                .layers
                .get(editor.layer)
                .is_some_and(|layer| layer.id == self.layer)
            && editor.confirm.is_none()
    }
}

#[derive(Clone)]
struct TextTarget {
    focus: egui::Id,
    selection: Selection,
    bottom: BottomTab,
    script_target: ScriptTarget,
    script_event: ObjectEvent,
    selected_source: Option<String>,
}

#[derive(Clone)]
pub(crate) struct ClipboardTarget {
    viewport: egui::ViewportId,
    scope: Scope,
    text: Option<TextTarget>,
}
impl ClipboardTarget {
    pub(crate) fn is_text(&self) -> bool {
        self.text.is_some()
    }
    fn matches(&self, editor: &Editor) -> bool {
        self.scope.matches(editor)
            && self.text.as_ref().is_none_or(|text| {
                text.selection == editor.selection
                    && text.bottom == editor.bottom
                    && text.script_target == editor.script_target
                    && text.script_event == editor.script_event
                    && text.selected_source == editor.selected_source
            })
    }
    fn restore_text_focus(&self, ctx: &egui::Context) {
        if let Some(text) = &self.text {
            ctx.memory_mut(|memory| memory.request_focus(text.focus));
        }
    }
}

struct Request {
    target: ClipboardTarget,
    event: Event,
    expires: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RichTextClipboard {
    pub(crate) plain_text: String,
    pub(crate) runs: Vec<neoflash_core::TextFormatRun>,
    pub(crate) generation: Option<u64>,
}

#[cfg(not(test))]
static PROCESS_RICH_TEXT_CLIPBOARD: std::sync::Mutex<Option<RichTextClipboard>> =
    std::sync::Mutex::new(None);

#[cfg(not(test))]
fn store_process_rich_clipboard(value: Option<RichTextClipboard>) {
    if let Ok(mut guard) = PROCESS_RICH_TEXT_CLIPBOARD.lock() {
        *guard = value;
    }
}

#[cfg(test)]
fn store_process_rich_clipboard(_value: Option<RichTextClipboard>) {}

#[cfg(not(test))]
fn load_process_rich_clipboard() -> Option<RichTextClipboard> {
    PROCESS_RICH_TEXT_CLIPBOARD
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

#[cfg(test)]
fn load_process_rich_clipboard() -> Option<RichTextClipboard> {
    None
}

pub(crate) struct ClipboardState {
    pub(crate) session: String,
    pub(crate) menu_open: bool,
    pub(crate) menu_viewport: Option<egui::ViewportId>,
    pub(crate) menu_target: Option<ClipboardTarget>,
    pub(crate) rich_text: Option<RichTextClipboard>,
    pending: Option<Request>,
    view: Option<(Id, u64, [f64; 2])>,
}
impl ClipboardState {
    pub(crate) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}
impl Default for ClipboardState {
    fn default() -> Self {
        // A live-document identity, never serialized into .nflash. An opened copy
        // of the same file is a separate library, even when every stored ID matches.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            session: format!("{}-{stamp}-{}", std::process::id(), new_id("clipboard")),
            menu_open: false,
            menu_viewport: None,
            menu_target: None,
            rich_text: None,
            pending: None,
            view: None,
        }
    }
}

impl Editor {
    pub(crate) fn clipboard_target(&self, ctx: &egui::Context) -> ClipboardTarget {
        ClipboardTarget {
            viewport: ctx.viewport_id(),
            scope: Scope::new(self),
            text: ctx
                .wants_keyboard_input()
                .then(|| ctx.memory(|memory| memory.focused()))
                .flatten()
                .map(|focus| TextTarget {
                    focus,
                    selection: self.selection.clone(),
                    bottom: self.bottom,
                    script_target: self.script_target,
                    script_event: self.script_event,
                    selected_source: self.selected_source.clone(),
                }),
        }
    }
    pub(crate) fn remember_paste_view(&mut self, center: [f64; 2]) {
        if center.iter().all(|value| value.is_finite()) {
            self.clipboard.view = Some((
                self.symbol_id().to_owned(),
                self.edit_session.revision,
                center,
            ));
        }
    }
    pub(crate) fn paste_center(&self) -> [f64; 2] {
        self.clipboard
            .view
            .as_ref()
            .filter(|(symbol, revision, _)| {
                symbol == self.symbol_id() && *revision == self.edit_session.revision
            })
            .map(|(_, _, center)| *center)
            .unwrap_or_else(|| {
                if !self.editing_scene() {
                    [0., 0.]
                } else {
                    [
                        self.project.width as f64 / 2.,
                        self.project.height as f64 / 2.,
                    ]
                }
            })
    }
    pub(crate) fn copy_stage_text_selection(&mut self, ctx: &egui::Context, cut: bool) -> bool {
        if !self.has_document() || self.confirm.is_some() {
            return false;
        }
        self.sync_stage_text_edit();
        if !self.stage_text_edit.is_active(self) {
            return false;
        }
        if !self.stage_text_edit.has_range_selection() {
            // Collapsed caret in inline text edit: no-op so Copy/Cut never copies/deletes the whole TextBox.
            return true;
        }
        let Some(sampled) = self.selected_sampled() else {
            return false;
        };
        let Some(mut state) = neoflash_core::TextFieldState::from_kind(&sampled.kind) else {
            return false;
        };
        if cut && !self.selected_editable() {
            self.message("Cut blocked: selected text is not editable.");
            return true;
        }
        let (lo, hi) = self.stage_text_edit.selection_range_utf16();
        let plain_text = neoflash_core::utf16_slice_safe(&state.text, lo, hi)
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let runs = state.slice_runs_utf16(lo, hi);
        let rich = RichTextClipboard {
            plain_text: plain_text.clone(),
            runs,
            generation: copy_rich_text(ctx, &plain_text),
        };
        for sess in self.sessions.iter_mut().flatten() {
            sess.clipboard.rich_text = None;
        }
        self.clipboard.rich_text = Some(rich.clone());
        self.stage_text_edit.copied_rich_text = Some(rich.clone());
        store_process_rich_clipboard(Some(rich));

        if cut {
            self.finish_stage_text_typing();
            self.finish_property_edit();
            let Ok(new_caret) = state.replace_utf16_range(lo, hi, "", None) else {
                return true;
            };
            let mut next_kind = sampled.kind.clone();
            state.write_to_kind(&mut next_kind);
            self.refresh_non_wrapping_text_box_bounds(&mut next_kind);
            self.playing = false;
            self.transact_named("Cut text", |s| {
                s.ensure_selected_key();
                if let Some(el) = s.selected_element_mut() {
                    crate::stage_text_edit::preserve_point_text_alpha_on_upgrade(
                        &el.kind,
                        &next_kind,
                        &mut el.transform.alpha,
                    );
                    el.kind = next_kind;
                }
            });
            self.stage_text_edit.caret_utf16 = new_caret;
            self.stage_text_edit.selection_anchor_utf16 = None;
            self.stage_text_edit.typing_format = None;
            self.stage_text_edit.preferred_x = None;
            self.message("Cut text.");
        } else {
            self.message("Copied text.");
        }
        true
    }

    pub(crate) fn capture_stage_clipboard_bundle(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<(StageClipboardBundle, usize), String> {
        self.finish_edits();
        self.normalize_targets();
        let selected = self.sampled_targets();
        if selected.is_empty() {
            return Err("Nothing selected.".into());
        }
        let scene = evaluate(&self.project, self.symbol_id(), self.frame);
        let painter = ctx.layer_painter(egui::LayerId::background());
        let rect_bounds = self.selection_bounds(&scene, &painter);
        let sel_bounds = rect_bounds.map(|b| {
            [
                f64::from(b.min.x),
                f64::from(b.min.y),
                f64::from(b.max.x),
                f64::from(b.max.y),
            ]
        });
        let anchor = rect_bounds
            .map(|bounds| [bounds.center().x as f64, bounds.center().y as f64])
            .unwrap_or_else(|| {
                let mut min = [f64::INFINITY; 2];
                let mut max = [f64::NEG_INFINITY; 2];
                for (_, element) in &selected {
                    min[0] = min[0].min(element.transform.x);
                    min[1] = min[1].min(element.transform.y);
                    max[0] = max[0].max(element.transform.x);
                    max[1] = max[1].max(element.transform.y);
                }
                [(min[0] + max[0]) / 2., (min[1] + max[1]) / 2.]
            });
        let count = selected.len();
        let elements: Vec<neoflash_core::Element> =
            selected.iter().map(|(_, el)| el.clone()).collect();
        let capture_project =
            build_capture_project(&self.project, self.symbol_id(), self.frame, &elements)
                .map_err(|e| e.to_string())?;
        let neoflash_text = ClipboardPayload::capture_at_frame(
            &capture_project,
            &capture_project.root,
            0,
            elements,
            &self.clipboard.session,
            anchor,
        )
        .and_then(|payload| payload.to_text())
        .map_err(|e| e.to_string())?;

        let mut vector_project = build_vector_export_project(
            &self.project,
            self.symbol_id(),
            self.frame,
            &selected,
            &capture_project,
        );
        let mut vector_sel_bounds = sel_bounds;
        if self.editing_in_place()
            && let Ok(mapping) = self.edit_stage_mapping()
        {
            let edit_to_stage = mapping.matrix();
            let vroot = vector_project.root.clone();
            if let Some(root_sym) = vector_project.symbol_mut(&vroot) {
                for layer in &mut root_sym.timeline.layers {
                    for kf in &mut layer.keyframes {
                        for el in &mut kf.elements {
                            apply_placement_matrix_to_element(el, edit_to_stage);
                        }
                    }
                }
            }
            crate::timeline_commands::upgrade_all_format_versions(&mut vector_project);
            vector_sel_bounds = sel_bounds.map(|[x0, y0, x1, y1]| {
                let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
                let mut sx0 = f64::INFINITY;
                let mut sy0 = f64::INFINITY;
                let mut sx1 = f64::NEG_INFINITY;
                let mut sy1 = f64::NEG_INFINITY;
                for (cx, cy) in corners {
                    let tx = edit_to_stage[0] * cx + edit_to_stage[2] * cy + edit_to_stage[4];
                    let ty = edit_to_stage[1] * cx + edit_to_stage[3] * cy + edit_to_stage[5];
                    sx0 = sx0.min(tx);
                    sy0 = sy0.min(ty);
                    sx1 = sx1.max(tx);
                    sy1 = sy1.max(ty);
                }
                [sx0, sy0, sx1, sy1]
            });
        }
        let pdf_bytes =
            neoflash_export::export_clipboard_pdf(&vector_project, None, 0, vector_sel_bounds).ok();
        let eps_bytes =
            neoflash_export::export_clipboard_eps(&vector_project, None, 0, vector_sel_bounds).ok();
        let svg_text = neoflash_export::export_clipboard_svg(
            &vector_project,
            None,
            0,
            vector_sel_bounds,
            Some(&neoflash_text),
        )
        .ok();

        Ok((
            StageClipboardBundle {
                neoflash_text,
                pdf_bytes,
                eps_bytes,
                svg_text,
            },
            count,
        ))
    }

    pub(crate) fn copy_selection(&mut self, ctx: &egui::Context, cut: bool) {
        if !self.has_document() || self.confirm.is_some() || self.is_standalone_script_session() {
            return;
        }
        if self.stage_text_edit.is_active(self) {
            self.copy_stage_text_selection(ctx, cut);
            return;
        }
        for sess in self.sessions.iter_mut().flatten() {
            sess.clipboard.rich_text = None;
        }
        self.clipboard.rich_text = None;
        self.stage_text_edit.copied_rich_text = None;
        store_process_rich_clipboard(None);

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if native_rich_clipboard_enabled(ctx) {
            match self.capture_stage_clipboard_bundle(ctx) {
                Ok((bundle, count)) => {
                    copy_native_stage_clipboard(ctx, &bundle.neoflash_text, || {
                        native::write_stage_clipboard(&native::StageClipboardWritePayload {
                            neoflash_text: &bundle.neoflash_text,
                            pdf_bytes: bundle.pdf_bytes.as_deref(),
                            eps_bytes: bundle.eps_bytes.as_deref(),
                            svg_text: bundle.svg_text.as_deref(),
                        })
                    });
                    if cut {
                        self.transact_named("Cut", |s| {
                            s.delete_selected();
                        });
                    }
                    self.message(format!(
                        "{} {count} object(s).",
                        if cut { "Cut" } else { "Copied" }
                    ));
                }
                Err(err) if err == "Nothing selected." => {}
                Err(error) => self.message(format!("Copy failed: {error}")),
            }
            return;
        }

        self.finish_edits();
        self.normalize_targets();
        let selected = self.sampled_targets();
        if selected.is_empty() {
            return;
        }
        let scene = evaluate(&self.project, self.symbol_id(), self.frame);
        let painter = ctx.layer_painter(egui::LayerId::background());
        let anchor = self
            .selection_bounds(&scene, &painter)
            .map(|bounds| [bounds.center().x as f64, bounds.center().y as f64])
            .unwrap_or_else(|| {
                // Empty/invisible symbol instances still have a copyable placement.
                let mut min = [f64::INFINITY; 2];
                let mut max = [f64::NEG_INFINITY; 2];
                for (_, element) in &selected {
                    min[0] = min[0].min(element.transform.x);
                    min[1] = min[1].min(element.transform.y);
                    max[0] = max[0].max(element.transform.x);
                    max[1] = max[1].max(element.transform.y);
                }
                [(min[0] + max[0]) / 2., (min[1] + max[1]) / 2.]
            });
        let count = selected.len();
        let elements: Vec<neoflash_core::Element> =
            selected.into_iter().map(|(_, element)| element).collect();
        let prepared =
            build_capture_project(&self.project, self.symbol_id(), self.frame, &elements)
                .and_then(|capture_project| {
                    ClipboardPayload::capture_at_frame(
                        &capture_project,
                        &capture_project.root,
                        0,
                        elements,
                        &self.clipboard.session,
                        anchor,
                    )
                })
                .and_then(|payload| payload.to_text());
        match prepared {
            Ok(text) => {
                ctx.copy_text(text);
                if cut {
                    // Prepare the complete clipboard before removing any artwork.
                    // Wrap in a named "Cut" transaction so Cut is recoverable in one Undo step.
                    self.transact_named("Cut", |s| {
                        s.delete_selected();
                    });
                }
                self.message(format!(
                    "{} {count} object(s).",
                    if cut { "Cut" } else { "Copied" }
                ));
            }
            Err(error) => self.message(format!("Copy failed: {error}")),
        }
    }
    pub(crate) fn is_stage_inline_text_editing(&mut self) -> bool {
        self.sync_stage_text_edit();
        self.stage_text_edit.is_active(self)
    }
    pub(crate) fn paste_objects(&mut self, ctx: &egui::Context, text: &str, in_place: bool) {
        self.paste_objects_with_generation(ctx, text, in_place, clipboard_generation(ctx));
    }
    pub(crate) fn paste_objects_with_generation(
        &mut self,
        ctx: &egui::Context,
        text: &str,
        in_place: bool,
        generation: Option<u64>,
    ) {
        let stage_flavors = if !self.has_document()
            || self.confirm.is_some()
            || self.is_standalone_script_session()
            || self.is_stage_inline_text_editing()
            || matches!(ClipboardPayload::from_text(text), Ok(Some(_)) | Err(_))
            || neoflash_import::contains_figma_clipboard_marker(text)
        {
            None
        } else {
            read_stage_flavors_for_paste_event(ctx, generation)
        };
        self.paste_objects_with_stage_flavors(
            ctx,
            text,
            in_place,
            generation,
            stage_flavors.as_ref(),
        );
    }
    pub(crate) fn paste_objects_with_stage_flavors(
        &mut self,
        ctx: &egui::Context,
        text: &str,
        in_place: bool,
        generation: Option<u64>,
        stage_flavors: Option<&StageClipboardFlavors>,
    ) {
        let explicit_in_place = in_place;
        // Preferences ▸ Clipboard ▸ Default paste = In place: plain Paste keeps the position too.
        let in_place = in_place || self.workspace.paste_in_place;
        if !self.has_document() || self.confirm.is_some() || self.is_standalone_script_session() {
            return;
        }
        // Active inline Stage TextBox editing owns Paste BEFORE `ClipboardPayload::from_text`:
        // never duplicate Stage objects or exit inline text editing while typing!
        if self.is_stage_inline_text_editing() {
            if explicit_in_place {
                return;
            }
            if stage_flavors.and_then(|f| f.neoflash.as_deref()).is_some()
                || matches!(ClipboardPayload::from_text(text), Ok(Some(_)) | Err(_))
            {
                return;
            }
            if !self.paste_plain_text_into_selected_text(text, generation) {
                self.message("The clipboard does not contain NeoFlash objects.");
            }
            return;
        }

        // Priority 1: Lossless NeoFlash round-trip (`app.neoflash.stage-clipboard`, raw
        // `NEOFLASH-CLIPBOARD\n...`, or `<svg>` carrying `<!--(neoflash)<b64>(/neoflash)-->`).
        let mut resolved_payload = None;
        for candidate_str in [
            stage_flavors.and_then(|f| f.neoflash.as_deref()),
            Some(text),
            stage_flavors.and_then(|f| f.plain_text.as_deref()),
            stage_flavors.and_then(|f| f.svg.as_deref()),
        ]
        .into_iter()
        .flatten()
        {
            match ClipboardPayload::from_text(candidate_str) {
                Ok(Some(p)) => {
                    resolved_payload = Some(p);
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    self.message(format!("Paste failed: {error}"));
                    return;
                }
            }
        }

        let Some(payload) = resolved_payload else {
            // Priority 2: Native Figma `fig-kiwi` HTML (`<!--(figma)...(/figma)-->`).
            let figma_source = neoflash_import::contains_figma_clipboard_marker(text)
                .then_some(text)
                .or_else(|| {
                    stage_flavors.and_then(|f| {
                        f.html
                            .as_deref()
                            .filter(|h| neoflash_import::contains_figma_clipboard_marker(h))
                            .or_else(|| {
                                f.plain_text
                                    .as_deref()
                                    .filter(|t| neoflash_import::contains_figma_clipboard_marker(t))
                            })
                    })
                });
            if let Some(figma_html) = figma_source {
                if figma_html.len() > neoflash_import::figma::MAX_STAGE_HTML_CLIPBOARD_BYTES {
                    self.message("Paste failed: Figma clipboard exceeds the 128 MiB limit.");
                    return;
                }
                let fallback_png = stage_flavors.and_then(|f| f.png.as_deref());
                match neoflash_import::import_figma_clipboard(figma_html, fallback_png) {
                    Ok(output) => {
                        self.paste_figma_clipboard_output(ctx, output, in_place);
                    }
                    Err(error) => self.message(format!("Paste failed: {error}")),
                }
                return;
            }

            let mut had_vector_flavor = false;

            // Priority 3: Adobe Illustrator / Vector PDF (`com.adobe.pdf` / `public.pdf`).
            if let Some(pdf_bytes) = stage_flavors.and_then(|f| f.pdf.as_deref()) {
                had_vector_flavor = true;
                if self.paste_pdf_clipboard(ctx, pdf_bytes, in_place) {
                    return;
                }
            }

            // Priority 4: Adobe Illustrator AICB / PostScript Level 3 EPS (`com.adobe.encapsulated-postscript`).
            if let Some(eps_bytes) = stage_flavors.and_then(|f| f.eps.as_deref()) {
                had_vector_flavor = true;
                if self.paste_eps_clipboard(ctx, eps_bytes, in_place) {
                    return;
                }
            }

            // Priority 5: Clean SVG (`public.svg-image`, `com.adobe.svg`, or `<svg...>` plain text).
            if let Some(svg) = stage_flavors
                .and_then(|f| f.svg.as_deref())
                .filter(|s| is_svg_clipboard_text(s))
            {
                had_vector_flavor = true;
                if self.paste_svg(ctx, svg, in_place) {
                    return;
                }
            } else if is_svg_clipboard_text(text) {
                had_vector_flavor = true;
                if self.paste_svg(ctx, text, in_place) {
                    return;
                }
            }

            if had_vector_flavor {
                // Vector flavors existed and failed validation/decoding; preserve `Paste failed: ...`
                // and NEVER silently paste a flat raster PNG screenshot in place of vector artwork!
                return;
            }

            // Priority 6: Plain text (`data:image/...` or selected Stage text replacement) / PNG raster fallback.
            if !text.trim().is_empty() {
                if let Some((media_type, bytes)) = parse_image_data_url(text) {
                    self.paste_image_bytes(ctx, &bytes, &media_type, in_place);
                    return;
                }
                if self.paste_plain_text_into_selected_text(text, generation) {
                    return;
                }
            }
            if let Some(png) = stage_flavors.and_then(|f| f.png.as_deref()) {
                self.paste_image_bytes(ctx, png, "image/png", in_place);
                return;
            }
            self.message("The clipboard does not contain NeoFlash objects.");
            return;
        };
        self.finish_edits();
        if !self.layer_editable() {
            self.message("Paste failed: destination layer is locked or hidden.");
            return;
        }
        let symbol = self.symbol_id().to_owned();
        let layer = self.symbol().timeline.layers[self.layer].id.clone();
        let center = self.paste_center();
        let anchor = payload.anchor();
        let offset = if in_place {
            [0., 0.]
        } else {
            [center[0] - anchor[0], center[1] - anchor[1]]
        };
        let paste_result = if payload.source_session() == self.clipboard.session {
            payload
                .paste_into(
                    &self.project,
                    &self.clipboard.session,
                    &symbol,
                    &layer,
                    self.frame,
                    offset,
                )
                .map_err(|e| e.to_string())
        } else {
            paste_cross_document_payload(
                &self.project,
                &payload,
                &symbol,
                &layer,
                self.frame,
                offset,
            )
        };
        match paste_result {
            Ok(result) => {
                let count = result.element_ids.len();
                self.set_playing(false);
                let tx_label = if in_place { "Paste in place" } else { "Paste" };
                self.transact_named(tx_label, |editor| {
                    editor.project = result.project;
                    editor.set_selection(
                        result
                            .element_ids
                            .into_iter()
                            .map(|element| SelectionTarget {
                                layer: layer.clone(),
                                element,
                            })
                            .collect(),
                    );
                });
                self.load_project_fonts(ctx);
                self.message(format!("Pasted {count} object(s)."));
            }
            Err(error) => self.message(format!("Paste failed: {error}")),
        }
    }
    pub(crate) fn cached_rich_text(
        &self,
        text: &str,
        generation: Option<u64>,
        process: Option<RichTextClipboard>,
    ) -> Option<RichTextClipboard> {
        let matches = |rich: &&RichTextClipboard| {
            generation.is_some() && rich.generation == generation && rich.plain_text == text
        };
        self.stage_text_edit
            .copied_rich_text
            .as_ref()
            .filter(matches)
            .or_else(|| self.clipboard.rich_text.as_ref().filter(matches))
            .or_else(|| {
                self.sessions
                    .iter()
                    .flatten()
                    .find_map(|sess| sess.clipboard.rich_text.as_ref().filter(matches))
            })
            .or_else(|| process.as_ref().filter(matches))
            .cloned()
    }
    fn paste_plain_text_into_selected_text(
        &mut self,
        raw_text: &str,
        generation: Option<u64>,
    ) -> bool {
        if self.selection.single().is_none() || !self.selected_editable() {
            return false;
        }
        self.sync_stage_text_edit();
        let Some(sampled) = self.selected_sampled() else {
            return false;
        };
        let Some(mut state) = neoflash_core::TextFieldState::from_kind(&sampled.kind) else {
            return false;
        };
        let mut normalized = raw_text.replace("\r\n", "\n").replace('\r', "\n");
        let mut pasted_runs: Vec<neoflash_core::TextFormatRun> = self
            .cached_rich_text(&normalized, generation, load_process_rich_clipboard())
            .map(|rich| rich.runs)
            .unwrap_or_default();
        if pasted_runs.is_empty()
            && let Some((html_plain, html_runs)) =
                extract_html_fragment_clipboard(&normalized, &state.format)
        {
            normalized = html_plain;
            pasted_runs = html_runs;
        }
        let candidate = match &sampled.kind {
            neoflash_core::ElementKind::Text { .. } => normalized.replace('\n', " "),
            neoflash_core::ElementKind::TextBox { multiline, .. } => {
                if *multiline {
                    normalized
                } else {
                    normalized.replace('\n', " ")
                }
            }
            _ => return false,
        };
        let inline_active = self.stage_text_edit.is_active(self);
        let (rep_start, rep_end) = if inline_active {
            self.stage_text_edit.selection_range_utf16()
        } else {
            (0, state.utf16_len())
        };
        let typing_style = if inline_active {
            self.stage_text_edit.typing_format.clone()
        } else {
            None
        };
        let state_before = state.clone();
        let new_caret = match state.replace_utf16_range_with_runs(
            rep_start,
            rep_end,
            &candidate,
            &pasted_runs,
            typing_style.as_ref(),
        ) {
            Ok(caret) => caret,
            Err(error) => {
                self.message(format!("Paste failed: {error}"));
                return true;
            }
        };
        if !candidate.is_empty() && state == state_before {
            self.message("Paste ignored: the text field rejected the inserted characters.");
            return true;
        }
        if let Err(error) = state.validate() {
            self.message(format!("Paste failed: {error}"));
            return true;
        }
        let mut next_kind = sampled.kind.clone();
        state.write_to_kind(&mut next_kind);
        self.refresh_non_wrapping_text_box_bounds(&mut next_kind);
        self.finish_stage_text_typing();
        self.finish_edits();
        self.playing = false;
        self.transact_named("Paste text", |s| {
            s.ensure_selected_key();
            if let Some(element) = s.selected_element_mut() {
                crate::stage_text_edit::preserve_point_text_alpha_on_upgrade(
                    &element.kind,
                    &next_kind,
                    &mut element.transform.alpha,
                );
                element.kind = next_kind;
            }
        });
        if inline_active {
            self.stage_text_edit.caret_utf16 = new_caret;
            self.stage_text_edit.selection_anchor_utf16 = None;
            self.stage_text_edit.preferred_x = None;
        }
        self.message("Pasted text.");
        true
    }
    pub(crate) fn paste_figma_clipboard_output(
        &mut self,
        ctx: &egui::Context,
        output: neoflash_import::ImportOutput,
        in_place: bool,
    ) -> bool {
        if !self.has_document() || self.confirm.is_some() || self.is_standalone_script_session() {
            return false;
        }
        let in_place = in_place || self.workspace.paste_in_place;
        self.finish_edits();
        let symbol_id = self.symbol_id().to_owned();
        if self.frame >= self.symbol().timeline.frame_count {
            self.message("Paste failed: destination frame is outside the timeline.");
            return false;
        }

        let sw = f64::from(self.project.width);
        let sh = f64::from(self.project.height);
        let placement_matrix = match (in_place, output.figma_bounds, output.local_bounds) {
            (false, _, Some(lb)) => {
                let target_center = self.paste_center();
                let src_cx = (lb.x_min + lb.x_max) * 0.5;
                let src_cy = (lb.y_min + lb.y_max) * 0.5;
                [
                    1.0,
                    0.0,
                    0.0,
                    1.0,
                    target_center[0] - src_cx,
                    target_center[1] - src_cy,
                ]
            }
            (true, Some(fb), Some(lb)) => {
                let near_stage = fb.x_min >= -sw
                    && fb.x_min <= 2.0 * sw
                    && fb.y_min >= -sh
                    && fb.y_min <= 2.0 * sh
                    && fb.x_max >= -sw
                    && fb.y_max >= -sh;
                let desired_top_left = if near_stage {
                    [fb.x_min, fb.y_min]
                } else {
                    [0.0, 0.0]
                };
                let stage_translation = [
                    1.0,
                    0.0,
                    0.0,
                    1.0,
                    desired_top_left[0] - lb.x_min,
                    desired_top_left[1] - lb.y_min,
                ];
                if self.editing_in_place() {
                    let Some(stage_to_edit) = self
                        .edit_stage_mapping()
                        .ok()
                        .and_then(|m| neoflash_core::invert_affine_matrix(m.matrix()))
                    else {
                        self.message("Paste failed: edited symbol placement is not invertible.");
                        return false;
                    };
                    neoflash_core::affine_multiply(stage_to_edit, stage_translation)
                } else {
                    stage_translation
                }
            }
            (true, None, _) if self.editing_in_place() => {
                let Some(stage_to_edit) = self
                    .edit_stage_mapping()
                    .ok()
                    .and_then(|m| neoflash_core::invert_affine_matrix(m.matrix()))
                else {
                    self.message("Paste failed: edited symbol placement is not invertible.");
                    return false;
                };
                stage_to_edit
            }
            _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        };

        let active_solo_hidden = self.has_any_solo_layers()
            && !self
                .symbol()
                .timeline
                .layers
                .get(self.layer)
                .is_some_and(|l| self.is_layer_id_effectively_soloed(&l.id));
        let mut candidate = self.project.clone();
        let (target_layer_idx, layer_id, element_ids, created_new_layer) =
            match merge_imported_clipboard_project(
                &mut candidate,
                output.project,
                &symbol_id,
                self.layer,
                active_solo_hidden,
                self.frame,
                placement_matrix,
                self.workspace.timeline_prefs.auto_keyframe,
                true,
            ) {
                Ok(res) => res,
                Err(error) => {
                    self.message(format!("Paste failed: {error}"));
                    return false;
                }
            };

        let count = element_ids.len();
        self.set_playing(false);
        let tx_label = if in_place { "Paste in place" } else { "Paste" };
        self.transact_named(tx_label, |editor| {
            editor.project = candidate;
            editor.layer = target_layer_idx;
            editor.set_selection(
                element_ids
                    .into_iter()
                    .map(|element| SelectionTarget {
                        layer: layer_id.clone(),
                        element,
                    })
                    .collect(),
            );
        });
        if created_new_layer && self.has_any_solo_layers() {
            self.solo_layers
                .entry(symbol_id)
                .or_default()
                .insert(layer_id);
        }
        self.load_project_fonts(ctx);

        let mut warn_count = 0usize;
        let mut first_warn: Option<String> = None;
        for d in &output.report.diagnostics {
            if d.severity == neoflash_import::Severity::Info {
                continue;
            }
            warn_count += 1;
            if first_warn.is_none() {
                first_warn = Some(format!("[{}] {}", d.code, d.message));
            }
            let sev = match d.severity {
                neoflash_import::Severity::Warning => {
                    crate::output_pipeline::OutputSeverity::Warning
                }
                neoflash_import::Severity::Error => crate::output_pipeline::OutputSeverity::Error,
                neoflash_import::Severity::Info => crate::output_pipeline::OutputSeverity::Info,
            };
            self.push_output_coalesced(crate::output_pipeline::OutputEvent::new(
                sev,
                crate::output_pipeline::OutputSource::Import,
                format!("[{}] {}: {}", d.code, d.location, d.message),
            ));
        }
        if let Some(summary) = first_warn {
            self.message(format!(
                "Pasted {count} object(s) from Figma ({warn_count} warning(s): {summary})"
            ));
        } else {
            self.message(format!("Pasted {count} object(s) from Figma."));
        }
        true
    }
    pub(crate) fn paste_pdf_clipboard(
        &mut self,
        ctx: &egui::Context,
        bytes: &[u8],
        in_place: bool,
    ) -> bool {
        if !self.has_document() || self.confirm.is_some() || self.is_standalone_script_session() {
            return false;
        }
        match neoflash_import::import_pdf_clipboard(bytes, "clipboard.pdf") {
            Ok(output) => self.paste_vector_clipboard_output(ctx, output, "PDF", in_place),
            Err(error) => {
                self.message(format!("Paste failed: {error}"));
                false
            }
        }
    }
    pub(crate) fn paste_eps_clipboard(
        &mut self,
        ctx: &egui::Context,
        bytes: &[u8],
        in_place: bool,
    ) -> bool {
        if !self.has_document() || self.confirm.is_some() || self.is_standalone_script_session() {
            return false;
        }
        match neoflash_import::import_eps_clipboard(bytes, "clipboard.eps") {
            Ok(output) => self.paste_vector_clipboard_output(ctx, output, "EPS", in_place),
            Err(error) => {
                self.message(format!("Paste failed: {error}"));
                false
            }
        }
    }
    pub(crate) fn paste_vector_clipboard_output(
        &mut self,
        ctx: &egui::Context,
        output: neoflash_import::VectorClipboardImport,
        format_label: &str,
        in_place: bool,
    ) -> bool {
        if !self.has_document() || self.confirm.is_some() || self.is_standalone_script_session() {
            return false;
        }
        let in_place = in_place || self.workspace.paste_in_place;
        self.finish_edits();
        let symbol_id = self.symbol_id().to_owned();
        if self.frame >= self.symbol().timeline.frame_count {
            self.message("Paste failed: destination frame is outside the timeline.");
            return false;
        }

        let local_w = f64::from(output.project.width).max(1.0);
        let local_h = f64::from(output.project.height).max(1.0);
        let sw = f64::from(self.project.width);
        let sh = f64::from(self.project.height);
        let placement_matrix = if !in_place {
            let center = self.paste_center();
            if self.editing_in_place() {
                let Some((edit_to_stage, stage_to_edit)) =
                    self.edit_stage_mapping().ok().and_then(|m| {
                        let mat = m.matrix();
                        neoflash_core::invert_affine_matrix(mat).map(|inv| (mat, inv))
                    })
                else {
                    self.message("Paste failed: edited symbol placement is not invertible.");
                    return false;
                };
                let stage_cx =
                    edit_to_stage[0] * center[0] + edit_to_stage[2] * center[1] + edit_to_stage[4];
                let stage_cy =
                    edit_to_stage[1] * center[0] + edit_to_stage[3] * center[1] + edit_to_stage[5];
                let stage_translation = [
                    1.0,
                    0.0,
                    0.0,
                    1.0,
                    stage_cx - local_w * 0.5,
                    stage_cy - local_h * 0.5,
                ];
                neoflash_core::affine_multiply(stage_to_edit, stage_translation)
            } else {
                [
                    1.0,
                    0.0,
                    0.0,
                    1.0,
                    center[0] - local_w * 0.5,
                    center[1] - local_h * 0.5,
                ]
            }
        } else {
            let [sb_x0, sb_y0, sb_x1, sb_y1] = output.stage_bounds;
            let near_stage = sb_x0.is_finite()
                && sb_y0.is_finite()
                && sb_x1.is_finite()
                && sb_y1.is_finite()
                && sb_x0 <= 2.0 * sw
                && sb_x1 >= -sw
                && sb_y0 <= 2.0 * sh
                && sb_y1 >= -sh;
            let desired_top_left = if near_stage {
                [sb_x0, sb_y0]
            } else {
                [0.0, 0.0]
            };
            let stage_translation = [1.0, 0.0, 0.0, 1.0, desired_top_left[0], desired_top_left[1]];
            if self.editing_in_place() {
                let Some(stage_to_edit) = self
                    .edit_stage_mapping()
                    .ok()
                    .and_then(|m| neoflash_core::invert_affine_matrix(m.matrix()))
                else {
                    self.message("Paste failed: edited symbol placement is not invertible.");
                    return false;
                };
                neoflash_core::affine_multiply(stage_to_edit, stage_translation)
            } else {
                stage_translation
            }
        };

        let active_solo_hidden = self.has_any_solo_layers()
            && !self
                .symbol()
                .timeline
                .layers
                .get(self.layer)
                .is_some_and(|l| self.is_layer_id_effectively_soloed(&l.id));
        let mut candidate = self.project.clone();
        let (target_layer_idx, layer_id, element_ids, created_new_layer) =
            match merge_imported_clipboard_project(
                &mut candidate,
                output.project,
                &symbol_id,
                self.layer,
                active_solo_hidden,
                self.frame,
                placement_matrix,
                self.workspace.timeline_prefs.auto_keyframe,
                false,
            ) {
                Ok(res) => res,
                Err(error) => {
                    self.message(format!("Paste failed: {error}"));
                    return false;
                }
            };

        let count = element_ids.len();
        self.set_playing(false);
        let tx_label = if in_place { "Paste in place" } else { "Paste" };
        self.transact_named(tx_label, |editor| {
            editor.project = candidate;
            editor.layer = target_layer_idx;
            editor.set_selection(
                element_ids
                    .into_iter()
                    .map(|element| SelectionTarget {
                        layer: layer_id.clone(),
                        element,
                    })
                    .collect(),
            );
        });
        if created_new_layer && self.has_any_solo_layers() {
            self.solo_layers
                .entry(symbol_id)
                .or_default()
                .insert(layer_id);
        }
        self.load_project_fonts(ctx);

        let mut warn_count = 0usize;
        let mut first_warn: Option<String> = None;
        for d in &output.report.diagnostics {
            if d.severity == neoflash_import::Severity::Info {
                continue;
            }
            warn_count += 1;
            if first_warn.is_none() {
                first_warn = Some(format!("[{}] {}", d.code, d.message));
            }
            let sev = match d.severity {
                neoflash_import::Severity::Warning => {
                    crate::output_pipeline::OutputSeverity::Warning
                }
                neoflash_import::Severity::Error => crate::output_pipeline::OutputSeverity::Error,
                neoflash_import::Severity::Info => crate::output_pipeline::OutputSeverity::Info,
            };
            self.push_output_coalesced(crate::output_pipeline::OutputEvent::new(
                sev,
                crate::output_pipeline::OutputSource::Import,
                format!("[{}] {}: {}", d.code, d.location, d.message),
            ));
        }
        if let Some(summary) = first_warn {
            self.message(format!(
                "Pasted {count} object(s) from {format_label} ({warn_count} warning(s): {summary})"
            ));
        } else {
            self.message(format!("Pasted {count} object(s) from {format_label}."));
        }
        true
    }
    pub(crate) fn paste_svg(&mut self, ctx: &egui::Context, text: &str, in_place: bool) -> bool {
        if !self.has_document() || self.confirm.is_some() || self.is_standalone_script_session() {
            return false;
        }
        if text.len() > MAX_STAGE_SVG_CLIPBOARD_BYTES {
            self.message("Paste failed: SVG exceeds the 64 MiB clipboard size limit.");
            return false;
        }
        let in_place = in_place || self.workspace.paste_in_place;
        self.finish_edits();
        let symbol_id = self.symbol_id().to_owned();
        if self.frame >= self.symbol().timeline.frame_count {
            self.message("Paste failed: destination frame is outside the timeline.");
            return false;
        }
        let stage_to_edit = if in_place && self.editing_in_place() {
            let Some(inv) = self
                .edit_stage_mapping()
                .ok()
                .and_then(|m| neoflash_core::invert_affine_matrix(m.matrix()))
            else {
                self.message("Paste failed: edited symbol placement is not invertible.");
                return false;
            };
            Some(inv)
        } else {
            None
        };
        let active_solo_hidden = self.has_any_solo_layers()
            && !self
                .symbol()
                .timeline
                .layers
                .get(self.layer)
                .is_some_and(|l| self.is_layer_id_effectively_soloed(&l.id));

        if self.workspace.clipboard_prefs.editable_vector_paste {
            if let Err(error) = validate_static_svg_clipboard(text) {
                self.message(format!("Paste failed: {error}"));
                return false;
            }
            let trimmed = text.trim_start_matches('\u{feff}');
            let svg_opts = neoflash_import::VectorImportOptions {
                layer_mapping: neoflash_import::VectorLayerMapping::FlattenToSingleLayer,
                text_handling: neoflash_import::VectorTextHandling::EditableText,
                mask_handling: neoflash_import::VectorMaskHandling::FlattenClip,
                transform_handling:
                    neoflash_import::VectorTransformHandling::BakeTransformsIntoPaths,
                resize_stage_to_viewbox: true,
            };
            if let Ok((imported_svg, svg_report)) =
                neoflash_import::import_svg_with_options(trimmed, "clipboard.svg", &svg_opts)
            {
                let has_unsupported = svg_report
                    .diagnostics
                    .iter()
                    .any(|d| d.code == "svg-unsupported-element");
                let has_elements = !has_unsupported
                    && imported_svg.symbol(&imported_svg.root).is_some_and(|s| {
                        s.timeline
                            .layers
                            .iter()
                            .flat_map(|l| &l.keyframes)
                            .any(|k| !k.elements.is_empty())
                    });
                if has_elements {
                    let (svg_w, svg_h, vb_matrix) = parse_svg_viewport_and_viewbox(trimmed)
                        .unwrap_or((
                            f64::from(imported_svg.width),
                            f64::from(imported_svg.height),
                            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                        ));
                    let base_placement = if let Some(inv) = stage_to_edit {
                        inv
                    } else if in_place {
                        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
                    } else {
                        let center = self.paste_center();
                        [
                            1.0,
                            0.0,
                            0.0,
                            1.0,
                            center[0] - svg_w * 0.5,
                            center[1] - svg_h * 0.5,
                        ]
                    };
                    let placement_matrix =
                        neoflash_core::affine_multiply(base_placement, vb_matrix);
                    let mut candidate = self.project.clone();
                    match merge_imported_clipboard_project(
                        &mut candidate,
                        imported_svg,
                        &symbol_id,
                        self.layer,
                        active_solo_hidden,
                        self.frame,
                        placement_matrix,
                        self.workspace.timeline_prefs.auto_keyframe,
                        false,
                    ) {
                        Ok((target_layer_idx, layer_id, element_ids, created_new_layer)) => {
                            let count = element_ids.len();
                            self.playing = false;
                            let tx_label = if in_place { "Paste in place" } else { "Paste" };
                            self.transact_named(tx_label, |editor| {
                                editor.project = candidate;
                                editor.layer = target_layer_idx;
                                editor.set_selection(
                                    element_ids
                                        .into_iter()
                                        .map(|element| SelectionTarget {
                                            layer: layer_id.clone(),
                                            element,
                                        })
                                        .collect(),
                                );
                            });
                            if created_new_layer && self.has_any_solo_layers() {
                                self.solo_layers
                                    .entry(symbol_id)
                                    .or_default()
                                    .insert(layer_id);
                            }
                            self.load_project_fonts(ctx);
                            self.message(format!("Pasted {count} object(s)."));
                            return true;
                        }
                        Err(error) => {
                            self.message(format!("Paste failed: {error}"));
                            return false;
                        }
                    }
                }
            }
        }

        let mut graphic = match crate::svg_graphics::prepare_svg_clipboard(
            text,
            self.workspace.clipboard_prefs.editable_vector_paste,
        ) {
            Ok(graphic) => graphic,
            Err(error) => {
                self.message(format!("Paste failed: {error}"));
                return false;
            }
        };
        let mut candidate = self.project.clone();
        if !graphic.resources.is_empty() {
            if let Err(error) =
                crate::graphic_import::merge_graphic_resources(&mut candidate, &mut graphic)
            {
                self.message(format!("Paste failed: {error}"));
                return false;
            }
            candidate.format_version = candidate.format_version.max(3);
        }
        let placement_matrix = if let Some(inv) = stage_to_edit {
            inv
        } else if in_place {
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
        } else {
            let center = self.paste_center();
            [
                1.0,
                0.0,
                0.0,
                1.0,
                center[0] - graphic.width / 2.,
                center[1] - graphic.height / 2.,
            ]
        };
        let mut occupied_ids = collect_occupied_project_ids(&candidate);
        let mut element_ids = Vec::with_capacity(graphic.elements.len());
        for element in &mut graphic.elements {
            element.id = alloc_unique_id("object", &mut occupied_ids);
            apply_placement_matrix_to_element(element, placement_matrix);
            element_ids.push(element.id.clone());
        }
        let fps = f64::from(candidate.fps);
        let auto_keyframe = self.workspace.timeline_prefs.auto_keyframe;
        let (target_layer_idx, layer_id, created_new_layer) = {
            let timeline = &mut candidate.symbol_mut(&symbol_id).unwrap().timeline;
            let frame_count = timeline.frame_count;
            let (target_idx, layer_id, created_new) = resolve_or_insert_paste_normal_layer(
                timeline,
                self.layer,
                active_solo_hidden,
                &mut occupied_ids,
            );
            let layer = &mut timeline.layers[target_idx];
            let layer_end = layer.properties.end_frame.unwrap_or(frame_count);
            let need_key = auto_keyframe
                || self.frame >= layer_end
                || layer.key_at(self.frame).is_none_or(|k| k.tween.is_some())
                || neoflash_core::motion_span(layer, self.frame).is_some();
            if need_key && !layer.keyframes.iter().any(|k| k.frame == self.frame) {
                let elements = neoflash_core::sample_layer_at_fps(layer, self.frame, fps);
                let tween = layer.key_at(self.frame).and_then(|k| k.tween);
                layer.keyframes.push(neoflash_core::Keyframe {
                    frame: self.frame,
                    elements,
                    tween,
                    label: String::new(),
                    ..Default::default()
                });
                layer.keyframes.sort_by_key(|k| k.frame);
            }
            layer
                .key_at_mut(self.frame)
                .unwrap()
                .elements
                .extend(graphic.elements);
            layer.normalize_motion_keys();
            (target_idx, layer_id, created_new)
        };
        if let Err(error) = candidate.validate() {
            self.message(format!("Paste failed: {error}"));
            return false;
        }
        let count = element_ids.len();
        self.playing = false;
        let tx_label = if in_place { "Paste in place" } else { "Paste" };
        self.transact_named(tx_label, |editor| {
            editor.project = candidate;
            editor.layer = target_layer_idx;
            editor.set_selection(
                element_ids
                    .into_iter()
                    .map(|element| SelectionTarget {
                        layer: layer_id.clone(),
                        element,
                    })
                    .collect(),
            );
        });
        if created_new_layer && self.has_any_solo_layers() {
            self.solo_layers
                .entry(symbol_id)
                .or_default()
                .insert(layer_id);
        }
        let suffix = graphic.notice.map(|n| format!(" {n}")).unwrap_or_default();
        self.message(format!("Pasted {count} object(s).{suffix}"));
        true
    }
    pub(crate) fn paste_image_bytes(
        &mut self,
        ctx: &egui::Context,
        bytes: &[u8],
        media_type: &str,
        in_place: bool,
    ) -> bool {
        if !self.has_document() || self.confirm.is_some() || self.is_standalone_script_session() {
            return false;
        }
        let in_place = in_place || self.workspace.paste_in_place;
        self.finish_edits();
        let symbol_id = self.symbol_id().to_owned();
        if self.frame >= self.symbol().timeline.frame_count {
            self.message("Paste failed: destination frame is outside the timeline.");
            return false;
        }
        let (width, height) = match decode_bounded_bitmap_dimensions(bytes) {
            Ok(dims) => dims,
            Err(error) => {
                self.message(format!("Paste failed: {error}"));
                return false;
            }
        };
        let ext = match media_type {
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "png",
        };
        let mut index = self.project.resources.len() + 1;
        let (cand_resource_id, path) = loop {
            let cand_id = format!("res-pasted-{index}");
            let cand_path = format!("bitmaps/pasted-{index}.{ext}");
            if !self
                .project
                .resources
                .iter()
                .any(|r| r.id == cand_id || r.path == cand_path)
            {
                break (cand_id, cand_path);
            }
            index += 1;
        };
        let unit = match neoflash_core::ResourceUnit::from_bytes(
            cand_resource_id,
            path,
            media_type.to_owned(),
            bytes,
        ) {
            Ok(unit) => unit,
            Err(error) => {
                self.message(format!("Paste failed: {error}"));
                return false;
            }
        };
        let mut candidate = self.project.clone();
        let resource_id = if let Some(existing) = candidate.resources.iter().find(|r| {
            !is_internal_registry_resource_path(&r.path)
                && r.media_type == unit.media_type
                && r.data_base64 == unit.data_base64
        }) {
            existing.id.clone()
        } else {
            let id = unit.id.clone();
            candidate.resources.push(unit);
            candidate.format_version = candidate.format_version.max(3);
            id
        };
        let mut occupied_ids = collect_occupied_project_ids(&candidate);
        let element_id = alloc_unique_id("bitmap", &mut occupied_ids);
        let placement_matrix = if in_place && self.editing_in_place() {
            let Some(inv) = self
                .edit_stage_mapping()
                .ok()
                .and_then(|m| neoflash_core::invert_affine_matrix(m.matrix()))
            else {
                self.message("Paste failed: edited symbol placement is not invertible.");
                return false;
            };
            inv
        } else if in_place {
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
        } else {
            let center = self.paste_center();
            [
                1.0,
                0.0,
                0.0,
                1.0,
                center[0] - f64::from(width) / 2.0,
                center[1] - f64::from(height) / 2.0,
            ]
        };
        let mut element = neoflash_core::Element {
            id: element_id.clone(),
            name: String::new(),
            transform: neoflash_core::Transform::default(),
            kind: neoflash_core::ElementKind::Bitmap {
                resource_id,
                width,
                height,
                smoothing: true,
            },
            ..Default::default()
        };
        apply_placement_matrix_to_element(&mut element, placement_matrix);
        let active_solo_hidden = self.has_any_solo_layers()
            && !self
                .symbol()
                .timeline
                .layers
                .get(self.layer)
                .is_some_and(|l| self.is_layer_id_effectively_soloed(&l.id));
        let fps = f64::from(candidate.fps);
        let auto_keyframe = self.workspace.timeline_prefs.auto_keyframe;
        let (target_layer_idx, layer_id, created_new_layer) = {
            let timeline = &mut candidate.symbol_mut(&symbol_id).unwrap().timeline;
            let frame_count = timeline.frame_count;
            let (target_idx, layer_id, created_new) = resolve_or_insert_paste_normal_layer(
                timeline,
                self.layer,
                active_solo_hidden,
                &mut occupied_ids,
            );
            let layer = &mut timeline.layers[target_idx];
            let layer_end = layer.properties.end_frame.unwrap_or(frame_count);
            let need_key = auto_keyframe
                || self.frame >= layer_end
                || layer.key_at(self.frame).is_none_or(|k| k.tween.is_some())
                || neoflash_core::motion_span(layer, self.frame).is_some();
            if need_key && !layer.keyframes.iter().any(|k| k.frame == self.frame) {
                let elements = neoflash_core::sample_layer_at_fps(layer, self.frame, fps);
                let tween = layer.key_at(self.frame).and_then(|k| k.tween);
                layer.keyframes.push(neoflash_core::Keyframe {
                    frame: self.frame,
                    elements,
                    tween,
                    label: String::new(),
                    ..Default::default()
                });
                layer.keyframes.sort_by_key(|k| k.frame);
            }
            layer.key_at_mut(self.frame).unwrap().elements.push(element);
            layer.normalize_motion_keys();
            (target_idx, layer_id, created_new)
        };
        if let Err(error) = candidate.validate() {
            self.message(format!("Paste failed: {error}"));
            return false;
        }
        self.playing = false;
        self.transact_named("Paste bitmap", |editor| {
            editor.project = candidate;
            editor.layer = target_layer_idx;
            editor.set_selection(vec![SelectionTarget {
                layer: layer_id.clone(),
                element: element_id,
            }]);
        });
        if created_new_layer && self.has_any_solo_layers() {
            self.solo_layers
                .entry(symbol_id)
                .or_default()
                .insert(layer_id);
        }
        let _ = ctx;
        self.message("Pasted 1 bitmap.");
        true
    }
    pub(crate) fn request_clipboard(&mut self, ctx: &egui::Context, command: ClipboardCommand) {
        // Headless input tests must inject clipboard contents instead of reading
        // unrelated user data or letting an empty OS clipboard hide double dispatch.
        #[cfg(test)]
        assert!(
            !matches!(
                command,
                ClipboardCommand::Paste | ClipboardCommand::PasteInPlace
            ),
            "headless Paste reached the OS clipboard; inject request_clipboard_with instead"
        );
        #[cfg(not(test))]
        if matches!(
            command,
            ClipboardCommand::Paste | ClipboardCommand::PasteInPlace
        ) {
            let target = self
                .clipboard
                .menu_target
                .as_ref()
                .filter(|target| target.viewport == ctx.viewport_id())
                .cloned()
                .unwrap_or_else(|| self.clipboard_target(ctx));
            if !target.is_text() && !self.is_stage_inline_text_editing() {
                self.request_stage_clipboard_with(ctx, command, || {
                    read_os_stage_clipboard_flavors(ctx)
                });
                return;
            }
        }
        self.request_clipboard_with(ctx, command, || {
            arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.get_text())
                .map_err(|error| error.to_string())
        });
    }
    pub(crate) fn request_stage_clipboard_with(
        &mut self,
        ctx: &egui::Context,
        command: ClipboardCommand,
        read_flavors: impl FnOnce() -> Result<StageClipboardFlavors, String>,
    ) {
        let target = self
            .clipboard
            .menu_target
            .as_ref()
            .filter(|target| target.viewport == ctx.viewport_id())
            .cloned()
            .unwrap_or_else(|| self.clipboard_target(ctx));
        if !target.matches(self) {
            self.message("Clipboard command canceled: the editing target changed.");
            return;
        }
        if self.is_standalone_script_session() && !target.is_text() {
            return;
        }
        if !target.is_text() && matches!(command, ClipboardCommand::Copy | ClipboardCommand::Cut) {
            self.copy_selection(ctx, command == ClipboardCommand::Cut);
            return;
        }
        if target.is_text() {
            if command == ClipboardCommand::PasteInPlace {
                return;
            }
            self.request_clipboard_with(ctx, command, || {
                read_flavors().map(|f| f.plain_text.unwrap_or_default())
            });
            return;
        }
        if self.is_stage_inline_text_editing() {
            if command == ClipboardCommand::PasteInPlace {
                return;
            }
            let flavors = match read_flavors() {
                Ok(flavors) => flavors,
                Err(error) => {
                    self.message(format!("Cannot read clipboard: {error}"));
                    return;
                }
            };
            if flavors.neoflash.is_some() {
                return;
            }
            if let Some(plain) = flavors.plain_text.filter(|t| !t.is_empty()) {
                self.paste_objects_with_stage_flavors(
                    ctx,
                    &plain.replace("\r\n", "\n"),
                    false,
                    flavors.generation,
                    None,
                );
            }
            return;
        }

        let flavors = match read_flavors() {
            Ok(flavors) => flavors,
            Err(error) => {
                self.message(format!("Cannot read clipboard: {error}"));
                return;
            }
        };
        let in_place = command == ClipboardCommand::PasteInPlace;
        let plain_norm = flavors
            .plain_text
            .as_deref()
            .unwrap_or("")
            .replace("\r\n", "\n");
        self.paste_objects_with_stage_flavors(
            ctx,
            &plain_norm,
            in_place,
            flavors.generation,
            Some(&flavors),
        );
    }
    pub(crate) fn request_clipboard_with(
        &mut self,
        ctx: &egui::Context,
        command: ClipboardCommand,
        read_text: impl FnOnce() -> Result<String, String>,
    ) {
        let target = self
            .clipboard
            .menu_target
            .as_ref()
            .filter(|target| target.viewport == ctx.viewport_id())
            .cloned()
            .unwrap_or_else(|| self.clipboard_target(ctx));
        if !target.matches(self) {
            self.message("Clipboard command canceled: the editing target changed.");
            return;
        }
        if self.is_standalone_script_session() && !target.is_text() {
            return;
        }
        if !target.is_text() && matches!(command, ClipboardCommand::Copy | ClipboardCommand::Cut) {
            self.copy_selection(ctx, command == ClipboardCommand::Cut);
            return;
        }
        if command == ClipboardCommand::PasteInPlace
            && (target.is_text() || self.is_stage_inline_text_editing())
        {
            return;
        }
        // Read only when the user invokes Paste. A direct native read has a
        // definite empty/error result, unlike RequestPaste's untagged async event.
        // It cannot leave an old menu mode waiting for an unrelated later paste.
        let event = match command {
            ClipboardCommand::Copy => Event::Copy,
            ClipboardCommand::Cut => Event::Cut,
            ClipboardCommand::Paste | ClipboardCommand::PasteInPlace => match read_text() {
                Ok(text) if text.is_empty() => return,
                Ok(text) => Event::Paste(text.replace("\r\n", "\n")),
                Err(error) => {
                    self.message(format!("Cannot read clipboard: {error}"));
                    return;
                }
            },
        };
        if !target.is_text() {
            if let Event::Paste(text) = event {
                self.paste_objects(ctx, &text, command == ClipboardCommand::PasteInPlace);
            }
            return;
        }
        // The text widget is bound to this authored frame. Do not let the next
        // playback tick move it to another key before deferred delivery.
        self.playing = false;
        // Text focus is restored on the next pass, after the menu's pointer click
        // has finished. Restoring it now would immediately surrender it again.
        self.clipboard.pending = Some(Request {
            target,
            event,
            expires: ctx.input(|input| input.time) + 5.,
        });
        ctx.request_repaint();
    }
    #[cfg(test)]
    pub(crate) fn clipboard_events(&mut self, ctx: &egui::Context) {
        self.clipboard_events_with_generation(ctx, || clipboard_generation(ctx));
    }
    pub(crate) fn clipboard_events_with_generation(
        &mut self,
        ctx: &egui::Context,
        read_generation: impl FnOnce() -> Option<u64>,
    ) {
        let (mut events, now) =
            ctx.input_mut(|input| (std::mem::take(&mut input.events), input.time));
        let user_intervened = events.iter().any(|event| {
            matches!(
                event,
                Event::PointerButton { pressed: true, .. }
                    | Event::Key { pressed: true, .. }
                    | Event::Copy
                    | Event::Cut
                    | Event::Paste(_)
            )
        });
        if let Some(request) = self.clipboard.pending.take() {
            if request.target.matches(self) && now <= request.expires && !user_intervened {
                if request.target.viewport == ctx.viewport_id() {
                    request.target.restore_text_focus(ctx);
                    events.insert(0, request.event);
                } else {
                    // A parent repaint must not deliver a child's text command
                    // into the Stage or a different editor widget.
                    ctx.request_repaint_of(request.target.viewport);
                    self.clipboard.pending = Some(request);
                }
            } else {
                self.message("Clipboard command canceled: the editing target changed.");
            }
        }
        let has_paste_payload = events.iter().any(|event| matches!(event, Event::Paste(_)));
        // All payloads were captured before dispatch. A Copy/Cut earlier in this
        // batch must not attach newly copied formatting to an older Paste.
        let generation = has_paste_payload.then(read_generation).flatten();
        let mut retained = Vec::with_capacity(events.len());
        for (index, event) in events.iter().enumerate() {
            if has_paste_payload && paste_shortcut_mode(self, event).is_some() {
                // Native payloads are authoritative for this batch. A separated
                // press must not trigger another OS read after payload delivery.
                // Only an adjacent press supplies the mode below; a shortcut
                // without any payload still reaches ordinary command routing.
                continue;
            }
            if !matches!(event, Event::Copy | Event::Cut | Event::Paste(_)) {
                retained.push(event.clone());
                continue;
            }
            if ctx.wants_keyboard_input() || self.confirm.is_some() || !self.has_document() {
                if self.confirm.is_none() && self.has_document() {
                    // App::update ticks playback after shortcuts and before
                    // painting the TextEdit that will consume this event.
                    self.playing = false;
                }
                retained.push(event.clone());
                continue;
            }
            if self.is_standalone_script_session() {
                continue;
            }
            match event {
                Event::Copy => self.copy_selection(ctx, false),
                Event::Cut => self.copy_selection(ctx, true),
                Event::Paste(text) => {
                    // An unpaired native/menu/accessibility payload means plain
                    // Paste. Explicit menu Paste in Place bypasses this route.
                    let in_place = index
                        .checked_sub(1)
                        .and_then(|previous| paste_shortcut_mode(self, &events[previous]))
                        .unwrap_or(false);
                    self.paste_objects_with_generation(ctx, text, in_place, generation);
                }
                _ => unreachable!(),
            }
        }
        ctx.input_mut(|input| input.events = retained);
    }
}

fn extract_html_fragment_clipboard(
    text: &str,
    base_format: &neoflash_core::TextFormat,
) -> Option<(String, Vec<neoflash_core::TextFormatRun>)> {
    const START_MARKER: &str = "<!--StartFragment-->";
    const END_MARKER: &str = "<!--EndFragment-->";
    let start_idx = text.find(START_MARKER)? + START_MARKER.len();
    let end_idx = text[start_idx..].find(END_MARKER)? + start_idx;
    let fragment = text[start_idx..end_idx].trim();
    if fragment.is_empty() {
        return None;
    }
    let mut clean_base = base_format.clone();
    clean_base.set_field_options(None);
    let resp = neoflash_core::as2_text::execute(neoflash_core::as2_text::Request::Html {
        source: fragment.to_owned(),
        format: clean_base,
        styles: neoflash_core::as2_text::StyleSheet::default(),
        condense_white: false,
        multi_run: true,
    })
    .ok()?;
    let plain = resp.get("text")?.as_str()?.to_owned();
    let runs: Vec<neoflash_core::TextFormatRun> = resp
        .get("format_runs")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    Some((plain, runs))
}

fn is_svg_clipboard_text(text: &str) -> bool {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if !trimmed.starts_with('<') {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("<svg")
        && lower
            .as_bytes()
            .get(4)
            .is_some_and(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
        || ((lower.starts_with("<?xml")
            || lower.starts_with("<!doctype")
            || lower.starts_with("<!--"))
            && lower.contains("<svg"))
}

fn validate_static_svg_clipboard(text: &str) -> Result<(), String> {
    let trimmed = text.trim_start_matches('\u{feff}');
    let xml =
        resvg::usvg::roxmltree::Document::parse(trimmed).map_err(|e| format!("SVG XML: {e}"))?;
    if xml.root_element().tag_name().name() != "svg" {
        return Err("The file has no SVG root element.".into());
    }
    for node in xml.descendants().filter(|node| node.is_element()) {
        let tag = node.tag_name().name();
        if matches!(
            tag,
            "script" | "animate" | "animateMotion" | "animateTransform" | "set" | "foreignObject"
        ) {
            return Err(format!(
                "SVG <{tag}> is not static artwork. Export a static SVG without scripts, animation or embedded HTML."
            ));
        }
        for attribute in node.attributes() {
            if attribute.name().starts_with("on") {
                return Err("SVG event scripts cannot be imported as static artwork.".into());
            }
            if matches!(attribute.name(), "href" | "xlink:href")
                && !attribute.value().starts_with('#')
                && !attribute.value().starts_with("data:")
            {
                return Err(
                    "SVG references an external file. Embed linked images in the SVG before importing."
                        .into(),
                );
            }
        }
    }
    Ok(())
}

const MAX_STAGE_SVG_CLIPBOARD_BYTES: usize = 64 * 1024 * 1024;
const MAX_PASTED_BITMAP_DIM: u32 = 4096;
const MAX_PASTED_BITMAP_ALLOC_BYTES: u64 = 64 * 1024 * 1024;

fn decode_bounded_bitmap_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.is_empty() {
        return Err("bitmap image data is empty.".into());
    }
    if bytes.len() as u64 > MAX_PASTED_BITMAP_ALLOC_BYTES {
        return Err("bitmap image exceeds the 64 MiB clipboard size limit.".into());
    }
    let fmt =
        image::guess_format(bytes).map_err(|e| format!("unsupported bitmap format ({e})."))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_PASTED_BITMAP_DIM);
    limits.max_image_height = Some(MAX_PASTED_BITMAP_DIM);
    limits.max_alloc = Some(MAX_PASTED_BITMAP_ALLOC_BYTES);

    let mut dim_reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), fmt);
    dim_reader.limits(limits.clone());
    let (w, h) = dim_reader
        .into_dimensions()
        .map_err(|e| format!("invalid or oversized bitmap header ({e})."))?;
    if w == 0
        || h == 0
        || w > MAX_PASTED_BITMAP_DIM
        || h > MAX_PASTED_BITMAP_DIM
        || u64::from(w).saturating_mul(u64::from(h)).saturating_mul(4)
            > MAX_PASTED_BITMAP_ALLOC_BYTES
    {
        return Err(format!(
            "bitmap dimensions ({w}×{h}) exceed the {MAX_PASTED_BITMAP_DIM}×{MAX_PASTED_BITMAP_DIM} px limit."
        ));
    }

    let mut decode_reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), fmt);
    decode_reader.limits(limits);
    let decoded = decode_reader
        .decode()
        .map_err(|e| format!("invalid bitmap image ({e})."))?;
    Ok((decoded.width().max(1), decoded.height().max(1)))
}

fn apply_placement_matrix_to_element(
    element: &mut neoflash_core::Element,
    placement_matrix: [f64; 6],
) {
    let is_pure_translation = (placement_matrix[0] - 1.0).abs() <= 1e-12
        && placement_matrix[1].abs() <= 1e-12
        && placement_matrix[2].abs() <= 1e-12
        && (placement_matrix[3] - 1.0).abs() <= 1e-12;
    if is_pure_translation {
        element.transform.x =
            (element.transform.x + placement_matrix[4]).clamp(-100_000.0, 100_000.0);
        element.transform.y =
            (element.transform.y + placement_matrix[5]).clamp(-100_000.0, 100_000.0);
    } else {
        let prev = element.transform;
        let cur = neoflash_core::transform_matrix(prev);
        let combined = neoflash_core::affine_multiply(placement_matrix, cur);
        if let Some(mut mapped) = neoflash_core::Transform::from_matrix(combined) {
            mapped.x = mapped.x.clamp(-100_000.0, 100_000.0);
            mapped.y = mapped.y.clamp(-100_000.0, 100_000.0);
            mapped.alpha = prev.alpha;
            mapped.color_transform = prev.color_transform;
            mapped.pivot = prev.pivot;
            mapped.primitive = prev.primitive;
            element.transform = mapped;
        }
    }
}

fn resolve_or_insert_paste_normal_layer(
    timeline: &mut neoflash_core::Timeline,
    preferred_layer_idx: usize,
    active_layer_solo_hidden: bool,
    occupied_ids: &mut std::collections::HashSet<Id>,
) -> (usize, Id, bool) {
    let use_current = !active_layer_solo_hidden
        && timeline
            .layers
            .get(preferred_layer_idx)
            .is_some_and(|layer| {
                layer.properties.role == neoflash_core::LayerRole::Normal
                    && layer.properties.mask.is_none()
                    && neoflash_core::layer_effectively_visible(timeline, layer)
                    && !neoflash_core::layer_effectively_locked(timeline, layer)
            });
    if use_current {
        return (
            preferred_layer_idx,
            timeline.layers[preferred_layer_idx].id.clone(),
            false,
        );
    }
    let base_name = "Pasted graphics";
    let mut name = base_name.to_owned();
    let mut n = 2usize;
    while timeline
        .layers
        .iter()
        .any(|l| l.name.eq_ignore_ascii_case(&name))
    {
        name = format!("{base_name} {n}");
        n += 1;
    }
    let new_layer_id = alloc_unique_id("layer", occupied_ids);
    timeline.layers.push(neoflash_core::Layer {
        id: new_layer_id.clone(),
        name,
        visible: true,
        locked: false,
        properties: neoflash_core::LayerProperties::default(),
        keyframes: Vec::new(),
    });
    (timeline.layers.len() - 1, new_layer_id, true)
}

fn parse_svg_viewport_and_viewbox(text: &str) -> Option<(f64, f64, [f64; 6])> {
    let xml = resvg::usvg::roxmltree::Document::parse(text).ok()?;
    let root = xml.root_element();
    let parse_len = |raw: &str| -> Option<f64> {
        raw.trim()
            .trim_end_matches("px")
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v > 0.0 && *v <= 1_000_000.0)
    };
    let mut w = root.attribute("width").and_then(parse_len);
    let mut h = root.attribute("height").and_then(parse_len);
    let vb = root.attribute("viewBox").and_then(|raw| {
        let nums: Vec<f64> = raw
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f64>().ok())
            .collect();
        (nums.len() == 4
            && nums.iter().all(|v| v.is_finite())
            && nums[2] > 0.0
            && nums[3] > 0.0
            && nums[2] <= 1_000_000.0
            && nums[3] <= 1_000_000.0)
            .then(|| [nums[0], nums[1], nums[2], nums[3]])
    });
    if let Some([_, _, vb_w, vb_h]) = vb {
        w = w.or(Some(vb_w));
        h = h.or(Some(vb_h));
    }
    let (svg_w, svg_h) = (w?, h?);
    let vb_matrix = if let Some([min_x, min_y, vb_w, vb_h]) = vb {
        let sx = svg_w / vb_w;
        let sy = svg_h / vb_h;
        let par = root
            .attribute("preserveAspectRatio")
            .unwrap_or("xMidYMid meet")
            .trim();
        if par.starts_with("none") {
            [sx, 0.0, 0.0, sy, -min_x * sx, -min_y * sy]
        } else {
            let s = sx.min(sy);
            let tx = (svg_w - vb_w * s) * 0.5 - min_x * s;
            let ty = (svg_h - vb_h * s) * 0.5 - min_y * s;
            [s, 0.0, 0.0, s, tx, ty]
        }
    } else {
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    };
    Some((svg_w, svg_h, vb_matrix))
}

#[allow(clippy::too_many_arguments)]
fn merge_imported_clipboard_project(
    candidate: &mut neoflash_core::Project,
    imported: neoflash_core::Project,
    symbol_id: &str,
    layer_idx: usize,
    active_layer_solo_hidden: bool,
    frame: u32,
    placement_matrix: [f64; 6],
    auto_keyframe: bool,
    is_figma: bool,
) -> Result<(usize, Id, Vec<Id>, bool), String> {
    use std::collections::{HashMap, HashSet};

    if !placement_matrix.iter().all(|v| v.is_finite()) {
        return Err("Paste placement transform is not finite.".into());
    }
    if let Some(symbol) = candidate.symbol(symbol_id) {
        if frame >= symbol.timeline.frame_count {
            return Err("Destination frame is outside the timeline.".into());
        }
    } else {
        return Err("Destination symbol does not exist.".into());
    }

    let imported_root_id = imported.root.clone();
    let imported_scene_ids: HashSet<Id> = imported.scenes.iter().cloned().collect();
    let mut root_symbol_opt = None;
    let mut donor_symbols = Vec::new();
    for sym in imported.symbols {
        if sym.id == imported_root_id {
            root_symbol_opt = Some(sym);
        } else if !imported_scene_ids.contains(&sym.id) {
            donor_symbols.push(sym);
        }
    }
    let mut root_symbol = root_symbol_opt
        .ok_or_else(|| "Imported clipboard project has no root symbol.".to_owned())?;

    let root_has_elements = root_symbol
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .any(|k| !k.elements.is_empty());
    if !root_has_elements {
        return Err("Clipboard contains no visual elements to paste.".into());
    }

    let needs_root_symbol_wrapper = root_symbol.timeline.layers.iter().any(|layer| {
        layer.properties.role == neoflash_core::LayerRole::Mask
            || layer.properties.mask.is_some()
            || layer.properties.blend_mode != neoflash_core::BlendMode::Normal
    });

    let (mut top_elements, mut top_bitmap_fills, mut top_appearances) = if needs_root_symbol_wrapper
    {
        let mut wrapped_id = format!("__wrapped_clip_{imported_root_id}");
        while donor_symbols.iter().any(|s| s.id == wrapped_id) {
            wrapped_id.push('_');
        }
        root_symbol.id = wrapped_id.clone();
        if is_figma {
            root_symbol.name = "Pasted Figma Clip".into();
            root_symbol.folder = "Figma Components".into();
        } else {
            root_symbol.name = "Pasted Vector Clip".into();
            root_symbol.folder = "Imported Artwork".into();
        }
        root_symbol.kind = neoflash_core::SymbolKind::MovieClip;
        root_symbol.class_name = None;
        root_symbol.linkage_id = None;
        donor_symbols.push(root_symbol);
        let wrapper_el = neoflash_core::Element {
            id: "wrapped-top-0".into(),
            name: if is_figma {
                "Pasted_Figma_Clip".into()
            } else {
                "Pasted_Vector_Clip".into()
            },
            transform: neoflash_core::Transform::default(),
            kind: neoflash_core::ElementKind::Instance {
                symbol_id: wrapped_id,
                playback: neoflash_core::InstancePlayback::Independent,
            },
            ..neoflash_core::Element::default()
        };
        (vec![wrapper_el], Vec::new(), Vec::new())
    } else {
        let mut els = Vec::new();
        let mut bfs = Vec::new();
        let mut apps = Vec::new();
        for layer in root_symbol.timeline.layers {
            if let Some(kf) = layer.keyframes.into_iter().find(|k| k.frame == 0) {
                els.extend(kf.elements);
                bfs.extend(kf.bitmap_fills);
                apps.extend(kf.appearances);
            }
        }
        (els, bfs, apps)
    };

    if top_elements.is_empty() {
        return Err("Clipboard contains no visual elements to paste.".into());
    }

    let mut seeds = SeedAssetRefs::default();
    for el in &top_elements {
        seeds.record_element(el);
    }
    for bf in &top_bitmap_fills {
        seeds.add_resource(&bf.resource_id);
    }
    for app in &top_appearances {
        seeds.record_appearance(app);
    }

    let empty_scenes = HashSet::new();
    let mut maps = import_cross_document_assets(
        candidate,
        &donor_symbols,
        &empty_scenes,
        &imported.resources,
        &imported.sources,
        seeds,
    )?;

    let mut element_ids = Vec::with_capacity(top_elements.len());
    let mut placement_ids = HashMap::<Id, Id>::new();
    for element in &mut top_elements {
        let old_id = element.id.clone();
        let new_eid = alloc_unique_id("object", &mut maps.occupied_ids);
        element.id = new_eid.clone();
        placement_ids.insert(old_id, new_eid.clone());
        element_ids.push(new_eid);
        match &mut element.kind {
            neoflash_core::ElementKind::Instance { symbol_id, .. } => {
                if let Some(mapped) = maps.sym_map.get(symbol_id) {
                    *symbol_id = mapped.clone();
                }
            }
            neoflash_core::ElementKind::Bitmap { resource_id, .. } => {
                if let Some(mapped) = maps.res_map.get(resource_id) {
                    *resource_id = mapped.clone();
                }
            }
            _ => {}
        }
        apply_placement_matrix_to_element(element, placement_matrix);
    }
    for bf in &mut top_bitmap_fills {
        if let Some(new_eid) = placement_ids.get(&bf.element_id) {
            bf.element_id = new_eid.clone();
        }
        if let Some(new_rid) = maps.res_map.get(&bf.resource_id) {
            bf.resource_id = new_rid.clone();
        }
    }
    for app in &mut top_appearances {
        if let Some(new_eid) = placement_ids.get(&app.element_id) {
            app.element_id = new_eid.clone();
        }
        remap_appearance_refs(app, &maps.swatch_map, &maps.style_map);
    }

    let fps = f64::from(candidate.fps);
    let (target_layer_idx, layer_id, created_new_layer) = {
        let timeline = &mut candidate
            .symbol_mut(symbol_id)
            .ok_or_else(|| "Destination symbol does not exist.".to_owned())?
            .timeline;
        let frame_count = timeline.frame_count;
        if frame >= frame_count {
            return Err("Destination frame is outside the timeline.".into());
        }
        let (target_idx, layer_id, created_new) = resolve_or_insert_paste_normal_layer(
            timeline,
            layer_idx,
            active_layer_solo_hidden,
            &mut maps.occupied_ids,
        );
        let layer_end = timeline.layers[target_idx]
            .properties
            .end_frame
            .unwrap_or(frame_count);
        let need_key = auto_keyframe
            || frame >= layer_end
            || timeline.layers[target_idx]
                .key_at(frame)
                .is_none_or(|k| k.tween.is_some())
            || neoflash_core::motion_span(&timeline.layers[target_idx], frame).is_some();
        if need_key
            && !timeline.layers[target_idx]
                .keyframes
                .iter()
                .any(|k| k.frame == frame)
        {
            let sampled_els = neoflash_core::sample_layer_in_at_fps(
                timeline,
                &timeline.layers[target_idx],
                frame,
                fps,
            );
            let layer = &mut timeline.layers[target_idx];
            let active = layer.key_at(frame);
            let tween = active.and_then(|k| k.tween);
            let tween_kind = active.map_or(neoflash_core::TweenKind::Classic, |k| k.tween_kind);
            let orient_to_path = active.is_some_and(|k| k.orient_to_path);
            let inherited_bfs = active.map(|k| k.bitmap_fills.clone()).unwrap_or_default();
            let inherited_apps = active.map(|k| k.appearances.clone()).unwrap_or_default();
            layer.keyframes.push(neoflash_core::Keyframe {
                frame,
                elements: sampled_els,
                bitmap_fills: inherited_bfs,
                appearances: inherited_apps,
                tween,
                tween_kind,
                orient_to_path,
                label: String::new(),
                ..Default::default()
            });
            layer.keyframes.sort_by_key(|k| k.frame);
        }
        let layer = &mut timeline.layers[target_idx];
        let dest_key = layer
            .keyframes
            .iter_mut()
            .find(|k| k.frame == frame)
            .ok_or_else(|| "Could not locate destination keyframe.".to_owned())?;
        dest_key.elements.extend(top_elements);
        for bf in top_bitmap_fills {
            dest_key.set_bitmap_fill(bf);
        }
        for app in top_appearances {
            dest_key.set_appearance(app);
        }
        layer.normalize_motion_keys();
        (target_idx, layer_id, created_new)
    };

    candidate.prune_authoring_groups();
    crate::timeline_commands::upgrade_all_format_versions(candidate);
    if let Err(err) = candidate.validate() {
        return Err(format!("Pasted document failed validation: {err}"));
    }

    Ok((target_layer_idx, layer_id, element_ids, created_new_layer))
}

fn parse_image_data_url(text: &str) -> Option<(String, Vec<u8>)> {
    if text.len() > neoflash_import::figma::MAX_STAGE_HTML_CLIPBOARD_BYTES {
        return None;
    }
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let meta_lower = meta.to_ascii_lowercase();
    let media_type = ["image/png", "image/jpeg", "image/gif", "image/webp"]
        .into_iter()
        .find(|m| meta_lower.starts_with(m))?
        .to_owned();
    if !meta_lower.ends_with(";base64") {
        return None;
    }
    let clean: String = payload
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let unit = neoflash_core::ResourceUnit {
        id: "tmp".into(),
        path: "tmp.png".into(),
        media_type: media_type.clone(),
        data_base64: clean.into(),
        audio_export: None,
        bitmap_export: None,
    };
    let bytes = unit.decoded_bytes().ok()?;
    Some((media_type, bytes))
}

const FONT_EMBEDDING_METADATA_PATH: &str = "fonts/embedding.json";

#[derive(Default)]
pub(crate) struct SeedAssetRefs {
    pub(crate) symbol_ids: Vec<Id>,
    pub(crate) resource_ids: Vec<Id>,
    pub(crate) swatch_ids: Vec<Id>,
    pub(crate) style_ids: Vec<Id>,
    pub(crate) font_families: std::collections::HashSet<String>,
    pub(crate) needs_sources: bool,
}

impl SeedAssetRefs {
    pub(crate) fn add_symbol(&mut self, id: &str) {
        if !id.is_empty() && !self.symbol_ids.iter().any(|existing| existing == id) {
            self.symbol_ids.push(id.to_owned());
        }
    }

    pub(crate) fn add_resource(&mut self, id: &str) {
        if !id.is_empty() && !self.resource_ids.iter().any(|existing| existing == id) {
            self.resource_ids.push(id.to_owned());
        }
    }

    pub(crate) fn add_swatch(&mut self, id: &str) {
        if !id.is_empty() && !self.swatch_ids.iter().any(|existing| existing == id) {
            self.swatch_ids.push(id.to_owned());
        }
    }

    pub(crate) fn add_style(&mut self, id: &str) {
        if !id.is_empty() && !self.style_ids.iter().any(|existing| existing == id) {
            self.style_ids.push(id.to_owned());
        }
    }

    pub(crate) fn record_appearance(&mut self, app: &neoflash_core::ElementAppearance) {
        if let Some(sw) = &app.fill_swatch_id {
            self.add_swatch(sw);
        }
        if let Some(sw) = &app.stroke_swatch_id {
            self.add_swatch(sw);
        }
        if let Some(st) = &app.style_id {
            self.add_style(st);
        }
        for entry in &app.entries {
            match entry {
                neoflash_core::AppearancePaintEntry::Fill {
                    swatch_id: Some(sw),
                    ..
                }
                | neoflash_core::AppearancePaintEntry::Stroke {
                    swatch_id: Some(sw),
                    ..
                } => self.add_swatch(sw),
                _ => {}
            }
        }
    }

    pub(crate) fn record_element(&mut self, el: &neoflash_core::Element) {
        if !el.scripts.is_empty() {
            self.needs_sources = true;
        }
        match &el.kind {
            neoflash_core::ElementKind::Instance { symbol_id, .. } => {
                self.add_symbol(symbol_id);
            }
            neoflash_core::ElementKind::Bitmap { resource_id, .. } => {
                self.add_resource(resource_id);
            }
            neoflash_core::ElementKind::Text { font_family, .. } => {
                let trimmed = font_family.trim();
                if !trimmed.is_empty() {
                    self.font_families.insert(trimmed.to_ascii_lowercase());
                }
            }
            neoflash_core::ElementKind::TextBox { format, .. } => {
                let trimmed = format.font.trim();
                if !trimmed.is_empty() {
                    self.font_families.insert(trimmed.to_ascii_lowercase());
                }
                if let Some(opts) = format.field_options() {
                    for run in opts.format_runs {
                        let run_font = run.format.font.trim();
                        if !run_font.is_empty() {
                            self.font_families.insert(run_font.to_ascii_lowercase());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn record_keyframe(&mut self, kf: &neoflash_core::Keyframe) {
        if let Some(snd) = &kf.sound {
            self.add_resource(&snd.resource);
        }
        if let Some(vid) = &kf.video {
            self.add_resource(&vid.resource);
        }
        for bf in &kf.bitmap_fills {
            self.add_resource(&bf.resource_id);
        }
        for app in &kf.appearances {
            self.record_appearance(app);
        }
        for el in &kf.elements {
            self.record_element(el);
        }
    }
}

pub(crate) struct ImportedAssetMaps {
    pub(crate) sym_map: std::collections::BTreeMap<Id, Id>,
    pub(crate) res_map: std::collections::BTreeMap<Id, Id>,
    pub(crate) swatch_map: std::collections::HashMap<Id, Id>,
    pub(crate) style_map: std::collections::HashMap<Id, Id>,
    pub(crate) occupied_ids: std::collections::HashSet<Id>,
}

fn is_internal_registry_resource_path(path: &str) -> bool {
    let lower = path.trim().to_ascii_lowercase();
    lower == neoflash_core::Project::SWATCHES_RESOURCE_PATH
        || lower == neoflash_core::Project::GLOBAL_SWATCHES_RESOURCE_PATH
        || lower == neoflash_core::Project::VECTOR_STYLES_RESOURCE_PATH
        || lower == neoflash_core::Project::DOCUMENT_ACCESSIBILITY_RESOURCE_PATH
        || lower == neoflash_core::Project::SHARED_LIBRARIES_RESOURCE_PATH
        || lower == neoflash_core::Project::PUBLISH_SETTINGS_RESOURCE_PATH
        || lower == FONT_EMBEDDING_METADATA_PATH
        || lower.starts_with("publish/")
        || lower.starts_with("accessibility/")
}

fn read_embedded_font_entries(
    resources: &[neoflash_core::ResourceUnit],
) -> Vec<crate::panels::font_embedding::EmbeddedFontEntry> {
    resources
        .iter()
        .find(|r| r.path.eq_ignore_ascii_case(FONT_EMBEDDING_METADATA_PATH))
        .and_then(|r| r.decoded_bytes().ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_embedded_font_entries(
    project: &mut neoflash_core::Project,
    entries: &[crate::panels::font_embedding::EmbeddedFontEntry],
    occupied_ids: &mut std::collections::HashSet<Id>,
) {
    if entries.is_empty() {
        project
            .resources
            .retain(|r| !r.path.eq_ignore_ascii_case(FONT_EMBEDDING_METADATA_PATH));
        return;
    }
    let Ok(bytes) = serde_json::to_vec(entries) else {
        return;
    };
    let res_id = project
        .resources
        .iter()
        .find(|r| r.path.eq_ignore_ascii_case(FONT_EMBEDDING_METADATA_PATH))
        .map(|r| r.id.clone())
        .unwrap_or_else(|| alloc_unique_id("resource", occupied_ids));
    let Ok(unit) = neoflash_core::ResourceUnit::from_bytes(
        res_id,
        FONT_EMBEDDING_METADATA_PATH,
        "application/json",
        &bytes,
    ) else {
        return;
    };
    if let Some(slot) = project
        .resources
        .iter_mut()
        .find(|r| r.path.eq_ignore_ascii_case(FONT_EMBEDDING_METADATA_PATH))
    {
        *slot = unit;
    } else {
        project.resources.push(unit);
    }
    project.format_version = project.format_version.max(3);
}

pub(crate) fn collect_occupied_project_ids(
    project: &neoflash_core::Project,
) -> std::collections::HashSet<Id> {
    let mut ids = std::collections::HashSet::new();
    ids.insert(project.root.clone());
    ids.extend(project.scenes.iter().cloned());
    for src in &project.sources {
        ids.insert(src.id.clone());
    }
    for res in &project.resources {
        ids.insert(res.id.clone());
    }
    for sw in project.global_swatches() {
        ids.insert(sw.id);
    }
    for st in project.vector_styles() {
        ids.insert(st.id);
    }
    for sym in &project.symbols {
        ids.insert(sym.id.clone());
        for layer in &sym.timeline.layers {
            ids.insert(layer.id.clone());
            for g in &layer.properties.groups {
                ids.insert(g.id.clone());
            }
            for kf in &layer.keyframes {
                for arm in &kf.ik_armatures {
                    ids.insert(arm.id.clone());
                }
                for el in &kf.elements {
                    ids.insert(el.id.clone());
                }
            }
        }
    }
    ids
}

pub(crate) fn alloc_unique_id(prefix: &str, occupied: &mut std::collections::HashSet<Id>) -> Id {
    loop {
        let candidate = new_id(prefix);
        if occupied.insert(candidate.clone()) {
            return candidate;
        }
    }
}

pub(crate) fn remap_appearance_refs(
    app: &mut neoflash_core::ElementAppearance,
    swatch_map: &std::collections::HashMap<Id, Id>,
    style_map: &std::collections::HashMap<Id, Id>,
) {
    if let Some(sw) = &mut app.fill_swatch_id
        && let Some(mapped) = swatch_map.get(sw)
    {
        *sw = mapped.clone();
    }
    if let Some(sw) = &mut app.stroke_swatch_id
        && let Some(mapped) = swatch_map.get(sw)
    {
        *sw = mapped.clone();
    }
    if let Some(st) = &mut app.style_id
        && let Some(mapped) = style_map.get(st)
    {
        *st = mapped.clone();
    }
    for entry in &mut app.entries {
        match entry {
            neoflash_core::AppearancePaintEntry::Fill {
                swatch_id: Some(sw),
                ..
            }
            | neoflash_core::AppearancePaintEntry::Stroke {
                swatch_id: Some(sw),
                ..
            } => {
                if let Some(mapped) = swatch_map.get(sw) {
                    *sw = mapped.clone();
                }
            }
            _ => {}
        }
    }
}

/// Remap all timeline-internal IDs (`layer.id`, `group.id`, `element.id`, side-tables) AND also
/// remap `vector_effects[*].vector_blend.target_element_id` plus `ik_armatures[*].id` (`model.rs`
/// `remap_internal_ids` preserves exact layer/keyframe/element order).
pub(crate) fn remap_timeline_internal_ids_complete(
    tl: &mut neoflash_core::Timeline,
    mut alloc: impl FnMut(&str) -> neoflash_core::Id,
    symbol_map: Option<&std::collections::BTreeMap<neoflash_core::Id, neoflash_core::Id>>,
    resource_map: Option<&std::collections::BTreeMap<neoflash_core::Id, neoflash_core::Id>>,
) {
    let old_layer_elem_ids: Vec<Vec<Vec<neoflash_core::Id>>> = tl
        .layers
        .iter()
        .map(|layer| {
            layer
                .keyframes
                .iter()
                .map(|kf| kf.elements.iter().map(|e| e.id.clone()).collect())
                .collect()
        })
        .collect();
    // An armature keeps one identity across its keyed poses so evaluation can
    // interpolate the rig. Allocate once per authored identity rather than once
    // per keyframe.
    let mut armature_map = std::collections::HashMap::<neoflash_core::Id, neoflash_core::Id>::new();
    for arm in tl
        .layers
        .iter()
        .flat_map(|layer| &layer.keyframes)
        .flat_map(|key| &key.ik_armatures)
    {
        armature_map
            .entry(arm.id.clone())
            .or_insert_with(|| alloc("ik"));
    }
    tl.remap_internal_ids(&mut alloc, symbol_map, resource_map);
    for (layer, old_kfs) in tl.layers.iter_mut().zip(old_layer_elem_ids.iter()) {
        let mut elem_map = std::collections::HashMap::<neoflash_core::Id, neoflash_core::Id>::new();
        for (kf, old_ids) in layer.keyframes.iter().zip(old_kfs.iter()) {
            for (old_id, el) in old_ids.iter().zip(kf.elements.iter()) {
                elem_map.insert(old_id.clone(), el.id.clone());
            }
        }
        for kf in &mut layer.keyframes {
            for fx in &mut kf.vector_effects {
                if let Some(blend) = &mut fx.vector_blend
                    && let Some(new_tid) = elem_map.get(&blend.target_element_id)
                {
                    blend.target_element_id = new_tid.clone();
                }
            }
            for arm in &mut kf.ik_armatures {
                if let Some(new_id) = armature_map.get(&arm.id) {
                    arm.id = new_id.clone();
                }
            }
        }
    }
}

fn canonicalize_timeline_for_dedup(tl: &neoflash_core::Timeline) -> neoflash_core::Timeline {
    let mut normalized = tl.clone();
    let mut seq = 0usize;
    remap_timeline_internal_ids_complete(
        &mut normalized,
        |prefix| {
            let id = format!("{prefix}-{seq}");
            seq += 1;
            id
        },
        None,
        None,
    );
    normalized
}

fn is_same_or_disambiguated_space(existing: &str, base: &str) -> bool {
    if existing.eq_ignore_ascii_case(base) {
        return true;
    }
    if existing.len() > base.len() + 1
        && existing[..base.len()].eq_ignore_ascii_case(base)
        && existing.as_bytes()[base.len()] == b' '
        && existing[base.len() + 1..]
            .chars()
            .all(|c| c.is_ascii_digit())
    {
        return true;
    }
    false
}

fn is_same_or_disambiguated_underscore(existing: &str, base: &str) -> bool {
    if existing == base {
        return true;
    }
    if let Some(rest) = existing.strip_prefix(base)
        && let Some(digits) = rest.strip_prefix('_')
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
    {
        return true;
    }
    false
}

fn vector_styles_equal_modulo_id_and_name(
    a: &neoflash_core::VectorStyle,
    b: &neoflash_core::VectorStyle,
) -> bool {
    is_same_or_disambiguated_space(&a.name, &b.name)
        && a.fill == b.fill
        && a.stroke == b.stroke
        && (a.stroke_width - b.stroke_width).abs() <= 1e-9
        && a.modifiers == b.modifiers
}

/// Build a vector-export `Project` from `capture_project`, attaching any active Stage `LayerRole::Mask`
/// layers that mask selected elements so `export_clipboard_pdf`, `export_clipboard_eps`, and
/// `export_clipboard_svg` preserve `W`/`W*` clipping masks alongside unmasked selection items.
fn build_vector_export_project(
    source_project: &neoflash_core::Project,
    symbol_id: &str,
    frame: u32,
    selected: &[(SelectionTarget, neoflash_core::Element)],
    capture_project: &neoflash_core::Project,
) -> neoflash_core::Project {
    let Some(src_sym) = source_project.symbol(symbol_id) else {
        return capture_project.clone();
    };
    let mask_for_layer = |lid: &str| -> Option<&neoflash_core::Layer> {
        let layer = src_sym.timeline.layers.iter().find(|l| l.id == lid)?;
        let mask_id = layer.properties.mask.as_deref()?;
        src_sym
            .timeline
            .layers
            .iter()
            .find(|m| m.id == mask_id && m.properties.role == neoflash_core::LayerRole::Mask)
    };

    if !selected
        .iter()
        .any(|(target, _)| mask_for_layer(&target.layer).is_some())
    {
        return capture_project.clone();
    }

    let mut vector_project = capture_project.clone();
    let root_id = vector_project.root.clone();
    let Some((base_bfs, base_apps)) = vector_project
        .symbol(&root_id)
        .and_then(|s| s.timeline.layers.first())
        .and_then(|l| l.keyframes.first())
        .map(|kf| (kf.bitmap_fills.clone(), kf.appearances.clone()))
    else {
        return capture_project.clone();
    };

    let mut occupied = collect_occupied_project_ids(&vector_project);
    let mut new_layers: Vec<neoflash_core::Layer> = Vec::new();
    let mut emitted_masks = std::collections::HashMap::<Id, Id>::new();
    let fps = f64::from(source_project.fps);

    let mut idx = 0usize;
    while idx < selected.len() {
        let first_mask = mask_for_layer(&selected[idx].0.layer);
        let first_mask_src_id = first_mask.map(|m| m.id.as_str());
        let mut end = idx + 1;
        while end < selected.len()
            && mask_for_layer(&selected[end].0.layer).map(|m| m.id.as_str()) == first_mask_src_id
        {
            end += 1;
        }

        let mapped_mask_id = if let Some(src_mask) = first_mask {
            if let Some(existing) = emitted_masks.get(&src_mask.id) {
                Some(existing.clone())
            } else {
                let mut mask_els =
                    neoflash_core::sample_layer_in_at_fps(&src_sym.timeline, src_mask, frame, fps);
                for mel in &mut mask_els {
                    mel.id = alloc_unique_id("mask-el", &mut occupied);
                }
                if mask_els.is_empty() {
                    None
                } else {
                    let mid = alloc_unique_id("export-mask", &mut occupied);
                    new_layers.push(neoflash_core::Layer {
                        id: mid.clone(),
                        name: src_mask.name.clone(),
                        visible: true,
                        locked: false,
                        properties: neoflash_core::LayerProperties {
                            role: neoflash_core::LayerRole::Mask,
                            mask_mode: src_mask.properties.mask_mode,
                            ..Default::default()
                        },
                        keyframes: vec![neoflash_core::Keyframe {
                            frame: 0,
                            elements: mask_els,
                            ..Default::default()
                        }],
                    });
                    emitted_masks.insert(src_mask.id.clone(), mid.clone());
                    Some(mid)
                }
            }
        } else {
            None
        };

        let run_elements: Vec<neoflash_core::Element> = selected[idx..end]
            .iter()
            .map(|(_, el)| el.clone())
            .collect();
        let run_ids: std::collections::HashSet<&str> =
            run_elements.iter().map(|e| e.id.as_str()).collect();
        let run_bfs: Vec<_> = base_bfs
            .iter()
            .filter(|bf| run_ids.contains(bf.element_id.as_str()))
            .cloned()
            .collect();
        let run_apps: Vec<_> = base_apps
            .iter()
            .filter(|app| run_ids.contains(app.element_id.as_str()))
            .cloned()
            .collect();

        let lid = alloc_unique_id("export-layer", &mut occupied);
        new_layers.push(neoflash_core::Layer {
            id: lid,
            name: format!("Selection {}", new_layers.len() + 1),
            visible: true,
            locked: false,
            properties: neoflash_core::LayerProperties {
                mask: mapped_mask_id,
                ..Default::default()
            },
            keyframes: vec![neoflash_core::Keyframe {
                frame: 0,
                elements: run_elements,
                bitmap_fills: run_bfs,
                appearances: run_apps,
                ..Default::default()
            }],
        });

        idx = end;
    }

    if let Some(root_sym) = vector_project.symbol_mut(&root_id) {
        root_sym.timeline.layers = new_layers;
    }
    crate::timeline_commands::upgrade_all_format_versions(&mut vector_project);
    if vector_project.validate().is_ok() {
        vector_project
    } else {
        capture_project.clone()
    }
}

/// Build a lean, validated donor `Project` containing only the exact Stage selection and its
/// transitive symbol, media, font, swatch, style, and shared-library dependencies. Unreferenced
/// heavy media and document-only publish/accessibility settings are omitted so copying small
/// selections in large projects stays fast and never collides with another tab's settings.
fn build_capture_project(
    project: &neoflash_core::Project,
    symbol_id: &str,
    frame: u32,
    elements: &[neoflash_core::Element],
) -> Result<neoflash_core::Project, neoflash_core::ClipboardError> {
    use std::collections::HashSet;

    if let Err(err) = project.validate() {
        return Err(neoflash_core::ClipboardError::Invalid(format!(
            "source document is invalid: {err}"
        )));
    }

    let selected_ids: HashSet<&str> = elements.iter().map(|el| el.id.as_str()).collect();
    let has_video_element = elements
        .iter()
        .any(|el| matches!(el.kind, neoflash_core::ElementKind::Video { .. }));
    let mut stage_bitmap_fills = Vec::new();
    let mut seen_bf = HashSet::new();
    let mut stage_appearances = Vec::new();
    let mut seen_app = HashSet::new();
    let mut stage_video = None;
    let mut stage_groups = Vec::new();

    if let Some(sym) = project.symbol(symbol_id) {
        for layer in &sym.timeline.layers {
            for group in &layer.properties.groups {
                let members: Vec<Id> = group
                    .members
                    .iter()
                    .filter(|m| selected_ids.contains(m.as_str()))
                    .cloned()
                    .collect();
                if !members.is_empty() {
                    stage_groups.push(neoflash_core::AuthoringGroup {
                        id: new_id("group"),
                        name: group.name.clone(),
                        members,
                    });
                }
            }
            if let Some(kf) = layer.key_at(frame) {
                if has_video_element
                    && stage_video.is_none()
                    && kf.video.is_some()
                    && kf.elements.iter().any(|el| {
                        selected_ids.contains(el.id.as_str())
                            && matches!(el.kind, neoflash_core::ElementKind::Video { .. })
                    })
                {
                    stage_video = kf.video.clone();
                }
                for bf in &kf.bitmap_fills {
                    if selected_ids.contains(bf.element_id.as_str())
                        && seen_bf.insert(bf.element_id.clone())
                    {
                        stage_bitmap_fills.push(bf.clone());
                    }
                }
                for app in &kf.appearances {
                    if selected_ids.contains(app.element_id.as_str())
                        && seen_app.insert(app.element_id.clone())
                    {
                        stage_appearances.push(app.clone());
                    }
                }
            }
        }
    }

    let mut seeds = SeedAssetRefs::default();
    for el in elements {
        seeds.record_element(el);
    }
    for bf in &stage_bitmap_fills {
        seeds.add_resource(&bf.resource_id);
    }
    for app in &stage_appearances {
        seeds.record_appearance(app);
    }
    if let Some(vid) = &stage_video {
        seeds.add_resource(&vid.resource);
    }

    let mut reachable_syms = HashSet::<Id>::new();
    let mut stack: Vec<Id> = seeds.symbol_ids.clone();
    let mut linkage_roots_seeded = false;
    loop {
        while let Some(sid) = stack.pop() {
            if project.is_scene(&sid) || !reachable_syms.insert(sid.clone()) {
                continue;
            }
            let Some(sym) = project.symbol(&sid) else {
                continue;
            };
            if !sym.timeline.scripts.is_empty()
                || sym.class_name.is_some()
                || sym.linkage_id.is_some()
            {
                seeds.needs_sources = true;
            }
            for layer in &sym.timeline.layers {
                for kf in &layer.keyframes {
                    let prev_len = seeds.symbol_ids.len();
                    seeds.record_keyframe(kf);
                    for new_sid in &seeds.symbol_ids[prev_len..] {
                        stack.push(new_sid.clone());
                    }
                }
            }
        }
        if seeds.needs_sources && !linkage_roots_seeded {
            linkage_roots_seeded = true;
            stack.extend(
                project
                    .symbols
                    .iter()
                    .filter(|symbol| symbol.linkage_id.is_some() && !project.is_scene(&symbol.id))
                    .map(|symbol| symbol.id.clone()),
            );
            continue;
        }
        break;
    }

    let donor_font_entries = read_embedded_font_entries(&project.resources);
    let mut matched_font_entries = Vec::new();
    if !seeds.font_families.is_empty() {
        for res in &project.resources {
            if !neoflash_core::is_font_resource_unit(res) {
                continue;
            }
            let entry = donor_font_entries.iter().find(|e| e.id == res.id);
            let fam = entry
                .map(|e| e.family.trim().to_ascii_lowercase())
                .filter(|f| !f.is_empty())
                .unwrap_or_else(|| {
                    neoflash_core::font_family_for_resource_unit(res).to_ascii_lowercase()
                });
            if seeds.font_families.contains(&fam)
                || seeds
                    .font_families
                    .contains(&res.id.trim().to_ascii_lowercase())
            {
                seeds.add_resource(&res.id);
                if let Some(e) = entry {
                    matched_font_entries.push(e.clone());
                }
            }
        }
    }

    let mut occupied = collect_occupied_project_ids(project);
    for el in elements {
        occupied.insert(el.id.clone());
    }
    let root_id = alloc_unique_id("capture-root", &mut occupied);
    let layer_id = alloc_unique_id("capture-layer", &mut occupied);
    for grp in &mut stage_groups {
        grp.id = alloc_unique_id("group", &mut occupied);
    }

    let mut symbols: Vec<neoflash_core::Symbol> = project
        .symbols
        .iter()
        .filter(|s| reachable_syms.contains(&s.id))
        .cloned()
        .collect();
    let library_folders: Vec<String> = symbols
        .iter()
        .map(|s| s.folder.clone())
        .filter(|f| !f.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    symbols.insert(
        0,
        neoflash_core::Symbol {
            id: root_id.clone(),
            name: "Capture Root".into(),
            folder: String::new(),
            kind: neoflash_core::SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: neoflash_core::Timeline {
                frame_count: 1,
                scripts: vec![],
                guides: vec![],
                layers: vec![neoflash_core::Layer {
                    id: layer_id,
                    name: "Selection".into(),
                    visible: true,
                    locked: false,
                    properties: neoflash_core::LayerProperties {
                        groups: stage_groups,
                        ..Default::default()
                    },
                    keyframes: vec![neoflash_core::Keyframe {
                        frame: 0,
                        elements: elements.to_vec(),
                        bitmap_fills: stage_bitmap_fills,
                        appearances: stage_appearances,
                        video: stage_video,
                        ..Default::default()
                    }],
                }],
            },
        },
    );

    let needed_res_set: HashSet<&str> = seeds.resource_ids.iter().map(String::as_str).collect();
    let resources: Vec<neoflash_core::ResourceUnit> = project
        .resources
        .iter()
        .filter(|r| {
            !is_internal_registry_resource_path(&r.path) && needed_res_set.contains(r.id.as_str())
        })
        .cloned()
        .collect();

    let mut capture_project = neoflash_core::Project {
        format_version: project.format_version,
        name: project.name.clone(),
        width: project.width,
        height: project.height,
        fps: project.fps,
        background: project.background,
        root: root_id,
        symbols,
        library_folders,
        sources: if seeds.needs_sources {
            project.sources.clone()
        } else {
            vec![]
        },
        resources,
        document_class: None,
        scenes: vec![],
        script_language: project.script_language,
        also_compile_compatible_scripts: project.also_compile_compatible_scripts,
    };

    if !seeds.swatch_ids.is_empty() {
        let wanted: HashSet<&str> = seeds.swatch_ids.iter().map(String::as_str).collect();
        let swatches: Vec<_> = project
            .global_swatches()
            .into_iter()
            .filter(|s| wanted.contains(s.id.as_str()))
            .collect();
        if !swatches.is_empty() {
            capture_project.set_global_swatches(&swatches);
        }
    }
    if !seeds.style_ids.is_empty() {
        let wanted: HashSet<&str> = seeds.style_ids.iter().map(String::as_str).collect();
        let styles: Vec<_> = project
            .vector_styles()
            .into_iter()
            .filter(|s| wanted.contains(s.id.as_str()))
            .collect();
        if !styles.is_empty() {
            capture_project.set_vector_styles(&styles);
        }
    }
    if !reachable_syms.is_empty() {
        for entry in project.shared_libraries() {
            if reachable_syms.contains(&entry.symbol_id) {
                capture_project.set_shared_library(&entry.symbol_id.clone(), Some(entry));
            }
        }
    }
    if !matched_font_entries.is_empty() {
        write_embedded_font_entries(&mut capture_project, &matched_font_entries, &mut occupied);
    }
    crate::timeline_commands::upgrade_all_format_versions(&mut capture_project);
    Ok(capture_project)
}

/// Transitively import and deduplicate all symbols, resources (bitmaps, audio, video, embedded
/// fonts + `fonts/embedding.json`), global swatches, vector styles, shared-library linkages, and
/// script sources referenced by `seeds` from a donor snapshot into `candidate` without clobbering
/// existing assets in `candidate` and without creating duplicate assets on repeated pastes.
pub(crate) fn import_cross_document_assets(
    candidate: &mut neoflash_core::Project,
    donor_symbols: &[neoflash_core::Symbol],
    donor_scene_ids: &std::collections::HashSet<&str>,
    donor_resources: &[neoflash_core::ResourceUnit],
    donor_sources: &[neoflash_core::SourceUnit],
    mut seeds: SeedAssetRefs,
) -> Result<ImportedAssetMaps, String> {
    use std::collections::{BTreeMap, HashMap, HashSet};

    let donor_sym_by_id: HashMap<&str, &neoflash_core::Symbol> =
        donor_symbols.iter().map(|s| (s.id.as_str(), s)).collect();

    // Post-order DFS so child symbols are imported/deduplicated before parent symbols.
    let mut visited = HashSet::<Id>::new();
    let mut visiting = HashSet::<Id>::new();
    let mut post_order = Vec::<Id>::new();
    fn visit_symbol(
        sid: &str,
        donor_sym_by_id: &HashMap<&str, &neoflash_core::Symbol>,
        donor_scene_ids: &HashSet<&str>,
        visited: &mut HashSet<Id>,
        visiting: &mut HashSet<Id>,
        post_order: &mut Vec<Id>,
        seeds: &mut SeedAssetRefs,
    ) -> Result<(), String> {
        if donor_scene_ids.contains(sid) || visited.contains(sid) {
            return Ok(());
        }
        let Some(sym) = donor_sym_by_id.get(sid).copied() else {
            return Err(format!("Missing referenced symbol '{sid}' in clipboard."));
        };
        if !visiting.insert(sid.to_owned()) {
            return Err(format!("Cyclic symbol reference '{sid}' in clipboard."));
        }
        if !sym.timeline.scripts.is_empty() || sym.class_name.is_some() || sym.linkage_id.is_some()
        {
            seeds.needs_sources = true;
        }
        let mut children = Vec::new();
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                let prev = seeds.symbol_ids.len();
                seeds.record_keyframe(kf);
                for child_id in &seeds.symbol_ids[prev..] {
                    children.push(child_id.clone());
                }
                // Also inspect instances whose symbol_id was already in seeds.symbol_ids so
                // topological post-order always visits every child before `sid`.
                for el in &kf.elements {
                    if let neoflash_core::ElementKind::Instance { symbol_id, .. } = &el.kind
                        && !children.iter().any(|c| c == symbol_id)
                    {
                        children.push(symbol_id.clone());
                    }
                }
            }
        }
        for child in children {
            visit_symbol(
                &child,
                donor_sym_by_id,
                donor_scene_ids,
                visited,
                visiting,
                post_order,
                seeds,
            )?;
        }
        visiting.remove(sid);
        visited.insert(sid.to_owned());
        post_order.push(sid.to_owned());
        Ok(())
    }

    let initial_roots = seeds.symbol_ids.clone();
    for root_sid in initial_roots {
        visit_symbol(
            &root_sid,
            &donor_sym_by_id,
            donor_scene_ids,
            &mut visited,
            &mut visiting,
            &mut post_order,
            &mut seeds,
        )?;
    }
    if seeds.needs_sources {
        let linkage_roots: Vec<_> = donor_symbols
            .iter()
            .filter(|symbol| symbol.linkage_id.is_some())
            .map(|symbol| symbol.id.clone())
            .collect();
        for root_sid in linkage_roots {
            visit_symbol(
                &root_sid,
                &donor_sym_by_id,
                donor_scene_ids,
                &mut visited,
                &mut visiting,
                &mut post_order,
                &mut seeds,
            )?;
        }
    }

    // Match embedded font resources for any referenced text families.
    let donor_font_entries = read_embedded_font_entries(donor_resources);
    let mut matched_donor_fonts = Vec::<crate::panels::font_embedding::EmbeddedFontEntry>::new();
    if !seeds.font_families.is_empty() {
        for res in donor_resources {
            if !neoflash_core::is_font_resource_unit(res) {
                continue;
            }
            let entry = donor_font_entries.iter().find(|e| e.id == res.id);
            let fam = entry
                .map(|e| e.family.trim().to_ascii_lowercase())
                .filter(|f| !f.is_empty())
                .unwrap_or_else(|| {
                    neoflash_core::font_family_for_resource_unit(res).to_ascii_lowercase()
                });
            if seeds.font_families.contains(&fam)
                || seeds
                    .font_families
                    .contains(&res.id.trim().to_ascii_lowercase())
            {
                seeds.add_resource(&res.id);
                if let Some(e) = entry {
                    matched_donor_fonts.push(e.clone());
                }
            }
        }
    }

    let mut occupied_ids = collect_occupied_project_ids(candidate);

    // 1. Import / deduplicate media & font resources.
    let mut occupied_paths: HashSet<String> = candidate
        .resources
        .iter()
        .map(|r| r.path.to_ascii_lowercase())
        .collect();
    let mut res_map = BTreeMap::<Id, Id>::new();
    for res_id in &seeds.resource_ids {
        let Some(donor_res) = donor_resources.iter().find(|r| &r.id == res_id) else {
            continue;
        };
        if is_internal_registry_resource_path(&donor_res.path) {
            continue;
        }
        if let Some(existing) = candidate.resources.iter().find(|r| {
            !is_internal_registry_resource_path(&r.path)
                && r.media_type == donor_res.media_type
                && r.data_base64 == donor_res.data_base64
                && (!donor_res.data_base64.is_empty() || r.path == donor_res.path)
        }) {
            res_map.insert(res_id.clone(), existing.id.clone());
            continue;
        }
        let new_id = if occupied_ids.insert(donor_res.id.clone()) {
            donor_res.id.clone()
        } else {
            alloc_unique_id("resource", &mut occupied_ids)
        };
        let new_path = if occupied_paths.insert(donor_res.path.to_ascii_lowercase()) {
            donor_res.path.clone()
        } else {
            let (stem, ext) = match donor_res.path.rsplit_once('.') {
                Some((s, e)) => (s, format!(".{e}")),
                None => (donor_res.path.as_str(), String::new()),
            };
            let mut n = 2usize;
            loop {
                let cand_path = format!("{stem}-{n}{ext}");
                if occupied_paths.insert(cand_path.to_ascii_lowercase()) {
                    break cand_path;
                }
                n += 1;
            }
        };
        let mut imported = donor_res.clone();
        imported.id = new_id.clone();
        imported.path = new_path;
        candidate.resources.push(imported);
        res_map.insert(res_id.clone(), new_id);
        candidate.format_version = candidate.format_version.max(3);
    }

    // 2. Merge embedded font metadata (`fonts/embedding.json`) with remapped resource IDs.
    if !matched_donor_fonts.is_empty() {
        let mut local_fonts = read_embedded_font_entries(&candidate.resources);
        let mut changed_fonts = false;
        for donor_entry in matched_donor_fonts {
            let Some(mapped_res_id) = res_map.get(&donor_entry.id) else {
                continue;
            };
            if local_fonts.iter().any(|e| &e.id == mapped_res_id) {
                continue;
            }
            let mut imported_entry = donor_entry;
            imported_entry.id = mapped_res_id.clone();
            if local_fonts
                .iter()
                .any(|e| e.name.eq_ignore_ascii_case(&imported_entry.name))
            {
                let base = imported_entry.name.clone();
                let mut n = 2usize;
                while local_fonts
                    .iter()
                    .any(|e| e.name.eq_ignore_ascii_case(&imported_entry.name))
                {
                    imported_entry.name = format!("{base} {n}");
                    n += 1;
                }
            }
            if !imported_entry.class_name.is_empty()
                && local_fonts
                    .iter()
                    .any(|e| e.class_name == imported_entry.class_name)
            {
                let base = imported_entry.class_name.clone();
                let mut n = 2usize;
                while local_fonts
                    .iter()
                    .any(|e| e.class_name == imported_entry.class_name)
                {
                    imported_entry.class_name = format!("{base}_{n}");
                    n += 1;
                }
            }
            local_fonts.push(imported_entry);
            changed_fonts = true;
        }
        if changed_fonts {
            write_embedded_font_entries(candidate, &local_fonts, &mut occupied_ids);
        }
    }

    // 3. Import / deduplicate global swatches.
    let mut swatch_map = HashMap::<Id, Id>::new();
    if !seeds.swatch_ids.is_empty() {
        let donor_swatches: Vec<neoflash_core::GlobalSwatch> = donor_resources
            .iter()
            .find(|r| {
                r.path
                    .eq_ignore_ascii_case(neoflash_core::Project::GLOBAL_SWATCHES_RESOURCE_PATH)
            })
            .and_then(|r| r.decoded_bytes().ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut local_swatches = candidate.global_swatches();
        let mut changed_swatches = false;
        for sw_id in &seeds.swatch_ids {
            let Some(dsw) = donor_swatches.iter().find(|s| &s.id == sw_id) else {
                continue;
            };
            if let Some(existing) = local_swatches.iter().find(|s| {
                is_same_or_disambiguated_space(&s.name, &dsw.name) && s.color == dsw.color
            }) {
                swatch_map.insert(sw_id.clone(), existing.id.clone());
                continue;
            }
            let new_id = if occupied_ids.insert(dsw.id.clone()) {
                dsw.id.clone()
            } else {
                alloc_unique_id("swatch", &mut occupied_ids)
            };
            let mut copy = dsw.clone();
            copy.id = new_id.clone();
            if local_swatches
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(&copy.name))
            {
                let base = copy.name.clone();
                let mut n = 2usize;
                while local_swatches
                    .iter()
                    .any(|s| s.name.eq_ignore_ascii_case(&copy.name))
                {
                    copy.name = format!("{base} {n}");
                    n += 1;
                }
            }
            local_swatches.push(copy);
            swatch_map.insert(sw_id.clone(), new_id);
            changed_swatches = true;
        }
        if changed_swatches {
            candidate.set_global_swatches(&local_swatches);
        }
    }

    // 4. Import / deduplicate vector styles.
    let mut style_map = HashMap::<Id, Id>::new();
    if !seeds.style_ids.is_empty() {
        let donor_styles: Vec<neoflash_core::VectorStyle> = donor_resources
            .iter()
            .find(|r| {
                r.path
                    .eq_ignore_ascii_case(neoflash_core::Project::VECTOR_STYLES_RESOURCE_PATH)
            })
            .and_then(|r| r.decoded_bytes().ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut local_styles = candidate.vector_styles();
        let mut changed_styles = false;
        for st_id in &seeds.style_ids {
            let Some(dst) = donor_styles.iter().find(|s| &s.id == st_id) else {
                continue;
            };
            if let Some(existing) = local_styles
                .iter()
                .find(|s| vector_styles_equal_modulo_id_and_name(s, dst))
            {
                style_map.insert(st_id.clone(), existing.id.clone());
                continue;
            }
            let new_id = if occupied_ids.insert(dst.id.clone()) {
                dst.id.clone()
            } else {
                alloc_unique_id("style", &mut occupied_ids)
            };
            let mut copy = dst.clone();
            copy.id = new_id.clone();
            if local_styles
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(&copy.name))
            {
                let base = copy.name.clone();
                let mut n = 2usize;
                while local_styles
                    .iter()
                    .any(|s| s.name.eq_ignore_ascii_case(&copy.name))
                {
                    copy.name = format!("{base} {n}");
                    n += 1;
                }
            }
            local_styles.push(copy);
            style_map.insert(st_id.clone(), new_id);
            changed_styles = true;
        }
        if changed_styles {
            candidate.set_vector_styles(&local_styles);
        }
    }

    // 5. Import / deduplicate script sources without clobbering destination sources.
    if seeds.needs_sources && !donor_sources.is_empty() {
        let mut occupied_src_paths: HashSet<String> = candidate
            .sources
            .iter()
            .map(|s| s.path.to_ascii_lowercase())
            .collect();
        for src in donor_sources {
            if candidate
                .sources
                .iter()
                .any(|existing| existing.language == src.language && existing.source == src.source)
            {
                continue;
            }
            let new_id = alloc_unique_id("source", &mut occupied_ids);
            let new_path = if occupied_src_paths.insert(src.path.to_ascii_lowercase()) {
                src.path.clone()
            } else {
                let (stem, ext) = match src.path.rsplit_once('.') {
                    Some((s, e)) => (s, format!(".{e}")),
                    None => (src.path.as_str(), String::new()),
                };
                let mut n = 2usize;
                loop {
                    let cand_path = format!("{stem}_{n}{ext}");
                    if occupied_src_paths.insert(cand_path.to_ascii_lowercase()) {
                        break cand_path;
                    }
                    n += 1;
                }
            };
            let mut imported = src.clone();
            imported.id = new_id;
            imported.path = new_path;
            candidate.sources.push(imported);
        }
        if candidate.has_program() {
            candidate.enable_program();
        }
    }

    // 6. Import / deduplicate transitive symbols in bottom-up post-order.
    let donor_shared_libs: Vec<neoflash_core::SharedLibraryLinkage> = donor_resources
        .iter()
        .find(|r| {
            r.path
                .eq_ignore_ascii_case(neoflash_core::Project::SHARED_LIBRARIES_RESOURCE_PATH)
        })
        .and_then(|r| r.decoded_bytes().ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    let mut sym_map = BTreeMap::<Id, Id>::new();
    for donor_sid in post_order {
        let donor_sym = donor_sym_by_id[donor_sid.as_str()];
        let mut remapped_tl = donor_sym.timeline.clone();
        remap_timeline_internal_ids_complete(
            &mut remapped_tl,
            |prefix| alloc_unique_id(prefix, &mut occupied_ids),
            Some(&sym_map),
            Some(&res_map),
        );
        for layer in &mut remapped_tl.layers {
            for kf in &mut layer.keyframes {
                for app in &mut kf.appearances {
                    remap_appearance_refs(app, &swatch_map, &style_map);
                }
            }
        }
        let target_canon = canonicalize_timeline_for_dedup(&remapped_tl);
        if let Some(existing) = candidate.symbols.iter().find(|s| {
            !candidate.is_scene(&s.id)
                && s.kind == donor_sym.kind
                && s.folder == donor_sym.folder
                && is_same_or_disambiguated_space(&s.name, &donor_sym.name)
                && match (&s.linkage_id, &donor_sym.linkage_id) {
                    (None, None) => true,
                    (Some(el), Some(dl)) => is_same_or_disambiguated_underscore(el, dl),
                    _ => false,
                }
                && match (&s.class_name, &donor_sym.class_name) {
                    (None, None) => true,
                    (Some(ec), Some(dc)) => is_same_or_disambiguated_underscore(ec, dc),
                    _ => false,
                }
                && canonicalize_timeline_for_dedup(&s.timeline) == target_canon
        }) {
            sym_map.insert(donor_sid, existing.id.clone());
            continue;
        }

        let new_sym_id = alloc_unique_id("symbol", &mut occupied_ids);
        let mut new_name = donor_sym.name.clone();
        if candidate
            .symbols
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(&new_name))
        {
            let base = donor_sym.name.clone();
            let mut n = 2usize;
            while candidate
                .symbols
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(&new_name))
            {
                new_name = format!("{base} {n}");
                n += 1;
            }
        }

        let new_linkage = donor_sym.linkage_id.as_ref().map(|dl| {
            if !candidate
                .symbols
                .iter()
                .any(|s| s.linkage_id.as_deref() == Some(dl.as_str()))
            {
                dl.clone()
            } else {
                let mut n = 2usize;
                loop {
                    let cand = format!("{dl}_{n}");
                    if cand.len() <= 512
                        && !candidate
                            .symbols
                            .iter()
                            .any(|s| s.linkage_id.as_deref() == Some(cand.as_str()))
                    {
                        break cand;
                    }
                    n += 1;
                }
            }
        });

        let new_class = donor_sym.class_name.as_ref().map(|dc| {
            let class_taken = |c: &str| {
                candidate.document_class.as_deref() == Some(c)
                    || candidate
                        .symbols
                        .iter()
                        .any(|s| s.class_name.as_deref() == Some(c))
            };
            if !class_taken(dc) {
                dc.clone()
            } else {
                let mut n = 2usize;
                loop {
                    let cand = format!("{dc}_{n}");
                    if !class_taken(&cand) {
                        break cand;
                    }
                    n += 1;
                }
            }
        });

        let imported = neoflash_core::Symbol {
            id: new_sym_id.clone(),
            name: new_name,
            folder: donor_sym.folder.clone(),
            kind: donor_sym.kind,
            timeline: remapped_tl,
            class_name: new_class,
            linkage_id: new_linkage,
        };
        candidate.add_library_folder(&imported.folder)?;
        candidate.symbols.push(imported);
        if let Some(shared) = donor_shared_libs
            .iter()
            .find(|entry| entry.symbol_id == donor_sid)
            .cloned()
        {
            candidate.set_shared_library(&new_sym_id, Some(shared));
        }
        sym_map.insert(donor_sid, new_sym_id);
    }

    Ok(ImportedAssetMaps {
        sym_map,
        res_map,
        swatch_map,
        style_map,
        occupied_ids,
    })
}

fn paste_cross_document_payload(
    destination: &neoflash_core::Project,
    payload: &ClipboardPayload,
    symbol_id: &str,
    layer_id: &str,
    frame: u32,
    offset: [f64; 2],
) -> Result<neoflash_core::PasteResult, String> {
    if !offset.iter().all(|v| v.is_finite()) {
        return Err("Paste offset is not finite.".into());
    }
    if let Err(err) = destination.validate() {
        return Err(format!("Destination document is invalid: {err}"));
    }
    let Some(symbol) = destination.symbol(symbol_id) else {
        return Err("Destination symbol does not exist.".into());
    };
    if frame >= symbol.timeline.frame_count {
        return Err("Destination frame is outside the timeline.".into());
    }
    let Some(layer) = symbol.timeline.layers.iter().find(|l| l.id == layer_id) else {
        return Err("Destination layer does not exist in the target symbol.".into());
    };
    if layer.properties.role == neoflash_core::LayerRole::Folder
        || neoflash_core::layer_effectively_locked(&symbol.timeline, layer)
        || !neoflash_core::layer_effectively_visible(&symbol.timeline, layer)
    {
        return Err("Destination layer is locked or hidden.".into());
    }

    let mut seeds = SeedAssetRefs::default();
    for el in payload.elements() {
        seeds.record_element(el);
    }
    for bf in payload.bitmap_fills() {
        seeds.add_resource(&bf.resource_id);
    }
    for app in payload.appearances() {
        seeds.record_appearance(app);
    }
    if let Some(vid) = payload.video() {
        seeds.add_resource(&vid.resource);
    }

    let mut candidate = destination.clone();
    let empty_scenes = std::collections::HashSet::new();
    let mut maps = import_cross_document_assets(
        &mut candidate,
        payload.symbols(),
        &empty_scenes,
        payload.resources(),
        payload.sources(),
        seeds,
    )?;

    let mut pasted = payload.elements().to_vec();
    let mut element_ids = Vec::with_capacity(pasted.len());
    let mut placement_ids = std::collections::HashMap::<Id, Id>::new();
    for element in &mut pasted {
        let old_id = element.id.clone();
        let new_eid = alloc_unique_id("object", &mut maps.occupied_ids);
        element.id = new_eid.clone();
        placement_ids.insert(old_id, new_eid.clone());
        element_ids.push(new_eid);
        match &mut element.kind {
            neoflash_core::ElementKind::Instance { symbol_id, .. } => {
                if let Some(mapped) = maps.sym_map.get(symbol_id) {
                    *symbol_id = mapped.clone();
                }
            }
            neoflash_core::ElementKind::Bitmap { resource_id, .. } => {
                if let Some(mapped) = maps.res_map.get(resource_id) {
                    *resource_id = mapped.clone();
                }
            }
            _ => {}
        }
        element.transform.x += offset[0];
        element.transform.y += offset[1];
    }

    let fps = f64::from(candidate.fps);
    {
        let timeline = &mut candidate.symbol_mut(symbol_id).unwrap().timeline;
        let layer_idx = timeline
            .layers
            .iter()
            .position(|l| l.id == layer_id)
            .unwrap();
        if !timeline.layers[layer_idx]
            .keyframes
            .iter()
            .any(|k| k.frame == frame)
        {
            let elements = neoflash_core::sample_layer_in_at_fps(
                timeline,
                &timeline.layers[layer_idx],
                frame,
                fps,
            );
            let layer = &mut timeline.layers[layer_idx];
            let active = layer
                .keyframes
                .iter()
                .filter(|k| k.frame < frame)
                .max_by_key(|k| k.frame);
            let (tween, tween_kind, orient_to_path) = active
                .filter(|k| {
                    k.tween.is_some()
                        && matches!(
                            k.tween_kind,
                            neoflash_core::TweenKind::Classic | neoflash_core::TweenKind::Shape
                        )
                })
                .map_or((None, neoflash_core::TweenKind::Classic, false), |k| {
                    (k.tween, k.tween_kind, k.orient_to_path)
                });
            layer.keyframes.push(neoflash_core::Keyframe {
                frame,
                elements,
                tween,
                tween_kind,
                orient_to_path,
                label: String::new(),
                ..Default::default()
            });
            layer.keyframes.sort_by_key(|k| k.frame);
        }
        let layer = &mut timeline.layers[layer_idx];
        let dest_key = layer
            .keyframes
            .iter_mut()
            .find(|k| k.frame == frame)
            .unwrap();
        dest_key.elements.extend(pasted);
        for original_bf in payload.bitmap_fills() {
            if let Some(new_eid) = placement_ids.get(&original_bf.element_id) {
                let mut bf = original_bf.clone();
                bf.element_id = new_eid.clone();
                if let Some(new_rid) = maps.res_map.get(&bf.resource_id) {
                    bf.resource_id = new_rid.clone();
                }
                dest_key.set_bitmap_fill(bf);
            }
        }
        for original_app in payload.appearances() {
            if let Some(new_eid) = placement_ids.get(&original_app.element_id) {
                let mut app = original_app.clone();
                app.element_id = new_eid.clone();
                remap_appearance_refs(&mut app, &maps.swatch_map, &maps.style_map);
                dest_key.set_appearance(app);
            }
        }
        if let Some(original_video) = payload.video()
            && dest_key.video.is_none()
            && dest_key.keyed_properties.is_none()
        {
            let mut video = original_video.clone();
            if let Some(new_rid) = maps.res_map.get(&video.resource) {
                video.resource = new_rid.clone();
            }
            dest_key.video = Some(video);
        }
        layer.normalize_motion_keys();
        for original in payload.groups() {
            let mut group = original.clone();
            group.id = alloc_unique_id("group", &mut maps.occupied_ids);
            group.members = group
                .members
                .into_iter()
                .filter_map(|m| placement_ids.get(&m).cloned())
                .collect();
            if !group.members.is_empty() {
                layer.properties.groups.push(group);
            }
        }
    }

    candidate.prune_authoring_groups();
    crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
    if let Err(err) = candidate.validate() {
        return Err(format!("Pasted document failed validation: {err}"));
    }
    Ok(neoflash_core::PasteResult {
        project: candidate,
        element_ids,
    })
}
