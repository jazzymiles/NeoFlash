use crate::{
    app::{BottomTab, Editor, ScriptTarget},
    selection::{Selection, SelectionTarget},
};
use eframe::egui::{self, Event, Key};
use neoflash_core::{ClipboardPayload, Id, ObjectEvent, evaluate, new_id};

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

pub(crate) struct ClipboardState {
    pub(crate) session: String,
    pub(crate) menu_open: bool,
    pub(crate) menu_viewport: Option<egui::ViewportId>,
    pub(crate) menu_target: Option<ClipboardTarget>,
    pending: Option<Request>,
    view: Option<(Id, u64, [f64; 2])>,
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
    pub(crate) fn copy_selection(&mut self, ctx: &egui::Context, cut: bool) {
        if self.confirm.is_some() {
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
        let prepared = ClipboardPayload::capture(
            &self.project,
            selected.into_iter().map(|(_, element)| element).collect(),
            &self.clipboard.session,
            anchor,
        )
        .and_then(|payload| payload.to_text());
        match prepared {
            Ok(text) => {
                ctx.copy_text(text);
                if cut {
                    // Prepare the complete clipboard before removing any artwork.
                    // The existing transaction keeps Cut recoverable with one Undo.
                    self.delete_selected();
                }
                self.message(format!(
                    "{} {count} object(s).",
                    if cut { "Cut" } else { "Copied" }
                ));
            }
            Err(error) => self.message(format!("Copy failed: {error}")),
        }
    }
    pub(crate) fn paste_objects(&mut self, ctx: &egui::Context, text: &str, in_place: bool) {
        // Preferences ▸ Clipboard ▸ Default paste = In place: plain Paste keeps the position too.
        let in_place = in_place || self.workspace.paste_in_place;
        if self.confirm.is_some() {
            return;
        }
        let payload = match ClipboardPayload::from_text(text) {
            Ok(Some(payload)) => payload,
            Ok(None) => {
                if is_svg_clipboard_text(text) {
                    self.paste_svg(text, in_place);
                } else if let Some((media_type, bytes)) = parse_image_data_url(text) {
                    self.paste_image_bytes(ctx, &bytes, &media_type, in_place);
                } else if !self.paste_plain_text_into_selected_text(text) {
                    self.message("The clipboard does not contain NeoFlash objects.");
                }
                return;
            }
            Err(error) => {
                self.message(format!("Paste failed: {error}"));
                return;
            }
        };
        self.finish_edits();
        let symbol = self.symbol_id().to_owned();
        let layer = self.symbol().timeline.layers[self.layer].id.clone();
        let center = self.paste_center();
        let anchor = payload.anchor();
        let offset = if in_place {
            [0., 0.]
        } else {
            [center[0] - anchor[0], center[1] - anchor[1]]
        };
        match payload.paste_into(
            &self.project,
            &self.clipboard.session,
            &symbol,
            &layer,
            self.frame,
            offset,
        ) {
            Ok(result) => {
                let count = result.element_ids.len();
                self.playing = false;
                self.transact(|editor| {
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
    fn paste_plain_text_into_selected_text(&mut self, raw_text: &str) -> bool {
        if self.selection.single().is_none() || !self.selected_editable() {
            return false;
        }
        let Some(sampled) = self.selected_sampled() else {
            return false;
        };
        let normalized = raw_text.replace("\r\n", "\n").replace('\r', "\n");
        let next_text = match &sampled.kind {
            neoflash_core::ElementKind::Text { .. } => normalized.replace('\n', " "),
            neoflash_core::ElementKind::TextBox {
                width,
                height,
                format,
                multiline,
                word_wrap,
                ..
            } => {
                let candidate = if *multiline {
                    normalized
                } else {
                    normalized.replace('\n', " ")
                };
                let candidate_kind = neoflash_core::ElementKind::TextBox {
                    text: candidate.clone(),
                    width: *width,
                    height: *height,
                    format: format.clone(),
                    word_wrap: *word_wrap,
                    multiline: *multiline,
                };
                if let Some(state) = neoflash_core::TextFieldState::from_kind(&candidate_kind)
                    && let Err(error) = state.validate()
                {
                    self.message(format!("Paste failed: {error}"));
                    return true;
                }
                candidate
            }
            _ => return false,
        };
        self.finish_edits();
        self.playing = false;
        self.transact_named("Paste text", |s| {
            s.ensure_selected_key();
            if let Some(element) = s.selected_element_mut() {
                match &mut element.kind {
                    neoflash_core::ElementKind::Text { text, .. }
                    | neoflash_core::ElementKind::TextBox { text, .. } => {
                        *text = next_text;
                    }
                    _ => {}
                }
            }
        });
        self.message("Pasted text.");
        true
    }
    fn paste_svg(&mut self, text: &str, in_place: bool) {
        self.finish_edits();
        if !self.layer_editable() {
            self.message("Paste failed: destination layer is locked or hidden.");
            return;
        }
        let symbol_id = self.symbol_id().to_owned();
        if self.frame >= self.symbol().timeline.frame_count {
            self.message("Paste failed: destination frame is outside the timeline.");
            return;
        }
        let mut graphic = match crate::svg_graphics::prepare_svg_clipboard(
            text,
            self.workspace.clipboard_prefs.editable_vector_paste,
        ) {
            Ok(graphic) => graphic,
            Err(error) => {
                self.message(format!("Paste failed: {error}"));
                return;
            }
        };
        let mut candidate = self.project.clone();
        if !graphic.resources.is_empty() {
            if let Err(error) =
                crate::graphic_import::merge_graphic_resources(&mut candidate, &mut graphic)
            {
                self.message(format!("Paste failed: {error}"));
                return;
            }
            candidate.format_version = candidate.format_version.max(3);
        }
        let offset = if in_place {
            [0., 0.]
        } else {
            let center = self.paste_center();
            [
                center[0] - graphic.width / 2.,
                center[1] - graphic.height / 2.,
            ]
        };
        let mut element_ids = Vec::with_capacity(graphic.elements.len());
        for element in &mut graphic.elements {
            element.transform.x += offset[0];
            element.transform.y += offset[1];
            element_ids.push(element.id.clone());
        }
        let layer_id = candidate.symbol(&symbol_id).unwrap().timeline.layers[self.layer]
            .id
            .clone();
        let fps = f64::from(candidate.fps);
        {
            let layer = &mut candidate.symbol_mut(&symbol_id).unwrap().timeline.layers[self.layer];
            if !layer.keyframes.iter().any(|k| k.frame == self.frame) {
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
                .keyframes
                .iter_mut()
                .find(|k| k.frame == self.frame)
                .unwrap()
                .elements
                .extend(graphic.elements);
            layer.normalize_motion_keys();
        }
        if let Err(error) = candidate.validate() {
            self.message(format!("Paste failed: {error}"));
            return;
        }
        let count = element_ids.len();
        self.playing = false;
        self.transact_named("Paste", |editor| {
            editor.project = candidate;
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
        let suffix = graphic.notice.map(|n| format!(" {n}")).unwrap_or_default();
        self.message(format!("Pasted {count} object(s).{suffix}"));
    }
    pub(crate) fn paste_image_bytes(
        &mut self,
        ctx: &egui::Context,
        bytes: &[u8],
        media_type: &str,
        in_place: bool,
    ) -> bool {
        let in_place = in_place || self.workspace.paste_in_place;
        self.finish_edits();
        if !self.layer_editable() {
            self.message("Paste failed: destination layer is locked or hidden.");
            return false;
        }
        let symbol_id = self.symbol_id().to_owned();
        if self.frame >= self.symbol().timeline.frame_count {
            self.message("Paste failed: destination frame is outside the timeline.");
            return false;
        }
        let (width, height) = match image::load_from_memory(bytes) {
            Ok(decoded) => (decoded.width().max(1), decoded.height().max(1)),
            Err(error) => {
                self.message(format!("Paste failed: invalid bitmap image ({error})."));
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
        let (resource_id, path) = loop {
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
            resource_id.clone(),
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
        candidate.resources.push(unit);
        candidate.format_version = candidate.format_version.max(3);
        let element_id = neoflash_core::new_id("bitmap");
        let (x, y) = if in_place {
            (0.0, 0.0)
        } else {
            let center = self.paste_center();
            (
                center[0] - f64::from(width) / 2.0,
                center[1] - f64::from(height) / 2.0,
            )
        };
        let element = neoflash_core::Element {
            id: element_id.clone(),
            name: String::new(),
            transform: neoflash_core::Transform {
                x,
                y,
                ..Default::default()
            },
            kind: neoflash_core::ElementKind::Bitmap {
                resource_id,
                width,
                height,
                smoothing: true,
            },
            ..Default::default()
        };
        let layer_id = candidate.symbol(&symbol_id).unwrap().timeline.layers[self.layer]
            .id
            .clone();
        let fps = f64::from(candidate.fps);
        {
            let layer = &mut candidate.symbol_mut(&symbol_id).unwrap().timeline.layers[self.layer];
            if !layer.keyframes.iter().any(|k| k.frame == self.frame) {
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
                .keyframes
                .iter_mut()
                .find(|k| k.frame == self.frame)
                .unwrap()
                .elements
                .push(element);
            layer.normalize_motion_keys();
        }
        if let Err(error) = candidate.validate() {
            self.message(format!("Paste failed: {error}"));
            return false;
        }
        self.playing = false;
        self.transact_named("Paste bitmap", |editor| {
            editor.project = candidate;
            editor.set_selection(vec![SelectionTarget {
                layer: layer_id.clone(),
                element: element_id,
            }]);
        });
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
            if !target.is_text()
                && let Ok(mut cb) = arboard::Clipboard::new()
                && cb.get_text().map_or(true, |t| t.trim().is_empty())
                && let Ok(img) = cb.get_image()
                && let Some(rgba) = image::RgbaImage::from_raw(
                    img.width as u32,
                    img.height as u32,
                    img.bytes.into_owned(),
                )
            {
                let mut cursor = std::io::Cursor::new(Vec::new());
                if image::DynamicImage::ImageRgba8(rgba)
                    .write_to(&mut cursor, image::ImageFormat::Png)
                    .is_ok()
                {
                    self.paste_image_bytes(
                        ctx,
                        &cursor.into_inner(),
                        "image/png",
                        command == ClipboardCommand::PasteInPlace,
                    );
                    return;
                }
            }
        }
        self.request_clipboard_with(ctx, command, || {
            arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.get_text())
                .map_err(|error| error.to_string())
        });
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
        if !target.is_text() && matches!(command, ClipboardCommand::Copy | ClipboardCommand::Cut) {
            self.copy_selection(ctx, command == ClipboardCommand::Cut);
            return;
        }
        if command == ClipboardCommand::PasteInPlace && target.is_text() {
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
    pub(crate) fn clipboard_events(&mut self, ctx: &egui::Context) {
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
            if ctx.wants_keyboard_input() || self.confirm.is_some() {
                if self.confirm.is_none() {
                    // App::update ticks playback after shortcuts and before
                    // painting the TextEdit that will consume this event.
                    self.playing = false;
                }
                retained.push(event.clone());
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
                    self.paste_objects(ctx, text, in_place);
                }
                _ => unreachable!(),
            }
        }
        ctx.input_mut(|input| input.events = retained);
    }
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

fn parse_image_data_url(text: &str) -> Option<(String, Vec<u8>)> {
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
