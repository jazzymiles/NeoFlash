use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelId {
    Timeline,
    Tools,
    Library,
    Properties,
    Actions,
    Accessibility,
    Output,
    Align,
    Transform,
    Color,
    Ease,
    Scene,
    MotionEditor,
    Debugger,
    History,
    Swatches,
    Info,
    Project,
    Filters,
    CompilerErrors,
    CodeSnippets,
    MovieExplorer,
    Brushes,
}
impl PanelId {
    pub fn title(self) -> &'static str {
        match self {
            Self::Timeline => "Timeline",
            Self::Tools => "Tools",
            Self::Library => "Library",
            Self::Properties => "Properties",
            Self::Actions => "Actions",
            Self::Accessibility => "Accessibility",
            Self::Output => "Output",
            Self::Align => "Align",
            Self::Transform => "Transform",
            Self::Color => "Color",
            Self::Ease => "Ease",
            Self::Scene => "Scene",
            Self::MotionEditor => "Motion editor",
            Self::Debugger => "Debugger",
            Self::History => "History",
            Self::Swatches => "Swatches",
            Self::Info => "Info",
            Self::Project => "Project",
            Self::Filters => "Filters",
            Self::CompilerErrors => "Compiler errors",
            Self::CodeSnippets => "Code snippets",
            Self::MovieExplorer => "Movie Explorer",
            Self::Brushes => "Brushes",
        }
    }
    /// Body padding of the docked panel, from the Figma outlines: inspector 16, Library 8,
    /// floating-style utility panels 12; Tools, Timeline and Actions draw edge-to-edge bars.
    pub fn body_padding(self) -> f32 {
        use neoflash_ui::tokens::space;
        match self {
            // Properties (16) and Accessibility (12) pad inside their scroll areas, see panels/actions.rs.
            Self::Tools
            | Self::Timeline
            | Self::Actions
            | Self::Properties
            | Self::Accessibility => 0.,
            Self::Library => space::s2(),
            _ => space::s3(),
        }
    }
    /// Smallest height when stacked in a side column: the panel header plus its first control row
    /// Minimum useful height when stacked inside a side dock (`R-077`).
    pub fn min_stacked_height(self) -> f32 {
        match self {
            Self::Library => {
                use neoflash_ui::{metrics as nf, tokens::space};
                nf::panel_header()
                    + nf::control()
                    + 2.0 * space::s2()
                    + nf::control()
                    + 3.0 * nf::row()
                    + space::s2()
                    + 1.0
                    + nf::control()
            }
            _ => minimum_window_size(self)[1],
        }
    }
    /// NF2 icon shown in dock tabs.
    pub fn icon(self) -> neoflash_ui::Icon {
        use neoflash_ui::icons::i16;
        match self {
            Self::Timeline => i16::FILM,
            Self::Tools => i16::SELECT,
            Self::Library => i16::SYMBOL,
            Self::Properties => i16::SETTINGS,
            Self::Actions => i16::CODE,
            Self::Accessibility => i16::ACCESS,
            Self::Output => i16::TERMINAL,
            Self::Align => i16::ALIGN_LEFT,
            Self::Transform => i16::TRANSFORM,
            Self::Color => i16::COLORWHEEL,
            Self::Ease => i16::EASE,
            Self::Scene => i16::SCENE,
            Self::MotionEditor => i16::TWEEN_MOTION,
            Self::Debugger => i16::DEBUG,
            Self::History => i16::HISTORY,
            Self::Swatches => i16::SWATCHES,
            Self::Info => i16::INFO_PANEL,
            Self::Project => i16::FOLDER,
            Self::Filters => i16::FILTER,
            Self::CompilerErrors => i16::WARNING,
            Self::CodeSnippets => i16::SNIPPET,
            Self::MovieExplorer => i16::MOVIECLIP,
            Self::Brushes => i16::BRUSH,
        }
    }
    pub const DETACHABLE: [Self; 23] = [
        Self::Tools,
        Self::Timeline,
        Self::Library,
        Self::Properties,
        Self::Actions,
        Self::Accessibility,
        Self::Output,
        Self::Align,
        Self::Transform,
        Self::Color,
        Self::Ease,
        Self::Scene,
        Self::MotionEditor,
        Self::Debugger,
        Self::History,
        Self::Swatches,
        Self::Info,
        Self::Project,
        Self::Filters,
        Self::CompilerErrors,
        Self::CodeSnippets,
        Self::MovieExplorer,
        Self::Brushes,
    ];
    /// Panels that are hidden in every built-in workspace and open beside Properties (or, for the
    /// Motion editor / Debugger / Compiler errors, beside Timeline / Output) the first time they are shown.
    pub const UTILITY: [Self; 16] = [
        Self::Align,
        Self::Transform,
        Self::Color,
        Self::Ease,
        Self::Scene,
        Self::MotionEditor,
        Self::Debugger,
        Self::History,
        Self::Swatches,
        Self::Info,
        Self::Project,
        Self::Filters,
        Self::CompilerErrors,
        Self::CodeSnippets,
        Self::MovieExplorer,
        Self::Brushes,
    ];
    pub fn detachable(self) -> bool {
        Self::DETACHABLE.contains(&self)
    }
    /// Stable identifiers are not serialized egui memory or document IDs.
    pub fn viewport_key(self) -> Option<&'static str> {
        match self {
            Self::Tools => Some("neoflash.panel.tools"),
            Self::Timeline => Some("neoflash.panel.timeline"),
            Self::Actions => Some("neoflash.panel.actions"),
            Self::Library => Some("neoflash.panel.library"),
            Self::Properties => Some("neoflash.panel.properties"),
            Self::Accessibility => Some("neoflash.panel.accessibility"),
            Self::Output => Some("neoflash.panel.output"),
            Self::Align => Some("neoflash.panel.align"),
            Self::Transform => Some("neoflash.panel.transform"),
            Self::Color => Some("neoflash.panel.color"),
            Self::Ease => Some("neoflash.panel.ease"),
            Self::Scene => Some("neoflash.panel.scene"),
            Self::MotionEditor => Some("neoflash.panel.motion_editor"),
            Self::Debugger => Some("neoflash.panel.debugger"),
            Self::History => Some("neoflash.panel.history"),
            Self::Swatches => Some("neoflash.panel.swatches"),
            Self::Info => Some("neoflash.panel.info"),
            Self::Project => Some("neoflash.panel.project"),
            Self::Filters => Some("neoflash.panel.filters"),
            Self::CompilerErrors => Some("neoflash.panel.compiler_errors"),
            Self::CodeSnippets => Some("neoflash.panel.code_snippets"),
            Self::MovieExplorer => Some("neoflash.panel.movie_explorer"),
            Self::Brushes => Some("neoflash.panel.brushes"),
        }
    }
    #[cfg(test)]
    pub const ALL: [Self; 23] = [
        Self::Timeline,
        Self::Tools,
        Self::Library,
        Self::Properties,
        Self::Actions,
        Self::Accessibility,
        Self::Output,
        Self::Align,
        Self::Transform,
        Self::Color,
        Self::Ease,
        Self::Scene,
        Self::MotionEditor,
        Self::Debugger,
        Self::History,
        Self::Swatches,
        Self::Info,
        Self::Project,
        Self::Filters,
        Self::CompilerErrors,
        Self::CodeSnippets,
        Self::MovieExplorer,
        Self::Brushes,
    ];
    pub const BOTTOM: [Self; 4] = [
        Self::Properties,
        Self::Actions,
        Self::Accessibility,
        Self::Output,
    ];
    pub fn bottom_tab(self) -> Option<BottomTab> {
        match self {
            Self::Properties => Some(BottomTab::Properties),
            Self::Actions => Some(BottomTab::Actions),
            Self::Accessibility => Some(BottomTab::Accessibility),
            Self::Output => Some(BottomTab::Output),
            _ => None,
        }
    }
    pub fn from_bottom_tab(tab: BottomTab) -> Self {
        match tab {
            BottomTab::Properties => Self::Properties,
            BottomTab::Actions => Self::Actions,
            BottomTab::Accessibility => Self::Accessibility,
            BottomTab::Output => Self::Output,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelVisibility {
    pub timeline: bool,
    pub tools: bool,
    pub library: bool,
    pub properties: bool,
    pub actions: bool,
    pub accessibility: bool,
    pub output: bool,
    pub align: bool,
    pub transform: bool,
    pub color: bool,
    pub ease: bool,
    pub scene: bool,
    pub motion_editor: bool,
    pub debugger: bool,
    pub history: bool,
    pub swatches: bool,
    pub info: bool,
    pub project: bool,
    pub filters: bool,
    pub compiler_errors: bool,
    pub code_snippets: bool,
    pub movie_explorer: bool,
    pub brushes: bool,
}
impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            timeline: true,
            tools: true,
            library: true,
            properties: true,
            actions: true,
            accessibility: false,
            output: true,
            align: false,
            transform: false,
            color: false,
            ease: false,
            scene: false,
            motion_editor: false,
            debugger: false,
            history: false,
            swatches: false,
            info: false,
            project: false,
            filters: false,
            compiler_errors: false,
            code_snippets: false,
            movie_explorer: false,
            brushes: false,
        }
    }
}
impl PanelVisibility {
    pub(super) fn slot(&mut self, panel: PanelId) -> &mut bool {
        match panel {
            PanelId::Timeline => &mut self.timeline,
            PanelId::Tools => &mut self.tools,
            PanelId::Library => &mut self.library,
            PanelId::Properties => &mut self.properties,
            PanelId::Actions => &mut self.actions,
            PanelId::Accessibility => &mut self.accessibility,
            PanelId::Output => &mut self.output,
            PanelId::Align => &mut self.align,
            PanelId::Transform => &mut self.transform,
            PanelId::Color => &mut self.color,
            PanelId::Ease => &mut self.ease,
            PanelId::Scene => &mut self.scene,
            PanelId::MotionEditor => &mut self.motion_editor,
            PanelId::Debugger => &mut self.debugger,
            PanelId::History => &mut self.history,
            PanelId::Swatches => &mut self.swatches,
            PanelId::Info => &mut self.info,
            PanelId::Project => &mut self.project,
            PanelId::Filters => &mut self.filters,
            PanelId::CompilerErrors => &mut self.compiler_errors,
            PanelId::CodeSnippets => &mut self.code_snippets,
            PanelId::MovieExplorer => &mut self.movie_explorer,
            PanelId::Brushes => &mut self.brushes,
        }
    }
    pub fn is_visible(self, panel: PanelId) -> bool {
        match panel {
            PanelId::Timeline => self.timeline,
            PanelId::Tools => self.tools,
            PanelId::Library => self.library,
            PanelId::Properties => self.properties,
            PanelId::Actions => self.actions,
            PanelId::Accessibility => self.accessibility,
            PanelId::Output => self.output,
            PanelId::Align => self.align,
            PanelId::Transform => self.transform,
            PanelId::Color => self.color,
            PanelId::Ease => self.ease,
            PanelId::Scene => self.scene,
            PanelId::MotionEditor => self.motion_editor,
            PanelId::Debugger => self.debugger,
            PanelId::History => self.history,
            PanelId::Swatches => self.swatches,
            PanelId::Info => self.info,
            PanelId::Project => self.project,
            PanelId::Filters => self.filters,
            PanelId::CompilerErrors => self.compiler_errors,
            PanelId::CodeSnippets => self.code_snippets,
            PanelId::MovieExplorer => self.movie_explorer,
            PanelId::Brushes => self.brushes,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelSizes {
    pub timeline_height: f32,
    pub bottom_height: f32,
    pub library_width: f32,
    pub layer_column_width: f32,
}
impl Default for PanelSizes {
    fn default() -> Self {
        Self {
            timeline_height: crate::theme::timeline_h(),
            bottom_height: 190.,
            library_width: 280.,
            layer_column_width: neoflash_ui::metrics::LAYER_COLUMN_MODE_SOLO,
        }
    }
}
impl PanelSizes {
    /// Stored preferences have static safety limits. Rendering additionally
    /// clamps these against the current viewport and space reserved for Stage.
    pub(super) fn sanitize(&mut self) {
        let default = Self::default();
        let size = |value: f32, fallback: f32, min: f32, max: f32| {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                fallback
            }
        };
        self.timeline_height = size(
            self.timeline_height,
            default.timeline_height,
            crate::theme::TIMELINE_MIN_HEIGHT,
            380.,
        );
        self.bottom_height = size(self.bottom_height, default.bottom_height, 145., 420.);
        self.library_width = size(self.library_width, default.library_width, 248., 360.);
        self.layer_column_width = size(
            self.layer_column_width,
            default.layer_column_width,
            140.,
            420.,
        );
    }
}
