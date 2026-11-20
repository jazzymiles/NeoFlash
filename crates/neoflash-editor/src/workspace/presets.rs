use super::*;

/// Layout-only state: excludes document data, history, clipboard and general preferences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspacePreset {
    // Required to distinguish complete presets from the older DockLayout-only representation.
    pub dock: DockLayout,
    #[serde(default)]
    pub panels: PanelVisibility,
    #[serde(default = "default_preset_bottom")]
    pub active_bottom: BottomTab,
    #[serde(default)]
    pub sizes: PanelSizes,
    #[serde(default)]
    pub detached: DetachedPanels,
    #[serde(default)]
    pub floating: Vec<FloatingDock>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub collapsed_floating: std::collections::BTreeSet<PanelId>,
    #[serde(default = "default_true")]
    pub actions_sidebar: bool,
    #[serde(default)]
    pub template: WorkspaceTemplate,
}

pub(super) fn default_preset_bottom() -> BottomTab {
    BottomTab::Properties
}

pub(super) fn deserialize_named_presets<'de, D>(
    deserializer: D,
) -> Result<std::collections::BTreeMap<String, WorkspacePreset>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StoredPreset {
        Complete(Box<WorkspacePreset>),
        Legacy(DockLayout),
    }
    let stored = std::collections::BTreeMap::<String, StoredPreset>::deserialize(deserializer)?;
    Ok(stored
        .into_iter()
        .map(|(name, preset)| {
            let preset = match preset {
                StoredPreset::Complete(preset) => *preset,
                StoredPreset::Legacy(dock) => WorkspacePreset::capture(&WorkspacePrefs {
                    dock,
                    ..WorkspacePrefs::default()
                }),
            };
            (name, preset)
        })
        .collect())
}

impl WorkspacePreset {
    pub(super) fn capture(prefs: &WorkspacePrefs) -> Self {
        Self {
            dock: prefs.dock.clone(),
            panels: prefs.panels,
            active_bottom: prefs.active_bottom,
            sizes: prefs.sizes,
            detached: prefs.detached,
            floating: prefs.floating.clone(),
            collapsed_floating: prefs.collapsed_floating.clone(),
            actions_sidebar: prefs.code_editor.actions_sidebar,
            template: prefs.template,
        }
    }

    pub(super) fn apply_to(&self, prefs: &mut WorkspacePrefs) {
        prefs.dock = self.dock.clone();
        prefs.panels = self.panels;
        prefs.active_bottom = self.active_bottom;
        prefs.sizes = self.sizes;
        prefs.detached = self.detached;
        prefs.floating = self.floating.clone();
        prefs.collapsed_floating = self.collapsed_floating.clone();
        prefs.code_editor.actions_sidebar = self.actions_sidebar;
        prefs.template = self.template;
        prefs.sanitize_layout();
    }

    pub(super) fn sanitize(&mut self) {
        let mut prefs = WorkspacePrefs::default();
        self.apply_to(&mut prefs);
        *self = Self::capture(&prefs);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspacePresetError {
    InvalidName,
    ReservedName,
    AlreadyExists,
    NotFound,
    LimitReached,
}

pub(super) fn preset_name(name: &str) -> Result<&str, WorkspacePresetError> {
    let name = normalized_preset_name(name)?;
    if WorkspaceTemplate::ALL
        .iter()
        .any(|template| template.label().eq_ignore_ascii_case(name))
    {
        return Err(WorkspacePresetError::ReservedName);
    }
    Ok(name)
}

pub(super) fn normalized_preset_name(name: &str) -> Result<&str, WorkspacePresetError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(WorkspacePresetError::InvalidName);
    }
    Ok(name)
}

impl WorkspacePrefs {
    pub const MAX_NAMED_PRESETS: usize = 32;

    pub fn active_workspace_label(&self) -> &str {
        self.active_named_preset.as_deref().unwrap_or_else(|| {
            if self.unsaved_layout {
                "Unsaved layout"
            } else {
                self.template.label()
            }
        })
    }

    /// Naming-dialog validation without capturing or changing the live layout.
    /// `true` means Save must explicitly offer Replace; Rename never replaces another preset.
    pub(crate) fn check_named_preset(
        &self,
        name: &str,
        rename_from: Option<&str>,
    ) -> Result<bool, WorkspacePresetError> {
        let name = preset_name(name)?;
        let exists = self.named_presets.contains_key(name);
        if let Some(old) = rename_from {
            if !self.named_presets.contains_key(old) {
                return Err(WorkspacePresetError::NotFound);
            }
            if exists && name != old {
                return Err(WorkspacePresetError::AlreadyExists);
            }
        } else if !exists && self.named_presets.len() >= Self::MAX_NAMED_PRESETS {
            return Err(WorkspacePresetError::LimitReached);
        }
        Ok(exists)
    }

