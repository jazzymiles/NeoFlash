//! Publish size levels: which runtime files an HTML5 package carries and how the WebAssembly
//! runtime is delivered. Every level plays the same project. Balanced keeps both a streamable
//! WASM file and a direct-file fallback; Compact may reduce code by omitting unused families;
//! Shared trades project self-containment for a separately published runtime.

use neoflash_core::Project;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write as _;

/// How far an HTML5 export trades portability for size, from everything inline to content only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "level", content = "runtime_url")]
pub enum SizeLevel {
    /// Every runtime file ships and WebAssembly is embedded in `bundle.js`; `index.html` also
    /// works when opened straight from disk.
    #[default]
    Full,
    /// WebAssembly ships as its own binary file for HTTP(S) streaming and the bindings carry a
    /// fallback so the folder can also be opened directly from disk.
    Balanced,
    /// Balanced, without the ActionScript families the project's scripts never name and without
    /// the WebAssembly code that only those families reach.
    Compact,
    /// Only the project ships; the runtime loads from a shared folder published once with
    /// [`crate::export_shared_runtime`] at this URL.
    Shared(String),
}

impl SizeLevel {
    /// The four levels in slider order, with `runtime_url` for the shared one.
    pub fn all(runtime_url: &str) -> [SizeLevel; 4] {
        [
            SizeLevel::Full,
            SizeLevel::Balanced,
            SizeLevel::Compact,
            SizeLevel::Shared(runtime_url.to_owned()),
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            SizeLevel::Full => "full",
            SizeLevel::Balanced => "balanced",
            SizeLevel::Compact => "compact",
            SizeLevel::Shared(_) => "shared",
        }
    }

    /// Compact and Shared also leave out what a project without scripts never runs.
    pub fn trims_content(&self) -> bool {
        matches!(self, SizeLevel::Compact | SizeLevel::Shared(_))
    }

    pub fn wasm_inline(&self) -> bool {
        matches!(self, SizeLevel::Full)
    }

    /// Parses a CLI level name; `shared` takes its URL separately.
    pub fn parse(name: &str, runtime_url: Option<&str>) -> Result<SizeLevel, String> {
        match name {
            "full" => Ok(SizeLevel::Full),
            "balanced" => Ok(SizeLevel::Balanced),
            "compact" => Ok(SizeLevel::Compact),
            "shared" => runtime_url
                .ok_or_else(|| "--size-level shared requires --runtime-url".to_owned())
                .and_then(validate_runtime_url)
                .map(SizeLevel::Shared),
            other => Err(format!(
                "Unknown size level '{other}'. Use full, balanced, compact or shared."
            )),
        }
    }
}

/// HTML wrapper template layout in `Publish settings`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HtmlTemplate {
    #[default]
    Responsive,
    FixedStage,
    Centered,
    Fullscreen,
}

impl HtmlTemplate {
    pub const ALL: [Self; 4] = [
        Self::Responsive,
        Self::FixedStage,
        Self::Centered,
        Self::Fullscreen,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Responsive => "Responsive",
            Self::FixedStage => "Fixed stage",
            Self::Centered => "Centered card",
            Self::Fullscreen => "Fullscreen",
        }
    }

    pub const fn publish_select_label(self) -> &'static str {
        match self {
            Self::Responsive => "Template: Responsive",
            Self::FixedStage => "Template: Fixed stage",
            Self::Centered => "Template: Centered card",
            Self::Fullscreen => "Template: Fullscreen",
        }
    }
}

/// Stage scaling mode in the HTML5 wrapper (`Publish settings`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HtmlScaleMode {
    #[default]
    FitKeepRatio,
    NoScale,
    ExactFit,
    FillCrop,
}

impl HtmlScaleMode {
    pub const ALL: [Self; 4] = [
        Self::FitKeepRatio,
        Self::NoScale,
        Self::ExactFit,
        Self::FillCrop,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FitKeepRatio => "Fit, keep ratio",
            Self::NoScale => "100% (No scale)",
            Self::ExactFit => "Exact fit (stretch)",
            Self::FillCrop => "Fill, crop",
        }
    }

    pub const fn publish_select_label(self) -> &'static str {
        match self {
            Self::FitKeepRatio => "Scale: Fit, keep ratio",
            Self::NoScale => "Scale: 100% (No scale)",
            Self::ExactFit => "Scale: Exact fit (stretch)",
            Self::FillCrop => "Scale: Fill, crop",
        }
    }

    pub const fn as_flash_scale_mode(self) -> &'static str {
        match self {
            Self::FitKeepRatio => "showAll",
            Self::NoScale => "noScale",
            Self::ExactFit => "exactFit",
            Self::FillCrop => "noBorder",
        }
    }
}

