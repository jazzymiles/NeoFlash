//! In-app Help (`Help ▸ NeoFlash Help…`) and the shell of the Scripting Reference
//! (`Help ▸ Scripting Reference…`), also reachable from the `Search functions… (F1)` command
//! palette and `About NeoFlash`.
//!
//! Both readers share a resizable shell with a search bar, a tree column, the
//! article or class in the middle and a side column. The reference columns and the guide
//! chapters live in `help_as_reference.rs`.

mod window;

use super::help_as_reference::{ApiBrowserState, RefActions, RefKeys, RefRow};
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use neoflash_ui::{
    Icon, TextStyle,
    components::{
        self as c, Badge, BadgeType, Button, DialogHeader, Search, Surface,
        brushes::{BrushStroke, brush_stroke_sample},
        canvas::{
            AnchorState, GuideState, MotionHandleState, MotionKeyState, Orientation, SelectionKind,
            ShapeHintState, TransformMode, paint_anchor, paint_bezier_handle,
            paint_gradient_transform, paint_guide, paint_motion_handle, paint_motion_key,
            paint_motion_path, paint_ruler_corner, paint_selection_box, paint_shape_hint,
            paint_shape_selection, paint_snap_ring, paint_stage_grid, paint_stage_ruler,
            paint_transform_box,
        },
        divider, kbd,
        timeline::{
            Geometry, GridKind, MarkerKind, SpanType, paint_grid, paint_marker, paint_span,
        },
    },
    icons::{self, i16},
    metrics::{self, FrameView, ICON_UI, STAGE_RULER},
    tokens::{radius, space},
    typography::{font, rich},
};
use std::path::PathBuf;

/// Number of general authoring chapters (`1..=10`). Chapters `11..=19` (indices `10..19`)
/// form the dedicated Scripting Reference (`Help ▸ Scripting Reference…`).
pub(crate) const AUTHORING_CHAPTER_COUNT: usize = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum HelpManualMode {
    #[default]
    AuthoringHelp,
    ScriptingReference,
}

impl HelpManualMode {
    pub(crate) const fn chapter_range(self) -> std::ops::Range<usize> {
        match self {
            Self::AuthoringHelp => 0..AUTHORING_CHAPTER_COUNT,
            Self::ScriptingReference => AUTHORING_CHAPTER_COUNT..HELP_CHAPTERS.len(),
        }
    }

    pub(crate) const fn for_page(page: usize) -> Self {
        if page < AUTHORING_CHAPTER_COUNT {
            Self::AuthoringHelp
        } else {
            Self::ScriptingReference
        }
    }

    pub(crate) const fn dialog_title(self) -> &'static str {
        match self {
            Self::AuthoringHelp => "NeoFlash Help",
            Self::ScriptingReference => "Scripting Reference",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LanguageScope {
    #[default]
    All,
    As1As2,
    As3,
    JsTs,
}

impl LanguageScope {
    pub(crate) const fn from_script_language(lang: neoflash_core::ScriptLanguage) -> Self {
        match lang {
            neoflash_core::ScriptLanguage::ActionScript1
            | neoflash_core::ScriptLanguage::ActionScript2 => Self::As1As2,
            neoflash_core::ScriptLanguage::ActionScript3 => Self::As3,
            neoflash_core::ScriptLanguage::JavaScript
            | neoflash_core::ScriptLanguage::TypeScript => Self::JsTs,
        }
    }

    pub(crate) const fn html_slug(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::As1As2 => "AS1_AS2",
            Self::As3 => "AS3",
            Self::JsTs => "JS_TS",
        }
    }

    pub(crate) const fn default_chapter(self, lang: neoflash_core::ScriptLanguage) -> usize {
        match self {
            Self::All | Self::As1As2 => AUTHORING_CHAPTER_COUNT, // 10 -> Ch. 11 AS1 & AS2
            Self::As3 => 12,                                     // 12 -> Ch. 13 AS3 Display List
            Self::JsTs => match lang {
                neoflash_core::ScriptLanguage::TypeScript => 17, // 17 -> Ch. 18 TypeScript
                _ => 16,                                         // 16 -> Ch. 17 JavaScript
            },
        }
    }

    pub(crate) const fn matches_chapter(self, idx: usize) -> bool {
        match self {
            Self::All => true,
            Self::As1As2 => matches!(idx, 9 | 10 | 11 | 15),
            Self::As3 => matches!(idx, 9 | 12 | 13 | 14 | 15),
            Self::JsTs => matches!(idx, 9 | 15 | 16 | 17 | 18),
        }
    }

    pub(crate) fn matches_badge(self, badge: &str) -> bool {
        match self {
            Self::All => true,
            Self::As1As2 => matches!(badge, "[AS1/AS2]" | "[Authoring]"),
            Self::As3 => matches!(badge, "[AS3]" | "[Authoring]"),
            Self::JsTs => matches!(badge, "[JS]" | "[TS]" | "[TS] [JS]" | "[Authoring]"),
        }
    }

    /// Name of the language in the reference window: its language switch, search field and footer.
    pub(crate) const fn reference_label(self) -> &'static str {
        match self {
            Self::As1As2 => "ActionScript 2",
            Self::As3 => "ActionScript 3",
            Self::JsTs | Self::All => "JavaScript / TypeScript",
        }
    }

    /// Guide chapters of the language, in list order. ActionScript 1 belongs to ActionScript 2;
    /// the migration guide closes every list.
    pub(crate) const fn guide_chapters(self) -> &'static [usize] {
        match self {
            Self::As1As2 => &[10, 11, 15],
            Self::As3 => &[12, 13, 14, 15],
            Self::JsTs => &[16, 17, 18, 15],
            Self::All => &[10, 11, 12, 13, 14, 15, 16, 17, 18],
        }
    }

    pub(crate) fn has_guide(self, page: usize) -> bool {
        self.guide_chapters().contains(&page)
    }

    /// The language whose guide list contains `page`.
    pub(crate) fn for_guide(page: usize) -> Self {
        [Self::As1As2, Self::As3, Self::JsTs]
            .into_iter()
            .find(|scope| scope.has_guide(page))
            .unwrap_or(Self::As1As2)
    }
}

/// Tab of the reference tree column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ReferenceTab {
    #[default]
    Api,
    Guides,
}

#[derive(Clone, Debug, Default)]
struct HelpManualState {
    open: bool,
    mode: HelpManualMode,
    windows: [window::ReaderWindow; 2],
    lang_scope: LanguageScope,
    page: usize,
    query: String,
    diagram_tab: usize,
    history: Vec<usize>,
    /// The chapter was asked for explicitly: the reference language follows it.
    pin_page: bool,
    tab: ReferenceTab,
    api: ApiBrowserState,
    /// Scroll the article to this offset on the next frame.
    scroll_article_to: Option<f32>,
    /// Scroll offset of the article in the last frame (current "On this page" entry).
    article_offset: f32,
    reading_position: Option<ArticlePosition>,
    focus_search: bool,
    /// The search field had the keyboard in the last frame.
    search_focused: bool,
}

fn state_id() -> egui::Id {
    egui::Id::new("neoflash-help-manual-state")
}

pub(crate) fn open_help_manual(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
        s.open = true;
        s.set_route(HelpManualMode::AuthoringHelp);
        if s.page >= AUTHORING_CHAPTER_COUNT {
            s.page = 0;
            s.diagram_tab = 0;
        }
    });
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn open_scripting_reference(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
        s.open = true;
        s.set_route(HelpManualMode::ScriptingReference);
        // `All` reads as "the project language" when the window is drawn.
        s.lang_scope = LanguageScope::All;
        s.pin_page = false;
        if s.page < AUTHORING_CHAPTER_COUNT || s.page >= HELP_CHAPTERS.len() {
            s.page = AUTHORING_CHAPTER_COUNT;
            s.diagram_tab = 0;
        }
    });
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn open_scripting_reference_page(ctx: &egui::Context, page: usize) {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
        s.open = true;
        let clamped = page.clamp(AUTHORING_CHAPTER_COUNT, HELP_CHAPTERS.len() - 1);
        s.set_route(HelpManualMode::ScriptingReference);
        s.tab = ReferenceTab::Guides;
        if !s.lang_scope.has_guide(clamped) {
            s.lang_scope = LanguageScope::All;
        }
        s.pin_page = true;
        s.page = clamped;
        s.diagram_tab = 0;
    });
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn help_manual_mode(ctx: &egui::Context) -> HelpManualMode {
    ctx.data(|d| {
        d.get_temp::<HelpManualState>(state_id())
            .map(|s| s.mode)
            .unwrap_or_default()
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn help_manual_language_scope(ctx: &egui::Context) -> LanguageScope {
    ctx.data(|d| {
        d.get_temp::<HelpManualState>(state_id())
            .map(|s| s.lang_scope)
            .unwrap_or_default()
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn set_help_manual_mode(ctx: &egui::Context, mode: HelpManualMode) {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
        s.set_route(mode);
        if mode == HelpManualMode::ScriptingReference {
            s.lang_scope = LanguageScope::All;
            s.pin_page = false;
        }
        let range = mode.chapter_range();
        if !range.contains(&s.page) {
            s.page = range.start;
            s.diagram_tab = 0;
        }
    });
}

/// Screenshot aid for `NEOFLASH_SCREENSHOT_DIALOG=scripting_reference|scripting_guides`.
/// `NEOFLASH_SCREENSHOT_REFERENCE=as3:MovieClip.gotoAndPlay` picks the language (`as2`, `as3`,
/// `js`) and the declaration to select, `as3:?draw` types a search instead; without it the
/// window follows the project language.
pub(crate) fn open_reference_for_screenshot(ctx: &egui::Context, guides: bool) {
    let request = std::env::var("NEOFLASH_SCREENSHOT_REFERENCE").unwrap_or_default();
    let (language, target) = request.split_once(':').unwrap_or((request.as_str(), ""));
    let scope = match language {
        "as2" => LanguageScope::As1As2,
        "as3" => LanguageScope::As3,
        "js" | "ts" => LanguageScope::JsTs,
        _ => LanguageScope::All,
    };
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
        s.open = true;
        s.mode = HelpManualMode::ScriptingReference;
        s.lang_scope = scope;
        s.pin_page = false;
        s.tab = if guides {
            ReferenceTab::Guides
        } else {
            ReferenceTab::Api
        };
        if let Some(query) = target.strip_prefix('?') {
            s.query = query.to_owned();
        } else if scope != LanguageScope::All
            && let Some(hit) = super::help_as_reference::api_search(scope, target)
                .into_iter()
                .next()
        {
            s.api
                .select(scope, &hit.class_key, hit.member_key.as_deref());
        }
    });
}

#[cfg(test)]
pub(crate) fn close_help_manual(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
        s.open = false;
    });
    ctx.plugin::<egui::text_selection::LabelSelectionState>()
        .lock()
        .clear_selection();
}

pub(crate) fn is_help_manual_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| {
        d.get_temp::<HelpManualState>(state_id())
            .is_some_and(|s| s.open)
    })
}

#[cfg(test)]
pub(crate) fn set_help_manual_page(ctx: &egui::Context, page: usize) {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
        let clamped = page.min(HELP_CHAPTERS.len() - 1);
        s.page = clamped;
        s.set_route(HelpManualMode::for_page(clamped));
        if clamped >= AUTHORING_CHAPTER_COUNT {
            s.tab = ReferenceTab::Guides;
            if !s.lang_scope.has_guide(clamped) {
                s.lang_scope = LanguageScope::All;
            }
        }
        s.pin_page = true;
        s.diagram_tab = 0;
    });
}

pub(crate) struct HelpChapterMeta {
    pub sidebar_label: &'static str,
    pub sidebar_badge: &'static str,
    pub dialect_badges: &'static [&'static str],
    pub title: &'static str,
    pub subtitle: &'static str,
    pub icon: Icon,
    pub keywords: &'static str,
    pub tags: &'static [&'static str],
    pub related: &'static [usize],
}

pub(crate) const HELP_CHAPTERS: [HelpChapterMeta; 19] = [
    HelpChapterMeta {
        sidebar_label: "1. Workspace & Stage",
        sidebar_badge: "",
        dialect_badges: &["[Authoring]"],
        title: "1. Workspace & Stage",
        subtitle: "Stage, Pasteboard, Rulers, Guides, Grid & Snapping priority",
        icon: i16::DESKTOP,
        keywords: "workspace stage pasteboard rulers guides grid snapping zoom hand pan focus mode units premiere theme density mid neutral gray dark graphite ui studio",
        tags: &["Authoring", "Stage"],
        related: &[1, 2, 6],
    },
    HelpChapterMeta {
        sidebar_label: "2. Drawing & Objects",
        sidebar_badge: "",
        dialect_badges: &["[Authoring]"],
        title: "2. Drawing Modes & Objects",
        subtitle: "Merge Drawing vs Object Drawing (J), Primitives, Groups & Combine",
        icon: i16::LAYERS,
        keywords: "merge drawing object drawing primitive group break apart union intersect punch crop boolean",
        tags: &["Authoring", "Vector", "Tools"],
        related: &[0, 2, 3, 4],
    },
    HelpChapterMeta {
        sidebar_label: "3. Select & Transform",
        sidebar_badge: "",
        dialect_badges: &["[Authoring]"],
        title: "3. Selection & Transform",
        subtitle: "Selection (V), Subselection (A), Free Transform (Q) & Lasso (L) modes",
        icon: i16::SELECT,
        keywords: "selection subselection free transform rotate skew scale distort taper envelope lasso magic wand",
        tags: &["Authoring", "Vector", "Tools"],
        related: &[1, 3, 5],
    },
    HelpChapterMeta {
        sidebar_label: "4. Vector & Pen States",
        sidebar_badge: "",
        dialect_badges: &["[Authoring]"],
        title: "4. Vector & Pen States",
        subtitle: "Pen (P) 9 pointer states, Line (N), Shapes (R/O) & Pencil (Y) modes",
        icon: i16::PEN,
        keywords: "pen anchor point bezier tangent line rectangle oval polystar pencil straighten smooth ink",
        tags: &["Authoring", "Vector", "Tools", "Graphics"],
        related: &[1, 2, 4, 14],
    },
    HelpChapterMeta {
        sidebar_label: "5. Brush, Eraser & Deco",
        sidebar_badge: "",
        dialect_badges: &["[Authoring]"],
        title: "5. Brush, Eraser & Deco",
        subtitle: "Brush (B) 5 paint modes, Paint Bucket (K), Gradient (F), Eraser (E) & Deco",
        icon: i16::BRUSH,
        keywords: "brush paint normal fills behind selection inside lock fill pressure tilt bucket gap ink bottle eyedropper eraser faucet spray deco",
        tags: &["Authoring", "Vector", "Tools", "Graphics"],
        related: &[1, 3, 8, 11],
    },
    HelpChapterMeta {
        sidebar_label: "6. Symbols & Library",
        sidebar_badge: "",
        dialect_badges: &["[Authoring]"],
        title: "6. Symbols, 9-Slice & Library",
        subtitle: "MovieClip, Button (4 states), Graphic syncing, 9-Slice & Colour Effects",
        icon: i16::SYMBOL,
        keywords: "symbol movieclip button up over down hit graphic loop play once single frame 9-slice scale9 color effect tint brightness alpha library edit in place",
        tags: &["Authoring", "Symbols", "Timeline", "DisplayList"],
        related: &[6, 7, 10, 12],
    },
    HelpChapterMeta {
        sidebar_label: "7. Timeline & Masks",
        sidebar_badge: "",
        dialect_badges: &["[Authoring]"],
        title: "7. Timeline, Layers & Masks",
        subtitle: "Keyframes, Span selection, Layer roles, Masking, Onion Skin & Scenes",
        icon: i16::FILM,
        keywords: "timeline frame keyframe blank span layer normal guide guided mask masked folder solo outline onion skin edit multiple frames scene razor split clip link unlink montage",
        tags: &["Authoring", "Timeline", "Animation"],
        related: &[5, 7, 10, 13],
    },
    HelpChapterMeta {
        sidebar_label: "8. Tweens, Motion & IK",
        sidebar_badge: "",
        dialect_badges: &["[Authoring]"],
        title: "8. Tweens, Motion & IK",
        subtitle: "Motion Tween ◆ keys, Motion Editor, Classic Tween, Shape Hints & IK Bones",
        icon: i16::TWEEN_MOTION,
        keywords: "motion tween property keyframe roving motion path motion editor ease curve classic tween motion guide orient to path shape tween shape hints bone bind armature inverse kinematics",
        tags: &["Authoring", "Timeline", "Animation"],
        related: &[5, 6, 8],
    },
    HelpChapterMeta {
        sidebar_label: "9. Filters, Text & Audio",
        sidebar_badge: "",
        dialect_badges: &["[Authoring]"],
        title: "9. Filters, Text & Audio",
        subtitle: "7 Live Filters, 14 Blend Modes, Classic/TLF Text, Font Embedding & Sound",
        icon: i16::FILTER,
        keywords: "filter drop shadow blur glow bevel gradient adjust color blend mode text static dynamic input embed font tlf sound event start stop stream envelope video",
        tags: &["Authoring", "Text", "Audio", "Graphics"],
        related: &[5, 6, 11, 14],
    },
    HelpChapterMeta {
        sidebar_label: "10. Scripting & Publish",
        sidebar_badge: "[AS/JS/TS]",
        dialect_badges: &["[AS1/AS2]", "[AS3]", "[JS]", "[TS]"],
        title: "10. Scripting, Import & Publish",
        subtitle: "AS2/AS3/JS/TS, <ESC> 50 Chords, Snippets, Debugger, FLA/SWF Import & HTML5 Publish",
        icon: i16::CODE,
        keywords: "actionscript as2 as3 javascript typescript actions <esc> esc chord <esc>fn <esc>tr <esc>st <esc>rt function code snippets debugger profiler import fla xfl swf psd ai svg figma avi webp bmp tiff trace bitmap publish html5 wasm banner sequence aep after effects expressions loadpublish sprite sheet atlas lottie shortcuts",
        tags: &[
            "Authoring",
            "Export",
            "AS1",
            "AS2",
            "AS3",
            "JavaScript",
            "TypeScript",
            "WebAPI",
            "JS/TS",
        ],
        related: &[10, 12, 15, 16, 17, 18],
    },
    HelpChapterMeta {
        sidebar_label: "1. AS1 & AS2 Timeline",
        sidebar_badge: "[AS1/AS2]",
        dialect_badges: &["[AS1/AS2]"],
        title: "11. AS1 & AS2 Fundamentals & Timeline",
        subtitle: "function Statement, arguments, Closures, Target Paths (_root/this), on/onClipEvent, Prototypes & 1-Frame Games",
        icon: i16::CODE,
        keywords: "actionscript 1.0 2.0 as1 as2 avm1 function return arguments callee caller call apply closure hoisting var local scope _root _parent _level0 this telltarget on onclipevent onenterframe onpress onrelease gotoandplay gotoandstop stop play attachmovie duplicatemovieclip createemptymovieclip swapdepths getnexthighestdepth prototype __proto__ constructor registerclass initclip setinterval clearinterval trace <esc>fn 1-frame game",
        tags: &["AS1", "AS2", "Timeline", "Symbols", "Events"],
        related: &[6, 11, 15, 16],
    },
    HelpChapterMeta {
        sidebar_label: "2. AS2 Core Classes",
        sidebar_badge: "[AS1/AS2]",
        dialect_badges: &["[AS1/AS2]"],
        title: "12. AS2 Core Classes & Built-Ins",
        subtitle: "Function (call/apply/prototype), arguments, MovieClip Drawing API, TextField, Sound, Key/Mouse & BitmapData",
        icon: i16::BOOKMARK,
        keywords: "as2 function call apply prototype arguments callee caller movieclip drawing api linestyle beginfill begingradientfill moveto lineto curveto textfield textformat sound attachsound loadsound key mouse stage bitmapdata matrix point rectangle colortransform loadvars xml sharedobject setinterval clearinterval",
        tags: &["AS1", "AS2", "Graphics", "Text", "Audio", "Events"],
        related: &[8, 10, 14, 15],
    },
    HelpChapterMeta {
        sidebar_label: "3. AS3 Display List",
        sidebar_badge: "[AS3]",
        dialect_badges: &["[AS3]"],
        title: "13. AS3 Architecture & Display List",
        subtitle: "AVM2 Packages, Functions (...rest, call/apply, Bound Closures), Strong Types, Vector.<T> & DisplayObjectContainer",
        icon: i16::CLASS,
        keywords: "actionscript 3.0 as3 avm2 function return rest arguments call apply closure package class interface extends implements int uint number vector dictionary displayobject interactiveobject displayobjectcontainer sprite movieclip shape bitmap simplebutton addchild removechild getbounds hittestpoint localtoglobal globaltolocal",
        tags: &["AS3", "DisplayList", "Symbols"],
        related: &[5, 13, 14, 15],
    },
    HelpChapterMeta {
        sidebar_label: "4. AS3 Events & Flow",
        sidebar_badge: "[AS3]",
        dialect_badges: &["[AS3]"],
        title: "14. AS3 Events, Input & Lifecycle",
        subtitle: "EventDispatcher, 3-Phase Capture/Target/Bubble Flow, Frame Order, Mouse/Keyboard & Timer",
        icon: i16::PLAY,
        keywords: "as3 function listener callback eventdispatcher addeventlistener removeeventlistener dispatchevent capture target bubbling stoppropagation stopimmediatepropagation enter_frame frame_constructed exit_frame added_to_stage mouseevent keyboardevent timerevent timer",
        tags: &["AS3", "Events", "Timeline", "DisplayList"],
        related: &[6, 12, 14, 15],
    },
    HelpChapterMeta {
        sidebar_label: "5. AS3 Graphics & Audio",
        sidebar_badge: "[AS3]",
        dialect_badges: &["[AS3]"],
        title: "15. AS3 Graphics, Filters, Sound & Data",
        subtitle: "flash.display.Graphics, Matrix/Filters, SoundChannel/SoundMixer, ByteArray & E4X XML",
        icon: i16::FILTER,
        keywords: "as3 graphics beginfill begingradientfill drawrect drawroundrect drawcircle drawtriangles cubiccurveto filters glowfilter blurfilter dropshadowfilter colormatrixfilter soundchannel soundtransform soundmixer bytearray compress lzma xml e4x json",
        tags: &["AS3", "Graphics", "Audio", "Text"],
        related: &[7, 8, 12, 13],
    },
    HelpChapterMeta {
        sidebar_label: "6. Language Migration",
        sidebar_badge: "[AS2/AS3/JS/TS]",
        dialect_badges: &["[AS1/AS2]", "[AS3]", "[JS]", "[TS]"],
        title: "16. AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide",
        subtitle: "Side-by-Side Equivalence Rosetta Stone, Function/Prototype Rules, Scale/Alpha & Porting Checklist",
        icon: i16::LINK,
        keywords: "migration porting comparison rosetta stone function arguments rest prototype class call apply as1 as2 as3 javascript typescript _alpha alpha _xscale scalex onrelease addeventlistener attachmovie addchild geturl navigatetourl gettimer",
        tags: &[
            "AS1",
            "AS2",
            "AS3",
            "JavaScript",
            "TypeScript",
            "WebAPI",
            "JS/TS",
            "DisplayList",
            "Events",
            "Export",
        ],
        related: &[9, 10, 12, 16, 17, 18],
    },
    HelpChapterMeta {
        sidebar_label: "7. JavaScript Runtime",
        sidebar_badge: "[JS]",
        dialect_badges: &["[JS]"],
        title: "17. JavaScript (ES2023+ & NeoFlash Runtime)",
        subtitle: "Functions, Arrow Callbacks, Closures, Frame/Object/Module Contexts (this, event, trace), Promises & JSX",
        icon: i16::CODE,
        keywords: "javascript js es2023 es2022 function arrow return rest closure call apply bind runtime frame object script this event trace gotoandplay gotoandstop play stop promise async await import export module jsx __nfjsx try catch map set",
        tags: &[
            "JavaScript",
            "JS/TS",
            "Timeline",
            "DisplayList",
            "Events",
            "Export",
        ],
        related: &[9, 15, 17, 18],
    },
    HelpChapterMeta {
        sidebar_label: "8. TypeScript & Checker",
        sidebar_badge: "[TS]",
        dialect_badges: &["[TS]"],
        title: "18. TypeScript & Built-In Type Checker",
        subtitle: "Typed & Generic Functions, Interfaces, Union Types, Bounded Static Checker, Quick Fixes & Source Maps",
        icon: i16::FILE_CODE,
        keywords: "typescript ts tsx function return generic parameter type checker ts_check interface type alias union intersection record partial pick omit readonly enum sourcemap vlq debugger quick fix",
        tags: &[
            "TypeScript",
            "JavaScript",
            "JS/TS",
            "Timeline",
            "Events",
            "Export",
        ],
        related: &[9, 15, 16, 18],
    },
    HelpChapterMeta {
        sidebar_label: "9. Browser & Web APIs",
        sidebar_badge: "[JS] [TS]",
        dialect_badges: &["[JS]", "[TS]"],
        title: "19. Browser & Web APIs (Canvas / DOM / Fetch / Audio / Storage)",
        subtitle: "CanvasRenderingContext2D, DOM, Fetch, Storage, Timers, Web Audio & CORS/CSP Limits",
        icon: i16::DESKTOP,
        keywords: "webapi browser dom window document canvas htmlcanvaselement canvasrenderingcontext2d fetch response localstorage sessionstorage settimeout requestanimationframe audiocontext pointerevent keyboardevent cors csp user gesture",
        tags: &[
            "WebAPI",
            "JavaScript",
            "TypeScript",
            "JS/TS",
            "Graphics",
            "Audio",
            "Events",
            "Export",
        ],
        related: &[8, 9, 15, 16, 17],
    },
];