    /// Create a complete layout preset; an existing name is never overwritten implicitly.
    #[allow(dead_code)]
    pub fn save_named_preset(&mut self, name: &str) -> bool {
        self.store_named_preset(name, false).is_ok()
    }

    /// `overwrite` must come from an explicit replacement choice in the naming UI.
    #[allow(dead_code)]
    pub fn store_named_preset(
        &mut self,
        name: &str,
        overwrite: bool,
    ) -> Result<(), WorkspacePresetError> {
        let name = preset_name(name)?;
        if self.named_presets.contains_key(name) {
            if !overwrite {
                return Err(WorkspacePresetError::AlreadyExists);
            }
        } else if self.named_presets.len() >= Self::MAX_NAMED_PRESETS {
            return Err(WorkspacePresetError::LimitReached);
        }
        let mut preset = WorkspacePreset::capture(self);
        preset.sanitize();
        self.named_presets.insert(name.to_owned(), preset);
        self.active_named_preset = Some(name.to_owned());
        self.unsaved_layout = false;
        Ok(())
    }

    /// Apply a previously saved named workspace layout preset.
    #[allow(dead_code)]
    pub fn apply_named_preset(&mut self, name: &str) -> bool {
        let name = name.trim();
        let Some(preset) = self.named_presets.get(name).cloned() else {
            return false;
        };
        preset.apply_to(self);
        self.active_named_preset = Some(name.to_owned());
        self.unsaved_layout = false;
        true
    }

    /// Rename atomically, preserving the saved arrangement rather than capturing the current one.
    #[allow(dead_code)]
    pub fn rename_named_preset(
        &mut self,
        old_name: &str,
        new_name: &str,
        overwrite: bool,
    ) -> Result<(), WorkspacePresetError> {
        let old_name = old_name.trim();
        let new_name = preset_name(new_name)?;
        if !self.named_presets.contains_key(old_name) {
            return Err(WorkspacePresetError::NotFound);
        }
        if old_name == new_name {
            return Ok(());
        }
        if self.named_presets.contains_key(new_name) && !overwrite {
            return Err(WorkspacePresetError::AlreadyExists);
        }
        let preset = self.named_presets.remove(old_name).expect("checked preset");
        self.named_presets.insert(new_name.to_owned(), preset);
        if self.active_named_preset.as_deref() == Some(old_name) {
            self.active_named_preset = Some(new_name.to_owned());
        } else if self.active_named_preset.as_deref() == Some(new_name) {
            // The displayed arrangement still belongs to the replaced preset, not the renamed one.
            self.active_named_preset = None;
            self.unsaved_layout = true;
        }
        Ok(())
    }

    /// Delete a user-saved workspace layout preset.
    #[allow(dead_code)]
    pub fn delete_named_preset(&mut self, name: &str) -> bool {
        let name = name.trim();
        if self.named_presets.remove(name).is_none() {
            return false;
        }
        if self.active_named_preset.as_deref() == Some(name) {
            self.active_named_preset = None;
            self.unsaved_layout = true;
        }
        true
    }
}

/// Built-in workspace templates (Window ▸ Workspace and the top-bar select). Each one is a complete
/// dock layout + panel visibility; applying it closes floating panel windows. Documents, history,
/// theme and drafts are never touched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTemplate {
    /// NF2 main window: tools left, Properties over Library, bottom dock Timeline · Actions · Output · Accessibility.
    #[default]
    Classic,
    /// Tall timeline for keyframing; Transform next to Properties.
    Animator,
    /// Drawing and layout: Properties, Color, Align, Transform visible; shorter timeline.
    Designer,
    /// Scripting: tall Actions editor with Output, timeline as a tab.
    Developer,
    /// Non-linear video & audio montage: tall preview Timeline, media Library/Project bin and Inspector.
    /// Serialized as `video_edit`; `premiere` is the pre-beta name kept for saved preferences.
    #[serde(alias = "premiere")]
    VideoEdit,
    /// Small screens: one-column tools, one inspector group, compact bottom dock.
    Essentials,
}