/// Stage anchor alignment in the HTML5 wrapper (`Publish settings` / `Stage.align`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HtmlStageAlign {
    #[default]
    Center,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl HtmlStageAlign {
    pub const ALL: [Self; 9] = [
        Self::Center,
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];

    pub const fn as_flash_code(self) -> &'static str {
        match self {
            Self::Center => "",
            Self::Top => "T",
            Self::Bottom => "B",
            Self::Left => "L",
            Self::Right => "R",
            Self::TopLeft => "TL",
            Self::TopRight => "TR",
            Self::BottomLeft => "BL",
            Self::BottomRight => "BR",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Center => "Center",
            Self::Top => "Top (T)",
            Self::Bottom => "Bottom (B)",
            Self::Left => "Left (L)",
            Self::Right => "Right (R)",
            Self::TopLeft => "Top-left (TL)",
            Self::TopRight => "Top-right (TR)",
            Self::BottomLeft => "Bottom-left (BL)",
            Self::BottomRight => "Bottom-right (BR)",
        }
    }

    pub const fn publish_select_label(self) -> &'static str {
        match self {
            Self::Center => "Align: Center",
            Self::Top => "Align: Top (T)",
            Self::Bottom => "Align: Bottom (B)",
            Self::Left => "Align: Left (L)",
            Self::Right => "Align: Right (R)",
            Self::TopLeft => "Align: Top-left (TL)",
            Self::TopRight => "Align: Top-right (TR)",
            Self::BottomLeft => "Align: Bottom-left (BL)",
            Self::BottomRight => "Align: Bottom-right (BR)",
        }
    }
}

/// Hardware acceleration hint for HTML5 rendering (`Preferences ▸ Publish`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HtmlAcceleration {
    #[default]
    GpuWebGl,
    Canvas2d,
    Software,
}

impl HtmlAcceleration {
    pub const ALL: [Self; 3] = [Self::GpuWebGl, Self::Canvas2d, Self::Software];

    pub const fn label(self) -> &'static str {
        match self {
            Self::GpuWebGl => "GPU (WebGL)",
            Self::Canvas2d => "Canvas 2D",
            Self::Software => "Software",
        }
    }
}

const fn default_true() -> bool {
    true
}

const fn default_jpeg_quality() -> u8 {
    80
}

/// Controls whether the HTML5 wrapper displays the technical preview footer (`Test Movie`)
/// or a clean stage (`Publish`), and which individual HUD elements are shown in preview mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerChromeConfig {
    /// When `true`, hides the technical preview footer completely (default for `Publish`).
    pub clean: bool,
    /// Show FPS & frame counter (`1 / 120 · 24 fps`) in the preview bar.
    #[serde(default = "default_true")]
    pub show_fps: bool,
    /// Show stage dimensions (`W×H`) next to the document title in the preview bar.
    #[serde(default = "default_true")]
    pub show_dimensions: bool,
    /// Show transport & zoom controls (`Pause`, `Restart`, `Zoom`, `Pan`) in the preview bar.
    #[serde(default = "default_true")]
    pub show_controls: bool,
    /// Show package weight, load summary, and `Size report` toggle in the preview bar.
    #[serde(default = "default_true")]
    pub show_load_summary: bool,
    /// Show `trace()` output and script diagnostics in the preview bar.
    #[serde(default = "default_true")]
    pub show_trace: bool,
}

impl Default for PlayerChromeConfig {
    fn default() -> Self {
        Self::preview_default()
    }
}

impl PlayerChromeConfig {
    /// Default technical preview HUD (`Test Movie` / `Test Scene`).
    pub const fn preview_default() -> Self {
        Self {
            clean: false,
            show_fps: true,
            show_dimensions: true,
            show_controls: true,
            show_load_summary: true,
            show_trace: true,
        }
    }