/// Portable text edition used by the browser handoff. The in-app manual and browser edition
/// share this 19-chapter inventory plus full NF2 styling, SVG diagrams, tags, and cross-links.
pub(crate) const HELP_BROWSER_TOPICS: [&[&str]; 19] = [
    &[
        "Use the Stage for authored content and the Pasteboard for temporary off-stage artwork; rulers, guides and the document grid share one snapping priority.",
        "The Hand and Zoom tools, Focus Mode, Fit Stage and percentage controls change only the view and never mutate authored geometry.",
        "Ruler guides can be dragged from either ruler, moved on the Pasteboard, locked, hidden or removed by dropping them back on a ruler.",
        "Object, pixel, grid, guide and alignment snapping can be combined; visible snap rings identify the winning target.",
        "Switch between Animator, Classic, Designer, Developer, Small Screen and Video edit montage workspaces, four NF2 themes (Dark, Light, Mid Neutral Gray, Dark Graphite) and three densities (Comfortable, Compact, Classic Flash).",
        "NeoFlash 0.1.0-beta.1 targets macOS 15+ (Apple Silicon) and provides Windows x64 and Ubuntu 24.04-compatible Linux x64 Technical Previews; the portable archives include their licence bundle, checksums and media-source notice.",
    ],
    &[
        "Merge Drawing lets fills and strokes cut, join and reshape each other; Object Drawing keeps every stroke or fill isolated until Break Apart.",
        "Primitive rectangles and ovals retain editable corner, radius and inner-radius controls, while ordinary shapes expose raw anchors and Bézier handles.",
        "Group, Break Apart, Union, Intersect, Punch and Crop preserve the distinction between editable objects and destructive Merge geometry.",
        "The J shortcut toggles Object Drawing for supported drawing tools without changing existing artwork.",
    ],
    &[
        "Selection (V) selects objects and Merge regions; Subselection (A) edits anchors, handles and multiple selected points.",
        "Free Transform (Q) supports scale, rotate/skew, distort and envelope modes plus an editable transformation point.",
        "Lasso (L) supports freehand, polygon and Magic Wand selection with smoothing and threshold settings.",
        "Arrow keys nudge selected objects or anchors by one pixel, and Shift increases the step to ten pixels.",
    ],
    &[
        "Pen (P) changes its pointer state for starting, continuing, closing, adding, deleting and converting anchors.",
        #[cfg(target_os = "macos")]
        "Option converts corner and smooth points, while dragging creates or adjusts symmetric and independent tangent handles.",
        #[cfg(not(target_os = "macos"))]
        "Alt converts corner and smooth points, while dragging creates or adjusts symmetric and independent tangent handles.",
        "Line (N), Rectangle (R), Oval (O), PolyStar and Pencil (Y) share snapping and Object Drawing behavior but retain their own live modifiers.",
        "Pencil modes Straighten, Smooth and Ink choose how aggressively the captured stroke is simplified.",
    ],
    &[
        "Brush (B) supports normal, fill, behind, selection and inside paint modes together with pressure, tilt, size, shape and Lock Fill. Graphics-tablet pressure, pen tilt and switching between pen tip and eraser are experimental in 0.1.0-beta.1 and have not been checked on every tablet and driver.",
        "Paint Bucket (K) closes configurable gaps, applies solid or gradient fills and can drag a gradient direction directly on the Stage.",
        "Gradient Transform (F) edits linear and radial boundaries, width, radius, rotation, centre and focal point handles.",
        "Eraser (E) includes normal, fills, lines, selected fills, inside and whole-object modes plus round or square tips and Faucet behavior.",
    ],
    &[
        "MovieClip symbols run independent timelines; Graphic symbols can loop, play once or display a single frame; Buttons use Up, Over, Down and Hit states.",
        "Edit in Place preserves the parent-stage context while symbol isolation edits only the selected symbol definition.",
        "9-slice scaling protects corners and edges, while alpha, tint, brightness and advanced colour effects remain instance properties.",
        "The Library supports folders, shared libraries, duplicate/swap/update workflows and stable symbol identity.",
    ],
    &[
        "Frames, keyframes, blank keyframes and selected spans are edited independently from layer properties and visibility controls.",
        "Normal, Guide, Guided, Mask, Masked and Folder layers retain their role when reordered or nested.",
        "Onion Skin and Edit Multiple Frames expose neighbouring poses without making authored evaluation stateful.",
        "Scenes are authored separately but play and publish as one ordered main timeline.",
        "Razor (C) and Modify > Timeline > Split Clip split Stream audio and non-looping video with exact In/Out trim windows, advance Graphic first frames, place keyframes on symbol/shape spans, and preserve reciprocal Link / Unlink Video & Audio tracks.",
    ],
    &[
        "Motion Tween stores keyed X, Y, Scale, Rotation, Skew, Alpha and Colour tracks; the Motion Editor adjusts curves and easing.",
        "Classic Tween interpolates matching object poses and can follow a motion guide with Orient to Path.",
        "Shape Tween morphs compatible shapes and uses shape hints to control correspondence.",
        "IK bones, bindings, pins and spring controls build editable armatures without replacing the original artwork.",
    ],
    &[
        "Seven live filters and fourteen blend modes remain editable and are evaluated by the editor and published runtime.",
        "Static, Dynamic and Input text support embedded fonts, paragraph and character controls, multi-column flow and authored links.",
        "Frame sounds use Event, Start, Stop or Stream synchronization, looping and editable stereo envelopes.",
        "Video and audio resources remain document resources; scrubbing is silent unless audible scrubbing is explicitly enabled.",
        "Properties > Video configures Stream/Event/Stop sync, interactive In/Out trim handles, Mute, cue points and one-step Extract audio onto a linked sound track for MP4, WebM and RIFF AVI clips.",
    ],
    &[
        "Frame, object and project scripts support five languages—ActionScript 3.0, ActionScript 2.0, ActionScript 1.0, TypeScript and JavaScript—governed by one unified project Script language policy (Document settings, with optional compatible-script compilation for AS1 in AS2 or JS in TS, while foreign scripts stay preserved in the document).",
        "Focused Actions and Project source editors support all 50 Classic Flash <ESC> two-key quick-typing chords (<ESC>br through <ESC>wt, including <ESC>fn for function blocks, <ESC>rt for return, <ESC>tr for trace and <ESC>st for stop) with automatic indentation and single-step Undo.",
        "Preferences ▸ AI configures optional OpenAI, Anthropic, Gemini or custom HTTPS/loopback connections (storing keys only in macOS Keychain or NEOFLASH_AI_TOKEN_* env vars) and Edit ▸ Send to AI… reviews a redacted context pack before applying code or timing proposals as a single Undo step.",
        "Import accepts legacy FLA, XFL, SWF, compiled SWC component libraries (linked into Library symbols, scripts and assets), Kiwi/Zstd Figma (.fig / .figma.json), PSD/PSB, AI, SVG, PDF, WAV/MP3 plus FFmpeg-transcoded OGG/FLAC/AAC/M4A audio, MP4/WebM/AVI video, WebP/GIF/BMP/TIFF and numbered image sequences with located diagnostics.",
        "Publish and export targets include HTML5 plus WebAssembly (Canvas or semantic Web App · DOM), zero-WASM Web Banner · Native, Desktop apps, 5-format Texture Atlases, AI/SVG/PNG/JPEG/WebP/GIF/BMP frame sequences, Flash Player 8/9/10 SWF, Lottie JSON & dotLottie (.lottie with Original/PNG/JPEG/WebP images), audio/video, Adobe After Effects (.aep / .aepx / .jsx), and the machine-readable --headless CLI.",
        #[cfg(target_os = "macos")]
        "Core shortcuts include C for Razor, F5/F6/F7 for Timeline frames, F8 for Convert to Symbol, Command+B for Break Apart, F9 for Actions, Option+Shift+Command+A for After Effects export and Shift+F12 for Publish.",
        #[cfg(not(target_os = "macos"))]
        "Core shortcuts include C for Razor, F5/F6/F7 for Timeline frames, F8 for Convert to Symbol, Ctrl+B for Break Apart, F9 for Actions, Ctrl+Alt+Shift+A for After Effects export and Shift+F12 for Publish.",
        #[cfg(target_os = "macos")]
        "Packaged macOS builds bundle ffmpeg and ffprobe in Contents/Resources/media/bin/, with optional NEOFLASH_FFMPEG override; exact Rust/media licences are inside and the corresponding source is verified by the supplied SHA256SUMS.txt. Keep .nflash backups and report issues via the Stage Bug? button or Help ▸ Send Beta Feedback… (delivered over HTTPS FormSubmit to developer@redinger.cc); the form sends only its visible note and images, so mention any separately available minimal repro and include Output diagnostics plus the About NeoFlash commit.",
        #[cfg(not(target_os = "macos"))]
        "Packaged builds bundle ffmpeg and ffprobe in media/bin/, with optional NEOFLASH_FFMPEG override; exact Rust/media licences are inside and the corresponding source is verified by the supplied SHA256SUMS.txt. Keep .nflash backups and report issues via the Stage Bug? button or Help ▸ Send Beta Feedback… (delivered over HTTPS FormSubmit to developer@redinger.cc); the form sends only its visible note and images, so mention any separately available minimal repro and include Output diagnostics plus the About NeoFlash commit.",
        "Beta 1 known issues and limits: VoiceOver cannot read the editor interface yet; there is no build for Intel (x86_64) Macs; updates are checked and downloaded via Preferences ▸ Updates (SHA-256 verified) and installed manually without silent self-replacement; the Windows preview is unsigned, so SmartScreen may warn before the first launch, and it has no connected debugger; the Linux preview is a portable archive tested on Ubuntu 24.04 x64 with X11 and Wayland, not an AppImage or deb package; Windows and Linux have had less testing than macOS; import of legacy Flash files is partial; graphics-tablet support is experimental.",
    ],
    &[
        "ActionScript 1.0 and 2.0 execute on the AVM1 timeline and target-path hierarchy (_level0, _root, _parent, this) with 1-based frame indexing.",
        "Named function declarations (hoisted with optional parameter/return types and return values), anonymous function literals, the local arguments object (length, callee, caller), Function.call / Function.apply explicit receiver binding, lexical var closures and 1-frame onEnterFrame / setInterval game loops are all supported.",
        "Button instances accept on(press, release, rollOver, keyPress) blocks where this refers to the parent timeline; MovieClips accept onClipEvent(load, enterFrame, data) blocks.",
        "Author-time instances occupy negative depths (-16383..-1), while attachMovie, duplicateMovieClip and createEmptyMovieClip use positive dynamic depths (0..1048575) via getNextHighestDepth().",
        "AS1 prototype inheritance (Function.prototype and Object.registerClass) and AS2 class definitions (class X extends MovieClip) both bind directly to Library symbols.",
    ],
    &[
        "MovieClip exposes procedural vector drawing (lineStyle, beginFill, beginGradientFill, moveTo, lineTo, curveTo, endFill, clear) and hitTest(x, y, shapeFlag).",
        "TextField and TextFormat control dynamic and input typography, wordWrap, multiline, embedFonts, scroll/maxscroll, and HTML/StyleSheet formatting.",
        "Sound manages event and streaming audio per timeline target with attachSound, loadSound, start(offset, loops), setVolume(0..100), and setPan(-100..100).",
        "Flash 8 flash.display.BitmapData, flash.geom (Matrix, Point, Rectangle, ColorTransform), LoadVars, XML, and SharedObject.getLocal complete the AS2 standard library.",
        "Stage.scaleMode, 9-anchor Stage.align and Stage.displayState coordinate responsive HTML5 canvas layout, while loadMovie and MovieClipLoader dynamically mount published NeoFlash bundles (loadPublish) alongside SWF, MP3, WAV, MP4, AVI, SVG, WebP, GIF, BMP and TIFF assets.",
    ],
    &[
        "ActionScript 3.0 (AVM2) introduces package namespaces, sealed classes, strict runtime types (int, uint, Number, Vector.<T>, Dictionary), and E4X XML.",
        "The Display List hierarchy (EventDispatcher -> DisplayObject -> InteractiveObject -> DisplayObjectContainer -> Sprite -> MovieClip) replaces AVM1 depth numbers with contiguous 0..numChildren-1 indices.",
        "Display containers manage children explicitly through addChild, addChildAt, removeChild, setChildIndex, swapChildren, and getObjectsUnderPoint.",
        "localToGlobal and globalToLocal convert Points accurately across nested affine transformations; getBounds and getRect compute bounding boxes in any target coordinate space.",
    ],
    &[
        "Every AS3 interactive node dispatches events through the W3C three-phase flow: Capture Phase (Stage down to parent), Target Phase, and Bubbling Phase (back up to Stage).",
        "event.target identifies the originating leaf dispatcher, whereas event.currentTarget identifies the DisplayObject whose listener is currently executing.",
        "Each frame tick runs a deterministic 6-step sequence: ENTER_FRAME -> child construction -> FRAME_CONSTRUCTED -> timeline frame scripts -> EXIT_FRAME -> RENDER.",
        "MouseEvent, KeyboardEvent (attached to stage), FocusEvent, and flash.utils.Timer provide structured input and timing callbacks.",
        "stage.scaleMode, 9-anchor stage.align and StageDisplayState.FULL_SCREEN drive high-DPI HTML5 Stage layout with exact inverse pointer hit-testing, while flash.display.Loader mounts published NeoFlash bundles (loadPublish) and SWF/SVG/AVI/WebP/BMP/TIFF media.",
    ],
    &[
        "flash.display.Graphics renders strokes, solid/gradient/bitmap fills, quadratic curveTo, cubicCurveTo, drawRoundRect, drawPath, and UV-mapped drawTriangles.",
        "Live display filters (BlurFilter, GlowFilter, DropShadowFilter, BevelFilter, ColorMatrixFilter) apply when assigned as a new Array to displayObject.filters.",
        "flash.media.Sound.play() returns a SoundChannel with live SoundTransform(volume, pan) control, while SoundMixer.computeSpectrum() fills a 512-float ByteArray.",
        "flash.utils.ByteArray supports big/little-endian binary streams plus ZLIB, DEFLATE, and LZMA compression alongside native JSON and E4X XML queries.",
    ],
    &[
        "AS1/AS2 _x, _y, _rotation, _visible, _name, _currentframe, and _totalframes map to AS3/JS x, y, rotation, visible, name, currentFrame, and totalFrames without underscores.",
        "AS2 percentage properties _alpha (0..100) and _xscale/_yscale (0..100) map to normalized unit floats alpha (0.0..1.0) and scaleX/scaleY (0.0..1.0) in AS3 and JS/TS.",
        "AS2 callback properties (mc.onRelease, mc.onEnterFrame) become mc.addEventListener(MouseEvent.CLICK, fn) and mc.addEventListener(Event.ENTER_FRAME, fn) in AS3.",
        "All five NeoFlash languages (AS3, AS2, AS1, TypeScript, JavaScript) compile to the same deterministic Rust + WebAssembly runtime, timeline evaluator, and Web Audio scheduler.",
    ],
    &[
        "Frame scripts run with this bound to the owning MovieClip; object scripts and event callbacks receive the target instance and structured event payload.",
        "Project modules resolve relative ESM imports (./, ../) across .js, .jsx, .ts, and .tsx sources inside the deterministic NeoFlash bundle without Node.js or npm.",
        "Modern ES2023+ syntax including classes, private fields, destructuring, optional chaining (?.), nullish coalescing (??), Map/Set, and Promise/async/await are supported.",
        "JSX and TSX expressions compile to deterministic __nfJsx(tag, props, ...children) calls for declarative display-list or DOM overlay construction.",
    ],
    &[
        "TypeScript type annotations, interfaces, type aliases, enums, generics, and union/intersection types are stripped cleanly during compilation after running the built-in checker.",
        "The bounded built-in TypeScript checker (ts_check) validates primitive/union assignability, interface/class member access, function arity, and utility types (Array<T>, Promise<T>, Record<K,V>, Partial<T>, Pick<T,K>, Omit<T,K>).",
        "Compiler diagnostics report exact file, 1-based line/column spans, and source-map mappings back to the authored .ts/.tsx script in the Compiler Errors panel.",
        "No external tsc or Node toolchain is required; unsupported dynamic metaprogramming or unmodeled third-party .d.ts ambient libraries are explicitly outside the bounded checker.",
    ],
    &[
        "Published HTML5 bundles run inside a standard browser host where window, document, performance, requestAnimationFrame, and CanvasRenderingContext2D are available.",
        "Network requests via fetch(url, init) and JSON parsing obey browser CORS, Content-Security-Policy (CSP), and HTTPS mixed-content rules.",
        "Web Audio (AudioContext) and HTMLMediaElement playback require a user gesture (pointer click or key press) before audio unmutes in modern browsers.",
        "Persistent client state uses window.localStorage and sessionStorage scoped to the page origin; filesystem (fs) and raw TCP/UDP sockets do not exist in the browser sandbox.",
        "HTML5 packages published by NeoFlash 0.1.0-beta.1 include an accessibility layer for screen readers and keyboard users: every accessible object gets a role, a name and a tab stop, and focus rings appear only during keyboard navigation. Not in Beta 1: VoiceOver inside the editor, builds for Intel (x86_64) Macs, automatic installation of updates and full graphics-tablet support.",
    ],
];

pub(crate) fn escape_html(value: &str, output: &mut String) {
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(ch),
        }
    }
}

/// Truncates `text` with `'…'` only when its measured galley width exceeds `max_width`.
pub(crate) fn fit_text_to_width(ui: &Ui, text: &str, style: TextStyle, max_width: f32) -> String {
    if max_width <= 8.0 {
        return String::new();
    }
    let font_id = font(style);
    let full_w = ui.fonts_mut(|f| {
        f.layout_no_wrap(text.to_owned(), font_id.clone(), egui::Color32::WHITE)
            .size()
            .x
    });
    if full_w <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut lo = 0usize;
    let mut hi = chars.len();
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        let w = ui.fonts_mut(|f| {
            f.layout_no_wrap(candidate, font_id.clone(), egui::Color32::WHITE)
                .size()
                .x
        });
        if w <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 {
        "…".to_owned()
    } else {
        chars[..lo].iter().collect::<String>() + "…"
    }
}

/// Draws single-line text inside `clip_rect`, automatically fitting with `'…'` if it would
/// overflow `clip_rect` horizontally. This guarantees diagram node titles and monospace
/// annotations never cross card borders or collide with adjacent boxes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_clipped_text(
    painter: &egui::Painter,
    ctx: &egui::Context,
    clip_rect: Rect,
    pos: Pos2,
    align: egui::Align2,
    text: &str,
    style: TextStyle,
    color: egui::Color32,
) {
    let avail_w = match align.x() {
        egui::Align::Min => (clip_rect.right() - pos.x).max(0.0),
        egui::Align::Center => {
            2.0 * (pos.x - clip_rect.left())
                .min(clip_rect.right() - pos.x)
                .max(0.0)
        }
        egui::Align::Max => (pos.x - clip_rect.left()).max(0.0),
    };
    if avail_w <= 4.0 {
        return;
    }
    let font_id = font(style);
    let fitted = ctx.fonts_mut(|f| {
        let full_w = f
            .layout_no_wrap(text.to_owned(), font_id.clone(), color)
            .size()
            .x;
        if full_w <= avail_w {
            return text.to_owned();
        }
        let chars: Vec<char> = text.chars().collect();
        let mut lo = 0usize;
        let mut hi = chars.len();
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let candidate: String = chars[..mid].iter().collect::<String>() + "…";
            let w = f.layout_no_wrap(candidate, font_id.clone(), color).size().x;
            if w <= avail_w {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        if lo == 0 {
            "…".to_owned()
        } else {
            chars[..lo].iter().collect::<String>() + "…"
        }
    });
    painter
        .with_clip_rect(clip_rect.intersect(painter.clip_rect()))
        .text(pos, align, fitted, font_id, color);
}

#[cfg(test)]
fn help_browser_html(theme: neoflash_ui::Theme) -> String {
    super::help_as_reference::render_full_browser_help_html(theme)
}

fn help_browser_html_for_state(
    theme: neoflash_ui::Theme,
    mode: HelpManualMode,
    page: usize,
    active_tag: Option<&str>,
) -> String {
    super::help_as_reference::render_browser_help_html_for_route(
        theme,
        Some(mode),
        Some(page),
        active_tag,
    )
}

#[cfg(test)]
fn write_help_browser_document(
    theme: neoflash_ui::Theme,
    mode: HelpManualMode,
    page: usize,
    active_tag: Option<&str>,
) -> Result<(PathBuf, String), String> {
    write_help_browser_document_for_project(theme, mode, page, active_tag, None, LanguageScope::All)
}

fn write_help_browser_document_for_project(
    theme: neoflash_ui::Theme,
    mode: HelpManualMode,
    page: usize,
    active_tag: Option<&str>,
    project_language: Option<neoflash_core::ScriptLanguage>,
    lang_scope: LanguageScope,
) -> Result<(PathBuf, String), String> {
    let directory = std::env::temp_dir()
        .join("neoflash-help")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Cannot create the browser Help folder: {error}"))?;
    let filename = match mode {
        HelpManualMode::AuthoringHelp => "help-manual.html",
        HelpManualMode::ScriptingReference => "scripting-reference.html",
    };
    let path = directory.join(filename);
    let clamped_page = page.min(HELP_CHAPTERS.len().saturating_sub(1));
    let effective_tag = active_tag.filter(|tag| HELP_CHAPTERS[clamped_page].tags.contains(tag));
    let html = if project_language.is_some() || lang_scope != LanguageScope::All {
        super::help_as_reference::render_browser_help_html_for_project(
            theme,
            Some(mode),
            Some(clamped_page),
            effective_tag,
            project_language,
            lang_scope,
        )
    } else {
        help_browser_html_for_state(theme, mode, clamped_page, effective_tag)
    };
    std::fs::write(&path, html)
        .map_err(|error| format!("Cannot write the browser Help document: {error}"))?;
    let url = crate::platform_open::file_url_with_fragment(
        &path,
        &format!("chapter-{}", clamped_page + 1),
    )
    .map_err(|error| format!("Cannot create the browser Help URL: {error}"))?;
    Ok((path, url))
}

impl crate::app::Editor {
    /// Resolves the script language currently active in `Actions` (if a non-empty script or
    /// project source file is open there), falling back to `project.effective_script_language()`.
    pub(crate) fn active_help_script_language(&self) -> neoflash_core::ScriptLanguage {
        match self.script_target {
            crate::app::ScriptTarget::Source => {
                if let Some(src_id) = self.selected_source.as_ref()
                    && let Some(unit) = self.project.sources.iter().find(|s| &s.id == src_id)
                {
                    return unit.language;
                }
            }
            crate::app::ScriptTarget::Frame => {
                if let Some(fs) = self
                    .symbol()
                    .timeline
                    .frame_script_for_layer(self.layer, self.frame)
                    && !fs.script.source.trim().is_empty()
                {
                    return fs.script.language;
                }
            }
            crate::app::ScriptTarget::Object => {
                if let Some(elem) = self.selected_element()
                    && let Some(os) = elem.scripts.iter().find(|s| s.event == self.script_event)
                    && !os.script.source.trim().is_empty()
                {
                    return os.script.language;
                }
            }
        }
        self.project.effective_script_language()
    }

    fn active_actions_source_text(&self) -> Option<&str> {
        match self.script_target {
            crate::app::ScriptTarget::Source => self
                .selected_source
                .as_ref()
                .and_then(|id| self.project.sources.iter().find(|s| &s.id == id))
                .map(|s| s.source.as_str()),
            crate::app::ScriptTarget::Frame => self
                .symbol()
                .timeline
                .frame_script_for_layer(self.layer, self.frame)
                .map(|fs| fs.script.source.as_str()),
            crate::app::ScriptTarget::Object => self
                .selected_element()
                .and_then(|e| e.scripts.iter().find(|s| s.event == self.script_event))
                .map(|s| s.script.source.as_str()),
        }
        .filter(|src| !src.trim().is_empty())
    }

    pub(crate) fn open_help_manual(&mut self, ctx: &egui::Context) {
        open_help_manual(ctx);
    }

    /// Opens the Scripting Reference in the language of the active script (the project language
    /// when no script is open). A token under the Actions caret becomes the search.
    pub(crate) fn open_scripting_reference(&mut self, ctx: &egui::Context) {
        let active_lang = self.active_help_script_language();
        let scope = LanguageScope::from_script_language(active_lang);

        let editor_id = self.actions_editor_id();
        let actions_focused = ctx.memory(|m| m.has_focus(editor_id));
        let contextual_token = self.active_actions_source_text().and_then(|source| {
            let state = egui::text_edit::TextEditState::load(ctx, editor_id)?;
            let range = state.cursor.char_range()?;
            let has_selection = range.primary.index != range.secondary.index;
            if !actions_focused && !has_selection {
                return None;
            }
            let candidate = crate::script_api::extract_reference_token(
                source,
                range.primary.index,
                range.secondary.index,
            )?;
            reference_lookup(&candidate, scope, active_lang).map(|lookup| (candidate, lookup))
        });

        ctx.data_mut(|d| {
            let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
            s.open = true;
            s.set_route(HelpManualMode::ScriptingReference);
            s.lang_scope = scope;
            s.pin_page = false;
            if let Some((token, lookup)) = contextual_token {
                s.apply_lookup(scope, token, lookup);
            } else if !scope.has_guide(s.page) {
                s.page = scope.default_chapter(active_lang);
                s.diagram_tab = 0;
            }
        });
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn open_scripting_reference_for_token(&mut self, ctx: &egui::Context, token: &str) {
        let active_lang = self.active_help_script_language();
        let scope = LanguageScope::from_script_language(active_lang);
        let trimmed = token.trim().to_owned();
        let lookup = reference_lookup(&trimmed, scope, active_lang).unwrap_or(ReferenceLookup {
            api: None,
            page: scope.default_chapter(active_lang),
        });
        ctx.data_mut(|d| {
            let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
            s.open = true;
            s.set_route(HelpManualMode::ScriptingReference);
            s.lang_scope = scope;
            s.pin_page = false;
            s.apply_lookup(scope, trimmed, lookup);
        });
    }

    /// Insert at cursor writes through the Actions editor, so it needs that editor on screen.
    fn reference_insert_available(&self) -> bool {
        if self.is_standalone_script_session() {
            return !self.project.sources.is_empty();
        }
        self.has_document()
            && self
                .workspace
                .is_panel_active(crate::workspace::PanelId::Actions)
            && self.effective_actions_location().is_some()
    }

    pub(crate) fn help_manual_dialog(&mut self, ctx: &egui::Context) {
        let mut state = ctx.data(|d| {
            d.get_temp::<HelpManualState>(state_id())
                .unwrap_or_default()
        });
        if !state.open {
            return;
        }
        if self
            .clipboard
            .take_pending_label_copy_for(ctx.viewport_id())
        {
            ctx.input_mut(|i| {
                if !i.events.iter().any(|e| matches!(e, egui::Event::Copy)) {
                    i.events.push(egui::Event::Copy);
                }
            });
        }
        if !state.search_focused && crate::clipboard::has_egui_label_selection(ctx) {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::C))
                && !ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)))
            {
                ctx.input_mut(|i| i.events.push(egui::Event::Copy));
            }
            if !super::dialog_helpers::is_any_popup_open(ctx)
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                ctx.plugin::<egui::text_selection::LabelSelectionState>()
                    .lock()
                    .clear_selection();
            }
        }
        let proj_lang = self.project.effective_script_language();
        state.normalize(proj_lang);
        let reference = state.mode == HelpManualMode::ScriptingReference;
        let p = neoflash_ui::palette(ctx);
        let can_insert = reference && self.reference_insert_available();
        super::help_as_reference::set_insert_available(ctx, can_insert);

        let viewport = ctx.content_rect();
        let bounds = state.windows[usize::from(reference)].bounds(viewport);
        let dialog_frame = Surface::Dialog.frame(ctx);
        let chrome = dialog_frame.total_margin().sum();
        let width = (bounds.width() - chrome.x).max(1.0);
        let height = (bounds.height() - chrome.y).max(1.0);
        let squeeze = ((width + chrome.x) / metrics::reference_width()).min(1.0);
        let api_tab = reference && state.tab == ReferenceTab::Api;
        let tree_w = (metrics::tree_width() * squeeze).round();
        let side_w = if api_tab {
            // 300 pt in Normal, scaled with the inspector width in the denser profiles.
            let extra = (bounds.width() - metrics::reference_width()).max(0.0);
            (300.0 * metrics::inspector() / metrics::INSPECTOR * squeeze + extra * 0.5).round()
        } else {
            (metrics::sidebar_wide() * squeeze).round()
        };

        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        let popup_open = super::dialog_helpers::is_any_popup_open(ctx);
        let focused = ctx.memory(|m| m.focused());
        let list_focus = focused.is_none() || state.search_focused;
        // ↑↓ and ↵ drive the lists even while the search field is being typed in.
        let mut keys = RefKeys::default();
        let mut insert_shortcut = false;
        if !popup_open && list_focus {
            ctx.input_mut(|i| {
                insert_shortcut =
                    reference && i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                keys.down = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
                keys.up = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
                keys.enter = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                if focused.is_none() {
                    keys.left = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft);
                    keys.right = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight);
                }
            });
            // Typing with nothing focused starts a search.
            if focused.is_none() {
                let typed: String = ctx.input(|i| {
                    i.events
                        .iter()
                        .filter_map(|event| match event {
                            egui::Event::Text(text) => Some(text.as_str()),
                            _ => None,
                        })
                        .collect()
                });
                if !typed.trim().is_empty() {
                    state.query.push_str(&typed);
                    state.focus_search = true;
                }
            }
        }

        let mut should_close = false;
        let mut open_cmd_search = false;
        let mut actions = RefActions::default();
        let mut nav_target: Option<usize> = None;
        let modal_id = egui::Id::new("neoflash-help-manual-modal");
        let modal = egui::Modal::new(modal_id)
            .area(
                egui::Modal::default_area(modal_id)
                    .anchor(egui::Align2::LEFT_TOP, bounds.min - viewport.min),
            )
            .frame(dialog_frame)
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.style_mut().interaction.selectable_labels = true;
                ui.style_mut().interaction.multi_widget_text_select = true;
                // The whole window is claimed first; every region is a child at a fixed rect, so
                // no tab, selection or search result can move or resize it.
                ui.set_min_size(vec2(width, height));
                ui.set_max_size(vec2(width, height));
                let full = Rect::from_min_size(ui.cursor().min, vec2(width, height));
                ui.allocate_rect(full, Sense::hover());
                let line = Stroke::new(1.0_f32, p.border_subtle);
                let region = |ui: &mut Ui, salt: &str, rect: Rect, layout: egui::Layout| {
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(salt)
                            .max_rect(rect)
                            .layout(layout),
                    );
                    child.set_clip_rect(ui.clip_rect().intersect(rect));
                    child
                };
                let column = egui::Layout::top_down(egui::Align::Min);

                // Header.
                let header_rect =
                    Rect::from_min_size(full.min, vec2(width, metrics::dialog_header()));
                let mut header = region(ui, "help-header", header_rect, column);
                // Register the header gesture before the Close button so the button wins.
                if header
                    .interact(header_rect, header.id().with("maximize"), Sense::click())
                    .double_clicked_by(egui::PointerButton::Primary)
                {
                    state.windows[usize::from(reference)].toggle_maximized();
                    ctx.request_repaint();
                }
                should_close |= DialogHeader {
                    title: state.mode.dialog_title(),
                }
                .show(&mut header);
                ui.painter()
                    .hline(full.x_range(), header_rect.bottom() - 0.5, line);

                // Reference bar: language, search, project badge, more.
                let bar_rect = Rect::from_min_size(
                    header_rect.left_bottom(),
                    vec2(width, metrics::control() + 2.0 * space::s4()),
                );
                let mut bar = region(
                    ui,
                    "help-bar",
                    bar_rect.shrink2(vec2(space::s5(), space::s4())),
                    egui::Layout::left_to_right(egui::Align::Center),
                );
                bar.spacing_mut().item_spacing.x = space::s3();
                if reference {
                    let before = state.lang_scope;
                    c::Segmented::show(
                        &mut bar,
                        &mut state.lang_scope,
                        &[
                            (
                                LanguageScope::As1As2,
                                LanguageScope::As1As2.reference_label(),
                            ),
                            (LanguageScope::As3, LanguageScope::As3.reference_label()),
                            (LanguageScope::JsTs, LanguageScope::JsTs.reference_label()),
                        ],
                        false,
                    );
                    if state.lang_scope != before {
                        if !state.lang_scope.has_guide(state.page) {
                            state.page = state.lang_scope.default_chapter(proj_lang);
                            state.diagram_tab = 0;
                        }
                        state.api.ensure_selection(state.lang_scope);
                        state.api.reveal_tree = true;
                    }
                }
                let scope = state.lang_scope;
                let placeholder = match (state.mode, state.tab) {
                    (HelpManualMode::AuthoringHelp, _) => "Search NeoFlash Help".to_owned(),
                    (_, ReferenceTab::Api) if scope == LanguageScope::JsTs => format!(
                        "Search {}: bindings, functions, classes, types",
                        scope.reference_label()
                    ),
                    (_, ReferenceTab::Api) => format!(
                        "Search {}: classes, methods, properties, events",
                        scope.reference_label()
                    ),
                    (_, ReferenceTab::Guides) => {
                        format!("Search guides for {}", scope.reference_label())
                    }
                };
                let project_badge = format!("Project: {}", proj_lang.display_label());
                let badge_w = text_width(&bar, &project_badge, TextStyle::Small) + 12.0;
                let more_w = metrics::control();
                let gap = space::s3();
                let rest = bar.available_width();
                // The badge yields to the search field in a narrow window; the footer then
                // carries the project language.
                let show_badge =
                    reference && rest - badge_w - more_w - 2.0 * gap >= metrics::field_m();
                let search_w = (rest - more_w - gap - if show_badge { badge_w + gap } else { 0.0 })
                    .max(metrics::field_num());
                let search = bar.add(Search::new(&mut state.query, &placeholder).width(search_w));
                let search_edit = search.id.with("edit");
                if std::mem::take(&mut state.focus_search) {
                    bar.memory_mut(|m| m.request_focus(search_edit));
                    let mut edit =
                        egui::text_edit::TextEditState::load(ctx, search_edit).unwrap_or_default();
                    let end = egui::text::CCursor::new(state.query.chars().count());
                    edit.cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(end)));
                    edit.store(ctx, search_edit);
                }
                state.search_focused = bar.memory(|m| m.has_focus(search_edit));
                if show_badge {
                    bar.add(Badge::new(&project_badge, BadgeType::Accent));
                }
                let more = bar.add(c::IconButton::new(i16::MORE, "More"));
                c::popup_below(&more, metrics::menu_width(), |ui| {
                    if ui
                        .add(c::MenuItem::new("Open in browser").icon(i16::EXTERNAL))
                        .clicked()
                    {
                        actions.open_in_browser = true;
                    }
                    if ui
                        .add(c::MenuItem::new("Command Search").shortcut("F1"))
                        .clicked()
                    {
                        should_close = true;
                        open_cmd_search = true;
                    }
                    c::menu_separator(ui);
                    if reference {
                        if ui.add(c::MenuItem::new("NeoFlash Help…")).clicked() {
                            state.set_route(HelpManualMode::AuthoringHelp);
                            if state.page >= AUTHORING_CHAPTER_COUNT {
                                state.page = 0;
                                state.diagram_tab = 0;
                            }
                        }
                    } else if ui.add(c::MenuItem::new("Scripting Reference…")).clicked() {
                        state.set_route(HelpManualMode::ScriptingReference);
                        state.lang_scope = LanguageScope::from_script_language(proj_lang);
                        state.page = state.lang_scope.default_chapter(proj_lang);
                        state.diagram_tab = 0;
                    }
                });
                ui.painter()
                    .hline(full.x_range(), bar_rect.bottom() - 0.5, line);

                // Body: three fixed columns.
                let footer_rect = Rect::from_min_max(
                    egui::pos2(full.left(), full.bottom() - metrics::status_bar()),
                    full.max,
                );
                let body = Rect::from_min_max(bar_rect.left_bottom(), footer_rect.right_top());
                let tree_rect = Rect::from_min_size(body.min, vec2(tree_w, body.height()));
                let side_rect =
                    Rect::from_min_max(egui::pos2(body.right() - side_w, body.top()), body.max);
                let mid_rect = Rect::from_min_max(
                    egui::pos2(tree_rect.right() + 1.0, body.top()),
                    egui::pos2(side_rect.left() - 1.0, body.bottom()),
                );
                ui.painter().rect_filled(tree_rect, 0.0, p.bg_app);
                if api_tab {
                    ui.painter().rect_filled(side_rect, 0.0, p.bg_app);
                }
                ui.painter()
                    .vline(tree_rect.right() + 0.5, body.y_range(), line);
                ui.painter()
                    .vline(side_rect.left() - 0.5, body.y_range(), line);

                // 1. Tree column.
                let mut tree_content = tree_rect;
                if reference {
                    let tabs_rect =
                        Rect::from_min_size(tree_rect.min, vec2(tree_w, metrics::panel_tab()));
                    let mut tabs = region(
                        ui,
                        "help-tabs",
                        tabs_rect,
                        egui::Layout::left_to_right(egui::Align::Center),
                    );
                    tabs.spacing_mut().item_spacing.x = 0.0;
                    for (tab, label) in
                        [(ReferenceTab::Api, "API"), (ReferenceTab::Guides, "Guides")]
                    {
                        if tabs
                            .add(c::PanelTab {
                                label,
                                active: state.tab == tab,
                            })
                            .clicked()
                        {
                            state.tab = tab;
                            state.api.focus_members = false;
                        }
                    }
                    ui.painter()
                        .hline(tree_rect.x_range(), tabs_rect.bottom() - 0.5, line);
                    tree_content.min.y = tabs_rect.bottom();
                }
                let mut tree = region(ui, "help-tree", tree_content, column);
                if api_tab {
                    super::help_as_reference::api_tree_column(
                        &mut tree,
                        &mut state.api,
                        scope,
                        &state.query,
                        keys,
                    );
                } else {
                    chapter_list_column(&mut tree, &mut state, keys);
                }

                // 2. Middle column and 3. side column.
                let mut mid = region(ui, "help-middle", mid_rect, column);
                let mut side = region(ui, "help-side", side_rect, column);
                if api_tab {
                    super::help_as_reference::api_class_column(
                        &mut mid,
                        &mut state.api,
                        scope,
                        keys,
                        &mut actions,
                    );
                    if let Some(output) = super::help_as_reference::api_detail_column(
                        &mut side,
                        &mut state.api,
                        scope,
                        &mut actions,
                    ) {
                        ArticlePosition::retain(
                            &mut state.reading_position,
                            ctx,
                            output,
                            &[],
                            false,
                        );
                    }
                } else {
                    let mut area = egui::ScrollArea::vertical()
                        .id_salt(("help-manual-scroll", state.page))
                        .auto_shrink([false, false]);
                    let requested_offset = state.scroll_article_to.take();
                    if let Some(offset) = requested_offset {
                        area = area.vertical_scroll_offset(offset);
                    }
                    let output = area.themed().show(&mut mid, |ui| {
                        outline_begin(ui.cursor().min.y);
                        egui::Frame::new()
                            .inner_margin(egui::Margin::same(space::s5() as i8))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                render_chapter(
                                    ui,
                                    &mut state,
                                    proj_lang,
                                    &mut nav_target,
                                    &mut actions,
                                );
                            });
                    });
                    let outline = outline_take();
                    state.article_offset = ArticlePosition::retain(
                        &mut state.reading_position,
                        ctx,
                        output,
                        &outline,
                        requested_offset.is_some(),
                    );
                    outline_column(&mut side, &outline, &mut state);
                }

                // Footer status row.
                let corner = radius::xl() as u8;
                ui.painter().rect_filled(
                    footer_rect,
                    egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: corner,
                        se: corner,
                    },
                    p.bg_app,
                );
                ui.painter()
                    .hline(full.x_range(), footer_rect.top() + 0.5, line);
                let mut footer = region(
                    ui,
                    "help-footer",
                    footer_rect.shrink2(vec2(space::s5(), 0.0)),
                    egui::Layout::left_to_right(egui::Align::Center),
                );
                let mut status = match (state.mode, state.tab) {
                    (HelpManualMode::AuthoringHelp, _) => {
                        format!("NeoFlash Help · {AUTHORING_CHAPTER_COUNT} chapters")
                    }
                    (_, ReferenceTab::Api) => super::help_as_reference::api_footer_text(scope),
                    (_, ReferenceTab::Guides) => format!(
                        "Guides · {} chapters · {}",
                        scope.guide_chapters().len(),
                        scope.reference_label()
                    ),
                };
                if reference && !show_badge {
                    status = format!("{status} · {project_badge}");
                }
                let hints: &[(&str, &str)] = if api_tab {
                    &[("↑↓", "move"), ("↵", "open"), ("⌘↵", "insert at cursor")]
                } else {
                    &[("↑↓", "move")]
                };
                let hints_w: f32 = hints
                    .iter()
                    .map(|(key, label)| {
                        text_width(&footer, &c::platform_shortcut(key), TextStyle::Small)
                            .max(metrics::kbd() - 10.0)
                            + 10.0
                            + text_width(&footer, label, TextStyle::Small)
                            + space::s1()
                            + space::s3()
                    })
                    .sum();
                let status_w = (footer.available_width() - hints_w - space::s3()).max(0.0);
                let fitted = fit_text_to_width(&footer, &status, TextStyle::Small, status_w);
                footer.label(rich(fitted, TextStyle::Small).color(p.text_tertiary));
                footer.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = space::s1();
                    for (i, (key, label)) in hints.iter().rev().enumerate() {
                        if i > 0 {
                            ui.add_space(space::s3() - space::s1());
                        }
                        ui.label(rich(*label, TextStyle::Small).color(p.text_tertiary));
                        kbd(ui, key, false);
                    }
                });
                state.windows[usize::from(reference)].resize_edges(ui, bounds, viewport);
            });

        if !should_close
            && matches!(
                kb.resolve(ctx, &modal, false),
                super::dialog_helpers::DialogKeyboardOutcome::Cancel
            )
        {
            should_close = true;
        }
        if let Some(target) = nav_target {
            state.navigate(target);
        }
        if insert_shortcut && state.tab == ReferenceTab::Api && actions.insert.is_none() {
            actions.insert =
                super::help_as_reference::api_selected_insertion(&state.api, state.lang_scope);
        }
        if let Some(code) = super::help_as_reference::take_article_insert(ctx) {
            let caret = code.chars().count();
            actions.insert = Some(crate::code_tools::CodeCommand::Insert {
                text: code,
                caret,
                import: None,
            });
        }
        if should_close {
            state.open = false;
            ctx.plugin::<egui::text_selection::LabelSelectionState>()
                .lock()
                .clear_selection();
        }
        let open_mode = state.mode;
        let open_page = state.page;
        let open_lang_scope = state.lang_scope;
        ctx.data_mut(|d| d.insert_temp(state_id(), state));
        if let Some(command) = actions.insert
            && can_insert
        {
            self.actions_ui.pending = Some(command);
        }
        if actions.open_in_browser {
            match write_help_browser_document_for_project(
                neoflash_ui::Theme::current(ctx),
                open_mode,
                open_page,
                None,
                Some(proj_lang),
                open_lang_scope,
            )
            .and_then(|(path, url)| {
                crate::platform_open::open_url(&url)
                    .map(|_| path)
                    .map_err(|error| format!("Cannot open Help in the browser: {error}"))
            }) {
                Ok(path) => self.message(format!("Opened Help in browser: {}", path.display())),
                Err(error) => self.notify_with_output(c::ToastType::Error, error),
            }
        }
        if open_cmd_search {
            self.open_function_search(ctx);
        }
    }
}