impl WorkspaceTemplate {
    pub const ALL: [Self; 6] = [
        Self::Classic,
        Self::Animator,
        Self::Designer,
        Self::Developer,
        Self::VideoEdit,
        Self::Essentials,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Animator => "Animator",
            Self::Designer => "Designer",
            Self::Developer => "Developer",
            Self::VideoEdit => "Video edit",
            Self::Essentials => "Essentials",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Classic => {
                "Tools, Properties over Library, Timeline with Actions and Output below."
            }
            Self::Animator => "Tall timeline for keyframing, Transform beside Properties.",
            Self::Designer => "Properties, Color, Align and Transform open; shorter timeline.",
            Self::Developer => "Tall Actions editor with Output; Timeline as a tab.",
            Self::VideoEdit => {
                "Non-linear video and audio montage with linked tracks, Razor cuts and filmstrip preview."
            }
            Self::Essentials => "One-column tools and a compact dock for small screens.",
        }
    }

    /// Dock layout, visibility and active bottom tab of this template.
    pub fn layout(self) -> (DockLayout, PanelVisibility, BottomTab) {
        use PanelId::*;
        let group = |id, tabs: Vec<PanelId>, weight: f32| DockGroup {
            id,
            active: tabs[0],
            tabs,
            weight,
        };
        let area = |size: f32, groups: Vec<DockGroup>| DockArea { size, groups };
        let tools = |size: f32| area(size + 6., vec![group(3, vec![Tools], 1.)]);
        let two_columns = neoflash_ui::metrics::tools_two_columns();
        let inspector = neoflash_ui::metrics::inspector() + 6.;
        let mut visible = PanelVisibility::default();
        let (dock, bottom) = match self {
            Self::Classic => (DockLayout::default(), BottomTab::Properties),
            Self::Animator => {
                visible.transform = true;
                let dock = DockLayout {
                    top: area(crate::theme::timeline_h(), vec![]),
                    bottom: area(
                        520.,
                        vec![group(
                            2,
                            vec![
                                Timeline,
                                Actions,
                                Output,
                                Accessibility,
                                MotionEditor,
                                Debugger,
                                CompilerErrors,
                            ],
                            1.,
                        )],
                    ),
                    left: tools(two_columns),
                    right: area(
                        inspector,
                        vec![
                            group(
                                5,
                                vec![
                                    Properties,
                                    Filters,
                                    Transform,
                                    Align,
                                    Color,
                                    Swatches,
                                    Info,
                                    Ease,
                                    History,
                                    Scene,
                                    Project,
                                    CodeSnippets,
                                    MovieExplorer,
                                    Brushes,
                                ],
                                0.55,
                            ),
                            group(4, vec![Library], 0.45),
                        ],
                    ),
                    next_group: 6,
                };
                (dock, BottomTab::Properties)
            }
            Self::Designer => {
                visible.align = true;
                visible.transform = true;
                visible.color = true;
                let dock = DockLayout {
                    top: area(crate::theme::timeline_h(), vec![]),
                    bottom: area(
                        neoflash_ui::metrics::BOTTOM_DOCK,
                        vec![group(
                            2,
                            vec![
                                Timeline,
                                Actions,
                                Output,
                                Accessibility,
                                MotionEditor,
                                Debugger,
                                CompilerErrors,
                            ],
                            1.,
                        )],
                    ),
                    left: tools(two_columns),
                    right: area(
                        inspector,
                        vec![
                            group(
                                5,
                                vec![
                                    Properties,
                                    Filters,
                                    Library,
                                    Info,
                                    Ease,
                                    History,
                                    Scene,
                                    Project,
                                    CodeSnippets,
                                    MovieExplorer,
                                    Brushes,
                                ],
                                0.5,
                            ),
                            group(6, vec![Color, Swatches, Align, Transform], 0.5),
                        ],
                    ),
                    next_group: 7,
                };
                (dock, BottomTab::Properties)
            }
            Self::Developer => {
                let mut bottom = group(
                    2,
                    vec![
                        Actions,
                        Output,
                        Timeline,
                        Accessibility,
                        MotionEditor,
                        Debugger,
                        CompilerErrors,
                    ],
                    1.,
                );
                bottom.active = Actions;
                let dock = DockLayout {
                    top: area(crate::theme::timeline_h(), vec![]),
                    bottom: area(560., vec![bottom]),
                    left: tools(two_columns),
                    right: area(
                        inspector,
                        vec![
                            group(
                                5,
                                vec![
                                    Properties,
                                    Filters,
                                    Align,
                                    Transform,
                                    Color,
                                    Swatches,
                                    Info,
                                    Ease,
                                    History,
                                    Scene,
                                    Project,
                                    CodeSnippets,
                                    MovieExplorer,
                                    Brushes,
                                ],
                                0.5,
                            ),
                            group(4, vec![Library], 0.5),
                        ],
                    ),
                    next_group: 6,
                };
                (dock, BottomTab::Actions)
            }
            Self::VideoEdit => {
                visible.project = true;
                visible.scene = true;
                let dock = DockLayout {
                    top: area(crate::theme::timeline_h(), vec![]),
                    bottom: area(
                        480.,
                        vec![group(
                            2,
                            vec![
                                Timeline,
                                Output,
                                Actions,
                                Accessibility,
                                MotionEditor,
                                Debugger,
                                CompilerErrors,
                            ],
                            1.,
                        )],
                    ),
                    left: tools(two_columns),
                    right: area(
                        inspector,
                        vec![
                            group(
                                5,
                                vec![
                                    Properties,
                                    Filters,
                                    Transform,
                                    Align,
                                    Color,
                                    Swatches,
                                    Info,
                                    Ease,
                                    History,
                                    Scene,
                                    CodeSnippets,
                                    MovieExplorer,
                                    Brushes,
                                ],
                                0.5,
                            ),
                            group(4, vec![Library, Project], 0.5),
                        ],
                    ),
                    next_group: 6,
                };
                (dock, BottomTab::Properties)
            }
            Self::Essentials => {
                let dock = DockLayout {
                    top: area(crate::theme::timeline_h(), vec![]),
                    bottom: area(
                        neoflash_ui::metrics::BOTTOM_DOCK,
                        vec![group(
                            2,
                            vec![
                                Timeline,
                                Actions,
                                Output,
                                Accessibility,
                                MotionEditor,
                                Debugger,
                                CompilerErrors,
                            ],
                            1.,
                        )],
                    ),
                    left: tools(neoflash_ui::metrics::tools_rail()),
                    right: area(
                        inspector,
                        vec![group(
                            5,
                            vec![
                                Properties,
                                Filters,
                                Library,
                                Align,
                                Transform,
                                Color,
                                Swatches,
                                Info,
                                Ease,
                                History,
                                Scene,
                                Project,
                                CodeSnippets,
                                MovieExplorer,
                                Brushes,
                            ],
                            1.,
                        )],
                    ),
                    next_group: 6,
                };
                (dock, BottomTab::Properties)
            }
        };
        (dock, visible, bottom)
    }
}

