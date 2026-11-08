//! Code snippets panel (`Panel / Code snippets`, `PP-03`; reference `scripting-panels.png`),
//! opened by Window ▸ Panels ▸ Code snippets.
//!
//! Displays the six NF2 snippet categories, live search, a language-aware preview adapted to the
//! selected stage instance (defaulting to `orbit_mc`), `Copy` to clipboard, and `Insert at frame N`
//! in a single `"Insert code snippet"` Undo transaction.
use super::planned::caption;
use crate::app::{Editor, ScriptTarget};
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Ui};
use neoflash_core::{ElementKind, FrameScript, Script, ScriptLanguage, SymbolKind};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

pub(crate) const SNIPPET_CATEGORIES: [&str; 6] = [
    "Actions",
    "Timeline navigation",
    "Animation",
    "Load and unload",
    "Audio and video",
    "Event handlers",
];

pub(crate) struct CodeSnippetDef {
    pub category: usize,
    pub title: &'static str,
    pub description: &'static str,
    pub uses_target: bool,
    pub as3_imports: &'static [&'static str],
    pub as3: &'static str,
    pub as2: &'static str,
    pub js: &'static str,
    pub ts: &'static str,
}

/// Default selected snippet in the NF2 reference (`"Rotate continuously"` under `"Animation"`).
pub(crate) const DEFAULT_SNIPPET_INDEX: usize = 6;