/// Where a looked-up token opens: an API declaration when the index has one, and the guide
/// chapter that covers it.
struct ReferenceLookup {
    /// Best API hit and whether its name equals the token.
    api: Option<(String, Option<String>, bool)>,
    page: usize,
}

/// Looks `token` up in the API index and the guide topics of `scope`; `None` when neither knows it.
fn reference_lookup(
    token: &str,
    scope: LanguageScope,
    language: neoflash_core::ScriptLanguage,
) -> Option<ReferenceLookup> {
    let api = super::help_as_reference::api_search(scope, token)
        .into_iter()
        .next()
        .map(|hit| (hit.class_key, hit.member_key, hit.rank == 0));
    let (topics, _) = super::help_as_reference::search_reference_hits(token, scope, language);
    let known = api.is_some()
        || !topics.is_empty()
        || crate::script_api::keyword_reference_summary(language, token).is_some();
    known.then(|| ReferenceLookup {
        api,
        page: topics
            .first()
            .map(|hit| hit.chapter_idx)
            .filter(|&idx| idx >= AUTHORING_CHAPTER_COUNT)
            .unwrap_or_else(|| scope.default_chapter(language)),
    })
}

impl HelpManualState {
    /// Switches between Help and the Scripting Reference. The search belongs to the route it
    /// was typed in, so it does not follow to the other one.
    fn set_route(&mut self, mode: HelpManualMode) {
        if self.mode != mode {
            self.query.clear();
            self.mode = mode;
        }
    }

    /// Keeps the route, language and chapter consistent before a frame is drawn. The reference
    /// is always in exactly one language: the project language unless a chapter was asked for.
    fn normalize(&mut self, project_language: neoflash_core::ScriptLanguage) {
        self.page = self.page.min(HELP_CHAPTERS.len() - 1);
        let pinned = std::mem::take(&mut self.pin_page);
        match self.mode {
            HelpManualMode::AuthoringHelp => {
                if self.page >= AUTHORING_CHAPTER_COUNT {
                    self.page = 0;
                    self.diagram_tab = 0;
                }
            }
            HelpManualMode::ScriptingReference => {
                let project_scope = LanguageScope::from_script_language(project_language);
                if self.lang_scope == LanguageScope::All {
                    self.lang_scope = project_scope;
                }
                if !self.lang_scope.has_guide(self.page) {
                    if pinned && self.page >= AUTHORING_CHAPTER_COUNT {
                        self.lang_scope = if project_scope.has_guide(self.page) {
                            project_scope
                        } else {
                            LanguageScope::for_guide(self.page)
                        };
                    } else {
                        self.page = self.lang_scope.default_chapter(project_language);
                        self.diagram_tab = 0;
                    }
                }
                self.api.ensure_selection(self.lang_scope);
            }
        }
    }

    /// Opens chapter `target` (a cross-link, a topic hit or the chapter list), remembering the
    /// current one for Back. A chapter of the other route switches the route.
    fn navigate(&mut self, target: usize) {
        if target >= HELP_CHAPTERS.len() || target == self.page {
            return;
        }
        self.history.push(self.page);
        if self.history.len() > 24 {
            self.history.remove(0);
        }
        self.open_page(target);
    }

    fn open_page(&mut self, page: usize) {
        self.page = page.min(HELP_CHAPTERS.len() - 1);
        self.set_route(HelpManualMode::for_page(self.page));
        if self.mode == HelpManualMode::ScriptingReference {
            self.tab = ReferenceTab::Guides;
            if !self.lang_scope.has_guide(self.page) {
                self.lang_scope = LanguageScope::All;
            }
        }
        self.pin_page = true;
        self.diagram_tab = 0;
        self.scroll_article_to = Some(0.0);
    }

    fn apply_lookup(&mut self, scope: LanguageScope, token: String, lookup: ReferenceLookup) {
        self.query = token;
        self.page = lookup.page;
        self.diagram_tab = 0;
        match lookup.api {
            Some((class, member, exact)) => {
                self.tab = ReferenceTab::Api;
                if exact {
                    self.api.select(scope, &class, member.as_deref());
                }
            }
            None => self.tab = ReferenceTab::Guides,
        }
    }
}

/// Width of `text` on one line.
pub(crate) fn text_width(ui: &Ui, text: &str, style: TextStyle) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font(style), egui::Color32::WHITE)
        .size()
        .x
}

/// `3. AS3 Display List` → `AS3 Display List`: guide lists are per language, so the manual's
/// chapter numbers do not apply.
pub(crate) fn plain_chapter_label(label: &str) -> &str {
    let digits = label.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        label[digits..].strip_prefix(". ").unwrap_or(label)
    } else {
        label
    }
}

/// Chapters listed in the tree column: Help chapters, or the guides of the selected language.
fn listed_chapters(state: &HelpManualState) -> Vec<usize> {
    let all: Vec<usize> = match state.mode {
        HelpManualMode::AuthoringHelp => (0..AUTHORING_CHAPTER_COUNT).collect(),
        HelpManualMode::ScriptingReference => state.lang_scope.guide_chapters().to_vec(),
    };
    let q = state.query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return all;
    }
    all.into_iter()
        .filter(|&idx| super::help_as_reference::chapter_matches_search_deep(idx, &q))
        .collect()
}

/// Tree column of Help and of the Guides tab: one row per chapter; the search field filters it.
fn chapter_list_column(ui: &mut Ui, state: &mut HelpManualState, keys: RefKeys) {
    let p = neoflash_ui::palette(ui.ctx());
    let chapters = listed_chapters(state);
    let guides = state.mode == HelpManualMode::ScriptingReference;
    let mut open: Option<usize> = None;
    if !chapters.is_empty() && (keys.down || keys.up) {
        let at = chapters.iter().position(|&idx| idx == state.page);
        let next = match (at, keys.down) {
            (Some(i), true) => (i + 1).min(chapters.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
            (None, _) => 0,
        };
        open = Some(chapters[next]);
    }
    egui::ScrollArea::vertical()
        .id_salt("help-manual-sidebar-scroll")
        .auto_shrink([false, false])
        .themed()
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::same(space::s1() as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 1.0;
                    if chapters.is_empty() {
                        let none = format!("No chapter mentions “{}”.", state.query.trim());
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(
                                space::s3() as i8,
                                space::s2() as i8,
                            ))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        rich(&none, TextStyle::Small).color(p.text_tertiary),
                                    )
                                    .wrap(),
                                );
                            });
                    }
                    for &idx in &chapters {
                        let chapter = &HELP_CHAPTERS[idx];
                        let label = if guides {
                            plain_chapter_label(chapter.sidebar_label)
                        } else {
                            chapter.sidebar_label
                        };
                        let row = RefRow {
                            icon: Some(if guides { i16::FILE_CODE } else { chapter.icon }),
                            selected: state.page == idx,
                            ..RefRow::new(label)
                        }
                        .show(ui);
                        let tip = format!(
                            "{}\n{}",
                            if guides {
                                plain_chapter_label(chapter.title)
                            } else {
                                chapter.title
                            },
                            chapter.subtitle
                        );
                        if c::tooltip(row, &tip, None).clicked() {
                            open = Some(idx);
                        }
                    }
                });
        });
    if let Some(idx) = open
        && idx != state.page
    {
        state.page = idx;
        state.diagram_tab = 0;
        state.scroll_article_to = Some(0.0);
    }
}

/// One heading of the article, collected while it is drawn.
pub(crate) struct OutlineEntry {
    /// 0 section, 1 block inside a section.
    level: u8,
    title: String,
    /// Offset from the top of the article.
    offset: f32,
}

// Keep the same part of the same section visible as text wraps at a new width.
#[derive(Clone, Debug)]
struct ArticlePosition {
    area: egui::Id,
    viewport: egui::Vec2,
    total: f32,
    visible_y: f32,
    headings_before: usize,
    fraction: f32,
}

impl ArticlePosition {
    fn section(outline: &[OutlineEntry], count: usize, total: f32) -> (f32, f32) {
        let start = count
            .checked_sub(1)
            .and_then(|i| outline.get(i))
            .map_or(0.0, |h| h.offset);
        let end = outline.get(count).map_or(total, |h| h.offset);
        (start, (end - start).max(1.0))
    }

    fn retain(
        anchor: &mut Option<Self>,
        ctx: &egui::Context,
        output: egui::scroll_area::ScrollAreaOutput<()>,
        outline: &[OutlineEntry],
        jumped: bool,
    ) -> f32 {
        let viewport = output.inner_rect.size();
        let total = output.content_size.y;
        let mut scroll = output.state;
        let y = scroll.offset.y;
        if let Some(previous) = anchor
            && previous.area == output.id
            && !jumped
        {
            if previous.viewport != viewport || previous.total != total {
                let (start, length) =
                    Self::section(outline, previous.headings_before.min(outline.len()), total);
                let offset =
                    (start + previous.fraction * length).clamp(0.0, (total - viewport.y).max(0.0));
                if offset != y {
                    scroll.offset.y = offset;
                    scroll.store(ctx, output.id);
                    ctx.request_repaint();
                }
                // Preserve the intended anchor when an enlarged viewport fits the entire article.
                previous.viewport = viewport;
                previous.total = total;
                previous.visible_y = offset;
                return offset;
            }
            if y == previous.visible_y {
                return y;
            }
        }
        let headings_before = outline.partition_point(|heading| heading.offset <= y);
        let (start, length) = Self::section(outline, headings_before, total);
        *anchor = Some(Self {
            area: output.id,
            viewport,
            total,
            visible_y: y,
            headings_before,
            fraction: ((y - start) / length).clamp(0.0, 1.0),
        });
        y
    }
}

thread_local! {
    static OUTLINE: std::cell::RefCell<Option<(f32, Vec<OutlineEntry>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Starts collecting headings; `top` is the screen position of the article's first line.
fn outline_begin(top: f32) {
    OUTLINE.with(|outline| *outline.borrow_mut() = Some((top, Vec::new())));
}

fn outline_take() -> Vec<OutlineEntry> {
    OUTLINE
        .with(|outline| outline.borrow_mut().take())
        .map(|(_, entries)| entries)
        .unwrap_or_default()
}

/// Registers a heading at the cursor for the "On this page" outline. Outside an article
/// (chapters drawn by tests or other hosts) it does nothing.
pub(crate) fn outline_heading(ui: &Ui, level: u8, title: &str) {
    OUTLINE.with(|outline| {
        if let Some((top, entries)) = outline.borrow_mut().as_mut() {
            entries.push(OutlineEntry {
                level,
                title: title.to_owned(),
                offset: ui.cursor().min.y - *top,
            });
        }
    });
}

/// "On this page": the headings of the open article; a click scrolls to the heading.
fn outline_column(ui: &mut Ui, outline: &[OutlineEntry], state: &mut HelpManualState) {
    let p = neoflash_ui::palette(ui.ctx());
    // The heading the reader is in: the last one above the top of the viewport.
    let current = outline
        .iter()
        .rposition(|entry| entry.offset <= state.article_offset + space::s5())
        .unwrap_or(0);
    egui::ScrollArea::vertical()
        .id_salt(("help-outline", state.page))
        .auto_shrink([false, false])
        .themed()
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(
                    space::s4() as i8,
                    space::s5() as i8,
                ))
                .show(ui, |ui| {
                    let width = ui.available_width();
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.y = metrics::row_gap();
                    ui.label(rich("On this page", TextStyle::Overline).color(p.text_tertiary));
                    ui.add_space(space::s1());
                    if outline.is_empty() {
                        ui.label(rich("No sections", TextStyle::Small).color(p.text_tertiary));
                    }
                    for (i, entry) in outline.iter().enumerate() {
                        let indent = f32::from(entry.level) * space::s3();
                        let shown = fit_text_to_width(
                            ui,
                            &entry.title,
                            TextStyle::Body,
                            (width - indent).max(0.0),
                        );
                        let color = if i == current {
                            p.accent_default
                        } else if entry.level == 0 {
                            p.text_primary
                        } else {
                            p.text_secondary
                        };
                        let (rect, response) = ui.allocate_exact_size(
                            vec2(width, TextStyle::Body.spec().line_height),
                            Sense::click(),
                        );
                        let hovered = response.hovered();
                        ui.painter().with_clip_rect(rect).text(
                            egui::pos2(rect.left() + indent, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &shown,
                            font(TextStyle::Body),
                            if hovered { p.accent_default } else { color },
                        );
                        if hovered {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let title = entry.title.clone();
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Link, true, &title)
                        });
                        let response = if shown != entry.title {
                            c::tooltip(response, &entry.title, None)
                        } else {
                            response
                        };
                        if response.clicked() {
                            state.scroll_article_to = Some(entry.offset.max(0.0));
                        }
                    }
                });
        });
}

/// The article: title, subtitle, topic hits for the search, the chapter, its links.
fn render_chapter(
    ui: &mut Ui,
    state: &mut HelpManualState,
    proj_lang: neoflash_core::ScriptLanguage,
    nav_target: &mut Option<usize>,
    actions: &mut RefActions,
) {
    let p = neoflash_ui::palette(ui.ctx());
    let page_idx = state.page.min(HELP_CHAPTERS.len() - 1);
    let meta = &HELP_CHAPTERS[page_idx];
    let guide = page_idx >= AUTHORING_CHAPTER_COUNT;
    let title = if guide {
        plain_chapter_label(meta.title)
    } else {
        meta.title
    };

    // Title row: Back (after a cross-link) and Open in browser stay at the right edge.
    let width = ui.available_width();
    let browser_w = text_width(ui, "Open in browser", TextStyle::Body) + 16.0 + 4.0 * space::s2();
    let back_w = if state.history.is_empty() {
        0.0
    } else {
        text_width(ui, "Back", TextStyle::Body) + 16.0 + 4.0 * space::s2()
    };
    let full_buttons = width - browser_w - back_w >= metrics::field_m();
    let buttons_w = if full_buttons {
        browser_w + back_w
    } else {
        (metrics::control() + space::s2()) * if state.history.is_empty() { 1.0 } else { 2.0 }
    };
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width((width - buttons_w - space::s2()).max(48.0));
            // The first title line sits on the centre line of the buttons.
            ui.add_space(
                ((metrics::control() - TextStyle::Heading.spec().line_height) / 2.0).max(0.0),
            );
            ui.add(egui::Label::new(rich(title, TextStyle::Heading).color(p.text_primary)).wrap());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            ui.spacing_mut().item_spacing.x = space::s2();
            let open = if full_buttons {
                ui.add(Button::ghost("Open in browser").icon(i16::EXTERNAL))
            } else {
                ui.add(c::IconButton::new(i16::EXTERNAL, "Open in browser"))
            };
            actions.open_in_browser |= open.clicked();
            if let Some(&previous) = state.history.last() {
                let back = if full_buttons {
                    ui.add(Button::ghost("Back").icon(i16::CHEVLEFT))
                } else {
                    ui.add(c::IconButton::new(i16::CHEVLEFT, "Back"))
                };
                let tip = format!("Back to {}", HELP_CHAPTERS[previous].title);
                if c::tooltip(back, &tip, None).clicked()
                    && let Some(previous) = state.history.pop()
                {
                    state.open_page(previous);
                }
            }
        });
    });
    ui.add(egui::Label::new(rich(meta.subtitle, TextStyle::Small).color(p.text_secondary)).wrap());
    ui.add_space(space::s4());

    if !state.query.trim().is_empty() {
        let query = state.query.clone();
        let scope = guide.then_some(state.lang_scope);
        super::help_as_reference::render_topic_hits(ui, &query, scope, proj_lang, nav_target);
    }

    match page_idx {
        0 => chapter_workspace_and_stage(ui, state),
        1 => chapter_drawing_modes_and_objects(ui, state),
        2 => chapter_selection_and_transform(ui, state),
        3 => chapter_vector_and_pen_states(ui, state),
        4 => chapter_brush_eraser_and_deco(ui, state),
        5 => chapter_symbols_9slice_and_library(ui, state),
        6 => chapter_timeline_layers_and_masks(ui, state),
        7 => chapter_tweens_motion_and_ik(ui, state),
        8 => chapter_filters_text_and_audio(ui, state),
        9 => chapter_scripting_import_and_publish(ui, state, nav_target),
        10 => super::help_as_reference::chapter_as1_as2_fundamentals(
            ui,
            &mut state.diagram_tab,
            nav_target,
        ),
        11 => super::help_as_reference::chapter_as2_core_classes(
            ui,
            &mut state.diagram_tab,
            nav_target,
        ),
        12 => super::help_as_reference::chapter_as3_display_list(
            ui,
            &mut state.diagram_tab,
            nav_target,
        ),
        13 => super::help_as_reference::chapter_as3_events_lifecycle(
            ui,
            &mut state.diagram_tab,
            nav_target,
        ),
        14 => super::help_as_reference::chapter_as3_graphics_filters_sound(
            ui,
            &mut state.diagram_tab,
            nav_target,
        ),
        15 => super::help_as_reference::chapter_as_migration_guide(
            ui,
            &mut state.diagram_tab,
            nav_target,
        ),
        16 => super::help_as_reference::chapter_javascript_runtime(
            ui,
            &mut state.diagram_tab,
            nav_target,
        ),
        17 => super::help_as_reference::chapter_typescript_checker(
            ui,
            &mut state.diagram_tab,
            nav_target,
        ),
        _ => super::help_as_reference::chapter_browser_web_apis(
            ui,
            &mut state.diagram_tab,
            nav_target,
        ),
    }

    if page_idx <= 9 {
        let links: Vec<(usize, &str)> = meta
            .related
            .iter()
            .filter_map(|&idx| HELP_CHAPTERS.get(idx).map(|ch| (idx, ch.title)))
            .collect();
        inline_cross_links(ui, nav_target, &links);
    }

    // Previous / next chapter of the list in the tree column.
    let chapters: Vec<usize> = match state.mode {
        HelpManualMode::AuthoringHelp => (0..AUTHORING_CHAPTER_COUNT).collect(),
        HelpManualMode::ScriptingReference => state.lang_scope.guide_chapters().to_vec(),
    };
    if let Some(at) = chapters.iter().position(|&idx| idx == page_idx) {
        let previous = at.checked_sub(1).map(|i| chapters[i]);
        let next = chapters.get(at + 1).copied();
        ui.add_space(space::s4());
        divider(ui, false);
        ui.add_space(space::s2());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    previous.is_some(),
                    Button::ghost("Previous").icon(i16::CHEVLEFT),
                )
                .clicked()
                && let Some(previous) = previous
            {
                *nav_target = Some(previous);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(next.is_some(), Button::ghost("Next topic"))
                    .clicked()
                    && let Some(next) = next
                {
                    *nav_target = Some(next);
                }
            });
        });
    }
}

// ── Article blocks: headings, paragraphs, lists and figures ──

/// "See also" row at the end of a chapter.
pub(crate) fn inline_cross_links(
    ui: &mut Ui,
    nav_target: &mut Option<usize>,
    links: &[(usize, &str)],
) {
    if links.is_empty() {
        return;
    }
    let p = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::s4());
    ui.label(rich("See also", TextStyle::Overline).color(p.text_tertiary));
    ui.add_space(space::s1());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::s2(), space::s1());
        for &(target_idx, title) in links {
            if ui.add(Button::ghost(title)).clicked() {
                *nav_target = Some(target_idx);
            }
        }
    });
}

/// Section heading of an article (listed in "On this page").
pub(crate) fn section_overline(ui: &mut Ui, title: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::s4());
    outline_heading(ui, 0, title);
    ui.add(egui::Label::new(rich(title, TextStyle::Title).color(p.text_primary)).wrap());
    ui.add_space(space::s1());
}

pub(crate) fn paragraph(ui: &mut Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.add(egui::Label::new(rich(text, TextStyle::DialogBody).color(p.text_secondary)).wrap());
}

fn render_kbd_group(ui: &mut Ui, expr: &str) {
    if expr.contains(" / ") {
        let formatted: Vec<String> = expr
            .split(" / ")
            .map(|part| neoflash_ui::components::platform_shortcut(part.trim()).into_owned())
            .collect();
        kbd(ui, &formatted.join(" / "), false);
    } else {
        kbd(ui, expr, false);
    }
}

