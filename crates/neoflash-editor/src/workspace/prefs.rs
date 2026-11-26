use super::*;

/// Opaque in-memory cache of resolved `existing_recent_files` (`T-390`).
#[derive(Debug, Default)]
pub struct RecentFilesCache(std::sync::Mutex<Option<(Vec<String>, Vec<std::path::PathBuf>)>>);

impl Clone for RecentFilesCache {
    fn clone(&self) -> Self {
        let cached = self.0.lock().ok().and_then(|guard| guard.clone());
        Self(std::sync::Mutex::new(cached))
    }
}

/// Preferences ▸ General (besides theme, UI scale, undo levels and Shift-select).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralPrefs {
    /// Interface language. Only English exists.
    pub language: String,
    pub on_launch: OnLaunch,
    /// Most recent first, bounded to 10 (File ▸ Open recent, On launch ▸ Last document).
    pub recent_files: Vec<String>,
    /// Ordered open document-tab paths from the previous session (`T-231`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_files: Vec<String>,
    /// Active tab path among `session_files` from the previous session (`T-231`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_session_file: Option<String>,
    pub autosave: bool,
    pub autosave_minutes: u32,
    /// Show keyboard shortcuts in tooltips.
    pub tooltip_shortcuts: bool,
    /// In-memory cache `(recent_files snapshot, filtered existing paths)` so `File ▸ Open recent`
    /// never repeats `Path::exists()` / `canonicalize()` syscalls across menu opens (`T-390`).
    #[serde(skip)]
    #[doc(hidden)]
    pub recent_files_cache: RecentFilesCache,
}
impl PartialEq for GeneralPrefs {
    fn eq(&self, other: &Self) -> bool {
        self.language == other.language
            && self.on_launch == other.on_launch
            && self.recent_files == other.recent_files
            && self.session_files == other.session_files
            && self.active_session_file == other.active_session_file
            && self.autosave == other.autosave
            && self.autosave_minutes == other.autosave_minutes
            && self.tooltip_shortcuts == other.tooltip_shortcuts
    }
}
impl Default for GeneralPrefs {
    fn default() -> Self {
        Self {
            language: "English".into(),
            on_launch: OnLaunch::Example,
            recent_files: Vec::new(),
            session_files: Vec::new(),
            active_session_file: None,
            autosave: true,
            autosave_minutes: 5,
            tooltip_shortcuts: true,
            recent_files_cache: RecentFilesCache::default(),
        }
    }
}

impl GeneralPrefs {
    pub const MAX_RECENT_FILES: usize = 10;
    pub const MAX_SESSION_FILES: usize = 32;

    /// Explicitly invalidate the in-memory `existing_recent_files_cached` snapshot.
    pub fn invalidate_recent_files_cache(&self) {
        if let Ok(mut guard) = self.recent_files_cache.0.lock() {
            *guard = None;
        }
    }

    /// Record the ordered open document-tab paths and active tab path (`T-231`).
    pub fn set_session_files(
        &mut self,
        paths: &[std::path::PathBuf],
        active: Option<&std::path::Path>,
    ) {
        let mut deduped: Vec<String> = Vec::new();
        let mut seen_canon: Vec<std::path::PathBuf> = Vec::new();
        for path in paths {
            let canon = crate::recovery::canonical_or_normalized_path(path);
            let entry = canon.to_string_lossy().trim().to_owned();
            if entry.is_empty() || entry.len() > 4096 || seen_canon.contains(&canon) {
                continue;
            }
            seen_canon.push(canon);
            deduped.push(entry);
            if deduped.len() >= Self::MAX_SESSION_FILES {
                break;
            }
        }
        self.session_files = deduped;
        self.active_session_file = active.and_then(|act| {
            let canon = crate::recovery::canonical_or_normalized_path(act);
            let entry = canon.to_string_lossy().trim().to_owned();
            (!entry.is_empty() && entry.len() <= 4096 && seen_canon.contains(&canon))
                .then_some(entry)
        });
    }

