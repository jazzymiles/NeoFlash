use super::*;

/// Build a `WatchRequest` carrying both the authored expression and its CDP `callFrameId` JS form.
pub(crate) fn compile_watch_expression(expression: &str, language: ScriptLanguage) -> WatchRequest {
    let authored = expression.trim().to_owned();
    let js_expression =
        match language {
            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => authored.clone(),
            ScriptLanguage::ActionScript1 => translate_actionscript_watch(&authored, false, true)
                .unwrap_or_else(|| authored.clone()),
            ScriptLanguage::ActionScript2 => translate_actionscript_watch(&authored, false, false)
                .unwrap_or_else(|| authored.clone()),
            ScriptLanguage::ActionScript3 => translate_actionscript_watch(&authored, true, false)
                .unwrap_or_else(|| authored.clone()),
        };
    WatchRequest::new(authored, js_expression)
}

#[derive(Clone, Debug, PartialEq)]
enum WatchToken {
    Ident(String),
    Number(String),
    Str(String),
    Punct(&'static str),
}

enum WatchAst {
    Literal(String),
    This,
    Ident(String),
    Member(Box<WatchAst>, String),
    Index(Box<WatchAst>, Box<WatchAst>),
    Call(Box<WatchAst>, Vec<WatchAst>),
    MethodCall(Box<WatchAst>, String, Vec<WatchAst>),
    Unary(&'static str, Box<WatchAst>),
    TypeofIdent(String),
    Binary(&'static str, Box<WatchAst>, Box<WatchAst>),
    TypeCast(&'static str, Box<WatchAst>),
    AsType(Box<WatchAst>, String),
    IsType(Box<WatchAst>, String),
    Ternary(Box<WatchAst>, Box<WatchAst>, Box<WatchAst>),
    Array(Vec<WatchAst>),
    Object(Vec<(String, WatchAst)>),
    Group(Box<WatchAst>),
}

fn lex_watch_tokens(src: &str, allow_diamond_ne: bool) -> Option<Vec<WatchToken>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset < src.len() {
        let rest = &src[offset..];
        if let Some(ch) = rest.chars().next()
            && ch.is_whitespace()
        {
            offset += ch.len_utf8();
            continue;
        }
        let ch = rest.chars().next()?;
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let mut end = offset + ch.len_utf8();
            while let Some(next) = src[end..].chars().next() {
                if next.is_alphanumeric() || next == '_' || next == '$' {
                    end += next.len_utf8();
                } else {
                    break;
                }
            }
            out.push(WatchToken::Ident(src[offset..end].to_owned()));
            offset = end;
        } else if ch.is_ascii_digit()
            || (ch == '.' && rest[1..].chars().next().is_some_and(|d| d.is_ascii_digit()))
        {
            let start = offset;
            offset += 1;
            if ch == '0' && src[offset..].starts_with(['x', 'X']) {
                offset += 1;
                let hex_start = offset;
                while src[offset..]
                    .chars()
                    .next()
                    .is_some_and(|d| d.is_ascii_hexdigit())
                {
                    offset += 1;
                }
                if offset == hex_start {
                    return None;
                }
            } else {
                while src[offset..]
                    .chars()
                    .next()
                    .is_some_and(|d| d.is_ascii_digit())
                {
                    offset += 1;
                }
                if ch != '.' && src[offset..].starts_with('.') {
                    offset += 1;
                    while src[offset..]
                        .chars()
                        .next()
                        .is_some_and(|d| d.is_ascii_digit())
                    {
                        offset += 1;
                    }
                }
                if src[offset..].starts_with(['e', 'E']) {
                    offset += 1;
                    if src[offset..].starts_with(['+', '-']) {
                        offset += 1;
                    }
                    let exp_start = offset;
                    while src[offset..]
                        .chars()
                        .next()
                        .is_some_and(|d| d.is_ascii_digit())
                    {
                        offset += 1;
                    }
                    if offset == exp_start {
                        return None;
                    }
                }
            }
            if src[offset..]
                .chars()
                .next()
                .is_some_and(|d| d.is_alphabetic() || d == '_')
            {
                return None;
            }
            out.push(WatchToken::Number(src[start..offset].to_owned()));
        } else if ch == '\'' || ch == '"' {
            let start = offset;
            offset += 1;
            let mut closed = false;
            while let Some(next) = src[offset..].chars().next() {
                offset += next.len_utf8();
                if next == ch {
                    closed = true;
                    break;
                }
                if matches!(next, '\n' | '\r') {
                    return None;
                }
                if next == '\\' {
                    let esc = src[offset..].chars().next()?;
                    offset += esc.len_utf8();
                }
            }
            if !closed {
                return None;
            }
            out.push(WatchToken::Str(src[start..offset].to_owned()));
        } else {
            // Side-effecting assignment / update tokens fall back to V8 syntax/reference reporting.
            const FORBIDDEN: &[&str] = &[
                ">>>=", "<<=", ">>=", "&&=", "||=", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=",
                "|=", "^=",
            ];
            if FORBIDDEN.iter().any(|op| rest.starts_with(op)) {
                return None;
            }
            const OPS: &[&str] = &[
                "===", "!==", ">>>", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "<>", "(",
                ")", "[", "]", "{", "}", ".", ",", "?", ":", "+", "-", "*", "/", "%", "!", "~",
                "&", "|", "^", "<", ">",
            ];
            let op = *OPS
                .iter()
                .find(|&&op| rest.starts_with(op) && (op != "<>" || allow_diamond_ne))?;
            out.push(WatchToken::Punct(op));
            offset += op.len();
        }
        if out.len() > 512 {
            return None;
        }
    }
    Some(out)
}

struct WatchParser {
    tokens: Vec<WatchToken>,
    pos: usize,
    as3: bool,
}

impl WatchParser {
    fn peek(&self) -> Option<&WatchToken> {
        self.tokens.get(self.pos)
    }
    fn eat_punct(&mut self, wanted: &str) -> bool {
        if matches!(self.peek(), Some(WatchToken::Punct(p)) if *p == wanted) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn eat_word(&mut self, wanted: &str) -> bool {
        if matches!(self.peek(), Some(WatchToken::Ident(w)) if w == wanted) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn ident(&mut self) -> Option<String> {
        match self.peek() {
            Some(WatchToken::Ident(name)) => {
                let n = name.clone();
                self.pos += 1;
                Some(n)
            }
            _ => None,
        }
    }
    fn qualified_type(&mut self) -> Option<String> {
        let mut out = self.ident()?;
        while self.eat_punct(".") {
            out.push('.');
            out.push_str(&self.ident()?);
        }
        Some(out)
    }
    fn parse_expr(&mut self, depth: usize) -> Option<WatchAst> {
        if depth > 48 {
            return None;
        }
        let cond = self.parse_or(depth + 1)?;
        if self.eat_punct("?") {
            let yes = self.parse_expr(depth + 1)?;
            if !self.eat_punct(":") {
                return None;
            }
            let no = self.parse_expr(depth + 1)?;
            Some(WatchAst::Ternary(
                Box::new(cond),
                Box::new(yes),
                Box::new(no),
            ))
        } else {
            Some(cond)
        }
    }
    fn parse_or(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_and(depth + 1)?;
        while self.eat_punct("||") || (!self.as3 && self.eat_word("or")) {
            let rhs = self.parse_and(depth + 1)?;
            lhs = WatchAst::Binary("||", Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_and(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_bitor(depth + 1)?;
        while self.eat_punct("&&") || (!self.as3 && self.eat_word("and")) {
            let rhs = self.parse_bitor(depth + 1)?;
            lhs = WatchAst::Binary("&&", Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_bitor(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_bitxor(depth + 1)?;
        while self.eat_punct("|") {
            let rhs = self.parse_bitxor(depth + 1)?;
            lhs = WatchAst::Binary("|", Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_bitxor(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_bitand(depth + 1)?;
        while self.eat_punct("^") {
            let rhs = self.parse_bitand(depth + 1)?;
            lhs = WatchAst::Binary("^", Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_bitand(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_equality(depth + 1)?;
        while self.eat_punct("&") {
            let rhs = self.parse_equality(depth + 1)?;
            lhs = WatchAst::Binary("&", Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_equality(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_relational(depth + 1)?;
        loop {
            let op = if self.eat_punct("===") {
                "==="
            } else if self.eat_punct("!==") {
                "!=="
            } else if self.eat_punct("==") || (!self.as3 && self.eat_word("eq")) {
                "=="
            } else if self.eat_punct("!=")
                || self.eat_punct("<>")
                || (!self.as3 && self.eat_word("ne"))
            {
                "!="
            } else {
                break;
            };
            let rhs = self.parse_relational(depth + 1)?;
            lhs = WatchAst::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_relational(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_shift(depth + 1)?;
        loop {
            if self.as3 && self.eat_word("as") {
                let ty = self.qualified_type()?;
                lhs = WatchAst::AsType(Box::new(lhs), ty);
                continue;
            }
            if self.as3 && self.eat_word("is") {
                let ty = self.qualified_type()?;
                lhs = WatchAst::IsType(Box::new(lhs), ty);
                continue;
            }
            let op = if self.eat_punct("<=") || (!self.as3 && self.eat_word("le")) {
                "<="
            } else if self.eat_punct(">=") || (!self.as3 && self.eat_word("ge")) {
                ">="
            } else if self.eat_punct("<") || (!self.as3 && self.eat_word("lt")) {
                "<"
            } else if self.eat_punct(">") || (!self.as3 && self.eat_word("gt")) {
                ">"
            } else if self.eat_word("in") {
                "in"
            } else if self.eat_word("instanceof") {
                "instanceof"
            } else {
                break;
            };
            let rhs = self.parse_shift(depth + 1)?;
            lhs = WatchAst::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_shift(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_additive(depth + 1)?;
        loop {
            let op = if self.eat_punct(">>>") {
                ">>>"
            } else if self.eat_punct("<<") {
                "<<"
            } else if self.eat_punct(">>") {
                ">>"
            } else {
                break;
            };
            let rhs = self.parse_additive(depth + 1)?;
            lhs = WatchAst::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_additive(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_multiplicative(depth + 1)?;
        loop {
            let op = if self.eat_punct("+") || (!self.as3 && self.eat_word("add")) {
                "+"
            } else if self.eat_punct("-") {
                "-"
            } else {
                break;
            };
            let rhs = self.parse_multiplicative(depth + 1)?;
            lhs = WatchAst::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_multiplicative(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_unary(depth + 1)?;
        loop {
            let op = if self.eat_punct("*") {
                "*"
            } else if self.eat_punct("/") {
                "/"
            } else if self.eat_punct("%") {
                "%"
            } else {
                break;
            };
            let rhs = self.parse_unary(depth + 1)?;
            lhs = WatchAst::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_unary(&mut self, depth: usize) -> Option<WatchAst> {
        if depth > 64 {
            return None;
        }
        if self.eat_punct("!") || (!self.as3 && self.eat_word("not")) {
            return Some(WatchAst::Unary("!", Box::new(self.parse_unary(depth + 1)?)));
        }
        if self.eat_punct("+") {
            return Some(WatchAst::Unary("+", Box::new(self.parse_unary(depth + 1)?)));
        }
        if self.eat_punct("-") {
            return Some(WatchAst::Unary("-", Box::new(self.parse_unary(depth + 1)?)));
        }
        if self.eat_punct("~") {
            return Some(WatchAst::Unary("~", Box::new(self.parse_unary(depth + 1)?)));
        }
        if self.eat_word("void") {
            return Some(WatchAst::Unary(
                "void ",
                Box::new(self.parse_unary(depth + 1)?),
            ));
        }
        if self.eat_word("typeof") {
            let inner = self.parse_unary(depth + 1)?;
            return Some(match inner {
                WatchAst::Ident(name) => WatchAst::TypeofIdent(name),
                other => WatchAst::Unary("typeof ", Box::new(other)),
            });
        }
        self.parse_postfix(depth + 1)
    }
    fn parse_args(&mut self, depth: usize) -> Option<Vec<WatchAst>> {
        let mut args = Vec::new();
        if !self.eat_punct(")") {
            loop {
                args.push(self.parse_expr(depth + 1)?);
                if self.eat_punct(")") {
                    break;
                }
                if !self.eat_punct(",") {
                    return None;
                }
            }
        }
        Some(args)
    }
    fn parse_postfix(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_primary(depth + 1)?;
        loop {
            if self.eat_punct(".") {
                let prop = self.ident()?;
                if self.eat_punct("(") {
                    let args = self.parse_args(depth + 1)?;
                    lhs = WatchAst::MethodCall(Box::new(lhs), prop, args);
                } else {
                    lhs = WatchAst::Member(Box::new(lhs), prop);
                }
            } else if self.eat_punct("[") {
                let idx = self.parse_expr(depth + 1)?;
                if !self.eat_punct("]") {
                    return None;
                }
                lhs = WatchAst::Index(Box::new(lhs), Box::new(idx));
            } else if self.eat_punct("(") {
                let args = self.parse_args(depth + 1)?;
                if let WatchAst::Ident(ref name) = lhs
                    && (name == "int" || (self.as3 && name == "uint"))
                    && args.len() == 1
                {
                    let op = if name == "int" { "| 0" } else { ">>> 0" };
                    let arg = args.into_iter().next().unwrap();
                    lhs = WatchAst::TypeCast(op, Box::new(arg));
                } else {
                    lhs = WatchAst::Call(Box::new(lhs), args);
                }
            } else {
                break;
            }
        }
        Some(lhs)
    }
    fn parse_primary(&mut self, depth: usize) -> Option<WatchAst> {
        if self.eat_punct("(") {
            let inner = self.parse_expr(depth + 1)?;
            if !self.eat_punct(")") {
                return None;
            }
            return Some(WatchAst::Group(Box::new(inner)));
        }
        if self.eat_punct("[") {
            let mut items = Vec::new();
            if !self.eat_punct("]") {
                loop {
                    items.push(self.parse_expr(depth + 1)?);
                    if self.eat_punct("]") {
                        break;
                    }
                    if !self.eat_punct(",") {
                        return None;
                    }
                }
            }
            return Some(WatchAst::Array(items));
        }
        if self.eat_punct("{") {
            let mut entries = Vec::new();
            if !self.eat_punct("}") {
                loop {
                    let key = match self.peek()? {
                        WatchToken::Ident(k) | WatchToken::Str(k) | WatchToken::Number(k) => {
                            let k = k.clone();
                            self.pos += 1;
                            k
                        }
                        _ => return None,
                    };
                    if !self.eat_punct(":") {
                        return None;
                    }
                    let val = self.parse_expr(depth + 1)?;
                    entries.push((key, val));
                    if self.eat_punct("}") {
                        break;
                    }
                    if !self.eat_punct(",") {
                        return None;
                    }
                }
            }
            return Some(WatchAst::Object(entries));
        }
        match self.peek()?.clone() {
            WatchToken::Number(n) | WatchToken::Str(n) => {
                self.pos += 1;
                Some(WatchAst::Literal(n))
            }
            WatchToken::Ident(w) => {
                self.pos += 1;
                match w.as_str() {
                    "true" | "false" | "null" | "undefined" | "NaN" | "Infinity" => {
                        Some(WatchAst::Literal(w))
                    }
                    "this" => Some(WatchAst::This),
                    "break" | "case" | "catch" | "class" | "const" | "continue" | "debugger"
                    | "default" | "delete" | "do" | "else" | "enum" | "export" | "extends"
                    | "finally" | "for" | "function" | "if" | "import" | "let" | "new"
                    | "return" | "super" | "switch" | "throw" | "try" | "var" | "while"
                    | "with" | "yield" | "await" => None,
                    _ => Some(WatchAst::Ident(w)),
                }
            }
            _ => None,
        }
    }
}

fn emit_watch_ast(ast: &WatchAst) -> String {
    match ast {
        WatchAst::Literal(v) => v.clone(),
        WatchAst::This => "__nfWatchThis".into(),
        WatchAst::Ident(name) => {
            format!("__nfWatchId({name:?}, () => typeof {name} !== \"undefined\", () => {name})")
        }
        WatchAst::Member(base, prop) => {
            format!("__nfWatchProp({}, {prop:?})", emit_watch_ast(base))
        }
        WatchAst::Index(base, idx) => format!(
            "__nfWatchProp({}, {})",
            emit_watch_ast(base),
            emit_watch_ast(idx)
        ),
        WatchAst::MethodCall(base, prop, args) => format!(
            "__nfWatchCall({}, {prop:?}, [{}])",
            emit_watch_ast(base),
            args.iter()
                .map(emit_watch_ast)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        WatchAst::Call(callee, args) => format!(
            "({})({})",
            emit_watch_ast(callee),
            args.iter()
                .map(emit_watch_ast)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        WatchAst::Unary(op, inner) => format!("({op}{})", emit_watch_ast(inner)),
        WatchAst::TypeofIdent(name) => format!(
            "__nfWatchTypeof({name:?}, () => typeof {name} !== \"undefined\", () => {name})"
        ),
        WatchAst::Binary(op, lhs, rhs) if *op == "in" || *op == "instanceof" => {
            format!("({} {op} {})", emit_watch_ast(lhs), emit_watch_ast(rhs))
        }
        WatchAst::Binary(op, lhs, rhs) => {
            format!("({} {op} {})", emit_watch_ast(lhs), emit_watch_ast(rhs))
        }
        WatchAst::TypeCast(op, inner) => format!("(Number({}) {op})", emit_watch_ast(inner)),
        WatchAst::AsType(inner, ty) => format!("__nfWatchAs({}, {ty:?})", emit_watch_ast(inner)),
        WatchAst::IsType(inner, ty) => format!("__nfWatchIs({}, {ty:?})", emit_watch_ast(inner)),
        WatchAst::Ternary(c, y, n) => format!(
            "({} ? {} : {})",
            emit_watch_ast(c),
            emit_watch_ast(y),
            emit_watch_ast(n)
        ),
        WatchAst::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(emit_watch_ast)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        WatchAst::Object(entries) => format!(
            "({{{}}})",
            entries
                .iter()
                .map(|(k, v)| format!("{k}: {}", emit_watch_ast(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        WatchAst::Group(inner) => format!("({})", emit_watch_ast(inner)),
    }
}

pub(super) fn translate_actionscript_watch(
    expr: &str,
    as3: bool,
    legacy_case: bool,
) -> Option<String> {
    if expr.is_empty() || expr.len() > 2_048 {
        return None;
    }
    let tokens = lex_watch_tokens(expr, !as3)?;
    if tokens.is_empty() {
        return None;
    }
    let mut parser = WatchParser {
        tokens,
        pos: 0,
        as3,
    };
    let ast = parser.parse_expr(0)?;
    if parser.pos != parser.tokens.len() {
        return None;
    }
    let body = emit_watch_ast(&ast);
    Some(format!(
        "(() => {{ \
const __nfWAs3 = {as3}, __nfWLegacy = {legacy_case}; \
const __nfWatchThis = (typeof __nfFrame !== \"undefined\" && __nfFrame !== null) ? __nfFrame : ((typeof this !== \"undefined\" && (typeof globalThis === \"undefined\" || this !== globalThis)) ? this : null); \
const __nfWatchHas = (o, k) => {{ if (o == null || (typeof o !== \"object\" && typeof o !== \"function\")) return false; if (typeof __nfContainerRef === \"function\") {{ try {{ return Boolean(__nfContainerRef(o, k, __nfWAs3, undefined, __nfWLegacy).has()); }} catch (_) {{}} }} return k in o; }}; \
const __nfWatchProp = (o, k) => {{ if (typeof __nfFamilyPropertyValue === \"function\") return __nfFamilyPropertyValue(o, k, __nfWAs3, undefined, __nfWLegacy); return o[k]; }}; \
const __nfWatchCall = (o, k, a) => {{ if (typeof __nfContainerMethod === \"function\") return __nfContainerMethod(o, k, __nfWAs3, undefined, __nfWLegacy)(...a); return o[k](...a); }}; \
const __nfWatchId = (n, hasLex, getLex) => {{ \
const onGlobal = typeof globalThis !== \"undefined\" && (n in globalThis); \
if (hasLex()) {{ const v = getLex(); if (!onGlobal || v !== globalThis[n] || !__nfWatchHas(__nfWatchThis, n)) return v; }} \
if (__nfWatchThis !== null && __nfWatchHas(__nfWatchThis, n)) return __nfWatchProp(__nfWatchThis, n); \
if (!__nfWAs3 && typeof __nfLegacy !== \"undefined\" && __nfLegacy !== null) {{ \
if (/^_level\\d+$/.test(n) && typeof __nfLegacy.level === \"function\") return __nfLegacy.level(Number(n.slice(6))); \
if (typeof __nfLegacy.globalPropRef === \"function\" && (n === \"_quality\" || n === \"_focusrect\" || n === \"_soundbuftime\" || n === \"_highquality\")) return __nfLegacy.globalPropRef(n).get(); \
if (n in __nfLegacy) return __nfLegacy[n]; \
if (__nfLegacy._global && (n in __nfLegacy._global)) return __nfLegacy._global[n]; \
}} \
if (typeof __nfProgram !== \"undefined\" && __nfProgram !== null && typeof __nfProgram.type === \"function\") {{ try {{ const t = __nfProgram.type(n); if (t !== undefined) return t; }} catch (_) {{}} }} \
return getLex(); \
}}; \
const __nfWatchTypeof = (n, hasLex, getLex) => {{ try {{ return typeof __nfWatchId(n, hasLex, getLex); }} catch (_) {{ return \"undefined\"; }} }}; \
const __nfWatchIs = (v, t) => {{ if (typeof __nfCatchType === \"function\" && (t === \"Number\" || t === \"String\" || t === \"Boolean\" || t === \"int\" || t === \"uint\" || t === \"Object\" || t === \"Array\" || t === \"Function\")) return Boolean(__nfCatchType(v, t)); if (typeof __nfProgram !== \"undefined\" && __nfProgram !== null && typeof __nfProgram.is === \"function\") {{ try {{ return Boolean(__nfProgram.is(v, t)); }} catch (_) {{}} }} return false; }}; \
const __nfWatchAs = (v, t) => (__nfWatchIs(v, t) ? v : null); \
return ({body}); \
}})()"
    ))
}