    /// Clean stage without technical footer (`Publish`).
    pub const fn clean_publish() -> Self {
        Self {
            clean: true,
            show_fps: false,
            show_dimensions: false,
            show_controls: false,
            show_load_summary: false,
            show_trace: false,
        }
    }

    /// Whether the footer should be completely hidden.
    pub const fn effective_clean(&self) -> bool {
        self.clean
            || (!self.show_fps
                && !self.show_dimensions
                && !self.show_controls
                && !self.show_load_summary
                && !self.show_trace)
    }
}

/// Policy for publishing frame-by-frame bitmap sequence layers (`T-218` / `T-255`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SequencePublishPolicy {
    /// Preserve frame-by-frame bitmap keyframes as authored.
    #[default]
    KeepFrames,
    /// Pack sequence bitmaps into the HTML5 sprite sheet automatically.
    PackAtlas,
    /// Transcode contiguous frame-by-frame bitmap spans into an embedded H.264 MP4 timeline video
    /// when `ffmpeg` is available (falls back to sprite-sheet packing with a diagnostic warning).
    TranscodeVideo,
}

impl SequencePublishPolicy {
    pub const ALL: [Self; 3] = [Self::KeepFrames, Self::PackAtlas, Self::TranscodeVideo];

    pub const fn label(self) -> &'static str {
        match self {
            Self::KeepFrames => "Keep individual frames",
            Self::PackAtlas => "Pack into sprite sheet",
            Self::TranscodeVideo => "Transcode to MP4 video",
        }
    }
}

/// Extended HTML5 wrapper, playback, asset, script, and chrome configuration for
/// [`crate::export_project_with_config`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HtmlPublishConfig {
    #[serde(default = "default_true")]
    pub include_html: bool,
    #[serde(default)]
    pub assets_folder: bool,
    #[serde(default)]
    pub template: HtmlTemplate,
    #[serde(default)]
    pub scale_mode: HtmlScaleMode,
    #[serde(default)]
    pub stage_align: HtmlStageAlign,
    #[serde(default = "default_true")]
    pub allow_fullscreen: bool,
    #[serde(default = "default_true")]
    pub loop_timeline: bool,
    #[serde(default = "default_true")]
    pub autoplay: bool,
    #[serde(default = "default_true")]
    pub preloader: bool,
    #[serde(default)]
    pub source_maps: bool,
    #[serde(default = "default_jpeg_quality")]
    pub jpeg_quality: u8,
    #[serde(default)]
    pub sprite_sheets: bool,
    #[serde(default)]
    pub sequence_policy: SequencePublishPolicy,
    #[serde(default = "default_true")]
    pub strict_mode: bool,
    #[serde(default = "default_true")]
    pub include_trace: bool,
    #[serde(default = "default_true")]
    pub minify: bool,
    #[serde(default)]
    pub acceleration: HtmlAcceleration,
    #[serde(default)]
    pub chrome: PlayerChromeConfig,
}

impl Default for HtmlPublishConfig {
    fn default() -> Self {
        Self {
            include_html: true,
            assets_folder: false,
            template: HtmlTemplate::Responsive,
            scale_mode: HtmlScaleMode::FitKeepRatio,
            stage_align: HtmlStageAlign::Center,
            allow_fullscreen: true,
            loop_timeline: true,
            autoplay: true,
            preloader: true,
            source_maps: false,
            jpeg_quality: 80,
            sprite_sheets: false,
            sequence_policy: SequencePublishPolicy::KeepFrames,
            strict_mode: true,
            include_trace: true,
            minify: true,
            acceleration: HtmlAcceleration::GpuWebGl,
            chrome: PlayerChromeConfig::preview_default(),
        }
    }
}

impl HtmlPublishConfig {
    /// Clean configuration used when publishing an HTML5 package (`Publish`).
    pub fn clean_publish_default() -> Self {
        Self {
            chrome: PlayerChromeConfig::clean_publish(),
            ..Self::default()
        }
    }
}

/// Options for [`crate::export_project_with_options`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PublishOptions {
    pub fonts: crate::FontExportPolicy,
    pub size: SizeLevel,
    #[serde(default)]
    pub audio: crate::AudioExportPolicy,
}