    /// Existing session-tab paths in saved left-to-right tab order (missing/duplicate files dropped).
    pub fn existing_session_files(&self) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut seen = Vec::new();
        for raw in &self.session_files {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.len() > 4096 {
                continue;
            }
            let path = std::path::PathBuf::from(trimmed);
            if path.is_file() {
                let canon = crate::recovery::canonical_or_normalized_path(&path);
                if !seen.contains(&canon) {
                    seen.push(canon);
                    out.push(path);
                    if out.len() >= Self::MAX_SESSION_FILES {
                        break;
                    }
                }
            }
        }
        out
    }

    /// Record `path` as the most recently opened or saved document (deduped, max 10).
    pub fn push_recent(&mut self, path: &std::path::Path) {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let entry = resolved.to_string_lossy().trim().to_owned();
        if entry.is_empty() || entry.len() > 4096 {
            return;
        }
        self.recent_files.retain(|existing| {
            if existing == &entry {
                return false;
            }
            std::fs::canonicalize(existing)
                .ok()
                .is_none_or(|canon| canon != resolved)
        });
        self.recent_files.insert(0, entry);
        self.recent_files.truncate(Self::MAX_RECENT_FILES);
        self.invalidate_recent_files_cache();
    }

    /// Existing recent document paths in most-recent-first order (missing files dropped, max 10).
    pub fn existing_recent_files(&self) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut seen_resolved = Vec::new();
        for raw in &self.recent_files {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let path = std::path::PathBuf::from(trimmed);
            if path.exists() {
                let resolved = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if !seen_resolved.contains(&resolved) && !out.contains(&path) {
                    seen_resolved.push(resolved);
                    out.push(path);
                    if out.len() >= Self::MAX_RECENT_FILES {
                        break;
                    }
                }
            }
        }
        if let Ok(mut guard) = self.recent_files_cache.0.lock() {
            *guard = Some((self.recent_files.clone(), out.clone()));
        }
        out
    }

    /// Cached existing recent document paths for menu presentation (`T-390`), avoiding repeated
    /// `stat()` / `realpath()` syscalls when `recent_files` has not changed.
    pub fn existing_recent_files_cached(&self) -> Vec<std::path::PathBuf> {
        if let Ok(guard) = self.recent_files_cache.0.lock()
            && let Some((cached_raw, cached_paths)) = guard.as_ref()
            && *cached_raw == self.recent_files
        {
            return cached_paths.clone();
        }
        self.existing_recent_files()
    }
}

/// Preferences ▸ General ▸ On launch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnLaunch {
    NewDocument,
    LastDocument,
    /// The bundled example document (behaviour before the preference existed).
    #[default]
    Example,
}

impl OnLaunch {
    pub const OPTIONS: [(Self, &'static str); 3] = [
        (Self::NewDocument, "New document"),
        (Self::LastDocument, "Last document"),
        (Self::Example, "Example"),
    ];
}

/// Preferences ▸ Code editor (Actions panel and project source files).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodeEditorPrefs {
    /// Font family; empty = the bundled NF2 mono font (Roboto Mono).
    pub font: String,
    pub font_size: f32,
    pub line_height: f32,
    pub line_numbers: bool,
    pub word_wrap: bool,
    pub auto_close_brackets: bool,
    pub code_hints: bool,
    pub hint_delay_ms: u32,
    /// Spaces inserted by Tab and used by Auto format.
    pub tab_size: u8,
    pub format_on_save: bool,
    pub highlight_current_line: bool,
    #[serde(
        default,
        skip_serializing_if = "crate::sources::SyntaxColorOverrides::is_empty"
    )]
    pub syntax_colors: crate::sources::SyntaxColorOverrides,
    #[serde(default = "default_true")]
    pub actions_sidebar: bool,
}
impl Default for CodeEditorPrefs {
    fn default() -> Self {
        Self {
            font: String::new(),
            font_size: 13.0,
            line_height: 1.6,
            line_numbers: true,
            word_wrap: false,
            auto_close_brackets: true,
            code_hints: true,
            hint_delay_ms: 300,
            tab_size: 4,
            format_on_save: true,
            highlight_current_line: true,
            syntax_colors: crate::sources::SyntaxColorOverrides::default(),
            actions_sidebar: true,
        }
    }
}

/// Preferences ▸ Timeline (besides the frame view options in `TimelineView` and the onion range).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelinePrefs {
    /// Clicking a frame selects the whole span between keyframes.
    pub span_selection: bool,
    /// New scenes get a named anchor on their first frame.
    pub scene_anchors: bool,
    /// Onion skin opacity of the nearest ghost, in percent; it falls to 15 % at the range end.
    pub onion_falloff: f32,
    /// Transforming on an in-between frame creates a keyframe (off: edits the span's keyframe).
    pub auto_keyframe: bool,
    pub tween_on_drag: TweenOnDrag,
}
impl Default for TimelinePrefs {
    fn default() -> Self {
        Self {
            span_selection: false,
            scene_anchors: false,
            onion_falloff: 70.0,
            auto_keyframe: false,
            tween_on_drag: TweenOnDrag::None,
        }
    }
}

/// Preferences ▸ Timeline ▸ Tween type on drag: the tween created when a drag on an in-between
/// frame creates a keyframe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TweenOnDrag {
    #[default]
    None,
    Classic,
    Motion,
}

/// Off / tolerance levels used by the Drawing ▸ Recognition selects (Flash wording).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tolerance {
    Off,
    MustBeClose,
    #[default]
    Normal,
    CanBeDistant,
}