impl WorkspacePrefs {
    /// Reset layout only. Theme is a separate user preference. The caller may
    /// remove known egui PanelState IDs, but must preserve authoring/input state.
    /// Replace the layout with a built-in template, keeping theme and remembered window geometry.
    pub fn apply_template(&mut self, template: WorkspaceTemplate) {
        let (dock, panels, active_bottom) = template.layout();
        *self = Self {
            theme: self.theme,
            density: self.density,
            stage_toolbar: self.stage_toolbar,
            color_picker_expanded: self.color_picker_expanded,
            ui_scale: self.ui_scale,
            undo_levels: self.undo_levels,
            shift_select: self.shift_select,
            onion_range: self.onion_range,
            default_ease: self.default_ease,
            text_font: std::mem::take(&mut self.text_font),
            text_size: self.text_size,
            paste_in_place: self.paste_in_place,
            general: std::mem::take(&mut self.general),
            code_editor: CodeEditorPrefs {
                actions_sidebar: true,
                ..std::mem::take(&mut self.code_editor)
            },
            timeline_prefs: std::mem::take(&mut self.timeline_prefs),
            drawing: std::mem::take(&mut self.drawing),
            tools: std::mem::take(&mut self.tools),
            text_prefs: std::mem::take(&mut self.text_prefs),
            clipboard_prefs: std::mem::take(&mut self.clipboard_prefs),
            publish: std::mem::take(&mut self.publish),
            shortcuts: std::mem::take(&mut self.shortcuts),
            view_defaults: self.view_defaults.take(),
            recent_colors: std::mem::take(&mut self.recent_colors),
            custom_swatches: std::mem::take(&mut self.custom_swatches),
            used_colors_only_on_stage: self.used_colors_only_on_stage,
            document_defaults: self.document_defaults.take(),
            named_presets: std::mem::take(&mut self.named_presets),
            dock,
            panels,
            active_bottom,
            template,
            ..Self::default()
        };
        if template == WorkspaceTemplate::VideoEdit {
            self.timeline.preview = FramePreview::Layer;
        }
        self.sanitize();
    }
}