impl PublishOptions {
    /// Validate before compilation or filesystem writes. The returned options carry the same
    /// normalized URL used by publication and estimates; other settings are preserved.
    pub fn validated(&self) -> Result<Self, String> {
        Ok(Self {
            fonts: self.fonts,
            size: match &self.size {
                SizeLevel::Shared(url) => SizeLevel::Shared(validate_runtime_url(url)?),
                level => level.clone(),
            },
            audio: self.audio,
        })
    }
}

/// Accept an HTTP(S) directory URL or a relative/root-relative directory path.
/// Query strings, fragments and credentials do not describe a runtime directory: appending
/// filenames to them would silently load a different URL. No network access is performed here.
pub fn validate_runtime_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    let invalid = || {
        "Shared runtime URL must be an HTTP(S) directory URL or a relative directory path, without credentials, whitespace, backslashes, query strings or fragments.".to_owned()
    };
    if value.is_empty()
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
        || value.contains(['\\', '?', '#'])
    {
        return Err(invalid());
    }
    let parsed = match url::Url::parse(value) {
        Ok(parsed) => {
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !value
                    .to_ascii_lowercase()
                    .starts_with(&format!("{}://", parsed.scheme()))
                || value
                    .get(parsed.scheme().len() + 3..)
                    .is_none_or(|authority| authority.starts_with('/'))
            {
                return Err(invalid());
            }
            parsed
        }
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            // A scheme-relative URL depends on the page's scheme and is not a directory path.
            if value.starts_with("//") {
                return Err(invalid());
            }
            let base = url::Url::parse("https://neoflash.invalid/").expect("static URL");
            base.join(value).map_err(|_| invalid())?
        }
        Err(_) => return Err(invalid()),
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(invalid());
    }
    // Keep relative paths relative; preserve '/' so the runtime may live at the host root.
    if value == "/" {
        Ok(value.into())
    } else if url::Url::parse(value).is_ok() {
        Ok(parsed.as_str().trim_end_matches('/').to_owned())
    } else {
        Ok(value.trim_end_matches('/').to_owned())
    }
}

/// An ActionScript 3 family large enough to leave out when the project never names it.
pub(crate) struct Family {
    pub file: &'static str,
    pub title: &'static str,
    /// Names the family reaches the project through besides its registered classes: stage or
    /// display properties it installs and packages only it provides.
    pub extra_tokens: &'static [&'static str],
}

pub(crate) const FAMILIES: &[Family] = &[
    Family {
        file: "air.js",
        title: "AIR desktop APIs",
        extra_tokens: &[
            "air.",
            "flash.filesystem",
            "flash.data",
            "flash.html",
            "nativeWindow",
            "deviceOrientation",
            "orientation",
            "cacheAsBitmapMatrix",
            "softKeyboard",
            "loadFilePromise",
        ],
    },
    Family {
        file: "as3-net-sys.js",
        title: "AS3 networking, workers and system services",
        extra_tokens: &["flash.concurrent", "flash.crypto", "flash.globalization"],
    },
    Family {
        file: "as3-fte-3d.js",
        title: "Text engine, Stage3D and shaders",
        extra_tokens: &[
            "flash.text.engine",
            "flash.display3D",
            "stage3Ds",
            "stageVideos",
            "softKeyboardRect",
            "loadBytes",
        ],
    },
    Family {
        file: "as3-media-events.js",
        title: "AS3 media, input devices and accessibility objects",
        extra_tokens: &[
            "stageVideos",
            "accessibilityProperties",
            "accessibilityImplementation",
        ],
    },
];

/// Host modules that only scripts reach. A project without any script or class linkage never
/// loads them. `net.js` and `byte-array.js` stay: authored sound and video play through them.
pub(crate) const SCRIPT_ONLY: &[&str] = &[
    "as2-strings.js",
    "as2-platform.js",
    "as2-object.js",
    "as2-transitions.js",
    "as2-depth-manager.js",
    "as2-print.js",
    "as2-boxing.js",
    "as2-capture.js",
    "as2-connections.js",
    "as2-stage.js",
    "as2-media.js",
    "as2-services.js",
    "as2-locale.js",
    "as2-files.js",
    "as2-network.js",
    "as2-data.js",
    "as2-binding.js",
    "as2-text.js",
    "as3-strings.js",
    "easing.js",
    "math.js",
    "date.js",
    "number-format.js",
    "broadcaster.js",
    "timers.js",
    "external.js",
    "xml.js",
    "loader.js",
    "amf.js",
    "storage.js",
];