impl Tolerance {
    /// Distance options for `Connect lines`, `Recognise lines`, `Recognise shapes` and
    /// `Click accuracy`.
    pub const OPTIONS: [(Self, &'static str); 4] = [
        (Self::Off, "Off"),
        (Self::MustBeClose, "Must be close"),
        (Self::Normal, "Normal"),
        (Self::CanBeDistant, "Can be distant"),
    ];

    /// Smoothing options for `Smooth curves`, mapped onto the four [`Tolerance`] levels
    /// (`Off` → `Off`, `Rough` → `MustBeClose`, `Normal` → `Normal`, `Smooth` → `CanBeDistant`).
    pub const SMOOTH_OPTIONS: [(Self, &'static str); 4] = [
        (Self::Off, "Off"),
        (Self::MustBeClose, "Rough"),
        (Self::Normal, "Normal"),
        (Self::CanBeDistant, "Smooth"),
    ];

    /// Stage hit-testing slop in screen pixels for `Click accuracy`.
    pub fn click_slop_px(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::MustBeClose => 1.0,
            Self::Normal => 4.0,
            Self::CanBeDistant => 8.0,
        }
    }

    /// Pencil Ramer–Douglas–Peucker tolerance in screen pixels for `Smooth curves`.
    /// `None` preserves raw sampled points without simplification.
    pub fn smooth_rdp_px(self) -> Option<f32> {
        match self {
            Self::Off => None,
            Self::MustBeClose => Some(0.35),
            Self::Normal => Some(0.8),
            Self::CanBeDistant => Some(2.4),
        }
    }

    /// Endpoint connection radius in screen pixels for `Connect lines`.
    pub fn connect_lines_px(self) -> Option<f32> {
        match self {
            Self::Off => None,
            Self::MustBeClose => Some(4.0),
            Self::Normal => Some(8.0),
            Self::CanBeDistant => Some(16.0),
        }
    }

    /// Maximum perpendicular deviation in screen pixels for `Recognise lines`.
    pub fn recognise_lines_px(self) -> Option<f32> {
        match self {
            Self::Off => None,
            Self::MustBeClose => Some(1.5),
            Self::Normal => Some(4.0),
            Self::CanBeDistant => Some(8.0),
        }
    }
}

/// Preferences ▸ Drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrawingPrefs {
    /// New shapes are drawing objects (Merge drawing is not available yet).
    pub object_drawing: bool,
    pub pen_preview: bool,
    pub solid_points: bool,
    /// Drawing tools show the crosshair cursor instead of the tool glyph.
    pub precise_cursors: bool,
    pub connect_lines: Tolerance,
    pub smooth_curves: Tolerance,
    pub recognise_lines: Tolerance,
    pub recognise_shapes: Tolerance,
    pub click_accuracy: Tolerance,
    /// Object snapping tolerance in screen pixels.
    pub snap_tolerance: f32,
    pub snap_align: bool,
}
impl Default for DrawingPrefs {
    fn default() -> Self {
        Self {
            object_drawing: true,
            pen_preview: true,
            solid_points: false,
            precise_cursors: false,
            connect_lines: Tolerance::Normal,
            smooth_curves: Tolerance::Normal,
            recognise_lines: Tolerance::Normal,
            recognise_shapes: Tolerance::Normal,
            click_accuracy: Tolerance::Normal,
            snap_tolerance: 8.0,
            snap_align: true,
        }
    }
}

/// Preferences ▸ Text (besides the default font and size).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextPrefs {
    pub anti_alias: TextAntiAlias,
    /// Report missing fonts that are drawn with a substitute.
    pub substitution_warning: bool,
    pub embed_glyphs: bool,
    pub spell_check: bool,
    pub vertical_rtl: bool,
    pub vertical_no_kerning: bool,
}
impl Default for TextPrefs {
    fn default() -> Self {
        Self {
            anti_alias: TextAntiAlias::Readability,
            substitution_warning: true,
            embed_glyphs: true,
            spell_check: true,
            vertical_rtl: false,
            vertical_no_kerning: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAntiAlias {
    #[default]
    Readability,
    Animation,
    Device,
}

/// Preferences ▸ Clipboard (besides Default paste).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardPrefs {
    /// Resolution of bitmaps copied to other apps.
    pub bitmap_ppi: u32,
    pub gradient_quality: Tolerance,
    pub copy_svg: bool,
    pub keep_layer_names: bool,
    /// SVG pasted from other apps becomes editable paths (off: one bitmap).
    pub editable_vector_paste: bool,
    pub ask_large_bitmaps: bool,
}
impl Default for ClipboardPrefs {
    fn default() -> Self {
        Self {
            bitmap_ppi: 144,
            gradient_quality: Tolerance::Normal,
            copy_svg: true,
            keep_layer_names: true,
            editable_vector_paste: true,
            ask_large_bitmaps: true,
        }
    }
}

/// Primary target selected in `Preferences ▸ Publish` (`DL-01`) and `Publish settings` (`DL-03`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishTarget {
    #[default]
    Html5,
    DesktopApp,
    VideoMp4,
    ImageSequence,
    SvgAnimation,
}

impl PublishTarget {
    pub const OPTIONS: [(Self, &'static str); 5] = [
        (Self::Html5, "HTML5 · Rust / WebAssembly"),
        (Self::DesktopApp, "Desktop app (macOS / Windows / Linux)"),
        (Self::VideoMp4, "Video (MP4)"),
        (Self::ImageSequence, "Image sequence / sprite sheet"),
        (Self::SvgAnimation, "SVG animation"),
    ];
}

/// Built-in publish profile presets in `Publish settings` (`DL-03`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishProfilePreset {
    #[default]
    Default,
    WebCompact,
    WebBalanced,
    WebAppDom,
    WebBannerNative,
    Criteo,
    HighQuality,
    DebugPreview,
}

impl PublishProfilePreset {
    pub const ALL: [Self; 8] = [
        Self::Default,
        Self::WebCompact,
        Self::WebBalanced,
        Self::WebAppDom,
        Self::WebBannerNative,
        Self::Criteo,
        Self::HighQuality,
        Self::DebugPreview,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::WebCompact => "Web Compact",
            Self::WebBalanced => "Web Balanced",
            Self::WebAppDom => "Web App · DOM",
            Self::WebBannerNative => "Web Banner · Native",
            Self::Criteo => "Criteo · HTML5 ZIP",
            Self::HighQuality => "High Quality",
            Self::DebugPreview => "Preview / Debug",
        }
    }
}

