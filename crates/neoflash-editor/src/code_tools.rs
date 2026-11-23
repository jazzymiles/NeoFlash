//! Text commands of the Actions toolbar: comments and auto-format.
//!
//! Pure functions over `(source, selection)` so they are unit-tested without egui. Selections are
//! char ranges (`start <= end`), as in egui's `CCursorRange`. Every command returns the new text and
//! selection; a command that changes nothing returns `None` so no history entry is created.
//!
//! Auto-format is token-safe and idempotent across AS1/AS2/AS3/JS/TS: it normalizes indentation,
//! operator/delimiter/statement spacing, and accidental statement-ending colons while keeping
//! strings, comments, regexes, `#include` directives and E4X/XML literals verbatim.

mod commands_edits;
mod completion;
mod esc_chords;
mod fmt_layout;
mod fmt_lexer;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub(crate) use commands_edits::{
    CodeCommand, Edit, apply_quick_fix, apply_quick_fix_edit, apply_with_tab_size, auto_close_char,
    auto_delete_pair, block_comment, indent_tab, insert_as3_import_line, is_auto_close_trigger,
    line_comment_with_tab_size, outdent_tab, suggest_quick_fixes_for_diagnostic, uncomment,
};
#[cfg(test)]
pub(crate) use commands_edits::{apply, line_comment};
#[allow(unused_imports)]
use commands_edits::{
    char_to_byte, find_package_open_brace, is_escaped_at, line_span, replace_lines, visual_indent,
};
#[allow(unused_imports)]
pub(crate) use completion::{
    CompletionItem, CompletionKind, ExtractedTypeInfo, complete_at_caret,
    diagnostics_and_quick_fixes, extract_types_and_members, infer_local_variable_types,
};
#[cfg(test)]
pub(crate) use esc_chords::expand_esc_chord;
#[allow(unused_imports)]
pub(crate) use esc_chords::{
    ESC_CHORD_ARMED_PROMPT, ESC_CHORD_CANCELLED_STATUS, ESC_CHORD_TIMEOUT_SECS, ESC_CHORDS,
    EscChordPhase, EscChordSelection, EscChordSpec, EscChordStepOutcome,
    esc_chord_first_char_prompt, expand_esc_chord_with_spec, find_esc_chord,
    is_esc_chord_first_char, normalize_esc_chord_char, step_esc_chord_input,
};
#[allow(unused_imports)]
pub(crate) use fmt_layout::auto_format_with_tab_size;
#[cfg(test)]
pub(crate) use fmt_lexer::auto_format;
#[allow(unused_imports)]
use fmt_lexer::{
    EXPR_PRECEDING_KEYWORDS, FMT_TYPE_KEYWORDS, FmtKind, FmtScope, FmtTok, MULTI_PUNCTS, PunctRole,
    SPACE_BEFORE_PAREN_KEYWORDS, can_precede_regex_or_unary, find_generic_close_on_line,
    fmt_ident_char, fmt_ident_start, lex_fmt_tokens, paren_followed_by_method_sig, try_scan_regex,
    try_scan_xml_literal,
};