/// All script text of a project: class sources, frame scripts, object handlers and text-like
/// resources a script could include. Family detection scans this, never compiled output.
pub(crate) fn script_corpus(project: &Project) -> String {
    let mut corpus = String::new();
    for class in project.document_class.iter().chain(
        project
            .symbols
            .iter()
            .filter_map(|symbol| symbol.class_name.as_ref()),
    ) {
        corpus.push_str(class);
        corpus.push('\n');
    }
    for unit in &project.sources {
        corpus.push_str(&unit.source);
        corpus.push('\n');
    }
    for symbol in &project.symbols {
        for frame in &symbol.timeline.scripts {
            corpus.push_str(&frame.script.source);
            corpus.push('\n');
        }
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    for handler in &element.scripts {
                        corpus.push_str(&handler.script.source);
                        corpus.push('\n');
                    }
                }
            }
        }
    }
    for resource in &project.resources {
        let textual = resource.media_type.starts_with("text/")
            || resource.media_type.contains("javascript")
            || resource.media_type.contains("actionscript");
        if textual {
            use base64::{Engine, engine::general_purpose::STANDARD};
            if let Ok(bytes) = STANDARD.decode(resource.data_base64.as_bytes()) {
                corpus.push_str(&String::from_utf8_lossy(&bytes));
                corpus.push('\n');
            }
        }
    }
    corpus
}

/// Class names a family registers (`'flash.pkg.Name': Name`), each as qualified and short name,
/// plus its extra tokens.
fn family_tokens(family: &Family, source: &str) -> BTreeSet<String> {
    let mut tokens: BTreeSet<String> = family
        .extra_tokens
        .iter()
        .map(|token| (*token).to_owned())
        .collect();
    for quote in ['\'', '"'] {
        let mut rest = source;
        while let Some(start) = rest.find(quote) {
            let after = &rest[start + 1..];
            let Some(end) = after.find(quote) else { break };
            let name = &after[..end];
            let keyed = after[end + 1..].trim_start().starts_with(':');
            if keyed && (name.starts_with("flash.") || name.starts_with("air.")) {
                tokens.insert(name.to_owned());
                if let Some((_, short)) = name.rsplit_once('.') {
                    tokens.insert(short.to_owned());
                }
            }
            rest = &after[end + 1..];
        }
    }
    tokens
}

fn is_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// Whole-word occurrence; a token ending in `.` is a package prefix.
fn mentions(corpus: &str, token: &str) -> bool {
    corpus.match_indices(token).any(|(at, _)| {
        let before = corpus[..at].chars().next_back();
        let after = corpus[at + token.len()..].chars().next();
        !before.is_some_and(is_identifier_char)
            && (token.ends_with('.') || !after.is_some_and(is_identifier_char))
    })
}

/// Which optional host modules a Compact package leaves out, keyed by file, with a reason.
pub(crate) fn omitted_modules(
    project: &Project,
    sources: &BTreeMap<&'static str, &'static str>,
) -> BTreeMap<&'static str, &'static str> {
    let corpus = script_corpus(project);
    let mut omitted = BTreeMap::new();
    if corpus.trim().is_empty() {
        for file in SCRIPT_ONLY {
            omitted.insert(*file, "the project has no scripts");
        }
    }
    for family in FAMILIES {
        let source = sources.get(family.file).copied().unwrap_or_default();
        let used = family_tokens(family, source)
            .iter()
            .any(|token| mentions(&corpus, token));
        if !used {
            omitted.insert(family.file, family.title);
        }
    }
    omitted
}

/// Identifier-like words of the JavaScript that ships (host modules, project program and
/// compiled scripts). A runtime method no shipped code names can never be called.
pub(crate) fn referenced_words<'a>(texts: impl IntoIterator<Item = &'a str>) -> HashSet<String> {
    let mut words = HashSet::new();
    for text in texts {
        for word in text.split(|c: char| !is_identifier_char(c)) {
            if !word.is_empty() && !word.starts_with(|c: char| c.is_ascii_digit()) {
                words.insert(word.to_owned());
            }
        }
    }
    words
}