pub(crate) const CODE_SNIPPETS: [CodeSnippetDef; 14] = [
    // 0: Actions
    CodeSnippetDef {
        category: 0,
        title: "Click to go to frame and stop",
        description: "Moves the playhead to frame 5 and stops when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onGoToAndStop);\nfunction onGoToAndStop(e:MouseEvent):void {\n  gotoAndStop(5);\n}",
        as2: "{target}.onRelease = function() {\n  gotoAndStop(5);\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.gotoAndStop(5);\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.gotoAndStop(5);\n});",
    },
    CodeSnippetDef {
        category: 0,
        title: "Click to go to web page",
        description: "Opens a URL in a new browser tab when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &[
            "flash.events.MouseEvent",
            "flash.net.URLRequest",
            "flash.net.navigateToURL",
        ],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onOpenWebPage);\nfunction onOpenWebPage(e:MouseEvent):void {\n  navigateToURL(new URLRequest(\"https://example.com\"), \"_blank\");\n}",
        as2: "{target}.onRelease = function() {\n  getURL(\"https://example.com\", \"_blank\");\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  window.open(\"https://example.com\", \"_blank\");\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  window.open(\"https://example.com\", \"_blank\");\n});",
    },
    CodeSnippetDef {
        category: 0,
        title: "Drag and drop",
        description: "Makes the selected movie clip draggable while the mouse button is pressed.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.MOUSE_DOWN, onStartDrag);\n{target}.addEventListener(MouseEvent.MOUSE_UP, onStopDrag);\nfunction onStartDrag(e:MouseEvent):void {\n  {target}.startDrag();\n}\nfunction onStopDrag(e:MouseEvent):void {\n  {target}.stopDrag();\n}",
        as2: "{target}.onPress = function() {\n  {target}.startDrag();\n};\n{target}.onRelease = function() {\n  {target}.stopDrag();\n};",
        js: "this.{target}.addEventListener(\"mouseDown\", () => {\n  this.{target}.startDrag();\n});\nthis.{target}.addEventListener(\"mouseUp\", () => {\n  this.{target}.stopDrag();\n});",
        ts: "this.{target}.addEventListener(\"mouseDown\", (): void => {\n  this.{target}.startDrag();\n});\nthis.{target}.addEventListener(\"mouseUp\", (): void => {\n  this.{target}.stopDrag();\n});",
    },
    // 1: Timeline navigation
    CodeSnippetDef {
        category: 1,
        title: "Stop at this frame",
        description: "Stops timeline playback when the playhead enters this frame.",
        uses_target: false,
        as3_imports: &[],
        as3: "stop();",
        as2: "stop();",
        js: "this.stop();",
        ts: "this.stop();",
    },
    CodeSnippetDef {
        category: 1,
        title: "Click to go to frame and play",
        description: "Jumps to frame 1 and resumes playback when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onGoToAndPlay);\nfunction onGoToAndPlay(e:MouseEvent):void {\n  gotoAndPlay(1);\n}",
        as2: "{target}.onRelease = function() {\n  gotoAndPlay(1);\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.gotoAndPlay(1);\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.gotoAndPlay(1);\n});",
    },
    CodeSnippetDef {
        category: 1,
        title: "Click to go to next frame and stop",
        description: "Advances the timeline by one frame when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onNextFrame);\nfunction onNextFrame(e:MouseEvent):void {\n  nextFrame();\n}",
        as2: "{target}.onRelease = function() {\n  nextFrame();\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.nextFrame();\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.nextFrame();\n});",
    },
    // 2: Animation
    CodeSnippetDef {
        category: 2,
        title: "Rotate continuously",
        description: "Rotates the selected movie clip every frame. Change 10 to adjust speed.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.addEventListener(\n  Event.ENTER_FRAME, spin);\nfunction spin(e:Event):void {\n  {target}.rotation += 10;\n}",
        as2: "{target}.onEnterFrame = function() {\n  {target}._rotation += 10;\n};",
        js: "this.{target}.addEventListener(\"enterFrame\", () => {\n  this.{target}.rotation += 10;\n});",
        ts: "this.{target}.addEventListener(\"enterFrame\", (): void => {\n  this.{target}.rotation += 10;\n});",
    },
    CodeSnippetDef {
        category: 2,
        title: "Fade in movie clip",
        description: "Gradually increases alpha every frame until the movie clip is fully opaque.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.alpha = 0;\n{target}.addEventListener(Event.ENTER_FRAME, onFadeIn);\nfunction onFadeIn(e:Event):void {\n  if ({target}.alpha < 1) {\n    {target}.alpha += 0.05;\n  }\n}",
        as2: "{target}._alpha = 0;\n{target}.onEnterFrame = function() {\n  if ({target}._alpha < 100) {\n    {target}._alpha += 5;\n  }\n};",
        js: "this.{target}.alpha = 0;\nthis.{target}.addEventListener(\"enterFrame\", () => {\n  if (this.{target}.alpha < 1) {\n    this.{target}.alpha += 0.05;\n  }\n});",
        ts: "this.{target}.alpha = 0;\nthis.{target}.addEventListener(\"enterFrame\", (): void => {\n  if (this.{target}.alpha < 1) {\n    this.{target}.alpha += 0.05;\n  }\n});",
    },
    // 3: Load and unload
    CodeSnippetDef {
        category: 3,
        title: "Click to load image",
        description: "Loads an external image asset onto the display list when clicked.",
        uses_target: true,
        as3_imports: &[
            "flash.display.Loader",
            "flash.events.MouseEvent",
            "flash.net.URLRequest",
        ],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onLoadImage);\nfunction onLoadImage(e:MouseEvent):void {\n  var loader:Loader = new Loader();\n  loader.load(new URLRequest(\"assets/image.png\"));\n  addChild(loader);\n}",
        as2: "{target}.onRelease = function() {\n  {target}.loadMovie(\"assets/image.png\");\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.{target}.load(\"assets/image.png\");\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.{target}.load(\"assets/image.png\");\n});",
    },
    CodeSnippetDef {
        category: 3,
        title: "Click to unload content",
        description: "Unloads external content or removes the target clip when clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onRemoveClip);\nfunction onRemoveClip(e:MouseEvent):void {\n  if ({target}.parent != null) {\n    {target}.parent.removeChild({target});\n  }\n}",
        as2: "{target}.onRelease = function() {\n  {target}.unloadMovie();\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  if (this.{target}.parent) {\n    this.{target}.parent.removeChild(this.{target});\n  }\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  if (this.{target}.parent) {\n    this.{target}.parent.removeChild(this.{target});\n  }\n});",
    },
    // 4: Audio and video
    CodeSnippetDef {
        category: 4,
        title: "Click to play or stop sound",
        description: "Toggles playback of an audio file when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &[
            "flash.events.MouseEvent",
            "flash.media.Sound",
            "flash.media.SoundChannel",
            "flash.net.URLRequest",
        ],
        as3: "var soundClip:Sound = new Sound();\nsoundClip.load(new URLRequest(\"assets/audio.mp3\"));\nvar soundChannel:SoundChannel = null;\n{target}.addEventListener(MouseEvent.CLICK, onToggleSound);\nfunction onToggleSound(e:MouseEvent):void {\n  if (soundChannel == null) {\n    soundChannel = soundClip.play();\n  } else {\n    soundChannel.stop();\n    soundChannel = null;\n  }\n}",
        as2: "var soundClip:Sound = new Sound();\nsoundClip.loadSound(\"assets/audio.mp3\", false);\nvar soundPlaying:Boolean = false;\n{target}.onRelease = function() {\n  if (soundPlaying) {\n    soundClip.stop();\n    soundPlaying = false;\n  } else {\n    soundClip.start();\n    soundPlaying = true;\n  }\n};",
        js: "const audio = new Audio(\"assets/audio.mp3\");\nthis.{target}.addEventListener(\"click\", () => {\n  if (audio.paused) {\n    audio.play();\n  } else {\n    audio.pause();\n  }\n});",
        ts: "const audio: HTMLAudioElement = new Audio(\"assets/audio.mp3\");\nthis.{target}.addEventListener(\"click\", (): void => {\n  if (audio.paused) {\n    audio.play();\n  } else {\n    audio.pause();\n  }\n});",
    },
    CodeSnippetDef {
        category: 4,
        title: "Click to stop all sounds",
        description: "Stops all currently playing sounds when the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent", "flash.media.SoundMixer"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onStopAllSounds);\nfunction onStopAllSounds(e:MouseEvent):void {\n  SoundMixer.stopAll();\n}",
        as2: "var allSounds:Sound = new Sound();\n{target}.onRelease = function() {\n  allSounds.stop();\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  this.stopAllSounds();\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  this.stopAllSounds();\n});",
    },
    // 5: Event handlers
    CodeSnippetDef {
        category: 5,
        title: "Mouse click event",
        description: "Runs custom handler code whenever the selected instance is clicked.",
        uses_target: true,
        as3_imports: &["flash.events.MouseEvent"],
        as3: "{target}.addEventListener(MouseEvent.CLICK, onMouseClick);\nfunction onMouseClick(e:MouseEvent):void {\n  trace(\"Clicked\");\n}",
        as2: "{target}.onRelease = function() {\n  trace(\"Clicked\");\n};",
        js: "this.{target}.addEventListener(\"click\", () => {\n  console.log(\"Clicked\");\n});",
        ts: "this.{target}.addEventListener(\"click\", (): void => {\n  console.log(\"Clicked\");\n});",
    },
    CodeSnippetDef {
        category: 5,
        title: "Enter frame event",
        description: "Runs custom handler code on every frame tick of the timeline.",
        uses_target: true,
        as3_imports: &["flash.events.Event"],
        as3: "{target}.addEventListener(Event.ENTER_FRAME, onFrameTick);\nfunction onFrameTick(e:Event):void {\n  trace(\"Frame tick\");\n}",
        as2: "{target}.onEnterFrame = function() {\n  trace(\"Frame tick\");\n};",
        js: "this.{target}.addEventListener(\"enterFrame\", () => {\n  console.log(\"Frame tick\");\n});",
        ts: "this.{target}.addEventListener(\"enterFrame\", (): void => {\n  console.log(\"Frame tick\");\n});",
    },
];

impl CodeSnippetDef {
    pub fn render_body(&self, language: ScriptLanguage, target: &str) -> String {
        let template = match language {
            ScriptLanguage::ActionScript3 => self.as3,
            ScriptLanguage::ActionScript2 | ScriptLanguage::ActionScript1 => self.as2,
            ScriptLanguage::JavaScript => self.js,
            ScriptLanguage::TypeScript => self.ts,
        };
        template.replace("{target}", target)
    }

    /// Combine `existing` frame script source with this snippet, adding any missing AS3 imports
    /// to the leading import block.
    pub fn merge_into(&self, existing: &str, language: ScriptLanguage, target: &str) -> String {
        let body = self.render_body(language, target);
        let mut out = existing.to_owned();
        if language == ScriptLanguage::ActionScript3 {
            for qualified in self.as3_imports {
                if let Some(line) = crate::script_api::missing_import(&out, qualified) {
                    let at = out
                        .match_indices("import ")
                        .filter(|(i, _)| *i == 0 || out.as_bytes()[i - 1] == b'\n')
                        .last()
                        .map_or(0, |(i, _)| {
                            out[i..].find('\n').map_or(out.len(), |n| i + n + 1)
                        });
                    let formatted = if at == out.len() && !out.ends_with('\n') && !out.is_empty() {
                        format!("\n{line}\n")
                    } else {
                        format!("{line}\n")
                    };
                    out.insert_str(at, &formatted);
                }
            }
        }
        if out.trim().is_empty() {
            body
        } else if out.ends_with("\n\n")
            || out.lines().all(|l| l.trim_start().starts_with("import "))
        {
            if out.ends_with('\n') {
                format!("{out}{body}")
            } else {
                format!("{out}\n{body}")
            }
        } else if out.ends_with('\n') {
            format!("{out}\n{body}")
        } else {
            format!("{out}\n\n{body}")
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CodeSnippetsState {
    pub(crate) query: String,
    pub(crate) open_categories: [bool; 6],
    pub(crate) selected: usize,
}

impl Default for CodeSnippetsState {
    fn default() -> Self {
        Self {
            query: String::new(),
            // Matches NF2 reference (`Actions` and `Animation` open, `"Rotate continuously"` selected).
            open_categories: [true, false, true, false, false, false],
            selected: DEFAULT_SNIPPET_INDEX,
        }
    }
}

fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn sanitize_instance_stem(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else if (ch == ' ' || ch == '-') && !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "orbit".to_owned()
    } else if trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("clip_{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

impl Editor {
    pub(crate) fn active_snippet_language(&self) -> ScriptLanguage {
        if let Some(fs) = self
            .symbol()
            .timeline
            .scripts
            .iter()
            .find(|fs| fs.frame == self.frame)
        {
            return fs.script.language;
        }
        if let Some(unit) = self.project.sources.first() {
            return unit.language;
        }
        for sym in &self.project.symbols {
            if let Some(fs) = sym.timeline.scripts.first() {
                return fs.script.language;
            }
            for layer in &sym.timeline.layers {
                for key in &layer.keyframes {
                    for el in &key.elements {
                        if let Some(os) = el.scripts.first() {
                            return os.script.language;
                        }
                    }
                }
            }
        }
        ScriptLanguage::ActionScript3
    }

    /// Returns `(target_identifier, Some((element_id, auto_assigned_name)))` when a selected
    /// unnamed symbol instance should receive an automatic instance name upon snippet insertion.
    fn snippet_target_instance(&self) -> (String, Option<(String, String)>) {
        if let Some(el) = self.selected_element() {
            let trimmed = el.name.trim();
            if is_valid_ident(trimmed) {
                return (trimmed.to_owned(), None);
            }
            if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                let stem = self
                    .project
                    .symbol(symbol_id)
                    .map_or_else(|| "orbit".to_owned(), |s| sanitize_instance_stem(&s.name));
                let suffix = match self
                    .project
                    .symbol(symbol_id)
                    .map(|s| s.kind)
                    .unwrap_or(SymbolKind::MovieClip)
                {
                    SymbolKind::Button => "_btn",
                    SymbolKind::MovieClip | SymbolKind::Graphic => "_mc",
                };
                let base = if stem.ends_with("_mc") || stem.ends_with("_btn") {
                    stem
                } else {
                    format!("{stem}{suffix}")
                };
                let existing: std::collections::HashSet<&str> = self
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .filter_map(|l| l.key_at(self.frame))
                    .flat_map(|k| &k.elements)
                    .map(|e| e.name.as_str())
                    .filter(|n| !n.is_empty())
                    .collect();
                let mut candidate = base.clone();
                let mut n = 2usize;
                while existing.contains(candidate.as_str()) {
                    candidate = format!("{base}_{n}");
                    n += 1;
                }
                return (candidate.clone(), Some((el.id.clone(), candidate)));
            }
        }
        ("orbit_mc".to_owned(), None)
    }

    pub fn insert_code_snippet(&mut self, snippet_index: usize) -> bool {
        if self.symbol().kind == SymbolKind::Button {
            return false;
        }
        let Some(snippet) = CODE_SNIPPETS.get(snippet_index) else {
            return false;
        };
        let language = self.active_snippet_language();
        let (target, auto_name) = if snippet.uses_target {
            self.snippet_target_instance()
        } else {
            ("orbit_mc".to_owned(), None)
        };
        self.finish_edits();
        if self.playing {
            self.set_playing(false);
        }
        self.transact_named("Insert code snippet", |s| {
            if let Some((el_id, new_name)) = auto_name
                && s.selected_editable()
            {
                s.ensure_selected_key();
                if let Some(el) = s.selected_element_mut().filter(|e| e.id == el_id) {
                    el.name = new_name;
                }
            }
            let frame = s.frame;
            let existing = s
                .symbol()
                .timeline
                .scripts
                .iter()
                .find(|fs| fs.frame == frame)
                .map_or("", |fs| fs.script.source.as_str())
                .to_owned();
            let merged = snippet.merge_into(&existing, language, &target);
            let scripts = &mut s.symbol_mut().timeline.scripts;
            scripts.retain(|fs| fs.frame != frame);
            scripts.push(FrameScript {
                frame,
                script: Script {
                    language,
                    source: merged,
                },
            });
        });
        self.script_target = ScriptTarget::Frame;
        self.open_actions();
        true
    }

    pub(crate) fn code_snippets_panel_ui(&mut self, ui: &mut Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        ui.spacing_mut().item_spacing.y = space::S1;

        let w = ui.available_width();
        ui.add(c::Search::new(&mut self.code_snippets.query, "Search snippets").width(w));

        let query = self.code_snippets.query.trim().to_ascii_lowercase();
        let filtering = !query.is_empty();
        let selected_idx = self
            .code_snippets
            .selected
            .min(CODE_SNIPPETS.len().saturating_sub(1));
        self.code_snippets.selected = selected_idx;

        let mut double_clicked = None;
        let tree_max_h = (ui.available_height() - 190.0).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("code-snippets-tree")
            .max_height(tree_max_h)
            .auto_shrink([false, true])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = space::S1;
                for (cat_idx, cat_name) in SNIPPET_CATEGORIES.iter().enumerate() {
                    let matching: Vec<usize> = CODE_SNIPPETS
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| {
                            s.category == cat_idx
                                && (!filtering
                                    || s.title.to_ascii_lowercase().contains(&query)
                                    || s.description.to_ascii_lowercase().contains(&query)
                                    || cat_name.to_ascii_lowercase().contains(&query))
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if filtering && matching.is_empty() {
                        continue;
                    }
                    let open = filtering || self.code_snippets.open_categories[cat_idx];
                    let folder_kind = if open {
                        c::TreeItemType::FolderOpen
                    } else {
                        c::TreeItemType::FolderClosed
                    };
                    let folder_resp = c::TreeItem::new(cat_name, folder_kind).show(ui);
                    if !filtering && (folder_resp.toggle || folder_resp.row.clicked()) {
                        self.code_snippets.open_categories[cat_idx] = !open;
                    }
                    if open {
                        for idx in matching {
                            let snippet = &CODE_SNIPPETS[idx];
                            let resp = c::TreeItem::new(
                                snippet.title,
                                c::TreeItemType::Symbol(i16::SNIPPET),
                            )
                            .depth(1)
                            .selected(self.code_snippets.selected == idx)
                            .show(ui);
                            if resp.row.clicked() {
                                self.code_snippets.selected = idx;
                            }
                            if resp.row.double_clicked() {
                                self.code_snippets.selected = idx;
                                double_clicked = Some(idx);
                            }
                        }
                    }
                }
            });

        let snippet = &CODE_SNIPPETS[self.code_snippets.selected];
        let language = self.active_snippet_language();
        let (target, _) = if snippet.uses_target {
            self.snippet_target_instance()
        } else {
            ("orbit_mc".to_owned(), None)
        };
        let preview_body = snippet.render_body(language, &target);

        ui.add_space(space::S1);
        egui::Frame::new()
            .fill(nf.bg_field)
            .stroke(egui::Stroke::new(1.0_f32, nf.border_subtle))
            .corner_radius(egui::CornerRadius::same(radius::MD as u8))
            .inner_margin(egui::Margin::same(space::S3 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                caption(ui, snippet.description);
                ui.add_space(space::S1);
                for line in preview_body.lines() {
                    ui.label(rich(line, TextStyle::Mono).color(nf.text_primary));
                }
            });

        ui.add_space(space::S1);
        let can_insert = self.symbol().kind != SymbolKind::Button;
        let insert_label = format!("Insert at frame {}", self.frame + 1);
        let mut insert_clicked = double_clicked;
        ui.horizontal(|ui| {
            if ui.add(c::Button::ghost("Copy")).clicked() {
                let copy_source = snippet.merge_into("", language, &target);
                ui.ctx().copy_text(copy_source);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(can_insert, c::Button::primary(&insert_label))
                    .clicked()
                {
                    insert_clicked = Some(self.code_snippets.selected);
                }
            });
        });

        if let Some(idx) = insert_clicked {
            self.insert_code_snippet(idx);
        }
    }
}
