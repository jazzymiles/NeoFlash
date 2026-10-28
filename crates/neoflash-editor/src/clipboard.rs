use crate::{
    app::{BottomTab, Editor, ScriptTarget},
    selection::{Selection, SelectionTarget},
};
use eframe::egui::{self, Event, Key};
use neoflash_core::{ClipboardPayload, Id, ObjectEvent, evaluate, new_id};

/// The pinned native adapter puts the initiating key immediately before its
/// Paste payload. Never infer intent from final frame modifiers or a release.
fn paste_shortcut_mode(event: &Event) -> Option<bool> {
    match event {
        Event::Key {
            key: Key::V,
            pressed: true,
            modifiers,
            ..
        } if modifiers.command => Some(modifiers.shift),
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
    symbol: Id,
    layer: Id,
    frame: u32,
}
impl Scope {
    fn new(editor: &Editor) -> Self {
        Self {
            session: editor.clipboard.session.clone(),
            symbol: editor.symbol_id().to_owned(),
            layer: editor.symbol().timeline.layers[editor.layer].id.clone(),
            frame: editor.frame,
        }
    }
    fn matches(&self, editor: &Editor) -> bool {
        self.session == editor.clipboard.session
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
    view: Option<(Id, [f64; 2])>,
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
            self.clipboard.view = Some((self.symbol_id().to_owned(), center));
        }
    }
    pub(crate) fn paste_center(&self) -> [f64; 2] {
        self.clipboard
            .view
            .as_ref()
            .filter(|(symbol, _)| symbol == self.symbol_id())
            .map(|(_, center)| *center)
            .unwrap_or_else(|| {
                if self.editing.len() > 1 {
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
        if self.confirm.is_some() {
            return;
        }
        let payload = match ClipboardPayload::from_text(text) {
            Ok(Some(payload)) => payload,
            Ok(None) => {
                self.message("The clipboard does not contain NeoFlash objects.");
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
    pub(crate) fn request_clipboard(&mut self, ctx: &egui::Context, command: ClipboardCommand) {
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
        let mut retained = Vec::with_capacity(events.len());
        for (index, event) in events.iter().enumerate() {
            if paste_shortcut_mode(event).is_some()
                && matches!(events.get(index + 1), Some(Event::Paste(_)))
            {
                // Consume transport metadata before shortcut/TextEdit routing.
                // The payload itself is still delivered exactly once below.
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
                        .and_then(|previous| paste_shortcut_mode(&events[previous]))
                        .unwrap_or(false);
                    self.paste_objects(ctx, text, in_place);
                }
                _ => unreachable!(),
            }
        }
        ctx.input_mut(|input| input.events = retained);
    }
}