/// The wasm-bindgen glue member (class method, accessor or exported function) that calls each
/// WebAssembly export. Exports used anywhere else in the glue map to `None` and always stay.
fn glue_owners(glue: &str) -> BTreeMap<String, Option<String>> {
    let public: HashSet<&str> = glue
        .lines()
        .filter_map(|line| line.trim().strip_prefix("exports."))
        .filter_map(|rest| rest.split_once(" = ").map(|(name, _)| name))
        .collect();
    let mut owners: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut class: Option<String> = None;
    let mut member: Option<String> = None;
    for line in glue.lines() {
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if indent == 4 {
            member = None;
            class = None;
            if let Some(rest) = text.strip_prefix("class ") {
                class = rest.split([' ', '{']).next().map(str::to_owned);
            } else if let Some(rest) = text.strip_prefix("function ") {
                let name = rest.split('(').next().unwrap_or_default();
                member = public.contains(name).then(|| name.to_owned());
            }
        } else if indent == 8 && class.is_some() && text.ends_with('{') {
            let head = text
                .trim_start_matches("static ")
                .trim_start_matches("get ")
                .trim_start_matches("set ")
                .trim_start_matches("async ");
            let name = head.split('(').next().unwrap_or_default();
            member = if name == "constructor" {
                class.clone()
            } else if !name.is_empty() && name.chars().all(is_identifier_char) {
                Some(name.to_owned())
            } else {
                None
            };
        }
        for (at, _) in line.match_indices("wasm.") {
            let name: String = line[at + 5..]
                .chars()
                .take_while(|c| is_identifier_char(*c))
                .collect();
            if name.is_empty() {
                continue;
            }
            let owner = if indent >= 8 || class.is_none() {
                member.clone()
            } else {
                None
            };
            let entry = owners.entry(name).or_insert(owner.clone());
            if *entry != owner {
                *entry = None;
            }
        }
    }
    owners
}

/// Whether shipped code can reach a glue member: by its name or, for object-graph methods that
/// the display host calls as `'object_' + name`, by the name without that prefix.
fn reachable(member: &str, words: &HashSet<String>) -> bool {
    words.contains(member)
        || member
            .strip_prefix("object_")
            .is_some_and(|rest| words.contains(rest))
}

/// Removes WebAssembly exports only unreachable glue members call, then drops the code nothing
/// else reaches. Instantiation is unchanged: the glue resolves exports only when called.
pub(crate) fn strip_wasm(
    wasm: &[u8],
    glue: &str,
    words: &HashSet<String>,
) -> Result<Vec<u8>, String> {
    let owners = glue_owners(glue);
    let mut module = walrus::Module::from_buffer(wasm)
        .map_err(|error| format!("Could not read the WebAssembly runtime: {error}"))?;
    let unreachable: Vec<_> = module
        .exports
        .iter()
        .filter(|export| {
            owners
                .get(&export.name)
                .and_then(Option::as_deref)
                .is_some_and(|member| !reachable(member, words))
        })
        .map(|export| export.id())
        .collect();
    for id in &unreachable {
        module.exports.delete(*id);
    }
    walrus::passes::gc::run(&mut module);
    Ok(module.emit_wasm())
}

/// Size of `bytes` after gzip, the compression every static host applies.
pub fn gzip_len(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(bytes)
        .expect("writing to a Vec cannot fail");
    encoder.finish().map_or(bytes.len(), |out| out.len())
}

/// FNV-1a identity of the runtime files, so a shared runtime and a project agree on a build.
pub(crate) fn runtime_id(parts: &[&[u8]]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.iter() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

/// One slice of a package estimate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SizePart {
    /// `wasm`, `runtime_js`, `content`, `scripts`, `fonts`, `resources` or `page`.
    pub kind: &'static str,
    pub bytes: usize,
    pub gzip_bytes: usize,
}