pub(crate) fn sub_bullet(ui: &mut Ui, lead: &str, shortcut: Option<&str>, body: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::s1();
        ui.label(rich("•", TextStyle::Body).color(p.text_tertiary));
        ui.label(rich(lead, TextStyle::BodyStrong).color(p.text_primary));
        if let Some(sc) = shortcut {
            render_kbd_group(ui, sc);
        }
        ui.label(rich("—", TextStyle::Body).color(p.text_tertiary));
        ui.label(rich(body, TextStyle::Body).color(p.text_secondary));
    });
}

/// A topic inside a section: heading, its shortcut and context note, then one line per fact.
/// `icon` and `badge_kind` belonged to the former card and carry no information.
pub(crate) fn mode_card(
    ui: &mut Ui,
    _icon: Option<Icon>,
    mode_name: &str,
    badge_text: &str,
    _badge_kind: BadgeType,
    pointer_or_hotkey: &str,
    behaviors: &[&str],
) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::s3());
    outline_heading(ui, 1, mode_name);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::s2(), space::s1());
        ui.add(
            egui::Label::new(rich(mode_name, TextStyle::BodyStrong).color(p.text_primary)).wrap(),
        );
        if !pointer_or_hotkey.is_empty() {
            render_kbd_group(ui, pointer_or_hotkey);
        }
        // The former dialect badges (`[AS3]`) read as plain context notes.
        let note = badge_text.replace(['[', ']'], "");
        if !note.is_empty() {
            ui.label(rich(note, TextStyle::Small).color(p.text_tertiary));
        }
    });
    ui.add_space(space::s1());
    for b in behaviors {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = space::s2();
            ui.label(rich("•", TextStyle::Body).color(p.text_tertiary));
            let bullet_w = (ui.available_width() - space::s1()).max(80.0);
            ui.vertical(|ui| {
                ui.set_max_width(bullet_w);
                ui.add(egui::Label::new(rich(*b, TextStyle::Body).color(p.text_secondary)).wrap());
            });
        });
    }
}

/// Allocates a themed illustration canvas box (`bg_canvas` with `border_subtle` frame) and passes
/// `(painter, rect)` to `draw`.
pub(crate) fn illustration_canvas(
    ui: &mut Ui,
    height: f32,
    caption_text: &str,
    draw: impl FnOnce(&egui::Painter, &egui::Context, Rect),
) {
    let p = neoflash_ui::palette(ui.ctx());
    let width = ui.available_width().max(260.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let r = egui::CornerRadius::same(radius::lg() as u8);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, r, p.bg_canvas);
    draw(&painter, ui.ctx(), rect);
    painter.rect_stroke(
        rect,
        r,
        Stroke::new(1.0_f32, p.border_subtle),
        StrokeKind::Inside,
    );
    ui.add(egui::Label::new(rich(caption_text, TextStyle::Small).color(p.text_tertiary)).wrap());
    ui.add_space(space::s2());
}

// ── Chapter 1: Workspace & Stage ──

fn chapter_workspace_and_stage(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "NeoFlash combines the classic frame-and-symbol authoring model of Adobe Flash Professional (CS4–CS6) with a deterministic Rust engine and native HTML5 / WebAssembly publishing.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Stage, Rulers, Guides, Grid & Snap Ring",
    );
    illustration_canvas(
        ui,
        176.0,
        "Illustration 1.1: Stage (bg/stage) inside the Pasteboard (bg/canvas), overlaid with NF2 Rulers, Ruler Corner (0,0), Cyan Guides, Stage Grid, and the Selection Snap Ring.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let inner = rect.shrink(6.0);
            let page =
                Rect::from_min_max(inner.min + vec2(38.0, 30.0), inner.max - vec2(24.0, 14.0));
            painter.rect_filled(page, 0.0, p.bg_stage);
            paint_stage_grid(painter, ctx, page, 1.0, vec2(20.0, 20.0), None);
            painter.rect_stroke(
                page,
                0.0,
                Stroke::new(1.0_f32, p.border_default),
                StrokeKind::Outside,
            );

            // Rulers & Corner
            let corner = Rect::from_min_size(inner.min, egui::Vec2::splat(STAGE_RULER));
            let h_ruler = Rect::from_min_max(
                egui::pos2(corner.right(), inner.top()),
                egui::pos2(inner.right(), corner.bottom()),
            );
            let v_ruler = Rect::from_min_max(
                egui::pos2(inner.left(), corner.bottom()),
                egui::pos2(corner.right(), inner.bottom()),
            );
            paint_stage_ruler(
                painter,
                ctx,
                h_ruler,
                Orientation::Horizontal,
                page.left(),
                1.0,
                Some(100.0),
            );
            paint_stage_ruler(
                painter,
                ctx,
                v_ruler,
                Orientation::Vertical,
                page.top(),
                1.0,
                Some(50.0),
            );
            paint_ruler_corner(painter, ctx, corner);

            // Guides: one selected horizontal guide, one locked vertical guide
            let extent = Rect::from_min_max(corner.max, inner.max);
            let gy = page.top() + 56.0;
            let gx = page.left() + 180.0;
            paint_guide(
                painter,
                ctx,
                Orientation::Horizontal,
                gy,
                extent,
                GuideState::Selected,
                None,
            );
            paint_guide(
                painter,
                ctx,
                Orientation::Vertical,
                gx,
                extent,
                GuideState::Locked,
                None,
            );

            // Symbol instance snapped to the guide intersection
            let obj = Rect::from_center_size(egui::pos2(gx, gy), vec2(76.0, 44.0));
            painter.rect_filled(obj, 4.0, p.accent_subtle);
            paint_selection_box(painter, ctx, obj, SelectionKind::Symbol);
            paint_snap_ring(painter, ctx, egui::pos2(gx, gy), true);

            paint_clipped_text(
                painter,
                ctx,
                page.shrink(4.0),
                egui::pos2(gx + 46.0, gy - 18.0),
                egui::Align2::LEFT_CENTER,
                "Snapped to Guide + Grid (Snap Ring enlarged)",
                TextStyle::MonoSmall,
                p.text_primary,
            );
        },
    );

    section_overline(ui, "Stage Navigation & Viewport Modes");
    mode_card(
        ui,
        Some(i16::HAND),
        "Hand Tool (H) & Temporary Spacebar Pan",
        "Navigation",
        BadgeType::Accent,
        "H / Space",
        &[
            "Hold Spacebar while any drawing or selection tool is active to temporarily pan the Stage canvas; releasing Spacebar immediately restores your active tool.",
            "Double-click the Hand Tool icon in the Tools panel (or choose View ▸ Magnification ▸ Fit in Window) to center and fit the Stage into the viewport.",
        ],
    );
    mode_card(
        ui,
        Some(i16::ZOOMIN),
        "Zoom Tool (Z) — Enlarge (+) vs Reduce (-) & Marquee Zoom",
        "Magnification",
        BadgeType::Accent,
        if cfg!(target_os = "macos") {
            "Z / Option-click"
        } else {
            "Z / Alt-click"
        },
        &[
            "Enlarge Mode (+): Single-clicking zooms in 2× centered on the click point; dragging a marquee rectangle magnifies that exact region to fill the viewport.",
            if cfg!(target_os = "macos") {
                "Reduce Mode (-): Hold Option while clicking—or switch the modifier in Tool Options—to zoom out."
            } else {
                "Reduce Mode (-): Hold Alt while clicking—or switch the modifier in Tool Options—to zoom out."
            },
            "Double-clicking the Zoom Tool icon in the Tools rail resets magnification to 100%, and View ▸ Magnification ▸ Show All frames every object on both the Stage and Pasteboard.",
        ],
    );
    mode_card(
        ui,
        Some(i16::SNAP_GUIDES),
        "Snapping Priority Hierarchy & Ruler Origin",
        "Precision",
        BadgeType::Accent,
        "View ▸ Snapping",
        &[
            "Deterministic Snap Priority: When multiple snap targets are enabled, NeoFlash evaluates them in strict priority order: 1) Ruler Guides → 2) Objects (vertices/centres/paths) → 3) Stage Grid → 4) Pixels.",
            "Ruler Origin (0,0): Drag from the top-left 20×20 Ruler Corner onto the Stage to relocate the coordinate origin; double-click the Ruler Corner to reset (0,0) to the top-left corner of the Stage (or symbol registration point inside Symbol-Editing Mode).",
            "Focus Mode (Tab): Press Tab (when not editing text) to hide all docked panels and maximize the Stage canvas; press Tab again to restore your workspace layout.",
        ],
    );
    mode_card(
        ui,
        Some(i16::DESKTOP),
        "Workspaces (Including Video Edit Montage), 4 NF2 Themes & 3 Density Profiles",
        "Layout & Theme",
        BadgeType::Accent,
        "Window ▸ Workspaces",
        &[
            "Workspace Presets: Switch between Animator, Classic, Designer, Developer, Small Screen, and Video edit (non-linear video/audio montage with Layer frame previews, Properties + Scene, Library + Project, and Timeline + Output + Actions).",
            "4 Interface Themes & UI Studio: Choose Dark, Light, Mid Neutral Gray, or Dark Graphite under Window ▸ Appearance, and inspect all four side by side in NeoFlash UI Studio (Compare 4 Themes).",
            "3 Density Profiles: Comfortable, Compact, and Classic Flash scale top-bar, toolbar, and Timeline row metrics proportionally without clipping.",
        ],
    );
}

// ── Chapter 2: Drawing Modes & Objects ──

fn chapter_drawing_modes_and_objects(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "Understanding how Flash and NeoFlash represent vector artwork on a layer is essential. The same tool (such as Rectangle, Oval, Brush, or Line) behaves differently depending on whether Merge Drawing, Object Drawing (J), or Primitive Mode is active.",
    );

    section_overline(
        ui,
        "Interactive Diagram — The 5 Graphic Object Regimes on Stage",
    );
    illustration_canvas(
        ui,
        158.0,
        "Illustration 2.1: Visual comparison of selection chrome across the 5 Stage object regimes: (1) Merge Drawing 4pt crosshatch, (2) Drawing Object / Group box, (3) Parametric Primitive handles, (4) Symbol Instance registration + transform point, (5) Text block handle.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let cols = 5;
            let pad = 10.0;
            let card_w = (rect.width() - pad * (cols as f32 + 1.0)) / cols as f32;
            let top = rect.top() + 12.0;
            let box_h = 84.0;

            let titles = [
                ("1. Merge Shape", "Planar split / hatch"),
                ("2. Object (J)", "Isolated container"),
                ("3. Primitive", "Live corner/angle"),
                ("4. Symbol (F8)", "+ Reg & ○ Pivot"),
                ("5. Text Block", "○ Auto / □ Fixed"),
            ];

            for (i, (title, sub)) in titles.iter().enumerate() {
                let x = rect.left() + pad + i as f32 * (card_w + pad);
                let cell = Rect::from_min_size(egui::pos2(x, top), vec2(card_w, box_h));
                painter.rect_filled(cell, 4.0, p.bg_panel);
                let shape = cell.shrink2(vec2(18.0, 18.0));
                painter.rect_filled(shape, 3.0, p.accent_subtle);

                match i {
                    0 => {
                        paint_shape_selection(painter, ctx, shape);
                    }
                    1 => {
                        paint_selection_box(painter, ctx, shape, SelectionKind::Group);
                    }
                    2 => {
                        paint_selection_box(painter, ctx, shape, SelectionKind::Primitive);
                    }
                    3 => {
                        paint_selection_box(painter, ctx, shape, SelectionKind::Symbol);
                    }
                    _ => {
                        paint_selection_box(painter, ctx, shape, SelectionKind::TextStatic);
                    }
                }

                let label_clip = Rect::from_min_max(
                    egui::pos2(cell.left() - 2.0, cell.bottom() + 2.0),
                    egui::pos2(cell.right() + 2.0, rect.bottom() - 2.0),
                );
                paint_clipped_text(
                    painter,
                    ctx,
                    label_clip,
                    egui::pos2(cell.center().x, cell.bottom() + 14.0),
                    egui::Align2::CENTER_CENTER,
                    title,
                    TextStyle::BodyStrong,
                    p.text_primary,
                );
                paint_clipped_text(
                    painter,
                    ctx,
                    label_clip,
                    egui::pos2(cell.center().x, cell.bottom() + 28.0),
                    egui::Align2::CENTER_CENTER,
                    sub,
                    TextStyle::Small,
                    p.text_secondary,
                );
            }
        },
    );

    section_overline(
        ui,
        "Per-Mode Behavior: Merge vs Object Drawing vs Primitives",
    );
    mode_card(
        ui,
        Some(i16::SCISSORS),
        "Mode 1: Merge Drawing Mode (Default when Object Drawing J is Off)",
        "Planar Topology",
        BadgeType::Warning,
        "J = Off",
        &[
            "Same-Colour Union: Drawing or moving a fill over another fill of the exact same colour on the same layer permanently merges them into a single unified contour.",
            "Different-Colour Cutout (Cookie-Cutter): Drawing a shape of a different colour over an existing fill, deselected and then dragged away, punches out the overlapped area from the underlying shape.",
            "Stroke Splitting: Overlapping lines automatically split at their intersection vertices so individual line segments can be selected and deleted.",
            "Partial Marquee & Lasso Slicing: Dragging a Selection marquee or Lasso loop across half of a Merge shape selects only that region (highlighted with the 4 pt two-tone hatch) and slices the fill cleanly when moved.",
        ],
    );
    mode_card(
        ui,
        Some(i16::RECT),
        "Mode 2: Object Drawing Mode (Toggle J in Tools Panel)",
        "Non-Destructive",
        BadgeType::Accent,
        "J = On",
        &[
            "Each stroke or shape drawn while Object Drawing (J) is active is wrapped in an independent Drawing Object container with an accent bounding box.",
            "Overlapping Drawing Objects on the same layer never split or punch holes in one another.",
            "Double-clicking a Drawing Object enters its isolated editing context; Modify ▸ Break Apart unpacks it back into raw Merge Drawing geometry.",
        ],
    );
    mode_card(
        ui,
        Some(i16::OVAL),
        "Mode 3: Parametric Shape Primitives (Rectangle Primitive & Oval Primitive)",
        "Live Parameters",
        BadgeType::Accent,
        "R / O",
        &[
            "Rectangle Primitive: Retains live, non-destructive Corner Radius controls (including negative values for inverted concave corners) that can be dragged on Stage or edited in Properties at any time.",
            "Oval Primitive: Retains live Start Angle (0–360°), End Angle (0–360°), Inner Radius (0–99% donut hole), and Close Path toggles for pie wedges, rings, and arcs.",
        ],
    );
    mode_card(
        ui,
        Some(i16::LAYERS),
        "Mode 4: Boolean Combine Objects & Break Apart Across Types",
        "Modify Menu",
        BadgeType::Neutral,
        "⌘B / ⌘G",
        &[
            "Modify ▸ Combine Objects: Union (merges into one Drawing Object), Intersect (keeps overlapping region with top object's style), Punch (top object cuts through all underlying objects and disappears), Crop (clips underlying objects to top object's silhouette).",
            "Modify ▸ Break Apart is context-sensitive: (1) On a Group or Drawing Object → unwraps to Merge shapes; (2) On a Symbol Instance → detaches from Library into current-frame elements; (3) On a Bitmap → converts to a vector shape painted with a Bitmap Fill; (4) On Text → 1st Break Apart splits string into individual character text boxes, 2nd Break Apart converts glyphs into editable Bézier outlines.",
        ],
    );
}

// ── Chapter 3: Selection, Subselection & Transform ──

fn chapter_selection_and_transform(ui: &mut Ui, state: &mut HelpManualState) {
    let tabs = [
        (0, "Selection (V) & Subselection (A)"),
        (1, "Free Transform (Q) — 4 Sub-Modes"),
        (2, "Lasso (L) & Magic Wand"),
    ];
    c::Segmented::show(ui, &mut state.diagram_tab, &tabs, false);
    ui.add_space(space::s2());

    match state.diagram_tab {
        0 => {
            illustration_canvas(
                ui,
                148.0,
                if cfg!(target_os = "macos") {
                    "Illustration 3.1: Left — Selection Tool (V) reshaping a curve segment by direct edge drag vs Option-dragging to split a new corner vertex. Right — Subselection Tool (A) exposing hollow/filled anchor squares and Bézier tangent handles."
                } else {
                    "Illustration 3.1: Left — Selection Tool (V) reshaping a curve segment by direct edge drag vs Alt-dragging to split a new corner vertex. Right — Subselection Tool (A) exposing hollow/filled anchor squares and Bézier tangent handles."
                },
                |painter, ctx, rect| {
                    let p = neoflash_ui::palette(ctx);
                    let mid_x = rect.center().x;
                    painter.vline(mid_x, rect.y_range(), Stroke::new(1.0_f32, p.border_subtle));

                    // Left: Selection Tool (V) curve bend
                    let l_center = egui::pos2((rect.left() + mid_x) * 0.5, rect.center().y + 6.0);
                    let a = l_center + vec2(-90.0, 26.0);
                    let b = l_center + vec2(90.0, 26.0);
                    let ctrl = l_center + vec2(0.0, -42.0);
                    let pts: Vec<Pos2> = (0..=32)
                        .map(|i| {
                            let t = i as f32 / 32.0;
                            let u = 1.0 - t;
                            egui::pos2(
                                u * u * a.x + 2.0 * u * t * ctrl.x + t * t * b.x,
                                u * u * a.y + 2.0 * u * t * ctrl.y + t * t * b.y,
                            )
                        })
                        .collect();
                    painter.add(egui::Shape::line(
                        pts,
                        Stroke::new(2.0_f32, p.accent_default),
                    ));
                    painter.circle_filled(l_center + vec2(0.0, -8.0), 4.0, p.warning_default);
                    let left_col =
                        Rect::from_min_max(rect.min, egui::pos2(mid_x - 4.0, rect.bottom()));
                    let right_col =
                        Rect::from_min_max(egui::pos2(mid_x + 4.0, rect.top()), rect.max);
                    paint_clipped_text(
                        painter,
                        ctx,
                        left_col,
                        egui::pos2(l_center.x, rect.top() + 16.0),
                        egui::Align2::CENTER_CENTER,
                        if cfg!(target_os = "macos") {
                            "Selection (V): Drag edge = Smooth Curve | Option+Drag = New Corner"
                        } else {
                            "Selection (V): Drag edge = Smooth Curve | Alt+Drag = New Corner"
                        },
                        TextStyle::Small,
                        p.text_primary,
                    );

                    // Right: Subselection Tool (A) anchors & handles
                    let r_center = egui::pos2((mid_x + rect.right()) * 0.5, rect.center().y + 10.0);
                    let p0 = r_center + vec2(-85.0, 22.0);
                    let p1 = r_center + vec2(0.0, -20.0);
                    let p2 = r_center + vec2(85.0, 22.0);
                    painter.line_segment([p0, p1], Stroke::new(1.5_f32, p.accent_default));
                    painter.line_segment([p1, p2], Stroke::new(1.5_f32, p.accent_default));
                    paint_bezier_handle(painter, ctx, p1, p1 + vec2(-48.0, -18.0));
                    paint_bezier_handle(painter, ctx, p1, p1 + vec2(48.0, 18.0));
                    paint_anchor(painter, ctx, p0, AnchorState::Unselected);
                    paint_anchor(painter, ctx, p1, AnchorState::Selected);
                    paint_anchor(painter, ctx, p2, AnchorState::Hover);
                    paint_clipped_text(
                        painter,
                        ctx,
                        right_col,
                        egui::pos2(r_center.x, rect.top() + 16.0),
                        egui::Align2::CENTER_CENTER,
                        if cfg!(target_os = "macos") {
                            "Subselection (A): □ Unselected · ■ Selected · Option+Drag = Corner"
                        } else {
                            "Subselection (A): □ Unselected · ■ Selected · Alt+Drag = Corner"
                        },
                        TextStyle::Small,
                        p.text_primary,
                    );
                },
            );
        }
        1 => {
            illustration_canvas(
                ui,
                148.0,
                "Illustration 3.2: Free Transform Tool (Q) sub-modes: (1) Scale 8 square handles, (2) Rotate & Skew round corners + edge bars, (3) Distort / Shift-Perspective Taper, (4) Envelope 8-anchor / 16-handle bicubic Coons mesh.",
                |painter, ctx, rect| {
                    let p = neoflash_ui::palette(ctx);
                    let cols = 4;
                    let pad = 12.0;
                    let w = (rect.width() - pad * (cols as f32 + 1.0)) / cols as f32;
                    for i in 0..cols {
                        let x = rect.left() + pad + i as f32 * (w + pad);
                        let cell =
                            Rect::from_min_size(egui::pos2(x, rect.top() + 26.0), vec2(w, 86.0));
                        let b = cell.shrink2(vec2(26.0, 16.0));
                        painter.rect_filled(b, 2.0, p.accent_subtle);
                        let label = match i {
                            0 => {
                                paint_transform_box(painter, ctx, b, TransformMode::Scale);
                                if cfg!(target_os = "macos") {
                                    "1. Scale (Shift Prop, Opt Corner)"
                                } else {
                                    "1. Scale (Shift Prop, Alt Corner)"
                                }
                            }
                            1 => {
                                paint_transform_box(painter, ctx, b, TransformMode::RotateSkew);
                                "2. Rotate & Skew (○ / ▬)"
                            }
                            2 => {
                                let quad = vec![
                                    b.left_top() + vec2(12.0, 0.0),
                                    b.right_top() + vec2(-12.0, 0.0),
                                    b.right_bottom() + vec2(8.0, 0.0),
                                    b.left_bottom() + vec2(-8.0, 0.0),
                                    b.left_top() + vec2(12.0, 0.0),
                                ];
                                painter.add(egui::Shape::line(
                                    quad.clone(),
                                    Stroke::new(1.0_f32, p.accent_default),
                                ));
                                for pt in &quad[..4] {
                                    paint_anchor(painter, ctx, *pt, AnchorState::Selected);
                                }
                                if cfg!(target_os = "macos") {
                                    "3. Distort & Shift+Cmd Taper"
                                } else {
                                    "3. Distort & Ctrl+Shift Taper"
                                }
                            }
                            _ => {
                                paint_transform_box(painter, ctx, b, TransformMode::Scale);
                                paint_bezier_handle(
                                    painter,
                                    ctx,
                                    b.center_top(),
                                    b.center_top() + vec2(-18.0, -10.0),
                                );
                                paint_bezier_handle(
                                    painter,
                                    ctx,
                                    b.center_top(),
                                    b.center_top() + vec2(18.0, 10.0),
                                );
                                "4. Envelope (8 pts / 16 tangents)"
                            }
                        };
                        let col_clip = Rect::from_min_max(
                            egui::pos2(cell.left() - 2.0, rect.top()),
                            egui::pos2(cell.right() + 2.0, rect.bottom()),
                        );
                        paint_clipped_text(
                            painter,
                            ctx,
                            col_clip,
                            egui::pos2(cell.center().x, rect.top() + 14.0),
                            egui::Align2::CENTER_CENTER,
                            label,
                            TextStyle::Small,
                            p.text_primary,
                        );
                    }
                },
            );
        }
        _ => {
            illustration_canvas(
                ui,
                148.0,
                if cfg!(target_os = "macos") {
                    "Illustration 3.3: Lasso Tool (L) modes: Freehand loop, Polygon straight-edge clicks, Hybrid (hold Option during Freehand drag), and Magic Wand contiguous colour threshold."
                } else {
                    "Illustration 3.3: Lasso Tool (L) modes: Freehand loop, Polygon straight-edge clicks, Hybrid (hold Alt during Freehand drag), and Magic Wand contiguous colour threshold."
                },
                |painter, ctx, rect| {
                    let p = neoflash_ui::palette(ctx);
                    let mid_x = rect.center().x;
                    let left_col =
                        Rect::from_min_max(rect.min, egui::pos2(mid_x - 4.0, rect.bottom()));
                    let right_col =
                        Rect::from_min_max(egui::pos2(mid_x + 4.0, rect.top()), rect.max);
                    let c1 = egui::pos2(rect.left() + rect.width() * 0.28, rect.center().y + 6.0);
                    let c2 = egui::pos2(rect.left() + rect.width() * 0.72, rect.center().y + 6.0);
                    let poly = vec![
                        c1 + vec2(-70.0, -28.0),
                        c1 + vec2(10.0, -36.0),
                        c1 + vec2(74.0, 10.0),
                        c1 + vec2(-20.0, 38.0),
                        c1 + vec2(-70.0, -28.0),
                    ];
                    painter.extend(egui::Shape::dashed_line(
                        &poly,
                        Stroke::new(1.2_f32, p.accent_default),
                        4.0,
                        3.0,
                    ));
                    for pt in &poly[..4] {
                        paint_anchor(painter, ctx, *pt, AnchorState::Unselected);
                    }
                    paint_clipped_text(
                        painter,
                        ctx,
                        left_col,
                        egui::pos2(c1.x, rect.top() + 16.0),
                        egui::Align2::CENTER_CENTER,
                        if cfg!(target_os = "macos") {
                            "Lasso Freehand + Hold Option for Straight Polygon Edges"
                        } else {
                            "Lasso Freehand + Hold Alt for Straight Polygon Edges"
                        },
                        TextStyle::Small,
                        p.text_primary,
                    );

                    let wand_rect = Rect::from_center_size(c2, vec2(130.0, 68.0));
                    painter.rect_filled(wand_rect, 4.0, p.accent_subtle);
                    paint_shape_selection(painter, ctx, wand_rect.shrink(14.0));
                    paint_clipped_text(
                        painter,
                        ctx,
                        right_col,
                        egui::pos2(c2.x, rect.top() + 16.0),
                        egui::Align2::CENTER_CENTER,
                        "Magic Wand: Selects Bitmap Pixels Within Threshold (0–200)",
                        TextStyle::Small,
                        p.text_primary,
                    );
                },
            );
        }
    }

    section_overline(ui, "Detailed Tool Modes & Pointer States");
    mode_card(
        ui,
        Some(i16::SELECT),
        if cfg!(target_os = "macos") {
            "Selection Tool (V) — 6 Contextual Pointer States & Temporary Command Override"
        } else {
            "Selection Tool (V) — 6 Contextual Pointer States & Temporary Ctrl Override"
        },
        "Primary Tool",
        BadgeType::Accent,
        if cfg!(target_os = "macos") {
            "V / Hold Command"
        } else {
            "V / Hold Ctrl"
        },
        &[
            "1) Empty Stage Marquee: Dragging on empty Stage draws a selection marquee (`NF2/Marquee`). Single click on a fill selects the fill; double-clicking a fill selects both the fill and its enclosing stroke; double-clicking a stroke selects all connected stroke segments.",
            if cfg!(target_os = "macos") {
                "2) Move / Duplicate Cursor (Arrow + Black Box): Dragging inside a selected item moves it (Shift constrains to 45° axes; Option-drag duplicates the selection)."
            } else {
                "2) Move / Duplicate Cursor (Arrow + Black Box): Dragging inside a selected item moves it (Shift constrains to 45° axes; Alt-drag duplicates the selection)."
            },
            "3) Curve Reshape Cursor (Arrow + Arc): Hovering over an unselected smooth edge and dragging bends the curve without moving its endpoint corners.",
            "4) Corner / Endpoint Cursor (Arrow + Right Angle): Hovering over a corner vertex or open line end moves that vertex.",
            if cfg!(target_os = "macos") {
                "5) Split New Corner (Option-drag on Curve): Holding Option while dragging an unselected curve segment splits the segment and pulls out a sharp corner vertex."
            } else {
                "5) Split New Corner (Alt-drag on Curve): Holding Alt while dragging an unselected curve segment splits the segment and pulls out a sharp corner vertex."
            },
            "6) Smooth & Straighten Modifiers: Clicking Smooth or Straighten in Tool Options progressively refines selected strokes (repeated Straighten triggers geometric shape recognition for detached ovals, rectangles, and triangles).",
        ],
    );
    mode_card(
        ui,
        Some(i16::SUBSELECT),
        "Subselection Tool (A) — Direct Node, Tangent & Curve Editing",
        "Bézier Nodes",
        BadgeType::Accent,
        "A",
        &[
            "Clicking a path outline reveals hollow square Anchor Points (`□`). Clicking an anchor fills it (`■`) and exposes its Bézier tangent handles (`●`).",
            if cfg!(target_os = "macos") {
                "Dragging a tangent handle rotates both handles symmetrically around a smooth point; holding Option breaks symmetry so each handle moves independently (corner point)."
            } else {
                "Dragging a tangent handle rotates both handles symmetrically around a smooth point; holding Alt breaks symmetry so each handle moves independently (corner point)."
            },
            "Deleting Nodes: Pressing Delete/Backspace in Subselection removes the selected anchor AND its adjoining curve segments (opening the path), whereas clicking the anchor with the Delete Anchor Point Tool (-) heals the path into a single continuous curve.",
        ],
    );
    mode_card(
        ui,
        Some(i16::TRANSFORM),
        "Free Transform Tool (Q) — 4 Sub-Modes & Pivot Point",
        "Transform",
        BadgeType::Accent,
        "Q",
        &[
            "Transformation Point (○): Drag the white centre circle to relocate the pivot for rotation and scaling; double-click the circle to snap it back to the object's geometric centre.",
            if cfg!(target_os = "macos") {
                "Sub-Mode 1 — Rotate and Skew: Corner handles only rotate (Shift = 45° steps, Option = pivot around opposite corner); edge handles only skew."
            } else {
                "Sub-Mode 1 — Rotate and Skew: Corner handles only rotate (Shift = 45° steps, Alt = pivot around opposite corner); edge handles only skew."
            },
            if cfg!(target_os = "macos") {
                "Sub-Mode 2 — Scale: Corner and edge handles only scale (Shift = proportional aspect ratio, Option = scale from opposite corner vs centre)."
            } else {
                "Sub-Mode 2 — Scale: Corner and edge handles only scale (Shift = proportional aspect ratio, Alt = scale from opposite corner vs centre)."
            },
            if cfg!(target_os = "macos") {
                "Sub-Mode 3 — Distort (Vector shapes): Drag any corner or edge freely (Command-drag in default mode); hold Shift+Command while dragging a corner to apply a symmetric perspective Taper (moving the dragged corner and the adjacent corner on the same axis equally in opposite directions)."
            } else {
                "Sub-Mode 3 — Distort (Vector shapes): Drag any corner or edge freely (Ctrl-drag in default mode); hold Ctrl+Shift while dragging a corner to apply a symmetric perspective Taper (moving the dragged corner and the adjacent corner on the same axis equally in opposite directions)."
            },
            "Sub-Mode 4 — Envelope (Vector shapes): Wraps the selection in an 8-point, 16-tangent-handle bicubic Coons patch for fluid non-linear warping.",
        ],
    );
}