pub(super) fn default_true() -> bool {
    true
}

pub(super) fn default_banner_max_gzip_kb() -> u32 {
    150
}

fn default_criteo_max_zip_kib() -> u32 {
    (neoflash_export::DEFAULT_CRITEO_MAX_ZIP_BYTES / 1024) as u32
}

pub(super) fn default_sequence_import_options() -> neoflash_import::ImageSequenceImportOptions {
    neoflash_import::ImageSequenceImportOptions {
        placement: neoflash_import::ImageSequencePlacement::ConvertToGraphicSymbol,
        frame_step: 1,
        ..neoflash_import::ImageSequenceImportOptions::default()
    }
}

/// Preferences ▸ Publish: defaults for File ▸ Publish.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PublishPrefs {
    /// Active profile preset in `Publish settings`.
    #[serde(default)]
    pub profile: PublishProfilePreset,
    /// Persisted HTML5 delivery and font policy; Full/Embed preserves older preferences.
    pub options: neoflash_export::PublishOptions,
    /// Publish into this folder instead of asking each time.
    pub output_folder: Option<String>,
    pub html_wrapper: bool,
    pub assets_folder: bool,
    pub source_maps: bool,
    pub jpeg_quality: u8,
    pub strict: bool,
    pub warnings_as_errors: bool,
    pub publish_on_save: bool,
    /// Default target in `Preferences ▸ Publish ▸ Target ▸ Default target` (`DL-01`).
    #[serde(default)]
    pub default_target: PublishTarget,
    /// Whether `HTML5 · Rust / WebAssembly` is enabled in `Publish settings ▸ Targets` (`DL-03`).
    #[serde(default = "default_true")]
    pub html5_enabled: bool,
    /// Whether `Desktop app (macOS / Windows)` is enabled in `Publish settings ▸ Targets` (`DL-03`).
    #[serde(default)]
    pub desktop_app_enabled: bool,
    /// Optional explicit output folder for `Desktop app (macOS / Windows)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_app_folder: Option<String>,
    /// Whether `Video (MP4)` is enabled in `Publish settings ▸ Targets` (`DL-03`).
    #[serde(default)]
    pub video_enabled: bool,
    /// Persisted `Video (MP4)` target settings (`DL-03` / `DL-05`).
    #[serde(default)]
    pub video_options: neoflash_export::VideoOptions,
    /// Optional explicit output file for the `Video (MP4)` publish target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_file: Option<String>,
    /// Whether `Image sequence / sprite sheet` is enabled in `Publish settings ▸ Targets` (`DL-03`).
    #[serde(default)]
    pub image_sequence_enabled: bool,
    /// Optional explicit output folder for `Image sequence / sprite sheet`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_sequence_folder: Option<String>,
    /// When `true`, `Image sequence / sprite sheet` exports a packed PNG + JSON sprite sheet instead of a frame sequence.
    #[serde(default)]
    pub image_sequence_sprite_sheet: bool,
    /// Frame-sequence export format in `Publish settings ▸ Image sequence / sprite sheet` (`T-255`).
    #[serde(default)]
    pub image_sequence_format: neoflash_export::ImageExportFormat,
    /// Sprite-sheet metadata format in `Publish settings ▸ Image sequence / sprite sheet` (`T-250`).
    #[serde(default)]
    pub atlas_format: neoflash_export::AtlasFormat,
    /// Sprite-sheet raster image format (`PNG`, `WebP`, `JPEG`) (`T-250`).
    #[serde(default)]
    pub atlas_image_format: neoflash_export::AtlasImageFormat,
    /// Border-bleed extrusion in pixels (`0..=8`) for sprite-sheet packing (`T-250`).
    #[serde(default)]
    pub atlas_extrude: u32,
    /// Whether `SVG animation` is enabled in `Publish settings ▸ Targets` (`DL-03`).
    #[serde(default)]
    pub svg_enabled: bool,
    /// Optional explicit output file for `SVG animation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svg_file: Option<String>,
    /// HTML wrapper layout template (`Responsive`, `FixedStage`, `Centered`, `Fullscreen`).
    #[serde(default)]
    pub template: neoflash_export::HtmlTemplate,
    /// Stage scaling mode in HTML5 output (`FitKeepRatio`, `NoScale`, `ExactFit`, `FillCrop`).
    #[serde(default)]
    pub scale_mode: neoflash_export::HtmlScaleMode,
    /// Stage anchor alignment in HTML5 output (`Center`, `TopLeft`, `BottomRight`, etc.).
    #[serde(default)]
    pub stage_align: neoflash_export::HtmlStageAlign,
    /// Allow browser Fullscreen API (`Stage.displayState` / Fullscreen button) in HTML5 output.
    #[serde(default = "default_true")]
    pub allow_fullscreen: bool,
    /// Hardware acceleration hint in `Preferences ▸ Publish`.
    #[serde(default)]
    pub acceleration: neoflash_export::HtmlAcceleration,
    /// Loop main timeline playback in HTML5 output.
    #[serde(default = "default_true")]
    pub loop_timeline: bool,
    /// Start timeline playback automatically in HTML5 output.
    #[serde(default = "default_true")]
    pub autoplay: bool,
    /// Show loading indicator while the HTML5 runtime initializes.
    #[serde(default = "default_true")]
    pub preloader: bool,
    /// Generate a bitmap sprite sheet manifest during HTML5 publish.
    #[serde(default)]
    pub sprite_sheets: bool,
    /// Publish-time image-sequence optimization policy (`KeepFrames`, `PackAtlas`, `TranscodeVideo`) (`T-218`).
    #[serde(default)]
    pub sequence_policy: neoflash_export::SequencePublishPolicy,
    /// Maximum gzip byte budget (in KiB) for the no-WASM `Web Banner · Native` profile (`T-216`).
    #[serde(default = "default_banner_max_gzip_kb")]
    pub banner_max_gzip_kb: u32,
    /// Whole archive budget for the Criteo preset, in KiB, independently of native HTML's gzip budget.
    #[serde(default = "default_criteo_max_zip_kib")]
    pub criteo_max_zip_kib: u32,
    /// Include `trace()` output in published HTML5 packages.
    #[serde(default)]
    pub include_trace: bool,
    /// Minify serialized bundle data in published HTML5 packages.
    #[serde(default = "default_true")]
    pub minify: bool,
    /// Publish clean stage output without the technical preview footer (`true` by default).
    #[serde(default = "default_true")]
    pub publish_clean: bool,
    /// Technical HUD configuration for `Test Movie` / `Preview` (`Preferences ▸ Publish`).
    #[serde(default)]
    pub preview_chrome: neoflash_export::PlayerChromeConfig,
    /// Target FLA/XFL version profile used by `File ▸ Save As` when exporting `.fla` or `.xfl`.
    #[serde(default)]
    pub fla_target_profile: neoflash_export::FlaTargetProfile,
    /// Compatibility options for `File ▸ Save As` FLA/XFL export.
    #[serde(default)]
    pub fla_export_options: neoflash_export::FlaExportOptions,
    /// HTML5 stage rendering mode (`Canvas` default, `Dom` for interactive websites and apps, `T-307`).
    #[serde(
        default,
        skip_serializing_if = "neoflash_core::HtmlRenderMode::is_default"
    )]
    pub render_mode: neoflash_core::HtmlRenderMode,
}
impl Default for PublishPrefs {
    fn default() -> Self {
        Self {
            profile: PublishProfilePreset::Default,
            options: neoflash_export::PublishOptions::default(),
            output_folder: None,
            html_wrapper: true,
            assets_folder: true,
            source_maps: false,
            jpeg_quality: 80,
            strict: true,
            warnings_as_errors: false,
            publish_on_save: false,
            default_target: PublishTarget::Html5,
            html5_enabled: true,
            desktop_app_enabled: false,
            desktop_app_folder: None,
            video_enabled: false,
            video_options: neoflash_export::VideoOptions::default(),
            video_file: None,
            image_sequence_enabled: false,
            image_sequence_folder: None,
            image_sequence_sprite_sheet: false,
            image_sequence_format: neoflash_export::ImageExportFormat::Png,
            atlas_format: neoflash_export::AtlasFormat::JsonHash,
            atlas_image_format: neoflash_export::AtlasImageFormat::Png,
            atlas_extrude: 0,
            svg_enabled: false,
            svg_file: None,
            template: neoflash_export::HtmlTemplate::Responsive,
            scale_mode: neoflash_export::HtmlScaleMode::FitKeepRatio,
            stage_align: neoflash_export::HtmlStageAlign::Center,
            allow_fullscreen: true,
            acceleration: neoflash_export::HtmlAcceleration::GpuWebGl,
            loop_timeline: true,
            autoplay: true,
            preloader: true,
            sprite_sheets: false,
            sequence_policy: neoflash_export::SequencePublishPolicy::KeepFrames,
            banner_max_gzip_kb: default_banner_max_gzip_kb(),
            criteo_max_zip_kib: default_criteo_max_zip_kib(),
            include_trace: false,
            minify: true,
            publish_clean: true,
            preview_chrome: neoflash_export::PlayerChromeConfig::preview_default(),
            fla_target_profile: neoflash_export::FlaTargetProfile::AnimateCC,
            fla_export_options: neoflash_export::FlaExportOptions::default(),
            render_mode: neoflash_core::HtmlRenderMode::Canvas,
        }
    }
}

