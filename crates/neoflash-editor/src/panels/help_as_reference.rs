//! Scripting Reference (`Help ▸ Scripting Reference…`): the API browser over the ActionScript 2 / 3
//! declaration index and the JavaScript / TypeScript catalogue, the guide chapters (11–19 of the
//! manual), the shared JS/TS Actions toolbox catalogue, and the SVG-illustrated HTML5 browser
//! edition of Help and the reference.
//!
//! The window shell is `help_manual.rs`.

mod api_browser_ui;
mod api_examples;
mod api_index;
mod browser_html;
mod chapters_as;
mod chapters_web;
mod js_ts_catalog;
mod samples_search;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub(crate) use api_browser_ui::{
    ApiBrowserState, ApiSelection, NO_SCRIPT_EDITOR, RefActions, RefKeys, RefRow, api_class_column,
    api_detail_column, api_footer_text, api_selected_insertion, api_tree_column, code_box,
    group_caption, group_caption_with_copy, group_thousands, render_topic_hits,
    set_insert_available, take_article_insert,
};
#[allow(unused_imports)]
pub(crate) use api_index::{
    API_GLOBAL_KEY, API_LANGUAGE_KEY, ApiClass, ApiHit, ApiIndex, ApiLink, ApiMember, ApiPackage,
    MemberFilter, MemberGroup, Support, api_index, api_search, scope_script_language,
};
#[cfg(test)]
pub(crate) use browser_html::render_full_browser_help_html;
pub(crate) use browser_html::{
    render_browser_help_html_for_project, render_browser_help_html_for_route,
};
pub(crate) use chapters_as::{
    chapter_as1_as2_fundamentals, chapter_as2_core_classes, chapter_as3_display_list,
    chapter_as3_events_lifecycle, chapter_as3_graphics_filters_sound,
};
pub(crate) use chapters_web::{
    chapter_as_migration_guide, chapter_browser_web_apis, chapter_javascript_runtime,
    chapter_typescript_checker,
};
pub(crate) use js_ts_catalog::{
    JS_TS_API_ENTRIES, JS_TS_CATEGORIES, JsTsApiEntry, JsTsEntryKind, search_js_ts_entries,
};
#[allow(unused_imports)]
pub(crate) use samples_search::{
    AS2_FN_ARGUMENTS_CALL_APPLY_SAMPLE, AS2_FN_DECLARATIONS_AND_LITERALS_SAMPLE,
    AS2_FN_PROTOTYPE_AND_CLOSURES_SAMPLE, AS2_ONE_FRAME_GAME_LOOP_SAMPLE,
    AS3_FUNCTIONS_REST_CALL_CLOSURES_SAMPLE, CLASSIC_ESC_CHORDS, CodeSample, ComputedSearchHit,
    JS_FUNCTIONS_CLOSURES_AND_CALL_SAMPLE, REFERENCE_SEARCH_ENTRIES, ReferenceTopicEntry,
    TS_TYPED_AND_GENERIC_FUNCTIONS_SAMPLE, api_signature_table, chapter_matches_search_deep,
    code_block, render_esc_chords_reference, search_reference_hits,
};
#[cfg(test)]
pub(crate) use tests::verify_all_reference_code_samples_compile;

use eframe::egui;

/// Formats an `egui::Color32` as a CSS `rgb(...)` or `rgba(...)` functional color string.
/// Uses decimal channel notation so no literal hex strings exist in source.
pub(crate) fn css_rgb(color: egui::Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a == 255 {
        format!("rgb({r}, {g}, {b})")
    } else {
        format!("rgba({r}, {g}, {b}, {:.3})", f32::from(a) / 255.0)
    }
}

/// All filterable topic tags across the 19 Help & Scripting Reference chapters.
pub(crate) const HELP_ALL_TAGS: &[&str] = &[
    "Authoring",
    "Stage",
    "Vector",
    "Tools",
    "Symbols",
    "Timeline",
    "Animation",
    "Text",
    "Audio",
    "Export",
    "AS1",
    "AS2",
    "AS3",
    "JavaScript",
    "TypeScript",
    "WebAPI",
    "Migration",
    "DisplayList",
    "Events",
    "Graphics",
];