/// What publishing at one level writes, measured from the files it would produce.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SizeEstimate {
    pub level: SizeLevel,
    pub files: usize,
    /// On disk / in the upload.
    pub bytes: usize,
    /// Transferred with gzip, which static hosts apply to every file here.
    pub gzip_bytes: usize,
    pub parts: Vec<SizePart>,
    /// Shared level only: the runtime folder visitors download once and then reuse.
    pub shared_runtime_bytes: Option<usize>,
    pub shared_runtime_gzip_bytes: Option<usize>,
    /// What this level gives up, for the publish dialog.
    pub tradeoffs: Vec<String>,
    /// Runtime host modules left out (Compact).
    pub omitted_modules: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_come_from_registered_builtins_and_match_whole_words() {
        let family = Family {
            file: "x.js",
            title: "X",
            extra_tokens: &["flash.display3D"],
        };
        let tokens = family_tokens(
            &family,
            "const builtins = { 'flash.filesystem.File': File, \"flash.data.SQLConnection\": S };\nlog('flash.not.Keyed');",
        );
        assert!(tokens.contains("flash.filesystem.File") && tokens.contains("File"));
        assert!(tokens.contains("SQLConnection") && tokens.contains("flash.display3D"));
        assert!(!tokens.contains("Keyed"));
        assert!(mentions("import flash.filesystem.*;", "flash.filesystem"));
        assert!(mentions("var f:File = new File();", "File"));
        assert!(!mentions("var f = new FileReference();", "File"));
        assert!(mentions("air.Introspector", "air."));
        assert!(!mentions("hair.color", "air."));
    }

    #[test]
    fn glue_owners_map_exports_to_reachable_members() {
        let glue = "    class WebPlayer {\n        constructor(a) {\n            wasm.webplayer_new(a);\n        }\n        object_set_mask(r) {\n            wasm.webplayer_object_set_mask(r);\n        }\n        unused_call() {\n            wasm.webplayer_unused_call();\n        }\n        free() {\n            wasm.__wbg_webplayer_free(1);\n        }\n    }\n    function helper() {\n        wasm.__wbindgen_export();\n    }\n    function parse_xml_json(s) {\n        wasm.parse_xml_json(s);\n    }\n    exports.parse_xml_json = parse_xml_json;\n";
        let owners = glue_owners(glue);
        assert_eq!(owners["webplayer_new"].as_deref(), Some("WebPlayer"));
        assert_eq!(owners["__wbindgen_export"], None);
        assert_eq!(owners["parse_xml_json"].as_deref(), Some("parse_xml_json"));
        let words = referenced_words(["new NeoFlashWasm.WebPlayer(); invoke(t,'set_mask');"]);
        assert!(reachable("WebPlayer", &words));
        assert!(reachable("object_set_mask", &words));
        assert!(!reachable("unused_call", &words));
        assert!(!reachable("parse_xml_json", &words));
    }

    #[test]
    fn runtime_urls_are_http_or_relative() {
        assert_eq!(
            validate_runtime_url("https://cdn.example.com/neoflash/1/").unwrap(),
            "https://cdn.example.com/neoflash/1"
        );
        for (input, expected) in [
            (" ../runtime/ ", "../runtime"),
            ("/runtime/", "/runtime"),
            ("/", "/"),
            ("./runtime-v1", "./runtime-v1"),
            (
                "HTTPS://CDN.EXAMPLE.COM:443/runtime/",
                "https://cdn.example.com/runtime",
            ),
            (
                "http://localhost:8080/runtime",
                "http://localhost:8080/runtime",
            ),
            ("https://[::1]/runtime", "https://[::1]/runtime"),
        ] {
            assert_eq!(validate_runtime_url(input).unwrap(), expected);
        }
        for invalid in [
            "javascript:alert(1)",
            "file:///tmp/x",
            "  ",
            "https://",
            "http:///runtime",
            "https:runtime",
            "//example.com/runtime",
            "https://example.com:bad/runtime",
            "https://user:password@example.com/runtime",
            "../runtime?version=1",
            "runtime#fragment",
            "runtime with spaces",
            "../run\\time",
            "run\ntime",
        ] {
            assert!(
                validate_runtime_url(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
        let options = PublishOptions {
            fonts: crate::FontExportPolicy::Device,
            size: SizeLevel::Shared(" ../runtime/ ".into()),
            audio: crate::AudioExportPolicy::Original,
        }
        .validated()
        .unwrap();
        assert_eq!(options.fonts, crate::FontExportPolicy::Device);
        assert_eq!(options.size, SizeLevel::Shared("../runtime".into()));
        assert!(SizeLevel::parse("shared", Some("javascript:alert(1)")).is_err());
    }
}