impl PublishPrefs {
    /// Clamps persisted/edited numeric fields to valid ranges (`T-224`).
    pub fn sanitize(&mut self) {
        self.jpeg_quality = self.jpeg_quality.clamp(1, 100);
        self.atlas_extrude = self.atlas_extrude.min(8);
        self.banner_max_gzip_kb = self.banner_max_gzip_kb.clamp(10, 5000);
        self.criteo_max_zip_kib = self.criteo_max_zip_kib.clamp(1, 5000);
        self.video_options.quality = self.video_options.quality.clamp(1, 100);
        if self.video_options.width > 0 {
            self.video_options.width = self.video_options.width.clamp(16, 7680);
        }
        if self.video_options.height > 0 {
            self.video_options.height = self.video_options.height.clamp(16, 4320);
        }
        if !self.video_options.fps.is_finite() {
            self.video_options.fps = 0.0;
        } else if self.video_options.fps > 0.0 {
            self.video_options.fps = self.video_options.fps.clamp(1.0, 120.0);
        } else {
            self.video_options.fps = 0.0;
        }
    }

    /// Returns `true` when at least one publish target checkbox is enabled.
    pub fn has_any_enabled_target(&self) -> bool {
        self.html5_enabled
            || self.desktop_app_enabled
            || self.video_enabled
            || self.image_sequence_enabled
            || self.svg_enabled
    }