// ── Chapter 4: Vector Drawing & Pen States ──

fn chapter_vector_and_pen_states(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "The Pen Tool (P) and geometric shape tools construct clean quadratic and cubic Bézier paths. In Flash Professional and NeoFlash, the Pen cursor changes badge to show which of the 9 path operations will occur on click.",
    );

    section_overline(
        ui,
        "Interactive Diagram — The 9 Pointer States of the Pen Tool (P)",
    );
    illustration_canvas(
        ui,
        152.0,
        "Illustration 4.1: All 9 Pen Tool (P) pointer badges documented in flash_reference.pdf (pp. 160–163) and how each modifies the active or selected Bézier path.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let badges = [
                ("1. Pen ×", "Start new path"),
                ("2. Pen", "Next point"),
                ("3. Pen +", "Insert on seg"),
                ("4. Pen −", "Smooth→Corner→Del"),
                ("5. Pen /", "Continue end"),
                ("6. Pen ○", "Close path"),
                ("7. Pen ⛓", "Join 2 paths"),
                ("8. Pen ∧", "Retract handle"),
                (
                    if cfg!(target_os = "macos") {
                        "9. Shift+C / Option"
                    } else {
                        "9. Shift+C / Alt"
                    },
                    "Convert point",
                ),
            ];
            let cols = 3;
            let rows = 3;
            let pad_x = 10.0;
            let pad_y = 8.0;
            let cw = (rect.width() - pad_x * (cols as f32 + 1.0)) / cols as f32;
            let ch = (rect.height() - pad_y * (rows as f32 + 1.0)) / rows as f32;
            for (idx, (code, desc)) in badges.iter().enumerate() {
                let r_idx = idx / cols;
                let c_idx = idx % cols;
                let x = rect.left() + pad_x + c_idx as f32 * (cw + pad_x);
                let y = rect.top() + pad_y + r_idx as f32 * (ch + pad_y);
                let cell = Rect::from_min_size(egui::pos2(x, y), vec2(cw, ch));
                painter.rect_filled(cell, 4.0, p.bg_panel);
                icons::paint(
                    painter,
                    ctx,
                    Rect::from_min_size(
                        cell.min + vec2(space::s2(), (ch - ICON_UI) * 0.5),
                        egui::Vec2::splat(ICON_UI),
                    ),
                    i16::PEN,
                    p.accent_default,
                );
                let text_clip = cell.shrink(4.0);
                paint_clipped_text(
                    painter,
                    ctx,
                    text_clip,
                    egui::pos2(cell.left() + 30.0, cell.center().y - 6.0),
                    egui::Align2::LEFT_CENTER,
                    code,
                    TextStyle::BodyStrong,
                    p.text_primary,
                );
                paint_clipped_text(
                    painter,
                    ctx,
                    text_clip,
                    egui::pos2(cell.left() + 30.0, cell.center().y + 7.0),
                    egui::Align2::LEFT_CENTER,
                    desc,
                    TextStyle::Small,
                    p.text_secondary,
                );
            }
        },
    );

    mode_card(
        ui,
        Some(i16::PEN),
        "Pen Tool Family — Two-Step Smooth Node Deletion & Open Paths",
        "9 Cursor States",
        BadgeType::Accent,
        "P / = / - / ⇧C",
        &[
            "Two-Step Smooth Point Deletion (Pen −): Clicking an existing smooth curve point with the Pen or Delete Anchor Point Tool (-) first converts it into a corner point (retracting its handles); clicking it a second time deletes the vertex while keeping the path closed/continuous.",
            if cfg!(target_os = "macos") {
                "Finishing an Open Path: Double-click the last point, press Escape, or Command-click away from the path to leave a stroke open without closing its fill."
            } else {
                "Finishing an Open Path: Double-click the last point, press Escape, or Ctrl-click away from the path to leave a stroke open without closing its fill."
            },
            if cfg!(target_os = "macos") {
                "Modifier Keys While Drawing: Hold Shift to constrain new anchors or tangent handles to 45° increments; hold Option to split paired tangent handles into an acute V-corner; hold Command to temporarily switch to Subselection."
            } else {
                "Modifier Keys While Drawing: Hold Shift to constrain new anchors or tangent handles to 45° increments; hold Alt to split paired tangent handles into an acute V-corner; hold Ctrl to temporarily switch to Subselection."
            },
        ],
    );
    mode_card(
        ui,
        Some(i16::PENCIL),
        "Pencil Tool (Y) — Straighten, Smooth & Ink Modes",
        "Freehand Stroke",
        BadgeType::Accent,
        "Y",
        &[
            "1) Straighten Mode: Straightens jittery segments and recognizes hand-drawn triangles, rectangles, squares, ovals, and circles, converting them into crisp geometric primitives.",
            "2) Smooth Mode: Fits smooth Bézier curves using the Smoothing slider (0–100, default 50) in the Properties inspector.",
            "3) Ink Mode: Disables shape recognition and heavy smoothing (the Smoothing slider is disabled) to preserve raw hand-drawn linework.",
        ],
    );
    mode_card(
        ui,
        Some(i16::RECT),
        "Line (N), Rectangle (R), Oval (O) & PolyStar — Interactive Drag Modifiers",
        "Geometric",
        BadgeType::Accent,
        "N / R / O",
        &[
            "Up / Down Arrow Keys While Dragging Rectangle: Adjusts the rounded corner radius live before releasing the mouse button.",
            if cfg!(target_os = "macos") {
                "Option + Single Click on Stage: Opens the numeric Rectangle / Oval Settings dialog to enter exact pixel Width, Height, Corner Radius, and Draw from Center."
            } else {
                "Alt + Single Click on Stage: Opens the numeric Rectangle / Oval Settings dialog to enter exact pixel Width, Height, Corner Radius, and Draw from Center."
            },
            "PolyStar Tool (under Rectangle flyout): Click Options… in Properties to choose Polygon or Star, Number of Sides (3–32), and Star Point Size (0.00–1.00; values closer to 0 produce sharper needle points).",
        ],
    );
}

// ── Chapter 5: Brush, Eraser & Deco ──

fn chapter_brush_eraser_and_deco(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "Unlike the Pencil Tool (which creates centre-line strokes), the Brush Tool (B) paints closed filled vector contours. Its 5 Paint Modes control which existing z-planes on the active layer are affected.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Brush Stroke, Gradient Transform (5 Handles) & Paint Modes",
    );
    illustration_canvas(
        ui,
        152.0,
        "Illustration 5.1: Left — NF2 Brush calligraphic stroke preview. Centre — Gradient Transform Tool (F) showing all 5 radial handles (Centre, Focal Point ▼, Width □, Size ○, Rotation ●). Right — The 5 Brush Paint Modes.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let w3 = rect.width() / 3.0;
            let r1 = Rect::from_min_max(rect.min, egui::pos2(rect.left() + w3, rect.bottom()));
            let r2 = Rect::from_min_max(
                egui::pos2(r1.right(), rect.top()),
                egui::pos2(r1.right() + w3, rect.bottom()),
            );
            let r3 = Rect::from_min_max(egui::pos2(r2.right(), rect.top()), rect.max);
            painter.vline(
                r1.right(),
                rect.y_range(),
                Stroke::new(1.0_f32, p.border_subtle),
            );
            painter.vline(
                r2.right(),
                rect.y_range(),
                Stroke::new(1.0_f32, p.border_subtle),
            );

            // Left: Brush stroke sample
            let stroke_box = r1.shrink2(vec2(18.0, 32.0));
            brush_stroke_sample(painter, stroke_box, BrushStroke::Chisel, 14.0);
            paint_clipped_text(
                painter,
                ctx,
                r1.shrink(4.0),
                egui::pos2(r1.center().x, r1.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "Brush (B): Closed Vector Fill",
                TextStyle::Small,
                p.text_primary,
            );
            paint_clipped_text(
                painter,
                ctx,
                r1.shrink(4.0),
                egui::pos2(r1.center().x, r1.bottom() - 16.0),
                egui::Align2::CENTER_CENTER,
                if cfg!(target_os = "macos") {
                    "[ / ] or Option+Cmd+Drag = Resize"
                } else {
                    "[ / ] or Ctrl+Alt+Drag = Resize"
                },
                TextStyle::MonoSmall,
                p.text_secondary,
            );

            // Centre: Gradient Transform 5 handles
            let g_center = r2.center() + vec2(0.0, 6.0);
            painter.circle_filled(g_center, 42.0, p.accent_subtle);
            paint_gradient_transform(painter, ctx, g_center, 42.0, -0.35);
            paint_clipped_text(
                painter,
                ctx,
                r2.shrink(4.0),
                egui::pos2(r2.center().x, r2.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "Gradient Transform (F): 5 Handles",
                TextStyle::Small,
                p.text_primary,
            );

            // Right: 5 Paint Modes summary
            let modes = [
                "1. Paint Normal (Over all)",
                "2. Paint Fills (Under strokes)",
                "3. Paint Behind (Behind all)",
                "4. Paint Selection (Selected only)",
                "5. Paint Inside (Start fill only)",
            ];
            for (i, m) in modes.iter().enumerate() {
                paint_clipped_text(
                    painter,
                    ctx,
                    r3.shrink(4.0),
                    egui::pos2(r3.left() + 14.0, r3.top() + 22.0 + i as f32 * 24.0),
                    egui::Align2::LEFT_CENTER,
                    m,
                    TextStyle::Small,
                    p.text_primary,
                );
            }
        },
    );

    mode_card(
        ui,
        Some(i16::BRUSH),
        "Brush Tool (B) — All 5 Paint Modes, Lock Fill & Stage Zoom Invariant",
        "5 Paint Modes",
        BadgeType::Accent,
        "B / [ / ]",
        &[
            "1) Paint Normal: Paints over both lines and fills on the active layer.",
            "2) Paint Fills: Paints over existing fills and empty areas, leaving all vector strokes untouched.",
            "3) Paint Behind: Paints only in empty areas of the Stage behind all existing strokes and fills on the layer.",
            "4) Paint Selection: Restricts brush strokes strictly to currently selected fills, ignoring unselected regions and strokes.",
            "5) Paint Inside: Restricts the stroke to the single enclosed fill where the mouse button was first pressed (starting on empty Stage paints only over empty Stage).",
            "Constant Screen Size Invariant: Brush tip size stays constant in screen pixels when zooming the Stage—painting at 50% zoom produces a stroke twice as wide in Stage units as painting at 100% zoom.",
        ],
    );
    mode_card(
        ui,
        Some(i16::ERASER),
        "Eraser Tool (E) — 5 Erase Modes, Faucet Modifier & Double-Click Clear",
        "5 Erase Modes",
        BadgeType::Warning,
        "E",
        &[
            "5 Erase Modes: Erase Normal (strokes + fills), Erase Fills (fills only; strokes untouched), Erase Lines (strokes only; fills untouched), Erase Selected Fills (only pre-selected fills), and Erase Inside (only the fill where the drag began).",
            "Faucet Modifier: Single-click any stroke segment or enclosed fill to erase the entire connected segment or fill region at once.",
            "Double-Click Eraser Tool Icon: Immediately deletes all content on the Stage and Pasteboard in the current frame.",
        ],
    );
    mode_card(
        ui,
        Some(i16::BUCKET),
        "Paint Bucket (K), Ink Bottle (S), Eyedropper (I), Spray Brush & Deco (U)",
        "Fill & Procedural",
        BadgeType::Accent,
        "K / S / I / U",
        &[
            "Paint Bucket (K) Gap Size: Choose Don't Close Gaps, or Close Small / Medium / Large Gaps to bridge open contour gaps automatically; Lock Fill tiles a single shared gradient/bitmap matrix across multiple shapes.",
            "Eyedropper (I): Sampling a gradient fill copies the gradient into the active Fill well and automatically enables Lock Fill (`brush_lock_fill = true`).",
            "Deco Tool (U): Generates procedural vector patterns—Vine Fill (branching vines with leaves/flowers), Grid Fill (tiled symbols), and Symmetry Brush (Reflect across line/point, Rotate around, Grid translation), plus the 10 CS5 procedural brushes documented in the audit.",
        ],
    );
    mode_card(
        ui,
        Some(crate::ui_icons::Icon::Gradient.nf2(false)),
        "Gradient Transform Tool (F) & Color Mixer Overflow (Extend / Reflect / Repeat)",
        "Linear & Radial",
        BadgeType::Accent,
        "F / Shift+F9",
        &[
            "Linear Gradient Boundaries & Handles: Displays two parallel perpendicular boundary lines with a Center circle (moves origin), a Width square [→] on the right line (scales span width without rotating), and a Rotation circle at the top-right corner (Shift constrains to 45°).",
            "Radial Gradient Boundaries & Handles: Displays an elliptical boundary ring + horizontal diameter axis line with a Center circle, a Focal point inverted triangle ▽ on the axis, a Width square [→] (stretches X axis), a Size/Radius circle with inward arrow (scales XY proportionally), and a Rotation circle.",
            "Color Panel & Popover: Places Fill on the left and Stroke on the right, renders a visual Overflow dropdown (Extend, Reflect, Repeat) + Linear RGB toggle, and lets you click either the 20 px gradient bar or the 20 px pointer rail below it to insert and immediately edit color stops.",
        ],
    );
}

// ── Chapter 6: Symbols, 9-Slice & Library ──

fn chapter_symbols_9slice_and_library(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "Symbols (Convert to Symbol or New Symbol) are reusable master assets stored in the Library. Placing a symbol on the Stage creates a lightweight Instance that can have its own transform, Colour Effect, Blend Mode, Filters, and Playback mode.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Button 4-State Timeline & 9-Slice Scaling Grid",
    );
    illustration_canvas(
        ui,
        148.0,
        "Illustration 6.1: Left — Button Symbol's 4 dedicated interaction frames (Up, Over, Down, and invisible Hit area). Right — 9-Slice Scaling (scale9Grid) preserving corner geometry while stretching central regions.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let mid = rect.center().x;
            painter.vline(mid, rect.y_range(), Stroke::new(1.0_f32, p.border_subtle));

            // Left: Button 4 states (Up, Over, Down, Hit)
            let left_col = Rect::from_min_max(rect.min, egui::pos2(mid - 4.0, rect.bottom()));
            let right_col = Rect::from_min_max(egui::pos2(mid + 4.0, rect.top()), rect.max);
            let states = ["Up", "Over", "Down", "Hit"];
            let cell_w = 56.0;
            let start_x = (rect.left() + mid - cell_w * 4.0) * 0.5;
            let row_y = rect.center().y - 6.0;
            for (i, name) in states.iter().enumerate() {
                let c = Rect::from_min_size(
                    egui::pos2(start_x + i as f32 * cell_w, row_y),
                    vec2(cell_w, 32.0),
                );
                painter.rect_filled(
                    c,
                    0.0,
                    if i == 3 {
                        p.accent_subtle
                    } else {
                        p.timeline_static
                    },
                );
                painter.rect_stroke(
                    c,
                    0.0,
                    Stroke::new(1.0_f32, p.timeline_grid),
                    StrokeKind::Inside,
                );
                paint_marker(painter, ctx, c, MarkerKind::Keyframe, false);
                paint_clipped_text(
                    painter,
                    ctx,
                    left_col,
                    egui::pos2(c.center().x, c.top() - 12.0),
                    egui::Align2::CENTER_CENTER,
                    name,
                    TextStyle::BodyStrong,
                    p.text_primary,
                );
            }
            paint_clipped_text(
                painter,
                ctx,
                left_col,
                egui::pos2((rect.left() + mid) * 0.5, rect.bottom() - 18.0),
                egui::Align2::CENTER_CENTER,
                "Button Symbol: 'Hit' frame defines invisible click polygon",
                TextStyle::Small,
                p.text_secondary,
            );

            // Right: 9-Slice grid
            let box_r = Rect::from_center_size(
                egui::pos2((mid + rect.right()) * 0.5, rect.center().y - 2.0),
                vec2(170.0, 78.0),
            );
            painter.rect_filled(box_r, 10.0, p.accent_subtle);
            painter.rect_stroke(
                box_r,
                10.0,
                Stroke::new(1.5_f32, p.accent_default),
                StrokeKind::Inside,
            );
            let gx1 = box_r.left() + 28.0;
            let gx2 = box_r.right() - 28.0;
            let gy1 = box_r.top() + 22.0;
            let gy2 = box_r.bottom() - 22.0;
            let dash_s = Stroke::new(1.0_f32, p.guide);
            for x in [gx1, gx2] {
                painter.extend(egui::Shape::dashed_line(
                    &[
                        egui::pos2(x, box_r.top() - 8.0),
                        egui::pos2(x, box_r.bottom() + 8.0),
                    ],
                    dash_s,
                    4.0,
                    3.0,
                ));
            }
            for y in [gy1, gy2] {
                painter.extend(egui::Shape::dashed_line(
                    &[
                        egui::pos2(box_r.left() - 8.0, y),
                        egui::pos2(box_r.right() + 8.0, y),
                    ],
                    dash_s,
                    4.0,
                    3.0,
                ));
            }
            paint_clipped_text(
                painter,
                ctx,
                box_r.shrink(4.0),
                box_r.center(),
                egui::Align2::CENTER_CENTER,
                "Scale X + Y",
                TextStyle::MonoSmall,
                p.text_primary,
            );
            paint_clipped_text(
                painter,
                ctx,
                right_col,
                egui::pos2(box_r.center().x, rect.bottom() - 18.0),
                egui::Align2::CENTER_CENTER,
                "9-Slice Guides: 4 Corners Fixed · Edges 1-Axis · Centre 2-Axis",
                TextStyle::Small,
                p.text_secondary,
            );
        },
    );

    mode_card(
        ui,
        Some(i16::MOVIECLIP),
        "Movie Clip vs Button vs Graphic — Runtime & Scrubbing Behavior",
        "3 Symbol Types",
        BadgeType::Accent,
        "F8 / ⌘F8",
        &[
            "1) Movie Clip: Runs on its own independent clock at runtime (continues playing even if the main Timeline is stopped on frame 1). Supports Instance Name targeting in AS2/AS3/JS/TS, Display Filters, Blend Modes, 3D Transforms, and Cache as Bitmap.",
            "2) Button: Interactive 4-frame state machine (Up, Over, Down, Hit). Enable Control ▸ Enable Simple Buttons to test button states directly on the authoring Stage.",
            "3) Graphic: Deterministically locked to the parent Timeline's playhead so multi-frame animations scrub live on the Stage. Offers 3 Looping modes in Properties: Loop, Play Once, and Single Frame (with interactive Frame Picker).",
        ],
    );
    mode_card(
        ui,
        Some(i16::COLORWHEEL),
        "Instance Colour Effects, Swap Symbol & Edit in Place",
        "Inspector",
        BadgeType::Accent,
        "Properties",
        &[
            "5 Colour Effect Styles: None, Brightness (-100% black to +100% white), Tint (RGB tint colour + 0–100% strength), Advanced (independent RGBA multiplier % and constant -255..+255 offset channels), and Alpha (0–100% opacity).",
            if cfg!(target_os = "macos") {
                "Swap Symbol & Option-drag Replacement: Click Swap… in Properties (or hold Option while dragging a symbol from the Library onto an instance on Stage) to replace the symbol while preserving all transforms, filters, and keyframes."
            } else {
                "Swap Symbol & Alt-drag Replacement: Click Swap… in Properties (or hold Alt while dragging a symbol from the Library onto an instance on Stage) to replace the symbol while preserving all transforms, filters, and keyframes."
            },
            "Edit in Place (Double-Click Instance on Stage): Edits the symbol inside its exact affine-transformed Stage position while dimming the surrounding parent scene context.",
        ],
    );
}

// ── Chapter 7: Timeline, Layers & Masks ──

fn chapter_timeline_layers_and_masks(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "The Timeline organizes content across depth (Layers and Layer Folders) and time (Frames, Keyframes, and Tween Spans). Every Timeline operation is transactional and preserves motion-span invariants.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Authentic NF2 Timeline Spans & Keyframe Markers",
    );
    illustration_canvas(
        ui,
        142.0,
        "Illustration 7.1: Live NF2 Timeline rendering showing Static (grey), Motion Tween (blue + ◆ property keys), Classic Tween (purple + arrow), Shape Tween (green + arrow), and Sound waveform spans.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let row_h = 24.0;
            let label_w = 132.0;
            let origin_x = rect.left() + label_w;
            let geo = Geometry::new(origin_x, FrameView::Normal, false);
            let rows = [
                ("Actions / Labels", SpanType::Static, i16::LAYER),
                ("Motion Tween", SpanType::MotionTween, i16::TWEEN_MOTION),
                ("Classic Tween", SpanType::ClassicTween, i16::TWEEN_CLASSIC),
                ("Shape Morph", SpanType::ShapeTween, i16::TWEEN_SHAPE),
            ];
            for (idx, (name, span_kind, icon)) in rows.iter().enumerate() {
                let y = rect.top() + 10.0 + idx as f32 * row_h;
                let l_rect = Rect::from_min_size(
                    egui::pos2(rect.left() + 8.0, y),
                    vec2(label_w - 12.0, row_h),
                );
                let f_rect = Rect::from_min_max(
                    egui::pos2(origin_x, y),
                    egui::pos2(rect.right() - 10.0, y + row_h),
                );
                painter.rect_filled(l_rect, 0.0, p.bg_panel);
                icons::paint(
                    painter,
                    ctx,
                    Rect::from_min_size(
                        l_rect.min + egui::Vec2::splat(space::s1()),
                        egui::Vec2::splat(ICON_UI),
                    ),
                    *icon,
                    p.text_secondary,
                );
                paint_clipped_text(
                    painter,
                    ctx,
                    l_rect.shrink(2.0),
                    egui::pos2(l_rect.left() + 24.0, l_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    name,
                    TextStyle::Small,
                    p.text_primary,
                );

                paint_grid(painter, ctx, f_rect, &geo, GridKind::Layer);
                let span_r = geo.span(f_rect, 1, 20);
                paint_span(painter, ctx, span_r, *span_kind, true);
                match idx {
                    0 => {
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 1),
                            MarkerKind::ActionKeyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 12),
                            MarkerKind::BlankKeyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 20),
                            MarkerKind::SpanEnd,
                            false,
                        );
                    }
                    1 => {
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 1),
                            MarkerKind::Keyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 8),
                            MarkerKind::PropertyKeyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 15),
                            MarkerKind::PropertyKeyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 20),
                            MarkerKind::PropertyKeyframe,
                            false,
                        );
                    }
                    _ => {
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 1),
                            MarkerKind::Keyframe,
                            false,
                        );
                        paint_marker(
                            painter,
                            ctx,
                            geo.cell(f_rect, 20),
                            MarkerKind::Keyframe,
                            false,
                        );
                    }
                }
            }
        },
    );

    mode_card(
        ui,
        Some(i16::KEYFRAME_ADD),
        "Frame & Keyframe Shortcuts + Span-Based vs Frame-Based Selection",
        "Timeline Editing",
        BadgeType::Accent,
        "F5 / F6 / F7",
        &[
            "Insert Frame (F5) & Remove Frames: Extends or shortens the duration of a span without adding keyframes.",
            "Insert Keyframe (F6), Insert Blank Keyframe (F7) & Clear Keyframe: F6 duplicates the current pose into a new keyframe (or records a ◆ property keyframe inside a Motion Tween); F7 inserts an empty keyframe; Clear Keyframe reverts a keyframe to a held frame.",
            if cfg!(target_os = "macos") {
                "Span-Based Selection (Preferences ▸ Timeline): Clicking inside a span selects the entire keyframe-to-end sequence; Command-clicking selects a single individual frame. Hold Option while dragging a keyframe to duplicate it."
            } else {
                "Span-Based Selection (Preferences ▸ Timeline): Clicking inside a span selects the entire keyframe-to-end sequence; Ctrl-clicking selects a single individual frame. Hold Alt while dragging a keyframe to duplicate it."
            },
        ],
    );
    mode_card(
        ui,
        Some(i16::MASK),
        "Layer Roles: Normal, Guide, Motion Guide, Mask, Masked & Folder",
        "6 Layer Roles",
        BadgeType::Accent,
        "Layer Properties",
        &[
            "Guide & Guided Layers: Guide layers are excluded from published output; linking a Normal layer beneath a Guide layer creates a Motion Guide for Classic Tweens.",
            "Mask & Masked Layers: Shapes/symbols on a Mask layer cut a visibility window for indented Masked layers beneath it (supports Geometry, Alpha, and Luminance masking plus Invert Mask).",
            if cfg!(target_os = "macos") {
                "Quick Column Modifiers: Option-clicking in the Eye (Visibility), Lock, or Outline column toggles all other layers except the clicked row; Solo isolates selected layers."
            } else {
                "Quick Column Modifiers: Alt-clicking in the Eye (Visibility), Lock, or Outline column toggles all other layers except the clicked row; Solo isolates selected layers."
            },
        ],
    );
    mode_card(
        ui,
        Some(i16::ONION),
        "Onion Skinning, Edit Multiple Frames & Scenes",
        "Multi-Frame",
        BadgeType::Neutral,
        "Timeline Bar",
        &[
            "Onion Skin & Onion Skin Outlines: Displays translucent ghost frames (or coloured wireframes) before and after the playhead within the draggable Onion range markers.",
            "Edit Multiple Frames: Allows selecting, moving, and transforming artwork across all unlocked frames inside the Onion range simultaneously.",
            "Scenes (Window ▸ Panels ▸ Scene): Author separate scenes on independent timelines while the runtime and HTML5 exporter seamlessly flatten them in play order.",
        ],
    );
    mode_card(
        ui,
        Some(i16::KEYFRAME_ADD),
        "Razor Tool (C), Split Clip & Linked Video/Audio Montage",
        "Montage",
        BadgeType::Accent,
        "C / Split Clip",
        &[
            "Razor Tool (C) & Split Clip at Playhead (Modify ▸ Timeline ▸ Split Clip at Playhead): Splits Video and Stream Audio spans cleanly—tightening the left clip's Out point, advancing the right clip's In point, and partitioning cue points—while inserting a keyframe (advancing Graphic first_frame) on standard Graphic, Button, MovieClip, and Shape spans.",
            "Link / Unlink Video & Audio: Modify ▸ Timeline ▸ Link Video & Audio pairs reciprocal video and audio tracks so Split Clip and ripple Insert, Delete, Move, and SetEnd edits stay frame-locked until unlinked.",
            "Video edit workspace (Window ▸ Workspaces ▸ Video edit): Switches to layer-strip previews with Properties + Scene above Library + Project and a wide Timeline + Output + Actions dock for non-linear montage.",
        ],
    );
}

// ── Chapter 8: Tweens, Motion & IK ──

