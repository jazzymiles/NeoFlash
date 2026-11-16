//! Shared compiler language service, contextual token extraction, and concise keyword/class
//! reference summaries for Actions and Scripting Reference.
pub(crate) use neoflash_script::language_service::*;

use neoflash_core::ScriptLanguage;

fn is_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '#')
}

/// Extracts a clean identifier, directive, or dotted `Class.member` lookup token from `source`
/// using the active selection (`primary_char != secondary_char`) or caret position.
pub(crate) fn extract_reference_token(
    source: &str,
    primary_char: usize,
    secondary_char: usize,
) -> Option<String> {
    let chars: Vec<char> = source.chars().collect();
    if chars.is_empty() {
        return None;
    }

    if primary_char != secondary_char {
        let lo = primary_char.min(secondary_char).min(chars.len());
        let hi = primary_char.max(secondary_char).min(chars.len());
        let selected: String = chars[lo..hi].iter().collect();
        let trimmed = selected.trim();
        if !trimmed.is_empty()
            && trimmed.chars().count() <= 48
            && !trimmed.contains('\n')
            && trimmed
                .chars()
                .all(|c| is_token_char(c) || matches!(c, '.' | '<' | '>' | '/'))
        {
            return Some(trimmed.to_owned());
        }
        return None;
    }

    let idx = primary_char.min(chars.len());
    let anchor = if idx < chars.len() && is_token_char(chars[idx]) {
        idx
    } else if idx > 0 && is_token_char(chars[idx - 1]) {
        idx - 1
    } else {
        return None;
    };

    let mut start = anchor;
    while start > 0 && is_token_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = anchor + 1;
    while end < chars.len() && is_token_char(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    if word.is_empty() || word.chars().all(|c| c.is_ascii_digit()) {
        None
    } else {
        Some(word)
    }
}

/// Returns a concise dialect-aware summary for core language statements, `Function` / `arguments`
/// constructs, and timeline/game primitives when looking up contextual reference help.
pub(crate) fn keyword_reference_summary(
    language: ScriptLanguage,
    token: &str,
) -> Option<&'static str> {
    let clean = token.trim();
    match (language, clean) {
        (ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2, "function") => Some(
            "AS1/AS2 function statement & expression: declares hoisted named functions or anonymous function literals with optional parameter/return type annotations.",
        ),
        (ScriptLanguage::ActionScript3, "function") => Some(
            "AS3 function statement & expression: declares typed functions, class methods (bound closures), default parameter values, and ...rest variadic signatures.",
        ),
        (ScriptLanguage::JavaScript | ScriptLanguage::TypeScript, "function") => Some(
            "JS/TS function declaration & expression: defines hoisted functions, generator/async functions, or typed TypeScript call signatures.",
        ),
        (ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2, "Function") => Some(
            "AS2 Function class: represents callable objects supporting fn.call(thisObj, ...), fn.apply(thisObj, argsArray), and constructor Function.prototype inheritance.",
        ),
        (ScriptLanguage::ActionScript3, "Function") => Some(
            "AS3 Function class: represents closures and bound class methods supporting fn.call(thisArg, ...), fn.apply(thisArg, argArray), and fn.length.",
        ),
        (ScriptLanguage::JavaScript | ScriptLanguage::TypeScript, "Function") => Some(
            "ECMAScript Function constructor & prototype: provides fn.call(thisArg, ...), fn.apply(thisArg, args), and fn.bind(thisArg, ...).",
        ),
        (
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2,
            "arguments" | "callee" | "caller",
        ) => Some(
            "AS1/AS2 arguments object: implicit local array-like call frame exposing arguments.length, arguments[i], arguments.callee (self recursion), and arguments.caller.",
        ),
        (ScriptLanguage::ActionScript3, "arguments" | "callee") => Some(
            "AS3 arguments object: local call parameter list (arguments.length, arguments.callee); prefer typed ...rest parameters in new AS3 code.",
        ),
        (_, "apply" | "Function.apply") => Some(
            "Function.apply(thisObject, argsArray): invokes a function with an explicit `this` receiver and forwards an Array of positional arguments.",
        ),
        (_, "call" | "Function.call") => Some(
            "Function.call(thisObject, arg1, ...): invokes a function with an explicit `this` receiver and comma-separated arguments.",
        ),
        (_, "return") => Some(
            "return [expression]: exits the active function immediately and yields a value to the caller (or undefined / void when omitted).",
        ),
        (
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2,
            "prototype" | "__proto__" | "registerClass",
        ) => Some(
            "AS1/AS2 prototype inheritance: attach shared methods on Constructor.prototype and link Library symbols with Object.registerClass(id, Constructor).",
        ),
        (_, "var") => Some(
            "var variable declaration: declares a function-scoped local variable on the activation object (captured by nested closures) or timeline property at top level.",
        ),
        (_, "trace") => Some(
            "trace(...): streams formatted diagnostic lines to the Output panel (F2) during Test Movie and in-editor Play.",
        ),
        (
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2,
            "onEnterFrame" | "onRelease" | "onPress" | "on" | "onClipEvent",
        ) => Some(
            "AS1/AS2 event handler callbacks: drive 1-frame game loops (this.onEnterFrame) and interactive Button/MovieClip pointer handlers.",
        ),
        (
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2,
            "setInterval" | "clearInterval",
        ) => Some(
            "AS1/AS2 interval timers: setInterval(fn, ms, ...args) schedules repeating callbacks and returns an ID cancelled by clearInterval(id).",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_reference_token_handles_caret_and_selection() {
        let src = "function calcDamage(base:Number):Number {\n    return arguments.callee;\n}";
        assert_eq!(
            extract_reference_token(src, 0, 0).as_deref(),
            Some("function")
        );
        assert_eq!(
            extract_reference_token(src, 8, 8).as_deref(),
            Some("function")
        );
        assert_eq!(
            extract_reference_token(src, 12, 12).as_deref(),
            Some("calcDamage")
        );
        // Explicit selection across dotted expression
        let sel_start = src.find("arguments.callee").unwrap();
        let sel_end = sel_start + "arguments.callee".len();
        assert_eq!(
            extract_reference_token(src, sel_start, sel_end).as_deref(),
            Some("arguments.callee")
        );
    }

    #[test]
    fn keyword_reference_summary_distinguishes_dialects() {
        let as2 = keyword_reference_summary(ScriptLanguage::ActionScript2, "function").unwrap();
        let as3 = keyword_reference_summary(ScriptLanguage::ActionScript3, "function").unwrap();
        let ts = keyword_reference_summary(ScriptLanguage::TypeScript, "function").unwrap();
        assert!(as2.contains("AS1/AS2"));
        assert!(as3.contains("AS3"));
        assert!(ts.contains("JS/TS"));
        assert!(
            keyword_reference_summary(ScriptLanguage::ActionScript2, "callee")
                .unwrap()
                .contains("arguments.callee")
        );
    }
}