    /// Enables the checkbox corresponding to `target`.
    pub fn enable_target(&mut self, target: PublishTarget) {
        match target {
            PublishTarget::Html5 => self.html5_enabled = true,
            PublishTarget::DesktopApp => self.desktop_app_enabled = true,
            PublishTarget::VideoMp4 => self.video_enabled = true,
            PublishTarget::ImageSequence => self.image_sequence_enabled = true,
            PublishTarget::SvgAnimation => self.svg_enabled = true,
        }
    }

    pub(crate) fn criteo_zip_budget(&self) -> Option<usize> {
        (self.profile == PublishProfilePreset::Criteo)
            .then_some(self.criteo_max_zip_kib as usize * 1024)
    }

    /// Apply a built-in [`PublishProfilePreset`] to these preferences.
    pub fn apply_profile(&mut self, preset: PublishProfilePreset) {
        self.profile = preset;
        match preset {
            PublishProfilePreset::Default => {
                self.options.size = neoflash_export::SizeLevel::Full;
                self.options.fonts = neoflash_export::FontExportPolicy::Embed;
                self.options.audio = neoflash_export::AudioExportPolicy::Original;
                self.render_mode = neoflash_core::HtmlRenderMode::Canvas;
                self.html_wrapper = true;
                self.source_maps = false;
                self.jpeg_quality = 80;
                self.include_trace = false;
                self.minify = true;
                self.publish_clean = true;
            }
            PublishProfilePreset::WebCompact => {
                self.options.size = neoflash_export::SizeLevel::Compact;
                self.options.fonts = neoflash_export::FontExportPolicy::Subset;
                self.options.audio = neoflash_export::AudioExportPolicy::Mp3_128;
                self.render_mode = neoflash_core::HtmlRenderMode::Canvas;
                self.html_wrapper = true;
                self.source_maps = false;
                self.jpeg_quality = 75;
                self.include_trace = false;
                self.minify = true;
                self.publish_clean = true;
            }
            PublishProfilePreset::WebBalanced => {
                self.options.size = neoflash_export::SizeLevel::Balanced;
                self.options.fonts = neoflash_export::FontExportPolicy::Embed;
                self.options.audio = neoflash_export::AudioExportPolicy::Mp3_160;
                self.render_mode = neoflash_core::HtmlRenderMode::Canvas;
                self.html_wrapper = true;
                self.source_maps = false;
                self.jpeg_quality = 85;
                self.include_trace = false;
                self.minify = true;
                self.publish_clean = true;
            }
            PublishProfilePreset::WebAppDom => {
                self.options.size = neoflash_export::SizeLevel::Full;
                self.options.fonts = neoflash_export::FontExportPolicy::Embed;
                self.options.audio = neoflash_export::AudioExportPolicy::Original;
                self.render_mode = neoflash_core::HtmlRenderMode::Dom;
                self.html_wrapper = true;
                self.source_maps = false;
                self.jpeg_quality = 85;
                self.include_trace = false;
                self.minify = true;
                self.publish_clean = true;
                self.html5_enabled = true;
            }
            PublishProfilePreset::WebBannerNative | PublishProfilePreset::Criteo => {
                self.options.size = neoflash_export::SizeLevel::Compact;
                self.options.fonts = neoflash_export::FontExportPolicy::Subset;
                self.options.audio = neoflash_export::AudioExportPolicy::Mp3_128;
                if preset == PublishProfilePreset::WebBannerNative {
                    self.render_mode = neoflash_core::HtmlRenderMode::Canvas;
                }
                self.html_wrapper = true;
                self.source_maps = false;
                self.jpeg_quality = 75;
                self.include_trace = false;
                self.minify = true;
                self.publish_clean = true;
                self.html5_enabled = true;
            }
            PublishProfilePreset::HighQuality => {
                self.options.size = neoflash_export::SizeLevel::Full;
                self.options.fonts = neoflash_export::FontExportPolicy::Embed;
                self.options.audio = neoflash_export::AudioExportPolicy::Original;
                self.render_mode = neoflash_core::HtmlRenderMode::Canvas;
                self.html_wrapper = true;
                self.source_maps = false;
                self.jpeg_quality = 95;
                self.include_trace = false;
                self.minify = true;
                self.publish_clean = true;
            }
            PublishProfilePreset::DebugPreview => {
                self.options.size = neoflash_export::SizeLevel::Full;
                self.options.fonts = neoflash_export::FontExportPolicy::Embed;
                self.options.audio = neoflash_export::AudioExportPolicy::Original;
                self.render_mode = neoflash_core::HtmlRenderMode::Canvas;
                self.html_wrapper = true;
                self.source_maps = true;
                self.jpeg_quality = 85;
                self.include_trace = true;
                self.minify = false;
                self.publish_clean = false;
            }
        }
    }