fn chapter_tweens_motion_and_ik(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "NeoFlash supports all four Flash animation paradigms: object-based Motion Tweens with ◆ property tracks, pose-to-pose Classic Tweens with Motion Guides, contour-morphing Shape Tweens with Shape Hints (a–z), and Inverse Kinematics (IK) armatures.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Motion Path (◆ Keys & Handles), Shape Hints (a–z) & Ease Curve",
    );
    illustration_canvas(
        ui,
        152.0,
        "Illustration 8.1: Left — Stage Motion Path with per-frame dots, ◆ property keyframes, and Bézier handles. Centre — Shape Tween Hints (Unplaced red, Start yellow, End green). Right — Cubic-Bézier Ease Graph.",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let w3 = rect.width() / 3.0;
            let r1 = Rect::from_min_max(rect.min, egui::pos2(rect.left() + w3, rect.bottom()));
            let r2 = Rect::from_min_max(
                egui::pos2(r1.right(), rect.top()),
                egui::pos2(r1.right() + w3, rect.bottom()),
            );
            let r3 = Rect::from_min_max(egui::pos2(r2.right(), rect.top()), rect.max);
            painter.vline(
                r1.right(),
                rect.y_range(),
                Stroke::new(1.0_f32, p.border_subtle),
            );
            painter.vline(
                r2.right(),
                rect.y_range(),
                Stroke::new(1.0_f32, p.border_subtle),
            );

            // Left: Motion Path
            let k0 = r1.min + vec2(32.0, 108.0);
            let k1 = r1.center() + vec2(0.0, -24.0);
            let k2 = egui::pos2(r1.right() - 32.0, r1.top() + 104.0);
            let frames: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    let u = 1.0 - t;
                    egui::pos2(
                        u * u * k0.x + 2.0 * u * t * k1.x + t * t * k2.x,
                        u * u * k0.y + 2.0 * u * t * k1.y + t * t * k2.y,
                    )
                })
                .collect();
            paint_motion_path(painter, ctx, &frames);
            let mid_pt = frames[8];
            paint_motion_handle(
                painter,
                ctx,
                mid_pt,
                mid_pt + vec2(-36.0, -14.0),
                MotionHandleState::Default,
            );
            paint_motion_handle(
                painter,
                ctx,
                mid_pt,
                mid_pt + vec2(36.0, 14.0),
                MotionHandleState::Hover,
            );
            paint_motion_key(painter, ctx, k0, MotionKeyState::Default);
            paint_motion_key(painter, ctx, mid_pt, MotionKeyState::Selected);
            paint_motion_key(painter, ctx, k2, MotionKeyState::Hover);
            paint_clipped_text(
                painter,
                ctx,
                r1.shrink(4.0),
                egui::pos2(r1.center().x, r1.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "Motion Path: ◆ Keys & Bézier Handles",
                TextStyle::Small,
                p.text_primary,
            );

            // Centre: Shape Hints (a, b, c)
            let sh_c = r2.center() + vec2(0.0, 8.0);
            let shape_box = Rect::from_center_size(sh_c, vec2(110.0, 62.0));
            painter.rect_filled(shape_box, 6.0, p.accent_subtle);
            painter.rect_stroke(
                shape_box,
                6.0,
                Stroke::new(1.5_f32, p.accent_default),
                StrokeKind::Inside,
            );
            paint_shape_hint(
                painter,
                ctx,
                shape_box.left_top(),
                'a',
                ShapeHintState::Start,
            );
            paint_shape_hint(
                painter,
                ctx,
                shape_box.right_top(),
                'b',
                ShapeHintState::End,
            );
            paint_shape_hint(
                painter,
                ctx,
                shape_box.right_bottom(),
                'c',
                ShapeHintState::Unplaced,
            );
            paint_clipped_text(
                painter,
                ctx,
                r2.shrink(4.0),
                egui::pos2(r2.center().x, r2.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "Shape Hints: Start · End · Unplaced",
                TextStyle::Small,
                p.text_primary,
            );

            // Right: Ease curve visualization
            let g_box = r3.shrink2(vec2(28.0, 26.0));
            painter.rect_filled(g_box, 4.0, p.bg_panel);
            painter.rect_stroke(
                g_box,
                4.0,
                Stroke::new(1.0_f32, p.border_subtle),
                StrokeKind::Inside,
            );
            let curve: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    let eased = t * t * (3.0 - 2.0 * t);
                    egui::pos2(
                        g_box.left() + t * g_box.width(),
                        g_box.bottom() - eased * g_box.height(),
                    )
                })
                .collect();
            painter.add(egui::Shape::line(
                curve,
                Stroke::new(2.0_f32, p.accent_default),
            ));
            paint_clipped_text(
                painter,
                ctx,
                r3.shrink(4.0),
                egui::pos2(r3.center().x, r3.top() + 14.0),
                egui::Align2::CENTER_CENTER,
                "Ease & Motion Editor Curves",
                TextStyle::Small,
                p.text_primary,
            );
        },
    );

    mode_card(
        ui,
        Some(i16::TWEEN_MOTION),
        "Motion Tween & Motion Editor — ◆ Property Keyframes & Roving Keys",
        "Property Tracks",
        BadgeType::Accent,
        "Insert ▸ Motion Tween",
        &[
            "Selective ◆ Property Keyframes: Moving or transforming an instance inside a Motion Span records a ◆ key only for the changed property (X, Y, ScaleX, ScaleY, Rotation, Skew, Alpha, Colour), leaving other tracks smooth.",
            "Interactive Stage Motion Path: Select the motion path with Selection (V) to bend segments directly, or with Subselection (A) to adjust individual ◆ keyframe handles. Enable Roving Keyframes for uniform speed across multi-segment paths.",
            "Motion Editor & Ease Panels: Inspect and edit individual property graphs, add custom cubic-Bézier ease curves (including overshoot |y| ≤ 100), or Copy / Paste Motion Special across instances.",
        ],
    );
    mode_card(
        ui,
        Some(i16::TWEEN_SHAPE),
        "Classic Tween (Motion Guide + Orient to Path) & Shape Tween (Shape Hints a–z)",
        "Classic & Morph",
        BadgeType::Accent,
        "⇧⌘H",
        &[
            "Classic Tween: Interpolates between explicit keyframes (● → ●). Link the layer under a Guide layer and enable Orient to Path to steer the instance along the guide stroke tangent.",
            "Shape Tween & Shape Hints (Modify ▸ Shape ▸ Add Shape Hint): Morphs raw vector shapes using arc-length resampling and paint blending. Place up to 26 lettered Shape Hints (a–z) counter-clockwise from the top-left corner (Yellow = start keyframe, Green = end keyframe, Red = unsnapped).",
        ],
    );
    mode_card(
        ui,
        Some(i16::ARMATURE),
        "Inverse Kinematics — Bone Tool (X) & Bind Tool (Z) Modes",
        "Armatures",
        BadgeType::Accent,
        "X / Z",
        &[
            "Symbol Armatures vs IK Shapes: Dragging with Bone (X) from one symbol instance to another chains them into a parent-child skeleton on a Pose Layer; dragging inside a selected vector shape creates a deformable IK Shape container.",
            "Posing Modifiers: Drag any bone with Selection (V) to solve inverse kinematics across the chain; hold Shift while dragging a bone to rotate only that bone and its children without moving parent bones.",
            if cfg!(target_os = "macos") {
                "Bind Tool (Z): Click a bone to highlight its bound contour points in yellow (squares = bound to 1 bone, triangles = bound to multiple bones); Shift-click adds points, Option-click unbinds points."
            } else {
                "Bind Tool (Z): Click a bone to highlight its bound contour points in yellow (squares = bound to 1 bone, triangles = bound to multiple bones); Shift-click adds points, Alt-click unbinds points."
            },
        ],
    );
}

// ── Chapter 9: Filters, Text & Audio ──

fn chapter_filters_text_and_audio(ui: &mut Ui, _state: &mut HelpManualState) {
    paragraph(
        ui,
        "NeoFlash provides live GPU/software display filters, 14 Porter-Duff and Flash compositing blend modes, full typography with selective Font Embedding, and sample-accurate timeline audio with 8-handle stereo envelopes.",
    );

    section_overline(
        ui,
        "Interactive Diagram — Stereo Audio Waveform & 8-Handle Edit Envelope",
    );
    illustration_canvas(
        ui,
        128.0,
        "Illustration 9.1: Timeline Sound waveform (`SpanType::Sound`) with interactive Left/Right volume envelope handles and In/Out trim markers (`Properties ▸ Sound ▸ Envelope…`).",
        |painter, ctx, rect| {
            let p = neoflash_ui::palette(ctx);
            let lane = rect.shrink2(vec2(24.0, 22.0));
            paint_span(painter, ctx, lane, SpanType::Sound, true);
            painter.rect_stroke(
                lane,
                0.0,
                Stroke::new(1.0_f32, p.border_default),
                StrokeKind::Inside,
            );

            // Envelope polyline + square handles
            let env = [
                egui::pos2(lane.left(), lane.bottom() - 8.0),
                egui::pos2(lane.left() + lane.width() * 0.15, lane.top() + 12.0),
                egui::pos2(lane.left() + lane.width() * 0.75, lane.top() + 16.0),
                egui::pos2(lane.right(), lane.bottom() - 6.0),
            ];
            painter.add(egui::Shape::line(
                env.to_vec(),
                Stroke::new(1.5_f32, p.accent_default),
            ));
            for pt in env {
                paint_anchor(painter, ctx, pt, AnchorState::Selected);
            }
        },
    );

    mode_card(
        ui,
        Some(i16::FILTER),
        "7 Live Filters & 14 Blend Modes (Instance & Layer Level)",
        "Compositing",
        BadgeType::Accent,
        "Filters Panel",
        &[
            "7 Live Filters: Drop Shadow, Blur, Glow, Bevel, Gradient Glow, Gradient Bevel, and Adjust Color (Brightness, Contrast, Saturation, Hue).",
            "14 Blend Modes: Normal, Layer, Darken, Multiply, Lighten, Screen, Overlay, Hard Light, Add, Subtract, Difference, Invert, Alpha, and Erase (available both on MovieClip/Button instances and directly on Timeline Layers).",
        ],
    );
    mode_card(
        ui,
        Some(i16::TEXT),
        "Text Engine (TLF CS5 / Static / Dynamic / Input), Multi-Column Flow & Font Embedding",
        "Typography",
        BadgeType::Accent,
        "T / Embedding…",
        &[
            "Point vs Area Text & Inline Editing: Click on Stage with Text (T) for auto-expanding Point Text (round top-right handle ○); drag for fixed-width word-wrapping Area Text (square handle □; double-click handle to revert to auto-width); double-click any TextBox with Selection (V) or Free Transform (Q) to edit text inline.",
            "Flash CS5 Character, Paragraph & Container Flow: Configure Highlight colour, Strikethrough, Underline, Superscript/Subscript, Leading (pt or %), Orientation (Horizontal, Vertical LTR/RTL), Advanced Character (Case, Digit Case/Width, Ligatures, Baseline Shift, Locale), 5-mode Paragraph alignment (including Justify All) with Space Before/After, and Container & Flow (1–50 Columns with Gutter px, 4-sided L/R/T/B Padding with Lock, Top/Center/Bottom/Justify Vertical Alignment, Border/Background slash-swatches, and 1st Line Offset).",
            "Font Embedding & 2-Step Break Apart: Select Unicode ranges or custom glyphs in Font Embedding… to embed outlines; run Modify ▸ Break Apart once to split words into individual letter boxes and a second time to convert glyphs into editable vector paths.",
        ],
    );
    mode_card(
        ui,
        Some(i16::SOUND),
        "Timeline Sound Sync Modes (Event, Start, Stop, Stream) & Edit Envelope",
        "Audio Engine",
        BadgeType::Accent,
        "Properties ▸ Sound",
        &[
            "1) Event: Triggers when the playhead enters the keyframe and plays independently to completion (re-entering layers another overlapping voice).",
            "2) Start: Like Event, but never starts a duplicate instance if that sound is already playing.",
            "3) Stop: Immediately silences the specified sound resource.",
            "4) Stream: Locks audio playback frame-by-frame to the Timeline clock and plays short slices while scrubbing the playhead.",
            "Edit Envelope: Up to 8 volume control points per stereo channel with presets (Fade In, Fade Out, Fade Left to Right, etc.) and In/Out trim bars.",
        ],
    );
    mode_card(
        ui,
        Some(i16::SOUND),
        "Timeline Video Sync, Trim, Cue Points & Audio Extraction (Properties ▸ Video)",
        "Video Engine",
        BadgeType::Accent,
        "Properties ▸ Video",
        &[
            "Video Sync Modes (Stream, Event, Stop), Loop & Mute: Stream locks MP4/WebM/AVI playback to the Timeline frame clock; Event triggers independent playback on keyframe entry; Stop halts active playback.",
            "Interactive In/Out Trim & Cue Points: Drag `SpanType::Video` trim handles or set millisecond offsets directly, split clips at the playhead, and author timed cue points that advance cleanly across clip splits.",
            "Extract Audio to Linked Track: Demuxes embedded PCM/MP3 audio from AVI containers or invokes `ffmpeg` to place a synchronized `audio/*` Stream track linked to the video layer in a single Undo step.",
        ],
    );
}

// ── Chapter 10: Scripting, Import & Publish ──