    /// HTML5 configuration used for `Publish` (`File ▸ Publish` and `Publish settings ▸ Publish`).
    /// Produces a clean stage without the technical preview bar unless `publish_clean` is false.
    pub fn publish_html_config(&self) -> neoflash_export::HtmlPublishConfig {
        let mut chrome = if self.publish_clean {
            neoflash_export::PlayerChromeConfig::clean_publish()
        } else {
            self.preview_chrome
        };
        chrome.relay_output_to_host = false;
        neoflash_export::HtmlPublishConfig {
            include_html: self.html_wrapper,
            assets_folder: self.assets_folder,
            template: self.template,
            scale_mode: self.scale_mode,
            stage_align: self.stage_align,
            allow_fullscreen: self.allow_fullscreen,
            loop_timeline: self.loop_timeline,
            autoplay: self.autoplay,
            preloader: self.preloader,
            source_maps: self.source_maps,
            jpeg_quality: self.jpeg_quality.clamp(1, 100),
            sprite_sheets: self.sprite_sheets,
            sequence_policy: self.sequence_policy,
            strict_mode: self.strict,
            include_trace: self.include_trace,
            minify: self.minify,
            acceleration: self.acceleration,
            chrome,
            render_mode: self.render_mode,
            breakpoints: Vec::new(),
            omit_file_fallback: false,
        }
    }

    /// HTML5 configuration used for `Test Movie` and `Test Scene` (`Preview`).
    /// Uses `preview_chrome` configured in `Preferences ▸ Publish` and enables live loopback
    /// `Output` telemetry relay (`T-356`).
    pub fn preview_html_config(&self) -> neoflash_export::HtmlPublishConfig {
        let mut chrome = self.preview_chrome;
        chrome.relay_output_to_host = true;
        neoflash_export::HtmlPublishConfig {
            include_html: true,
            assets_folder: false,
            template: neoflash_export::HtmlTemplate::Responsive,
            scale_mode: self.scale_mode,
            stage_align: self.stage_align,
            allow_fullscreen: self.allow_fullscreen,
            loop_timeline: self.loop_timeline,
            autoplay: true,
            preloader: true,
            source_maps: true,
            jpeg_quality: self.jpeg_quality.clamp(1, 100),
            sprite_sheets: false,
            sequence_policy: neoflash_export::SequencePublishPolicy::KeepFrames,
            strict_mode: self.strict,
            include_trace: true,
            minify: true,
            acceleration: self.acceleration,
            chrome,
            render_mode: self.render_mode,
            breakpoints: Vec::new(),
            omit_file_fallback: true,
        }
    }
}

/// Preferences ▸ Shortcuts: the active set and per-command overrides (command id → shortcut labels).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutPrefs {
    /// Active set name; empty = "NeoFlash default".
    pub set: String,
    pub overrides: std::collections::BTreeMap<String, Vec<String>>,
    /// Saved named user sets (set name → per-command overrides).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub sets: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<String>>>,
}

pub(super) fn truncate_utf8(s: &mut String, max_bytes: usize) {
    if s.len() > max_bytes {
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}

pub(super) fn sanitize_shortcut_override_map(
    map: &mut std::collections::BTreeMap<String, Vec<String>>,
) {
    let mut cleaned = std::collections::BTreeMap::new();
    for (k, mut v) in std::mem::take(map) {
        if cleaned.len() >= 256
            || k.is_empty()
            || k.len() > 128
            || crate::commands::CommandId::from_id(&k).is_none()
        {
            continue;
        }
        v.truncate(4);
        v.retain_mut(|chord| {
            truncate_utf8(chord, 64);
            if let Some(parsed) = crate::commands::Shortcut::parse(chord) {
                *chord = parsed.format();
                true
            } else {
                false
            }
        });
        cleaned.insert(k, v);
    }
    *map = cleaned;
}

impl ShortcutPrefs {
    pub const DEFAULT_SET_NAME: &'static str = "NeoFlash default";

    pub fn is_default_set(&self) -> bool {
        self.set.is_empty() || self.set == Self::DEFAULT_SET_NAME
    }

    pub fn active_set_label(&self) -> &str {
        if self.is_default_set() {
            Self::DEFAULT_SET_NAME
        } else {
            &self.set
        }
    }

    pub(super) fn sync_active_set(&mut self) {
        if !self.is_default_set() {
            self.sets.insert(self.set.clone(), self.overrides.clone());
        }
    }

    /// Update the active set's binding for `cmd` (empty `chords` = unbound; matching defaults
    /// removes the override entry).
    pub fn set_shortcuts(
        &mut self,
        cmd: crate::commands::CommandId,
        chords: Vec<crate::commands::Shortcut>,
    ) {
        if chords.as_slice() == cmd.default_shortcuts() {
            self.overrides.remove(cmd.as_str());
        } else {
            let formatted = chords
                .iter()
                .map(crate::commands::Shortcut::format)
                .collect();
            self.overrides.insert(cmd.as_str().to_owned(), formatted);
        }
        self.sync_active_set();
    }

    /// Restore `cmd` to its built-in default shortcut in the active set.
    pub fn reset_command(&mut self, cmd: crate::commands::CommandId) {
        self.overrides.remove(cmd.as_str());
        self.sync_active_set();
    }

    /// Switch the active shortcut set (`""` or `"NeoFlash default"` loads built-in defaults).
    pub fn select_set(&mut self, name: &str) {
        self.sync_active_set();
        if name.is_empty() || name == Self::DEFAULT_SET_NAME {
            self.set.clear();
            self.overrides.clear();
        } else {
            self.set = name.to_owned();
            self.overrides = self.sets.get(name).cloned().unwrap_or_default();
            self.sets.entry(self.set.clone()).or_default();
        }
    }

    /// Duplicate the active set under a unique `"<name> copy"` name and make it active.
    pub fn duplicate_active_set(&mut self) -> String {
        self.sync_active_set();
        let base = format!("{} copy", self.active_set_label());
        let mut candidate = base.clone();
        let mut n = 2usize;
        while self.sets.contains_key(&candidate) || candidate == Self::DEFAULT_SET_NAME {
            candidate = format!("{base} {n}");
            n += 1;
        }
        truncate_utf8(&mut candidate, 128);
        self.sets.insert(candidate.clone(), self.overrides.clone());
        self.set = candidate.clone();
        candidate
    }

    /// Delete the active user set and return to `"NeoFlash default"`.
    pub fn delete_active_set(&mut self) -> bool {
        if self.is_default_set() {
            return false;
        }
        self.sets.remove(&self.set);
        self.set.clear();
        self.overrides.clear();
        true
    }

    pub fn sanitize(&mut self) {
        truncate_utf8(&mut self.set, 128);
        let trimmed = self.set.trim();
        if trimmed.is_empty() || trimmed == Self::DEFAULT_SET_NAME {
            self.set.clear();
        } else if trimmed.len() != self.set.len() {
            self.set = trimmed.to_owned();
        }
        sanitize_shortcut_override_map(&mut self.overrides);
        let mut clean_sets = std::collections::BTreeMap::new();
        for (mut name, mut map) in std::mem::take(&mut self.sets) {
            truncate_utf8(&mut name, 128);
            let name = name.trim().to_owned();
            if name.is_empty() || name == Self::DEFAULT_SET_NAME || clean_sets.len() >= 32 {
                continue;
            }
            sanitize_shortcut_override_map(&mut map);
            clean_sets.insert(name, map);
        }
        if !self.set.is_empty() {
            if clean_sets.contains_key(&self.set) || clean_sets.len() < 32 {
                clean_sets.insert(self.set.clone(), self.overrides.clone());
            } else {
                self.set.clear();
            }
        }
        self.sets = clean_sets;
    }
}

/// Release channel selected in `Preferences ▸ Updates` (`T-298`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

impl UpdateChannel {
    pub const OPTIONS: [(Self, &'static str); 2] = [(Self::Stable, "Stable"), (Self::Beta, "Beta")];
}

/// Persisted updater preferences and daily check timestamp (`T-298`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatePrefs {
    pub channel: UpdateChannel,
    /// Opt-in daily background check against GitHub Releases (never downloads or installs silently).
    pub check_automatically: bool,
    /// UNIX epoch seconds of the most recent completed check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check_unix: Option<u64>,
}