fn chapter_scripting_import_and_publish(
    ui: &mut Ui,
    _state: &mut HelpManualState,
    nav_target: &mut Option<usize>,
) {
    paragraph(
        ui,
        "NeoFlash supports five scripting languages in the same authoring environment—ActionScript 3.0, ActionScript 2.0, ActionScript 1.0, TypeScript, and JavaScript—governed by a unified per-project script language policy, and publishes self-contained HTML5 Canvas + WebAssembly bundles, semantic Web App · DOM bundles, zero-WASM native HTML5 banners, Desktop apps, Adobe After Effects (.aep/.jsx) compositions, 5-format texture atlases, vector/raster frame sequences, SVG, Lottie/dotLottie, Flash Player 8/9/10 SWF, audio/video, and headless CLI automation.",
    );

    section_overline(
        ui,
        "Scripting, Unified Language Policy, AI Assist & Debugging",
    );
    mode_card(
        ui,
        Some(i16::CODE),
        "Actions (F9), Project Script Policy, Send to AI…, Code Snippets & Debugger",
        "5 Languages",
        BadgeType::Accent,
        "F9 / ⌘J / F2 / ⌥F2",
        &[
            "Unified Project Script Policy (Document Settings ⌘J & Properties): Each document selects ActionScript 3.0, ActionScript 2.0, ActionScript 1.0, TypeScript, or JavaScript as its project script language. Optional 'Also compile compatible scripts' compiles AS1 inside AS2 projects and JS inside TS projects, while foreign-language scripts remain safely preserved (marked Skipped) until converted or retagged.",
            "Frame, Object & Project Scripts + Code Snippets: Attach scripts to keyframes ('a' marker in the Timeline), MovieClip/Button instances (`on(release)`, `onClipEvent`), or `Project` source files (`.as`, `.ts`, `.js`), with language-matched templates in `Code Snippets` and interactive breakpoints, Call Stack, Locals, Watch, and Bandwidth Profiler in `Debugger`.",
            "Optional AI Assist (`Preferences ▸ AI` & `Edit ▸ Send to AI…`): Connect OpenAI, Anthropic, Gemini, or custom HTTPS/loopback endpoints with API credentials stored exclusively in macOS Keychain or `NEOFLASH_AI_TOKEN_<ID>` environment variables (never in `.nflash` or preferences JSON). `Send to AI…` previews the exact redacted context pack for consent before sending, and `Insert at cursor` / `Apply timing` commits as a single Undo step.",
        ],
    );

    section_overline(
        ui,
        "Open Dedicated Scripting Reference Chapters (Help ▸ Scripting Reference…)",
    );
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::s2(), space::s1());
        for &(target_idx, label) in &[
            (10usize, "Ch.11 AS1 & AS2 Fundamentals"),
            (11, "Ch.12 AS2 Core Classes"),
            (12, "Ch.13 AS3 Display List"),
            (13, "Ch.14 AS3 Events & Lifecycle"),
            (14, "Ch.15 AS3 Graphics, Filters & Sound"),
            (15, "Ch.16 AS1/AS2 ↔ AS3 ↔ JS/TS Migration"),
            (16, "Ch.17 JavaScript (ES2023+ & Runtime)"),
            (17, "Ch.18 TypeScript & Type Checker"),
            (18, "Ch.19 Browser & Web APIs"),
        ] {
            if ui.add(Button::new(label)).clicked() {
                *nav_target = Some(target_idx);
            }
        }
    });

    section_overline(
        ui,
        "Import Pipeline, Multi-Target Publishing & Headless CLI",
    );
    mode_card(
        ui,
        Some(i16::PUBLISH),
        "Figma / FLA / XFL / SWF / SWC / Audio Import, Web App · DOM & --headless CLI",
        "Interchange",
        BadgeType::Accent,
        "⌘R / ⇧F12 / ⌥⇧⌘A",
        &[
            "Comprehensive Import & Library Linking: Opens Figma `.fig` (Kiwi & Zstd-compressed) and `.figma.json` layered designs, binary `.fla` (Flash 3–CS4), `.xfl`/`.fla` (CS5–Animate), `.swf` (FWS/CWS/ZWS), `.swc` component libraries (standalone via `File ▸ Open…` or linked into an existing project via `File ▸ Import` with conflict/dependency diagnostics), layered `.psd`/`.psb`, `.ai`/`.svg`/`.pdf`/`fxg` vectors, rasters, audio (`.mp3`/`.wav`, plus `.ogg`/`.flac`/`.aac`/`.m4a` transcoded to 16-bit PCM WAV via `ffmpeg`), video (`.mp4`/`.webm`/`.avi`), and numbered sequences.",
            "Canvas + WASM, Web App · DOM & Zero-WASM Native Banner: Publishes high-DPI HTML5 Canvas + WASM bundles (`Stage.scaleMode`, 9-anchor `Stage.align`, `Allow fullscreen`, `loadPublish`, SHA-256 `Assets folder`, per-bitmap `WebP`/`JPEG`/`PNG`, `sprite_sheet.png`), semantic accessible `Web App · DOM` (`HtmlRenderMode::Dom` for responsive HTML/DOM elements and CSS transforms), or zero-WASM `Web Banner · Native` (`<50 KB` gzipped), plus Flash Player 8 (AVM1) / 9 / 10 (AVM2) `.swf`, Lottie JSON & `.lottie` dotLottie (`Original`/`PNG`/`JPEG`/`WebP`), After Effects `.aep`/`.jsx`, 5-format Texture Atlases, sequences, and audio/video.",
            "Headless Automation CLI (`neoflash-editor --headless`): Machine-readable JSON/text commands (`capabilities`, `new`, `inspect`, `validate`, `compile`, `eval`, `jsfl`, `batch`, `import`, `export`) build, test, and publish `.nflash` projects in CI without opening a GUI window (supporting `--target html5|fla|xfl|swf|svg|lottie|dotlottie|atlas|sequence|video|audio|banner|desktop` and `--html-preview`).",
        ],
    );

    section_overline(
        ui,
        "NeoFlash 0.1.0-beta.1 — Install, Manual Update, Bundled Media, Beta Feedback & Known Issues",
    );
    mode_card(
        ui,
        Some(i16::DESKTOP),
        "NeoFlash 0.1.0-beta.1 — Release Operations & Boundaries",
        "0.1.0-beta.1",
        BadgeType::Accent,
        "About NeoFlash / F2",
        &[
            #[cfg(target_os = "macos")]
            "Release Identity & Manual Update: NeoFlash 0.1.0-beta.1 targets macOS 15+ (Apple Silicon). Preferences ▸ Updates checks GitHub Releases (Stable or Beta channel, with opt-in daily checks) and downloads SHA-256 verified archives for manual replacement without silent self-install. To install or update macOS, quit NeoFlash and replace NeoFlash.app in /Applications; preferences, custom workspaces, recent files, and crash-recovery snapshots under ~/Library/Application Support/NeoFlash are preserved.",
            #[cfg(target_os = "windows")]
            "Release Identity & Manual Update: NeoFlash 0.1.0-beta.1 provides a Windows x64 Technical Preview. Preferences ▸ Updates checks GitHub Releases (Stable or Beta channel, with opt-in daily checks) and downloads SHA-256 verified archives for manual replacement without silent self-install. To install or update Windows, extract the portable ZIP into a clean or existing NeoFlash folder and launch neoflash.exe (if Windows SmartScreen warns before the first launch, choose More info -> Run anyway); preferences, custom workspaces, recent files, and crash-recovery snapshots in your user NeoFlash data directory (%APPDATA%\\NeoFlash) are preserved.",
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            "Release Identity & Manual Update: NeoFlash 0.1.0-beta.1 provides an Ubuntu 24.04-compatible Linux x64 Technical Preview. Preferences ▸ Updates checks GitHub Releases (Stable or Beta channel, with opt-in daily checks) and downloads SHA-256 verified archives for manual replacement without silent self-install. To install or update Linux, extract the portable tar.gz archive and run the neoflash executable; preferences, custom workspaces, recent files, and crash-recovery snapshots under ~/.local/share/NeoFlash are preserved.",
            #[cfg(target_os = "macos")]
            "Bundled Media & Licences: Packaged macOS builds include ffmpeg and ffprobe in Contents/Resources/media/bin/. Exact application, Rust and media licence evidence is already included; verify the application and corresponding-source archives with the supplied SHA256SUMS.txt. Set NEOFLASH_FFMPEG / NEOFLASH_FFPROBE only for a custom override.",
            #[cfg(not(target_os = "macos"))]
            "Bundled Media & Licences: Packaged builds include ffmpeg and ffprobe in media/bin/. Exact application, Rust and media licence evidence is already included; verify the portable archive and corresponding-source archive with the supplied SHA256SUMS.txt. Set NEOFLASH_FFMPEG / NEOFLASH_FFPROBE only for a custom override.",
            "Project Backups & Beta Feedback (`Bug?` / `Help ▸ Send Beta Feedback…`): Keep versioned .nflash JSON projects in Git or regular file backups. In beta builds, click the bottom-left Stage `Bug?` pill or choose `Help ▸ Send Beta Feedback…` (available even with zero open documents) to send only the visible report and up to 5 screenshots over HTTPS FormSubmit to `developer@redinger.cc`; no device metadata or project bytes are added. Mention a separately available minimal .nflash reproduction, and paste Output panel (F2) diagnostics plus the About NeoFlash commit identifier into the report.",
            "Beta 1 known issues and limits: VoiceOver cannot read the editor interface yet; there is no build for Intel (x86_64) Macs; updates are checked and downloaded via Preferences ▸ Updates (SHA-256 verified) and installed manually without silent self-replacement; the Windows preview is unsigned, so SmartScreen may warn before the first launch, and it has no connected debugger; the Linux preview is a portable archive tested on Ubuntu 24.04 x64 with X11 and Wayland, not an AppImage or deb package; Windows and Linux have had less testing than macOS; import of legacy Flash files is partial; graphics-tablet support is experimental.",
        ],
    );

    section_overline(ui, "Essential Keyboard Shortcuts Quick Reference");
    sub_bullet(
        ui,
        "Selection / Subselection / Free Transform / Gradient Transform / Razor",
        Some("V / A / Q / F / C"),
        "Primary selection, node editing, bounding-box/envelope transform, gradient matrix handles, and timeline clip/keyframe splitting.",
    );
    sub_bullet(
        ui,
        "Pen / Add / Delete / Convert Anchor Point",
        Some("P / = / - / ⇧C"),
        "Bézier curve construction and non-destructive node editing.",
    );
    sub_bullet(
        ui,
        "Text / Line / Rectangle / Oval / Pencil / Brush",
        Some("T / N / R / O / Y / B"),
        "Core typography, geometric primitives, and freehand vector tools.",
    );
    sub_bullet(
        ui,
        "Deco / Bone / Bind / Paint Bucket / Ink Bottle / Eyedropper / Eraser",
        Some("U / X / Z / K / S / I / E"),
        "Procedural patterns, inverse kinematics, fill/stroke styling, and erasing.",
    );
    sub_bullet(
        ui,
        "Object Drawing Toggle / Hand / Zoom",
        Some("J / H / Z"),
        "Toggle isolated Object Drawing mode, pan stage (or hold Space), and zoom.",
    );
    sub_bullet(
        ui,
        "Timeline Frames & Keyframes",
        Some("F5 / ⇧F5 / F6 / F7 / ⇧F6"),
        "Insert frame, remove frame, insert keyframe, insert blank keyframe, clear keyframe.",
    );
    sub_bullet(
        ui,
        "Convert to Symbol / New Symbol / Break Apart / Group",
        Some("F8 / ⌘F8 / ⌘B / ⌘G"),
        "Symbol packaging, ungrouping, and 2-step text-to-outlines conversion.",
    );
    sub_bullet(
        ui,
        "Test Movie / Publish / Actions / Library / Properties",
        Some("⌘↵ / ⇧F12 / F9 / ⌘L / ⌘F3"),
        "Preview runtime, publish bundle, and toggle core authoring panels.",
    );

    super::help_as_reference::render_esc_chords_reference(ui);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Editor;
    use neoflash_core::Project;

    #[test]
    fn help_manual_renders_all_chapters_diagrams_and_search_without_mutating_document() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        let before = serde_json::to_value(&editor.project).unwrap();

        assert!(!is_help_manual_open(&ctx));
        open_help_manual(&ctx);
        assert!(is_help_manual_open(&ctx));
        assert_eq!(help_manual_mode(&ctx), HelpManualMode::AuthoringHelp);

        for page in 0..HELP_CHAPTERS.len() {
            set_help_manual_page(&ctx, page);
            for tab in 0..3 {
                ctx.data_mut(|d| {
                    d.get_temp_mut_or_default::<HelpManualState>(state_id())
                        .diagram_tab = tab;
                });
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    editor.help_manual_dialog(ctx);
                });
            }
        }

        // Filtering never moves the article underneath the reader. The user selects a
        // matching chapter explicitly from the stable sidebar.
        ctx.data_mut(|d| {
            let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
            s.page = 0;
            s.mode = HelpManualMode::AuthoringHelp;
            s.query = "armature".to_string();
        });
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            editor.help_manual_dialog(ctx);
        });
        let matched_page = ctx.data(|d| {
            d.get_temp::<HelpManualState>(state_id())
                .map(|s| s.page)
                .unwrap_or(0)
        });
        assert_eq!(HELP_CHAPTERS[matched_page].title, "1. Workspace & Stage");

        close_help_manual(&ctx);
        assert!(!is_help_manual_open(&ctx));
        assert_eq!(before, serde_json::to_value(&editor.project).unwrap());
    }

    #[test]
    fn separate_help_and_scripting_reference_entry_points_open_distinct_chapter_scopes() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());

        editor.open_help_manual(&ctx);
        assert!(is_help_manual_open(&ctx));
        assert_eq!(help_manual_mode(&ctx), HelpManualMode::AuthoringHelp);
        assert_eq!(
            ctx.data(|d| d.get_temp::<HelpManualState>(state_id()).unwrap().page),
            0
        );

        // Project::demo() uses JavaScript -> opens JS/TS scope at Chapter 17 (idx 16).
        assert_eq!(
            editor.project.effective_script_language(),
            neoflash_core::ScriptLanguage::JavaScript
        );
        editor.open_scripting_reference(&ctx);
        assert!(is_help_manual_open(&ctx));
        assert_eq!(help_manual_mode(&ctx), HelpManualMode::ScriptingReference);
        assert_eq!(help_manual_language_scope(&ctx), LanguageScope::JsTs);
        assert_eq!(
            ctx.data(|d| d.get_temp::<HelpManualState>(state_id()).unwrap().page),
            16
        );

        // Switching project script language to ActionScript 3.0 opens AS3 scope at Chapter 13 (idx 12).
        editor.project.script_language = Some(neoflash_core::ScriptLanguage::ActionScript3);
        editor.open_scripting_reference(&ctx);
        assert_eq!(help_manual_language_scope(&ctx), LanguageScope::As3);
        assert_eq!(
            ctx.data(|d| d.get_temp::<HelpManualState>(state_id()).unwrap().page),
            12
        );

        // Switching project script language to ActionScript 2.0 opens AS1/AS2 scope at Chapter 11 (idx 10).
        editor.project.script_language = Some(neoflash_core::ScriptLanguage::ActionScript2);
        editor.open_scripting_reference(&ctx);
        assert_eq!(help_manual_language_scope(&ctx), LanguageScope::As1As2);
        assert_eq!(
            ctx.data(|d| d.get_temp::<HelpManualState>(state_id()).unwrap().page),
            AUTHORING_CHAPTER_COUNT
        );

        // Switching project script language to TypeScript opens JS/TS scope at Chapter 18 (idx 17).
        editor.project.script_language = Some(neoflash_core::ScriptLanguage::TypeScript);
        editor.open_scripting_reference(&ctx);
        assert_eq!(help_manual_language_scope(&ctx), LanguageScope::JsTs);
        assert_eq!(
            ctx.data(|d| d.get_temp::<HelpManualState>(state_id()).unwrap().page),
            17
        );

        open_scripting_reference_page(&ctx, 16);
        assert_eq!(help_manual_mode(&ctx), HelpManualMode::ScriptingReference);
        assert_eq!(
            ctx.data(|d| d.get_temp::<HelpManualState>(state_id()).unwrap().page),
            16
        );

        set_help_manual_mode(&ctx, HelpManualMode::AuthoringHelp);
        assert_eq!(help_manual_mode(&ctx), HelpManualMode::AuthoringHelp);
        assert_eq!(
            ctx.data(|d| d.get_temp::<HelpManualState>(state_id()).unwrap().page),
            0
        );
    }

    #[test]
    fn t361_scripting_reference_active_language_scope_contextual_lookup_and_function_examples() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut proj = Project::blank();
        proj.script_language = Some(neoflash_core::ScriptLanguage::ActionScript2);
        let mut editor = Editor::from_project(&ctx, proj);
        let before_doc = serde_json::to_value(&editor.project).unwrap();

        // 1. Open Scripting Reference in an AS2 project: minimum-window (1024x720 @ 1.5x) bounds and language bar.
        ctx.set_zoom_factor(1.5);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1024.0, 720.0));
        let input = || egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run(input(), |_| {});
        editor.open_scripting_reference(&ctx);
        assert_eq!(help_manual_language_scope(&ctx), LanguageScope::As1As2);

        let output1 = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
        let output = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
        let bounds = ctx.memory(|m| {
            m.area_rect(egui::Id::new("neoflash-help-manual-modal"))
                .expect("Scripting Reference modal area")
        });
        assert!(
            screen.contains_rect(bounds),
            "Scripting Reference must fit 1024x720 at 1.5x zoom: {bounds:?}"
        );
        let nodes1 = &output1
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("AccessKit initial tree")
            .nodes;
        let nodes2 = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("AccessKit incremental tree")
            .nodes;
        let has_a11y = |needle: &str| {
            nodes1
                .iter()
                .chain(nodes2.iter())
                .any(|(_, n)| n.label() == Some(needle) || n.value() == Some(needle))
        };
        let painted_texts: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        // The window is in exactly one language: three choices, no "All languages", the project
        // language named in the bar (or, in a window this narrow, in the footer).
        assert!(
            painted_texts
                .iter()
                .any(|t| t.contains("Project: ActionScript 2.0")),
            "Scripting Reference must name the project language: {painted_texts:?}"
        );
        assert!(has_a11y("ActionScript 2"));
        assert!(has_a11y("ActionScript 3"));
        assert!(has_a11y("JavaScript / TypeScript"));
        assert!(!has_a11y("All languages"));
        assert!(has_a11y(
            "Search ActionScript 2: classes, methods, properties, events"
        ));

        // The bar never overflows at 1024x720 @ 1.5x: segments, search field, More.
        let find_bounds = |needle: &str| {
            // The first frame only measures the window; read geometry from the second.
            nodes2
                .iter()
                .chain(nodes1.iter())
                .find_map(|(_, n)| {
                    (n.label() == Some(needle) || n.value() == Some(needle))
                        .then(|| n.bounds())
                        .flatten()
                })
                .unwrap_or_else(|| panic!("missing bounds for {needle}"))
        };
        let seg_as2 = find_bounds("ActionScript 2");
        let seg_as3 = find_bounds("ActionScript 3");
        let seg_jsts = find_bounds("JavaScript / TypeScript");
        let search = find_bounds("Search ActionScript 2: classes, methods, properties, events");
        let more = find_bounds("More");
        assert!(seg_as2.x1 <= seg_as3.x0 + 1.0);
        assert!(seg_as3.x1 <= seg_jsts.x0 + 1.0);
        assert!(seg_jsts.x1 <= search.x0);
        assert!(search.x1 <= more.x0);
        assert!(more.x1 <= 1024.0);

        // 2. Searching "function" lists API hits without moving the window or mutating the document.
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<HelpManualState>(state_id())
                .query = "function".to_owned();
        });
        let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
        let bounds_after_search = ctx.memory(|m| {
            m.area_rect(egui::Id::new("neoflash-help-manual-modal"))
                .expect("Scripting Reference modal area after search")
        });
        assert_eq!(bounds, bounds_after_search);
        assert_eq!(before_doc, serde_json::to_value(&editor.project).unwrap());

        // 3. Contextual lookup from the Actions selection ("arguments.callee") pre-fills the search,
        //    selects the declaration in the API tab and keeps its guide chapter ready.
        ctx.set_zoom_factor(1.0);
        let root_id = editor.project.root.clone();
        if let Some(root_sym) = editor.project.symbols.iter_mut().find(|s| s.id == root_id) {
            let layer_id = root_sym.timeline.layers[0].id.clone();
            root_sym.timeline.scripts.push(neoflash_core::FrameScript {
                frame: 0,
                layer: Some(layer_id),
                script: neoflash_core::Script {
                    language: neoflash_core::ScriptLanguage::ActionScript2,
                    source: "function fact(n:Number):Number { return arguments.callee(n - 1); }"
                        .to_owned(),
                },
            });
        }
        editor.script_target = crate::app::ScriptTarget::Frame;
        let editor_id = editor.actions_editor_id();
        let mut te_state = egui::text_edit::TextEditState::default();
        te_state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(40),
                egui::text::CCursor::new(56),
            )));
        te_state.store(&ctx, editor_id);

        editor.open_scripting_reference(&ctx);
        let st = ctx
            .data(|d| d.get_temp::<HelpManualState>(state_id()))
            .expect("HelpManualState");
        assert_eq!(st.mode, HelpManualMode::ScriptingReference);
        assert_eq!(st.lang_scope, LanguageScope::As1As2);
        assert_eq!(st.query, "arguments.callee");
        assert_eq!(st.page, 10);
        assert_eq!(st.tab, ReferenceTab::Api);
        let member = st
            .api
            .selected_member(LanguageScope::As1As2)
            .expect("arguments.callee selected");
        assert_eq!(member.name, "callee");

        // 4. Explicit token lookup selects Function.apply and keeps Chapter 12 (idx 11) as its guide.
        editor.open_scripting_reference_for_token(&ctx, "Function.apply");
        let st_apply = ctx
            .data(|d| d.get_temp::<HelpManualState>(state_id()))
            .expect("HelpManualState after token lookup");
        assert_eq!(st_apply.lang_scope, LanguageScope::As1As2);
        assert_eq!(st_apply.query, "Function.apply");
        assert_eq!(st_apply.page, 11);
        assert_eq!(
            st_apply
                .api
                .selected_member(LanguageScope::As1As2)
                .map(|m| m.label.as_str()),
            Some("apply()")
        );
    }

    #[test]
    fn browser_help_is_a_complete_escaped_text_edition() {
        let html = help_browser_html(neoflash_ui::Theme::Dark);
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("NeoFlash Help &amp; Reference"));
        for (chapter, topics) in HELP_CHAPTERS.iter().zip(HELP_BROWSER_TOPICS) {
            let mut escaped_title = String::new();
            escape_html(chapter.title, &mut escaped_title);
            assert!(
                html.contains(&escaped_title),
                "missing chapter title in HTML: {}",
                chapter.title
            );
            for topic in topics {
                let mut escaped_topic = String::new();
                escape_html(topic, &mut escaped_topic);
                assert!(
                    html.contains(&escaped_topic),
                    "missing topic in HTML: {topic}"
                );
            }
        }
        assert!(html.contains("HTML5 plus WebAssembly"));
        assert!(!html.contains("<script"));
    }

    #[test]
    fn help_modal_bounds_stay_fixed_across_search_and_chapters() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 900.0))),
            ..Default::default()
        };
        let render = |editor: &mut Editor, ctx: &egui::Context| {
            let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
            let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
            ctx.memory(|memory| {
                memory
                    .area_rect(egui::Id::new("neoflash-help-manual-modal"))
                    .expect("Help modal area")
            })
        };

        open_help_manual(&ctx);
        let initial = render(&mut editor, &ctx);
        ctx.data_mut(|data| {
            let state = data.get_temp_mut_or_default::<HelpManualState>(state_id());
            state.query = "armature".into();
        });
        let filtered = render(&mut editor, &ctx);
        set_help_manual_page(&ctx, HELP_CHAPTERS.len() - 1);
        let last_chapter = render(&mut editor, &ctx);

        assert_eq!(initial, filtered);
        assert_eq!(initial, last_chapter);
    }

    #[test]
    fn help_modal_fits_the_minimum_window_at_supported_ui_scales() {
        for zoom in [1.0_f32, 1.25, 1.5] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(zoom);
            // RawInput is expressed in native viewport points. `zoom_factor` then reduces
            // the logical content rectangle available to widgets, matching a scaled app.
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(1024.0, 720.0));
            let input = || egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            // In the real app scale is established before Help opens. Prime a frame so the
            // synthetic context has the same viewport transform before modal geometry is saved.
            let _ = ctx.run(input(), |_| {});
            let mut editor = Editor::from_project(&ctx, Project::demo());
            open_help_manual(&ctx);
            let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
            let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
            let bounds = ctx.memory(|memory| {
                memory
                    .area_rect(egui::Id::new("neoflash-help-manual-modal"))
                    .expect("Help modal area")
            });
            assert!(
                screen.contains_rect(bounds),
                "Help must fit 1024×720 at {zoom}×: {bounds:?} within {screen:?}"
            );

            ctx.data_mut(|data| {
                let state = data.get_temp_mut_or_default::<HelpManualState>(state_id());
                state.query = "publish".into();
                state.page = HELP_CHAPTERS.len() - 1;
            });
            let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
            let after = ctx.memory(|memory| {
                memory
                    .area_rect(egui::Id::new("neoflash-help-manual-modal"))
                    .expect("Help modal area after search")
            });
            assert_eq!(
                bounds, after,
                "search/chapter must not move Help at {zoom}×"
            );
        }
    }

    #[test]
    fn help_controls_do_not_overlap_at_minimum_window_and_high_scale() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx.set_zoom_factor(1.5);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1024.0, 720.0));
        let input = || egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run(input(), |_| {});
        let mut editor = Editor::from_project(&ctx, Project::demo());
        open_help_manual(&ctx);
        ctx.data_mut(|data| {
            let state = data.get_temp_mut_or_default::<HelpManualState>(state_id());
            state.query = "publish".into();
            state.page = 4;
        });
        let _ = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
        let output = ctx.run(input(), |ctx| editor.help_manual_dialog(ctx));
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("Help accessibility tree")
            .nodes;
        let bounds = |label: &str| {
            nodes
                .iter()
                .filter_map(|(_, node)| {
                    (node.label() == Some(label))
                        .then(|| node.bounds())
                        .flatten()
                })
                .collect::<Vec<_>>()
        };
        let search = bounds("Search NeoFlash Help")[0];
        let more = bounds("More")[0];
        let browser = bounds("Open in browser")[0];
        let close = bounds("Close")[0];
        for rect in [search, more, browser, close] {
            assert!(rect.x0 >= 0.0 && rect.x1 <= 1024.0);
            assert!(rect.y0 >= 0.0 && rect.y1 <= 720.0);
        }
        assert!(search.x1 <= more.x0);
        // Header, bar and article are stacked: nothing shares a row with the close button.
        assert!(close.y1 <= search.y0 && search.y1 <= browser.y0);
        // Help has neither a language switch nor tags.
        let labels: Vec<&str> = nodes.iter().filter_map(|(_, node)| node.label()).collect();
        assert!(!labels.contains(&"ActionScript 3"));
        assert!(!labels.iter().any(|label| label.starts_with('#')));
    }

    #[test]
    fn actionscript_and_js_ts_help_chapters_tags_cross_links_and_styled_svg_browser_html_work() {
        assert_eq!(HELP_CHAPTERS.len(), 19);
        assert_eq!(
            HELP_CHAPTERS[10].title,
            "11. AS1 & AS2 Fundamentals & Timeline"
        );
        assert_eq!(HELP_CHAPTERS[11].title, "12. AS2 Core Classes & Built-Ins");
        assert_eq!(
            HELP_CHAPTERS[12].title,
            "13. AS3 Architecture & Display List"
        );
        assert_eq!(HELP_CHAPTERS[13].title, "14. AS3 Events, Input & Lifecycle");
        assert_eq!(
            HELP_CHAPTERS[14].title,
            "15. AS3 Graphics, Filters, Sound & Data"
        );
        assert_eq!(
            HELP_CHAPTERS[15].title,
            "16. AS1/AS2 ↔ AS3 ↔ JS/TS Migration Guide"
        );
        assert_eq!(
            HELP_CHAPTERS[16].title,
            "17. JavaScript (ES2023+ & NeoFlash Runtime)"
        );
        assert_eq!(
            HELP_CHAPTERS[17].title,
            "18. TypeScript & Built-In Type Checker"
        );
        assert_eq!(
            HELP_CHAPTERS[18].title,
            "19. Browser & Web APIs (Canvas / DOM / Fetch / Audio / Storage)"
        );

        for (idx, ch) in HELP_CHAPTERS.iter().enumerate() {
            assert!(
                !ch.sidebar_label.is_empty(),
                "chapter {idx} ({}) must have a concise sidebar label",
                ch.title
            );
            assert!(
                !ch.tags.is_empty(),
                "chapter {idx} ({}) must have tags",
                ch.title
            );
            assert!(
                !ch.related.is_empty(),
                "chapter {idx} ({}) must have cross-links",
                ch.title
            );
            for &rel in ch.related {
                assert!(rel < HELP_CHAPTERS.len(), "valid cross-link target {rel}");
            }
        }

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        // Every guide opens from a cross-link with Back history, in every reference language.
        open_scripting_reference(&ctx);
        for page in AUTHORING_CHAPTER_COUNT..HELP_CHAPTERS.len() {
            ctx.data_mut(|d| {
                let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
                s.open_page(page);
                s.history = vec![9];
            });
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                editor.help_manual_dialog(ctx);
            });
            let st = ctx
                .data(|d| d.get_temp::<HelpManualState>(state_id()))
                .unwrap();
            assert_eq!(st.page, page);
            assert_eq!(st.tab, ReferenceTab::Guides);
            assert!(st.lang_scope.has_guide(page));
        }

        let html = help_browser_html(neoflash_ui::Theme::Dark);
        assert!(html.contains("--bg-app:rgb("));
        assert!(html.contains("--accent-default:rgb("));
        for (theme, slug, label) in [
            (neoflash_ui::Theme::Dark, "dark", "Dark"),
            (neoflash_ui::Theme::Light, "light", "Light"),
            (
                neoflash_ui::Theme::MidNeutralGray,
                "mid-neutral-gray",
                "Mid Neutral Gray",
            ),
            (
                neoflash_ui::Theme::DarkGraphite,
                "dark-graphite",
                "Dark Graphite",
            ),
        ] {
            let themed = help_browser_html(theme);
            assert!(themed.contains(&format!("data-initial-theme=\"{slug}\"")));
            assert!(themed.contains(&format!(
                "id=\"nf2-theme-{slug}\" class=\"sr-toggle\" aria-label=\"{label} theme\" checked"
            )));
            assert!(themed.contains(&format!("Theme: {label}</label>")));
            assert!(themed.contains(&format!("body:has(#nf2-theme-{slug}:checked)")));
        }
        assert!(html.contains("class=\"theme-btn theme-from-mid-neutral-gray\""));
        assert!(html.contains("class=\"theme-btn theme-from-dark-graphite\""));
        assert!(html.contains("id=\"nf2-tag-AS1\""));
        assert!(html.contains("id=\"nf2-tag-AS2\""));
        assert!(html.contains("id=\"nf2-tag-AS3\""));
        assert!(html.contains("id=\"nf2-tag-JavaScript\""));
        assert!(html.contains("id=\"nf2-tag-TypeScript\""));
        assert!(html.contains("id=\"nf2-tag-WebAPI\""));
        assert!(html.contains("class=\"cross-link\""));
        assert!(html.contains("class=\"ref-table\""));
        assert!(html.contains("<pre class=\"code-block\"><code>"));
        assert!(
            html.matches("<svg ").count() >= 19,
            "browser HTML must include inline SVG illustrations for all 19 chapters"
        );
    }

    #[test]
    fn all_scripting_reference_code_samples_compile_cleanly_in_neoflash_script() {
        super::super::help_as_reference::verify_all_reference_code_samples_compile();
    }

    #[test]
    fn browser_help_routes_scripting_reference_and_uses_valid_css_styled_svgs() {
        let html = help_browser_html_for_state(
            neoflash_ui::Theme::DarkGraphite,
            HelpManualMode::ScriptingReference,
            13,
            Some("AS3"),
        );
        assert!(html.contains("data-initial-theme=\"dark-graphite\""));
        assert!(html.contains("data-initial-mode=\"SCRIPTING\""));
        assert!(html.contains(
            "id=\"nf2-mode-SCRIPTING\" data-mode=\"SCRIPTING\" class=\"sr-toggle\" checked"
        ));
        assert!(html.contains("id=\"nf2-tag-AS3\" data-tag=\"AS3\" class=\"sr-toggle\" checked"));
        assert!(
            html.contains("<section class=\"chapter-card is-opened-chapter\" id=\"chapter-14\"")
        );
        assert!(html.contains("href=\"#chapter-14\">Opened: Chapter 14 →</a>"));
        assert_eq!(
            html.matches("<svg xmlns=\"http://www.w3.org/2000/svg\"")
                .count(),
            19,
            "all 19 chapter SVGs must include explicit SVG XML namespace and dimensions"
        );
        assert!(
            !html.contains("fill=\"var(") && !html.contains("stroke=\"var("),
            "SVG shapes/text must style CSS custom properties via style=\"...\", never XML presentation attributes"
        );
        assert!(!html.contains("<script"));
        if let Ok(dir) = std::env::var("NEOFLASH_DUMP_HELP_HTML_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).expect("create dump dir");
            std::fs::write(dir.join("scripting-reference.html"), &html)
                .expect("write scripting-reference.html");
            let authoring = help_browser_html_for_state(
                neoflash_ui::Theme::Dark,
                HelpManualMode::AuthoringHelp,
                0,
                None,
            );
            std::fs::write(dir.join("help-manual.html"), authoring)
                .expect("write help-manual.html");
        }
    }

    #[test]
    fn t278_beta1_help_golden_examples_and_accessibility_smoke() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());

        // 1. Native Help & Scripting Reference + portable `file://` Browser HTML across all 4 NF2 themes.
        for theme in neoflash_ui::Theme::ALL {
            neoflash_ui::theme::apply_visuals(&ctx, theme);
            for mode in [
                HelpManualMode::AuthoringHelp,
                HelpManualMode::ScriptingReference,
            ] {
                set_help_manual_mode(&ctx, mode);
                for page in mode.chapter_range() {
                    set_help_manual_page(&ctx, page);
                    let _ = ctx.run(egui::RawInput::default(), |ctx| {
                        editor.help_manual_dialog(ctx);
                    });
                }

                let page = mode.chapter_range().start;
                let (html_path, file_url) =
                    write_help_browser_document(theme, mode, page, None).expect("write help html");
                assert!(
                    file_url.starts_with("file://"),
                    "expected portable file:// URL, got {file_url}"
                );
                let disk_html =
                    std::fs::read_to_string(&html_path).expect("read generated browser help html");
                assert!(!disk_html.contains("<script"));
                for needle in [
                    "0.1.0-beta.1",
                    "macOS 15+",
                    "Apple Silicon",
                    "Windows x64",
                    "Linux x64",
                    "Ubuntu 24.04",
                    "media/bin/",
                    "SHA256SUMS.txt",
                    "NEOFLASH_FFMPEG",
                    "About NeoFlash",
                    ".nflash",
                    "Output",
                    "VoiceOver",
                    "x86_64",
                    "graphics-tablet",
                    "experimental",
                    "screen readers",
                ] {
                    assert!(
                        disk_html.contains(needle),
                        "browser help HTML ({theme:?}, {mode:?}) missing synchronized Beta 1 section token: {needle}"
                    );
                }
                if cfg!(target_os = "macos") {
                    for needle in [
                        "~/Library/Application Support/NeoFlash",
                        "Contents/Resources/media/bin/",
                        "NeoFlash.app",
                    ] {
                        assert!(
                            disk_html.contains(needle),
                            "macOS browser help HTML missing {needle}"
                        );
                    }
                } else if cfg!(target_os = "windows") {
                    assert!(disk_html.contains("%APPDATA%\\NeoFlash"));
                    assert!(!disk_html.contains("/Applications"));
                } else {
                    assert!(disk_html.contains("~/.local/share/NeoFlash"));
                    assert!(!disk_html.contains("/Applications"));
                }
            }
        }

        // 2. Representative golden examples load, validate, evaluate, and publish cleanly to HTML5.
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("canonical workspace root");
        let temp_root = std::env::temp_dir().join(format!(
            "neoflash-t278-smoke-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let runtime_dir = temp_root.join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        std::fs::write(runtime_dir.join("neoflash_web.js"), "var NeoFlashWasm;")
            .expect("write dummy neoflash_web.js");
        std::fs::write(runtime_dir.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0")
            .expect("write dummy neoflash_web_bg.wasm");

        let golden_examples = [
            ("timeline-tweens", "examples/timeline-tweens.nflash"),
            ("text-form", "examples/text-form.nflash"),
            ("timeline-sound", "examples/timeline-sound.nflash"),
            ("video-playback", "examples/video-playback.nflash"),
            (
                "as2-arcade",
                "examples/as2/08-interactive-arcade-miniapp.nflash",
            ),
            (
                "as3-arcade",
                "examples/as3/08-interactive-arcade-miniapp.nflash",
            ),
        ];

        for (slug, rel_path) in golden_examples {
            let full_path = workspace_root.join(rel_path);
            let raw = std::fs::read_to_string(&full_path)
                .unwrap_or_else(|e| panic!("failed to read {rel_path}: {e}"));
            let project: Project = serde_json::from_str(&raw)
                .unwrap_or_else(|e| panic!("failed to parse {rel_path}: {e}"));
            project
                .validate()
                .unwrap_or_else(|e| panic!("{rel_path} failed validation: {e}"));

            let frame0 = neoflash_core::evaluate(&project, &project.root, 0);
            assert!(
                !frame0.is_empty() || !project.symbols.is_empty(),
                "{rel_path} evaluated empty symbol table"
            );
            let mut player = neoflash_core::Player::new(&project);
            player.tick(&project);

            let out_dir = temp_root.join(format!("pub-{slug}"));
            let index_path = neoflash_export::export_project_with_config(
                &project,
                &out_dir,
                &runtime_dir,
                &neoflash_export::PublishOptions::default(),
                &neoflash_export::HtmlPublishConfig::default(),
            )
            .unwrap_or_else(|e| panic!("failed to publish {rel_path} to HTML5: {e}"));
            assert!(index_path.exists(), "missing index.html for {rel_path}");
            assert!(
                out_dir.join("bundle.js").exists(),
                "missing bundle.js for {rel_path}"
            );
            assert!(
                out_dir.join("runtime.js").exists(),
                "missing runtime.js for {rel_path}"
            );
            assert!(
                out_dir.join("stage.js").exists(),
                "missing stage.js for {rel_path}"
            );
            assert!(
                out_dir.join("accessibility.js").exists(),
                "missing accessibility.js for {rel_path}"
            );
            let index_html = std::fs::read_to_string(&index_path).expect("read index.html");
            assert!(
                index_html.contains("id=\"stage-a11y-overlay\""),
                "{rel_path} published index.html must include stage-a11y-overlay"
            );
        }

        // 2(f). Public FLA -> editable project -> HTML5 round-trip.
        let fla_path =
            workspace_root.join("examples/fla-corpus/upstream/ruffle/avm1-frame-labels/test.fla");
        let imported = neoflash_import::import_project(&fla_path)
            .expect("import public FLA fixture into editable project");
        imported
            .validate()
            .expect("imported public FLA project must validate");
        let _fla_frame0 = neoflash_core::evaluate(&imported, &imported.root, 0);
        let mut fla_player = neoflash_core::Player::new(&imported);
        fla_player.tick(&imported);

        let fla_out_dir = temp_root.join("pub-public-fla");
        let fla_index = neoflash_export::export_project_with_config(
            &imported,
            &fla_out_dir,
            &runtime_dir,
            &neoflash_export::PublishOptions::default(),
            &neoflash_export::HtmlPublishConfig::default(),
        )
        .expect("export imported public FLA project to HTML5");
        assert!(fla_index.exists());
        assert!(fla_out_dir.join("bundle.js").exists());

        // 3. Keyboard-only focus navigation, focus-ring gating (`keyboardFocusActive`), and
        // published `stage-a11y-overlay` ARIA DOM (`role`, `aria-label`, `tabindex`) smoke check.
        let btn_id = egui::Id::new("t278-focus-smoke-btn");
        let mut focus_ring_on_click = true;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: egui::pos2(24.0, 24.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (_, resp) =
                        ui.allocate_exact_size(egui::vec2(80.0, 28.0), egui::Sense::click());
                    ctx.memory_mut(|m| m.request_focus(resp.id));
                    focus_ring_on_click = neoflash_ui::theme::keyboard_focus(ui, &resp);
                });
            },
        );
        assert!(
            !focus_ring_on_click,
            "pointer press must clear keyboard-only focus ring gating"
        );

        let mut focus_ring_on_tab = false;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: Some(egui::Key::Tab),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let rect =
                        egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(80.0, 28.0));
                    let resp = ui.interact(rect, btn_id, egui::Sense::click());
                    ctx.memory_mut(|m| m.request_focus(btn_id));
                    focus_ring_on_tab = neoflash_ui::theme::keyboard_focus(ui, &resp);
                });
            },
        );
        assert!(
            focus_ring_on_tab,
            "Tab key navigation must enable keyboard focus ring"
        );

        let mut a11y_editor = Editor::from_project(&ctx, Project::blank());
        a11y_editor
            .project
            .set_document_accessibility(&neoflash_core::DocumentAccessibility {
                accessible: true,
                title: "Beta 1 Accessible Stage".to_string(),
                description: "Keyboard focusable interactive canvas".to_string(),
                auto_label: true,
                language: "en".to_string(),
                reading_order_top_down: true,
                focus_ring_visible: true,
            });
        a11y_editor.add_element(
            neoflash_core::ElementKind::Rectangle {
                width: 120.0,
                height: 36.0,
                fill: [60, 107, 240, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            48.0,
            48.0,
        );
        a11y_editor.set_accessibility(Some(neoflash_core::Accessibility {
            name: "Launch Game".to_string(),
            description: "Starts the interactive level".to_string(),
            role: neoflash_core::model::AccessibleRole::Button,
            tab_order: 1,
            disabled: false,
        }));
        let a11y_proj = a11y_editor.project.clone();

        let read_order_diags =
            neoflash_export::validate_published_accessibility_read_order(&a11y_proj, None, 0);
        assert!(
            read_order_diags.is_empty(),
            "accessible project read order must validate cleanly, got {read_order_diags:?}"
        );
        let a11y_nodes =
            neoflash_export::resolve_published_accessibility_overlay_nodes(&a11y_proj, None, 0);
        assert!(
            a11y_nodes.iter().any(|n| {
                n.role == "button"
                    && n.aria_label.as_deref() == Some("Launch Game")
                    && n.tab_index == 1
            }),
            "expected resolved accessible button node in overlay, got {a11y_nodes:?}"
        );
        let overlay_html = neoflash_export::render_accessibility_overlay_html(&a11y_proj, None, 0);
        assert!(overlay_html.contains("id=\"stage-a11y-overlay\""));
        assert!(overlay_html.contains("role=\"region\""));
        assert!(overlay_html.contains("aria-label=\"Stage accessibility overlay\""));
        assert!(overlay_html.contains("role=\"button\""));
        assert!(overlay_html.contains("aria-label=\"Launch Game\""));
        assert!(overlay_html.contains("tabindex=\"1\""));
        assert!(overlay_html.contains("data-focus-policy=\"keyboard-distinct\""));

        let a11y_out_dir = temp_root.join("pub-a11y-smoke");
        neoflash_export::export_project_with_config(
            &a11y_proj,
            &a11y_out_dir,
            &runtime_dir,
            &neoflash_export::PublishOptions::default(),
            &neoflash_export::HtmlPublishConfig::default(),
        )
        .expect("export accessible project");
        let stage_js = std::fs::read_to_string(a11y_out_dir.join("stage.js"))
            .expect("read published stage.js");
        let runtime_js = std::fs::read_to_string(a11y_out_dir.join("runtime.js"))
            .expect("read published runtime.js");
        let accessibility_js = std::fs::read_to_string(a11y_out_dir.join("accessibility.js"))
            .expect("read published accessibility.js");
        assert!(
            stage_js.contains("keyboardFocusRing") && stage_js.contains("focusReason === 'key'"),
            "published stage.js must gate focus ring on keyboardFocusRing / focusReason === 'key'"
        );
        assert!(
            stage_js.contains("syncAccessibilityOverlay")
                && stage_js.contains("aria-label")
                && stage_js.contains("tabindex")
                && runtime_js.contains("stage-a11y-overlay"),
            "published stage.js and runtime.js must manage stage-a11y-overlay ARIA DOM attributes"
        );
        assert!(
            !accessibility_js.is_empty(),
            "published accessibility.js must be present and non-empty"
        );

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn r096_help_prose_and_kbd_groups_avoid_literal_glyphs_and_use_platform_labels() {
        const GLYPHS: &[char] = &['⌘', '⌥', '⇧', '⌃', '↵', '⌤', '⌫', '⌦', '⇥'];
        for (chapter_idx, topics) in HELP_BROWSER_TOPICS.iter().enumerate() {
            for topic in *topics {
                assert!(
                    !topic.chars().any(|ch| GLYPHS.contains(&ch)),
                    "HELP_BROWSER_TOPICS[{chapter_idx}] must not bake literal shortcut glyphs into prose: {topic}"
                );
            }
        }
        let html = help_browser_html(neoflash_ui::Theme::Dark);
        assert!(
            !html.chars().any(|ch| GLYPHS.contains(&ch)),
            "generated browser Help HTML must not bake literal shortcut glyphs into HTML prose"
        );

        let ctx = egui::Context::default();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                render_kbd_group(ui, "⌘↵ / ⇧F12 / ⌥F2");
            });
        });
        let chip_text: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        let expected = if cfg!(target_os = "macos") {
            "⌘↵ / ⇧F12 / ⌥F2"
        } else {
            "Ctrl+Enter / Shift+F12 / Alt+F2"
        };
        assert_eq!(chip_text, vec![expected.to_owned()]);
    }

    // ── T-389: Scripting Reference v3 (API browser + guides) ──

    const WIDE: Rect = Rect::from_min_max(Pos2::ZERO, egui::pos2(1600.0, 1000.0));

    fn frame(
        editor: &mut Editor,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(WIDE),
                events,
                ..Default::default()
            },
            |ctx| editor.help_manual_dialog(ctx),
        )
    }

    fn window_state(ctx: &egui::Context) -> HelpManualState {
        ctx.data(|d| d.get_temp::<HelpManualState>(state_id()))
            .expect("Help window state")
    }

    fn window_rect(editor: &mut Editor, ctx: &egui::Context) -> Rect {
        let _ = frame(editor, ctx, vec![]);
        let _ = frame(editor, ctx, vec![]);
        ctx.memory(|m| {
            m.area_rect(egui::Id::new("neoflash-help-manual-modal"))
                .expect("Help window area")
        })
    }

    /// Text painted in a frame with the position of its first glyph row.
    fn painted(output: &egui::FullOutput) -> Vec<(String, Pos2)> {
        output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_owned(), t.pos)),
                _ => None,
            })
            .collect()
    }

    /// Accessible names of the window once it is laid out (the first frame only measures it).
    fn labels(editor: &mut Editor, ctx: &egui::Context) -> Vec<String> {
        let _ = frame(editor, ctx, vec![]);
        frame(editor, ctx, vec![])
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("AccessKit tree")
            .nodes
            .iter()
            // Static text carries its string as the value.
            .filter_map(|(_, node)| node.label().or(node.value()).map(str::to_owned))
            .collect()
    }

    fn project_in(language: neoflash_core::ScriptLanguage) -> Project {
        let mut project = Project::blank();
        project.script_language = Some(language);
        project
    }

    #[test]
    fn t389_reference_opens_in_the_project_language_with_exactly_three_choices() {
        use neoflash_core::ScriptLanguage as L;
        for (language, scope) in [
            (L::ActionScript1, LanguageScope::As1As2),
            (L::ActionScript2, LanguageScope::As1As2),
            (L::ActionScript3, LanguageScope::As3),
            (L::JavaScript, LanguageScope::JsTs),
            (L::TypeScript, LanguageScope::JsTs),
        ] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut editor = Editor::from_project(&ctx, project_in(language));
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(WIDE),
                    ..Default::default()
                },
                |_| {},
            );
            // The menu route knows nothing but the project.
            open_scripting_reference(&ctx);
            let labels = labels(&mut editor, &ctx);
            let st = window_state(&ctx);
            assert_eq!(st.lang_scope, scope, "{language:?}");
            assert_eq!(st.tab, ReferenceTab::Api);
            assert!(st.api.selected_class(scope).is_some());
            for choice in [
                "ActionScript 2",
                "ActionScript 3",
                "JavaScript / TypeScript",
            ] {
                assert!(labels.iter().any(|l| l == choice), "{choice} missing");
            }
            assert!(!labels.iter().any(|l| l.contains("All languages")));
            // The search field states what it searches, the badge which language the project uses.
            let scope_hint = format!("Search {}: ", scope.reference_label());
            assert!(
                labels.iter().any(|l| l.starts_with(&scope_hint)),
                "{labels:?}"
            );
            let badge = format!(
                "Project: {}",
                editor.project.effective_script_language().display_label()
            );
            assert!(labels.contains(&badge), "{badge} missing: {labels:?}");

            // A chosen language survives closing; the menu route returns to the project language.
            ctx.data_mut(|d| {
                d.get_temp_mut_or_default::<HelpManualState>(state_id())
                    .lang_scope =
                    LanguageScope::for_guide(if scope == LanguageScope::As3 { 16 } else { 12 });
            });
            let _ = frame(&mut editor, &ctx, vec![]);
            assert_ne!(window_state(&ctx).lang_scope, scope);
            editor.open_scripting_reference(&ctx);
            let _ = frame(&mut editor, &ctx, vec![]);
            assert_eq!(window_state(&ctx).lang_scope, scope);

            // The Guides tab says so in the same field.
            ctx.data_mut(|d| {
                d.get_temp_mut_or_default::<HelpManualState>(state_id()).tab = ReferenceTab::Guides;
            });
            let guides = frame(&mut editor, &ctx, vec![]);
            let hint = format!("Search guides for {}", scope.reference_label());
            assert!(
                painted(&guides).iter().any(|(text, _)| *text == hint),
                "{hint} missing"
            );
        }
    }

    #[test]
    fn t389_api_search_lists_class_members_and_a_click_opens_the_member_detail() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(
            &ctx,
            project_in(neoflash_core::ScriptLanguage::ActionScript3),
        );
        let _ = frame(&mut editor, &ctx, vec![]);
        editor.open_scripting_reference(&ctx);
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<HelpManualState>(state_id())
                .query = "gotoAndPlay".into();
        });
        let _ = frame(&mut editor, &ctx, vec![]);
        let results = frame(&mut editor, &ctx, vec![]);
        let texts = painted(&results);
        let (_, at) = texts
            .iter()
            .find(|(text, _)| text == "MovieClip.gotoAndPlay()")
            .unwrap_or_else(|| panic!("search must list MovieClip.gotoAndPlay(): {texts:?}"));
        // Typing never moves the selection; the hit opens on click.
        assert!(
            window_state(&ctx)
                .api
                .selected_member(LanguageScope::As3)
                .is_none()
        );
        let pos = *at + vec2(6.0, 4.0);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = frame(&mut editor, &ctx, vec![egui::Event::PointerMoved(pos)]);
        let _ = frame(&mut editor, &ctx, vec![button(true)]);
        let _ = frame(&mut editor, &ctx, vec![button(false)]);
        let detail = frame(&mut editor, &ctx, vec![]);

        let st = window_state(&ctx);
        let class = st.api.selected_class(LanguageScope::As3).expect("class");
        assert_eq!(class.key, "flash.display.MovieClip");
        let member = st.api.selected_member(LanguageScope::As3).expect("member");
        assert_eq!(member.label, "gotoAndPlay()");
        assert_eq!(
            member.declaration,
            "public function gotoAndPlay(\n    frame:Object,\n    scene:String = null\n):void"
        );
        assert_eq!(
            member.support,
            Some(super::super::help_as_reference::Support::Working)
        );
        let mut texts = painted(&detail);
        if let Some(pos) = window_state(&ctx).reading_position
            && let Some(mut scroll) = egui::scroll_area::State::load(&ctx, pos.area)
        {
            scroll.offset.y = 320.0;
            scroll.store(&ctx, pos.area);
            let bottom = frame(&mut editor, &ctx, vec![]);
            texts.extend(painted(&bottom));
        }
        let shown = |needle: &str| texts.iter().any(|(text, _)| text == needle);
        assert!(shown(&member.usage), "usage box missing: {texts:?}");
        assert!(
            shown(&member.declaration),
            "signature box missing: {texts:?}"
        );
        assert!(shown("frame") && shown("scene"), "parameters missing");
        assert!(shown(&member.example), "example missing");
        assert!(shown("Insert at cursor"));
        assert!(shown("gotoAndStop()"), "See also link missing");
    }

    #[test]
    fn reader_pointer_resize_maximize_restore_preserves_route_and_search() {
        use crate::tests::primary_button;
        use neoflash_ui::metrics::{Density, set_density};

        fn drag(editor: &mut Editor, ctx: &egui::Context, from: Pos2, to: Pos2) {
            frame(editor, ctx, vec![egui::Event::PointerMoved(from)]);
            frame(editor, ctx, vec![primary_button(from, true)]);
            frame(editor, ctx, vec![egui::Event::PointerMoved(to)]);
            frame(editor, ctx, vec![primary_button(to, false)]);
        }

        fn double_click_header(editor: &mut Editor, ctx: &egui::Context) {
            let at =
                window_rect(editor, ctx).center_top() + vec2(0.0, metrics::dialog_header() / 2.0);
            // The second double-click is a separate gesture, after egui's triple-click interval.
            let later = ctx.input(|i| i.time + 1.0);
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(WIDE),
                    time: Some(later),
                    events: vec![egui::Event::PointerMoved(at)],
                    ..Default::default()
                },
                |ctx| editor.help_manual_dialog(ctx),
            );
            for _ in 0..2 {
                frame(editor, ctx, vec![primary_button(at, true)]);
                frame(editor, ctx, vec![primary_button(at, false)]);
            }
        }

        fn scroll_to(editor: &mut Editor, ctx: &egui::Context, offset: f32) -> f32 {
            let id = window_state(ctx).reading_position.unwrap().area;
            let mut scroll = egui::scroll_area::State::load(ctx, id).unwrap();
            scroll.offset.y = offset;
            scroll.store(ctx, id);
            frame(editor, ctx, vec![]);
            window_state(ctx).reading_position.unwrap().visible_y
        }

        for density in Density::ALL {
            set_density(density);
            let ctx = egui::Context::default();
            let mut editor = Editor::from_project(
                &ctx,
                project_in(neoflash_core::ScriptLanguage::ActionScript3),
            );
            for reference in [false, true] {
                if reference {
                    editor.open_scripting_reference_for_token(&ctx, "BitmapData.drawWithQuality");
                } else {
                    open_help_manual(&ctx);
                }
                ctx.data_mut(|d| {
                    d.get_temp_mut_or_default::<HelpManualState>(state_id())
                        .query = "frame".into()
                });
                let initial = window_rect(&mut editor, &ctx);
                let page = window_state(&ctx).page;
                let member = |state: &HelpManualState| {
                    state
                        .api
                        .selected_member(LanguageScope::As3)
                        .map(|m| m.key.clone())
                };
                let selected = member(&window_state(&ctx));
                if reference {
                    assert!(selected.is_some(), "reference must have a selected member");
                }
                let from = initial.right_bottom() - vec2(2.0, 2.0);
                let delta = vec2(50.0, 42.0);
                drag(&mut editor, &ctx, from, from + delta);
                let grown = window_rect(&mut editor, &ctx);
                assert_eq!(grown.min, initial.min);
                assert_eq!(grown.max, initial.max + delta);
                let left = grown.left_center() + vec2(2.0, 0.0);
                drag(&mut editor, &ctx, left, left + vec2(96.0, 0.0));
                let resized = window_rect(&mut editor, &ctx);
                assert_eq!(resized.min, grown.min + vec2(96.0, 0.0));
                assert_eq!(resized.max, grown.max);

                let y = scroll_to(&mut editor, &ctx, 200.0);
                assert!(
                    y > 0.0,
                    "{density:?} reference={reference}: content must scroll"
                );
                double_click_header(&mut editor, &ctx);
                assert_eq!(window_rect(&mut editor, &ctx), WIDE);
                double_click_header(&mut editor, &ctx);
                assert_eq!(window_rect(&mut editor, &ctx), resized);
                let state = window_state(&ctx);
                assert_eq!(state.page, page);
                assert_eq!(state.query, "frame");
                assert_eq!(member(&state), selected);
                assert!((state.reading_position.unwrap().visible_y - y).abs() < 1.0);
                // Scrolling again in the restored viewport supersedes the original anchor.
                let new_y = scroll_to(&mut editor, &ctx, y / 2.0);
                assert!(new_y < y);
                double_click_header(&mut editor, &ctx);
                double_click_header(&mut editor, &ctx);
                assert_eq!(window_rect(&mut editor, &ctx), resized);
                assert!(
                    (window_state(&ctx).reading_position.unwrap().visible_y - new_y).abs() < 1.0
                );

                let small = Rect::from_min_size(Pos2::ZERO, vec2(700.0, 480.0));
                for _ in 0..2 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(small),
                            ..Default::default()
                        },
                        |ctx| editor.help_manual_dialog(ctx),
                    );
                }
                let actual = ctx.memory(|m| {
                    m.area_rect(egui::Id::new("neoflash-help-manual-modal"))
                        .unwrap()
                });
                assert!(small.contains_rect(actual));
                let close = actual.right_top()
                    + vec2(
                        -space::s4() - metrics::control() / 2.0,
                        metrics::dialog_header() / 2.0,
                    );
                for event in [
                    egui::Event::PointerMoved(close),
                    primary_button(close, true),
                    primary_button(close, false),
                ] {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(small),
                            events: vec![event],
                            ..Default::default()
                        },
                        |ctx| editor.help_manual_dialog(ctx),
                    );
                }
                assert!(
                    !window_state(&ctx).open,
                    "Close remains reachable in a small viewport"
                );
            }
        }
        set_density(Density::Comfortable);
    }

    #[test]
    fn t389_window_rect_is_one_size_per_density_across_tabs_and_selections() {
        use neoflash_ui::metrics::{Density, set_density};
        for density in Density::ALL {
            set_density(density);
            let ctx = egui::Context::default();
            let mut editor = Editor::from_project(
                &ctx,
                project_in(neoflash_core::ScriptLanguage::ActionScript3),
            );
            let _ = frame(&mut editor, &ctx, vec![]);
            editor.open_scripting_reference(&ctx);
            let api_class = window_rect(&mut editor, &ctx);
            assert_eq!(
                api_class.size(),
                vec2(metrics::reference_width(), metrics::dialog_xl_height()),
                "{density:?}"
            );

            let mut rects = vec![("API, default class", api_class)];
            let mut with = |label: &'static str, change: &dyn Fn(&mut HelpManualState)| {
                ctx.data_mut(|d| change(d.get_temp_mut_or_default::<HelpManualState>(state_id())));
                rects.push((label, window_rect(&mut editor, &ctx)));
            };
            with("API, member selected", &|s| {
                let hit = super::super::help_as_reference::api_search(
                    LanguageScope::As3,
                    "Graphics.drawRoundRect",
                )
                .remove(0);
                s.api.select(
                    LanguageScope::As3,
                    &hit.class_key,
                    hit.member_key.as_deref(),
                );
            });
            with("API, another class", &|s| {
                s.api.select(LanguageScope::As3, "flash.ui.Keyboard", None);
            });
            with("API, search results", &|s| s.query = "draw".into());
            with("API, other language", &|s| {
                s.query.clear();
                s.lang_scope = LanguageScope::JsTs;
            });
            with("Guides, first chapter", &|s| {
                s.lang_scope = LanguageScope::As3;
                s.tab = ReferenceTab::Guides;
                s.page = 12;
            });
            with("Guides, another chapter", &|s| s.page = 15);
            with("Guides, search", &|s| s.query = "function".into());
            for (label, rect) in &rects {
                assert_eq!(
                    *rect, api_class,
                    "{density:?}: {label} moved or resized the window"
                );
            }

            // Help keeps the same shell.
            open_help_manual(&ctx);
            let help = window_rect(&mut editor, &ctx);
            set_help_manual_page(&ctx, 6);
            assert_eq!(help, window_rect(&mut editor, &ctx));
            assert_eq!(help.size(), api_class.size(), "{density:?}");
        }
        set_density(Density::Comfortable);
    }

    #[test]
    fn t389_insert_at_cursor_needs_a_script_editor_and_reuses_the_toolbox_insertion() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(
            &ctx,
            project_in(neoflash_core::ScriptLanguage::ActionScript3),
        );
        let _ = frame(&mut editor, &ctx, vec![]);
        editor.open_scripting_reference(&ctx);
        let hit =
            super::super::help_as_reference::api_search(LanguageScope::As3, "MouseEvent.CLICK")
                .remove(0);
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<HelpManualState>(state_id())
                .api
                .select(
                    LanguageScope::As3,
                    &hit.class_key,
                    hit.member_key.as_deref(),
                );
        });
        let insert = || egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };

        // No script editor on screen: the shortcut and the button do nothing.
        editor.set_panel_visible(crate::workspace::PanelId::Actions, false);
        assert!(!editor.reference_insert_available());
        let _ = frame(&mut editor, &ctx, vec![insert()]);
        assert_eq!(editor.actions_ui.pending, None);

        // With Actions open the member goes through the toolbox insertion: static members are
        // written through their class and bring their import.
        editor.open_actions();
        assert!(editor.reference_insert_available());
        let _ = frame(&mut editor, &ctx, vec![insert()]);
        assert_eq!(
            editor.actions_ui.pending,
            Some(crate::code_tools::CodeCommand::Insert {
                text: "MouseEvent.CLICK".into(),
                caret: 16,
                import: Some("flash.events.MouseEvent".into()),
            })
        );
        assert!(
            is_help_manual_open(&ctx),
            "inserting keeps the reference open"
        );
    }

    #[test]
    fn t389_keyboard_moves_through_tree_and_members() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(
            &ctx,
            project_in(neoflash_core::ScriptLanguage::ActionScript3),
        );
        let _ = frame(&mut editor, &ctx, vec![]);
        editor.open_scripting_reference(&ctx);
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<HelpManualState>(state_id())
                .query = "gotoAnd".into();
        });
        let _ = frame(&mut editor, &ctx, vec![]);
        // ↵ opens the first result (prefix matches, alphabetical) and moves to its member list.
        let _ = frame(&mut editor, &ctx, vec![key(egui::Key::Enter)]);
        let st = window_state(&ctx);
        assert_eq!(
            st.api
                .selected_member(LanguageScope::As3)
                .map(|m| m.name.as_str()),
            Some("gotoAndPlay")
        );
        assert!(st.api.focus_members);
        // ↓ then walks the members of the class; ← returns to the results.
        let _ = frame(&mut editor, &ctx, vec![key(egui::Key::ArrowDown)]);
        let st = window_state(&ctx);
        assert_eq!(
            st.api
                .selected_member(LanguageScope::As3)
                .map(|m| m.name.as_str()),
            Some("gotoAndStop")
        );
        let _ = frame(&mut editor, &ctx, vec![key(egui::Key::ArrowLeft)]);
        assert!(!window_state(&ctx).api.focus_members);

        // Guides: ↑↓ move through the guide list of the language.
        ctx.data_mut(|d| {
            let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
            s.query.clear();
            s.tab = ReferenceTab::Guides;
            s.page = 12;
        });
        let _ = frame(&mut editor, &ctx, vec![key(egui::Key::ArrowDown)]);
        assert_eq!(window_state(&ctx).page, 13);
        let _ = frame(&mut editor, &ctx, vec![key(egui::Key::ArrowUp)]);
        assert_eq!(window_state(&ctx).page, 12);
    }

    #[test]
    fn t389_guides_keep_every_heading_table_and_sample_as_plain_blocks() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(
            &ctx,
            project_in(neoflash_core::ScriptLanguage::ActionScript2),
        );
        let _ = frame(&mut editor, &ctx, vec![]);
        open_scripting_reference_page(&ctx, 10);
        let labels = labels(&mut editor, &ctx);
        let st = window_state(&ctx);
        assert_eq!(
            (st.lang_scope, st.tab),
            (LanguageScope::As1As2, ReferenceTab::Guides)
        );
        // The guide list is per language, without the manual's chapter numbers.
        for guide in [
            "AS1 & AS2 Timeline",
            "AS2 Core Classes",
            "Language Migration",
        ] {
            assert!(
                labels.iter().any(|l| l == guide),
                "{guide} missing: {labels:?}"
            );
        }
        assert!(!labels.iter().any(|l| l == "AS3 Display List"));
        // "On this page" lists the section, the tables and the samples of the chapter.
        for heading in [
            "Global Timeline Navigation & Frame Control",
            "Core AS1 / AS2 Timeline Functions & MovieClip Methods",
            "ActionScript 1.0 (Flash 4 / Flash 5) Slash Paths, Legacy Operators & Directives",
            "AS1 / AS2 function Statement, arguments Object & Function Class (Full Surface)",
            "AS2 Case 7 — Complete 1-Frame Game Pattern (onPress/onRelease, onEnterFrame, Key, setInterval & trace)",
            "AS2 Frame Script — Spawning Library Symbols, EnterFrame Physics & Hit Testing",
        ] {
            assert!(
                labels.iter().any(|l| l == heading),
                "outline lost “{heading}”"
            );
        }
        // No tag chips, chapter counter or dialect badges are left.
        assert!(!labels.iter().any(|l| {
            l.starts_with('#') || l.starts_with("Ref Ch.") || l.starts_with("AS1 / AS2 1 of")
        }));
        assert!(!labels.iter().any(|l| l == "[AS1/AS2]" || l == "Tags:"));
    }

    #[test]
    fn t414_reference_insert_available_on_standalone_as_tab_when_actions_panel_closed() {
        let tmp = std::env::temp_dir().join(format!(
            "neoflash-t414-help-ref-{}-{}",
            std::process::id(),
            neoflash_core::new_id("dir")
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let as_path = tmp.join("Widget.as");
        std::fs::write(&as_path, "package {\n    public class Widget {}\n}\n").unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.sessions.clear();
        editor.set_panel_visible(crate::workspace::PanelId::Actions, false);
        assert!(
            !editor
                .workspace
                .is_panel_active(crate::workspace::PanelId::Actions)
        );
        assert!(editor.open_standalone_script_session(&ctx, as_path));
        assert!(editor.is_standalone_script_session());
        assert!(
            editor.reference_insert_available(),
            "Insert at cursor must be enabled on standalone .as tabs even when Actions panel is closed"
        );

        editor.open_scripting_reference(&ctx);
        let hit =
            super::super::help_as_reference::api_search(LanguageScope::As3, "MouseEvent.CLICK")
                .remove(0);
        ctx.data_mut(|d| {
            let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
            s.lang_scope = LanguageScope::As3;
            s.tab = ReferenceTab::Api;
            s.api.select(
                LanguageScope::As3,
                &hit.class_key,
                hit.member_key.as_deref(),
            );
        });
        let insert_ev = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        let _ = frame(&mut editor, &ctx, vec![insert_ev]);
        assert!(
            matches!(
                editor.actions_ui.pending,
                Some(crate::code_tools::CodeCommand::Insert { .. })
            ),
            "⌘↵ in Scripting Reference must queue Insert command on standalone .as tab"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn t440_selectable_text_drag_cmd_c_and_edit_copy_across_help_and_reference_with_stage_selection()
     {
        use crate::commands::CommandId;
        use crate::macos_menu::{MenuSyncFingerprint, NativeMenuSpec};
        use crate::tests::primary_button;

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(
            &ctx,
            project_in(neoflash_core::ScriptLanguage::ActionScript3),
        );
        // Place and select a Rectangle on Stage behind the modal!
        editor.add_element(
            neoflash_core::ElementKind::Rectangle {
                width: 140.0,
                height: 90.0,
                fill: neoflash_core::Color::default(),
                stroke: neoflash_core::Color::default(),
                stroke_width: 1.0,
            },
            120.0,
            100.0,
        );
        assert!(
            !editor.selection.is_empty(),
            "Stage must have an active object selection behind the Help/Reference modal"
        );
        // While no modal is open, Stage selection enables Edit ▸ Copy.
        assert!(
            editor
                .command_menu_state(Some(&ctx), CommandId::EditCopy)
                .enabled
        );

        // Open Scripting Reference: with no text/label selected yet, Edit ▸ Copy MUST be disabled
        // so invoking Copy never overwrites the clipboard with background Stage artwork!
        editor.open_scripting_reference(&ctx);
        let _ = frame(&mut editor, &ctx, vec![]);
        assert!(!crate::clipboard::has_egui_label_selection(&ctx));
        assert!(
            !editor
                .command_menu_state(Some(&ctx), CommandId::EditCopy)
                .enabled,
            "Edit ▸ Copy must not be enabled for a background Stage selection while Help/Reference modal is open"
        );
        let menu_without_sel = NativeMenuSpec::build(&editor, Some(&ctx));
        assert!(matches!(
            menu_without_sel.find_command(CommandId::EditCopy),
            Some(crate::macos_menu::NativeMenuItemSpec::Action { enabled: false, .. })
        ));
        assert!(!editor.execute_menu_command(&ctx, CommandId::EditCopy));

        let drag_and_copy = |editor: &mut Editor,
                             ctx: &egui::Context,
                             needle: &str,
                             start_char: usize,
                             end_char: usize,
                             use_menu_cmd: bool|
         -> String {
            let out = frame(editor, ctx, vec![]);
            let (pos, galley) = out
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == needle => {
                        Some((t.pos, std::sync::Arc::clone(&t.galley)))
                    }
                    _ => None,
                })
                .or_else(|| {
                    out.shapes.iter().find_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.text().contains(needle) => {
                            Some((t.pos, std::sync::Arc::clone(&t.galley)))
                        }
                        _ => None,
                    })
                })
                .unwrap_or_else(|| panic!("expected painted text containing {needle:?}"));
            let p0 = pos
                + galley
                    .pos_from_cursor(egui::text::CCursor::new(start_char))
                    .center()
                    .to_vec2();
            let p1 = pos
                + galley
                    .pos_from_cursor(egui::text::CCursor::new(end_char))
                    .center()
                    .to_vec2();

            ctx.plugin::<egui::text_selection::LabelSelectionState>()
                .lock()
                .clear_selection();
            let fp_before = MenuSyncFingerprint::capture(editor, Some(ctx));
            let _ = frame(editor, ctx, vec![egui::Event::PointerMoved(p0)]);
            let _ = frame(editor, ctx, vec![primary_button(p0, true)]);
            let _ = frame(editor, ctx, vec![egui::Event::PointerMoved(p1)]);
            let _ = frame(editor, ctx, vec![primary_button(p1, false)]);

            assert!(
                crate::clipboard::has_egui_label_selection(ctx),
                "dragging across {needle:?} must create an active LabelSelectionState"
            );
            let fp_after = MenuSyncFingerprint::capture(editor, Some(ctx));
            assert_ne!(
                fp_before, fp_after,
                "macOS menu fingerprint must update immediately when label selection becomes active"
            );
            assert!(
                editor
                    .command_menu_state(Some(ctx), CommandId::EditCopy)
                    .enabled,
                "Edit ▸ Copy must enable when Help/Reference label text is selected"
            );
            let menu_with_sel = NativeMenuSpec::build(editor, Some(ctx));
            assert!(matches!(
                menu_with_sel.find_command(CommandId::EditCopy),
                Some(crate::macos_menu::NativeMenuItemSpec::Action { enabled: true, .. })
            ));

            let copy_out = if use_menu_cmd {
                assert!(editor.execute_menu_command(ctx, CommandId::EditCopy));
                frame(editor, ctx, vec![])
            } else {
                let cmd_c = egui::Event::Key {
                    key: egui::Key::C,
                    physical_key: Some(egui::Key::C),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                };
                frame(editor, ctx, vec![cmd_c])
            };
            let copied = copy_out
                .platform_output
                .commands
                .iter()
                .find_map(|cmd| match cmd {
                    egui::OutputCommand::CopyText(t) => Some(t.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    panic!(
                        "copying selection on {needle:?} (menu={use_menu_cmd}) must emit CopyText"
                    )
                });
            assert!(
                !copied.contains("\"elements\""),
                "must copy Help text, never Stage JSON payload: {copied}"
            );
            copied
        };

        // 1–4. Scripting Reference API detail: code_box, description, Parameters, and In NeoFlash!
        ctx.data_mut(|d| {
            let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
            s.lang_scope = LanguageScope::As3;
            s.tab = ReferenceTab::Api;
            let hit = super::super::help_as_reference::api_search(
                LanguageScope::As3,
                "Array.lastIndexOf",
            )
            .remove(0);
            s.api.select(
                LanguageScope::As3,
                &hit.class_key,
                hit.member_key.as_deref(),
            );
        });
        let _ = frame(&mut editor, &ctx, vec![]);

        // (a) code_box (`Usage` / `Signature`) via ⌘C
        let copied_code = drag_and_copy(
            &mut editor,
            &ctx,
            "public function lastIndexOf",
            7,
            27,
            false,
        );
        assert_eq!(copied_code, "function lastIndexOf");

        // (b) member.description via Edit ▸ Copy menu command (while Rectangle is selected on Stage!)
        let desc = window_state(&ctx)
            .api
            .selected_member(LanguageScope::As3)
            .unwrap()
            .description
            .clone();
        let expected_desc: String = desc.chars().take(12).collect();
        let copied_desc = drag_and_copy(&mut editor, &ctx, &expected_desc, 0, 12, true);
        assert_eq!(copied_desc, expected_desc);

        // (c) Parameter name (`searchElement`) via ⌘C
        let copied_param = drag_and_copy(&mut editor, &ctx, "searchElement", 0, 13, false);
        assert_eq!(copied_param, "searchElement");

        // (d) In NeoFlash status message (including Partial boundary note on JS/TS `import`) via Edit ▸ Copy
        ctx.data_mut(|d| {
            let s = d.get_temp_mut_or_default::<HelpManualState>(state_id());
            s.lang_scope = LanguageScope::JsTs;
            s.tab = ReferenceTab::Api;
            s.api.select(
                LanguageScope::JsTs,
                "Modules & JSX",
                Some("Modules & JSX|import"),
            );
        });
        let _ = frame(&mut editor, &ctx, vec![]);
        let copied_status = drag_and_copy(&mut editor, &ctx, "Partial.", 0, 8, true);
        assert_eq!(copied_status, "Partial.");

        let scroll_text_into_view = |editor: &mut Editor, ctx: &egui::Context, needle: &str| {
            let win = window_rect(editor, ctx);
            let id = window_state(ctx).reading_position.unwrap().area;
            for step in 0..16 {
                let out = frame(editor, ctx, vec![]);
                if let Some(y) = out.shapes.iter().find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == needle => Some(t.pos.y),
                    _ => None,
                }) {
                    let mut scroll = egui::scroll_area::State::load(ctx, id).unwrap();
                    scroll.offset.y = (scroll.offset.y + (y - win.center().y)).max(0.0);
                    scroll.store(ctx, id);
                    let _ = frame(editor, ctx, vec![]);
                    return;
                }
                let mut scroll = egui::scroll_area::State::load(ctx, id).unwrap();
                scroll.offset.y = (step as f32 + 1.0) * 260.0;
                scroll.store(ctx, id);
            }
            panic!("could not scroll {needle:?} into view");
        };

        // 5a. Scripting Reference Guides `api_signature_table` (Ch.13 DisplayObjectContainer table)
        open_scripting_reference_page(&ctx, 12);
        let _ = frame(&mut editor, &ctx, vec![]);
        scroll_text_into_view(&mut editor, &ctx, "getObjectsUnderPoint");
        let copied_guide_table =
            drag_and_copy(&mut editor, &ctx, "getObjectsUnderPoint", 0, 20, false);
        assert_eq!(copied_guide_table, "getObjectsUnderPoint");

        // 5b. NeoFlash Help Ch.10 50-chord `<ESC>` `api_signature_table` + multi-cell row drag + Escape deselection!
        set_help_manual_page(&ctx, 9);
        let _ = frame(&mut editor, &ctx, vec![]);
        scroll_text_into_view(&mut editor, &ctx, "<ESC>fn");
        let out = frame(&mut editor, &ctx, vec![]);
        let esc_fn_pos = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "<ESC>fn" => Some(t.pos),
                _ => None,
            })
            .expect("<ESC>fn cell in Ch.10 table");
        let purpose_end = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "Declare named function block" => {
                    Some(t.pos + t.galley.size())
                }
                _ => None,
            })
            .expect("Declare named function block cell in Ch.10 table");
        let start = esc_fn_pos + vec2(1.0, 4.0);
        let end = purpose_end - vec2(1.0, 2.0);
        let _ = frame(&mut editor, &ctx, vec![egui::Event::PointerMoved(start)]);
        let _ = frame(&mut editor, &ctx, vec![primary_button(start, true)]);
        let _ = frame(&mut editor, &ctx, vec![egui::Event::PointerMoved(end)]);
        let _ = frame(&mut editor, &ctx, vec![primary_button(end, false)]);
        assert!(crate::clipboard::has_egui_label_selection(&ctx));
        assert!(editor.execute_menu_command(&ctx, CommandId::EditCopy));
        let row_copy_out = frame(&mut editor, &ctx, vec![]);
        let row_copied = row_copy_out
            .platform_output
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                egui::OutputCommand::CopyText(t) => Some(t.clone()),
                _ => None,
            })
            .expect("dragging across Ch.10 <ESC>fn row and invoking Edit ▸ Copy must copy text");
        assert!(
            row_copied.contains("fn")
                && row_copied.contains("function name ( ) { ... }")
                && row_copied.contains("Declare named function block"),
            "unexpected multi-cell table copy: {row_copied:?}"
        );

        // First Escape clears the active text selection WITHOUT closing NeoFlash Help; second Escape closes it!
        let esc = || egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = frame(&mut editor, &ctx, vec![esc()]);
        assert!(
            !crate::clipboard::has_egui_label_selection(&ctx),
            "first Escape must clear the label text selection"
        );
        assert!(
            is_help_manual_open(&ctx),
            "first Escape while text is selected must NOT close Help"
        );
        let _ = frame(&mut editor, &ctx, vec![esc()]);
        assert!(
            !is_help_manual_open(&ctx),
            "second Escape with no active text selection closes Help"
        );
        // Immediately after Help closes, Edit ▸ Copy re-enables for the selected Stage Rectangle!
        assert!(
            editor
                .command_menu_state(Some(&ctx), CommandId::EditCopy)
                .enabled,
            "Edit ▸ Copy must immediately re-enable for the selected Stage object once Help closes"
        );
        assert!(editor.execute_menu_command(&ctx, CommandId::EditCopy));
    }
}
