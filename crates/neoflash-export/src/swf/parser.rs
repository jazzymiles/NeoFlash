//! ActionScript 1/2/3 tokenizer and AST parser for bounded SWF bytecode compilation.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SwfUnaryOp {
    Not,
    Neg,
    Pos,
    BitNot,
    Typeof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SwfBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    InstanceOf,
    Is,
    As,
    In,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SwfAssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    BitAndAssign,
    BitOrAssign,
    BitXorAssign,
    ShlAssign,
    ShrAssign,
    UShrAssign,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SwfXmlRawPiece {
    Raw(String),
    ElemExpr(String),
    AttrExpr(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SwfXmlCompiledPiece {
    Raw(String),
    ElemExpr(Box<SwfExpr>),
    AttrExpr(Box<SwfExpr>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SwfXmlSelector {
    Named(String),
    Computed(Box<SwfExpr>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SwfLValue {
    Ident(String),
    Member(Box<SwfExpr>, String),
    Index(Box<SwfExpr>, Box<SwfExpr>),
    XmlAttr {
        object: Box<SwfExpr>,
        attr: SwfXmlSelector,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SwfExpr {
    Null,
    Undefined,
    Bool(bool),
    Int(i32),
    Float(f64),
    Str(String),
    Ident(String),
    ArrayLit(Vec<SwfExpr>),
    ObjectLit(Vec<(String, SwfExpr)>),
    Function {
        name: Option<String>,
        params: Vec<String>,
        body: Vec<SwfStmt>,
    },
    Member(Box<SwfExpr>, String),
    Index(Box<SwfExpr>, Box<SwfExpr>),
    XmlLit {
        is_list: bool,
        pieces: Vec<SwfXmlCompiledPiece>,
    },
    XmlAttr {
        object: Box<SwfExpr>,
        attr: SwfXmlSelector,
    },
    XmlDescendants {
        object: Box<SwfExpr>,
        target: SwfXmlSelector,
        is_attr: bool,
    },
    Unary {
        op: SwfUnaryOp,
        expr: Box<SwfExpr>,
    },
    Binary {
        op: SwfBinaryOp,
        lhs: Box<SwfExpr>,
        rhs: Box<SwfExpr>,
    },
    Call {
        callee: Box<SwfExpr>,
        args: Vec<SwfExpr>,
    },
    New {
        callee: Box<SwfExpr>,
        args: Vec<SwfExpr>,
    },
    Delete(Box<SwfLValue>),
    Ternary {
        cond: Box<SwfExpr>,
        then_expr: Box<SwfExpr>,
        else_expr: Box<SwfExpr>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SwfStmt {
    VarDecl {
        name: String,
        init: Option<SwfExpr>,
    },
    Assign {
        target: SwfLValue,
        op: SwfAssignOp,
        value: SwfExpr,
    },
    IncDec {
        target: SwfLValue,
        increment: bool,
    },
    Expr(SwfExpr),
    Block(Vec<SwfStmt>),
    If {
        cond: SwfExpr,
        then_branch: Vec<SwfStmt>,
        else_branch: Option<Vec<SwfStmt>>,
    },
    While {
        post_test: bool,
        cond: SwfExpr,
        body: Vec<SwfStmt>,
    },
    For {
        init: Option<Box<SwfStmt>>,
        cond: Option<SwfExpr>,
        update: Option<Box<SwfStmt>>,
        body: Vec<SwfStmt>,
    },
    ForIn {
        is_each: bool,
        is_var_decl: bool,
        var_name: String,
        object: SwfExpr,
        body: Vec<SwfStmt>,
    },
    Switch {
        discriminant: SwfExpr,
        cases: Vec<(Option<SwfExpr>, Vec<SwfStmt>)>,
    },
    Break,
    Continue,
    Try {
        try_body: Vec<SwfStmt>,
        catches: Vec<(String, Option<String>, Vec<SwfStmt>)>,
        finally_body: Option<Vec<SwfStmt>>,
    },
    Throw(SwfExpr),
    FunctionDecl {
        name: String,
        params: Vec<String>,
        body: Vec<SwfStmt>,
    },
    ClassDecl {
        name: String,
        super_name: Option<String>,
        ctor: Option<(Vec<String>, Vec<SwfStmt>)>,
        methods: Vec<(bool, String, Vec<String>, Vec<SwfStmt>)>,
        fields: Vec<(bool, String, Option<SwfExpr>)>,
    },
    Return(Option<SwfExpr>),
    DefaultXmlNamespace(SwfExpr),
    EventHandler {
        is_clip_event: bool,
        events: Vec<String>,
        body: Vec<SwfStmt>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SwfToken {
    Ident(String),
    Int(i32),
    Float(f64),
    Str(String),
    XmlLiteral {
        is_list: bool,
        pieces: Vec<SwfXmlRawPiece>,
    },
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semi,
    Comma,
    Dot,
    DotDot,
    At,
    Colon,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    PlusPlus,
    MinusMinus,
    Assign,
    FatArrow,
    CompoundAssign(SwfAssignOp),
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Not,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    Tilde,
    Question,
}

pub(super) fn tokenize_swf_script(source: &str) -> Result<Vec<SwfToken>, String> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < len {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            let mut closed = false;
            while i + 1 < len {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                return Err("unterminated block comment in script".to_owned());
            }
            continue;
        }
        if b == b'"' || b == b'\'' {
            let quote = b;
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < len {
                let ch = source[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '\\' {
                    if i >= len {
                        return Err("unterminated escape sequence in string literal".to_owned());
                    }
                    let esc = source[i..].chars().next().unwrap();
                    i += esc.len_utf8();
                    match esc {
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        other => s.push(other),
                    }
                } else if ch as u32 == u32::from(quote) {
                    closed = true;
                    break;
                } else if ch == '\n' || ch == '\r' {
                    return Err("unterminated string literal in script".to_owned());
                } else {
                    s.push(ch);
                }
            }
            if !closed {
                return Err("unterminated string literal in script".to_owned());
            }
            tokens.push(SwfToken::Str(s));
            continue;
        }
        if b.is_ascii_digit() {
            let start = i;
            if b == b'0' && i + 1 < len && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
                i += 2;
                let hex_start = i;
                while i < len && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
                if i == hex_start {
                    return Err("invalid hex literal in script".to_owned());
                }
                let hex_str = &source[hex_start..i];
                let val = i64::from_str_radix(hex_str, 16)
                    .map_err(|_| format!("invalid hex literal '0x{hex_str}'"))?;
                if let Ok(iv) = i32::try_from(val) {
                    tokens.push(SwfToken::Int(iv));
                } else {
                    tokens.push(SwfToken::Float(val as f64));
                }
                continue;
            }
            let mut is_float = false;
            while i < len && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < len
                && bytes[i] == b'.'
                && tokens.last() != Some(&SwfToken::Dot)
                && (i + 1 >= len || bytes[i + 1].is_ascii_digit())
            {
                is_float = true;
                i += 1;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
                is_float = true;
                i += 1;
                if i < len && (bytes[i] == b'+' || bytes[i] == b'-') {
                    i += 1;
                }
                let exp_start = i;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i == exp_start {
                    return Err("invalid numeric exponent in script".to_owned());
                }
            }
            let num_str = &source[start..i];
            if !is_float && let Ok(iv) = num_str.parse::<i32>() {
                tokens.push(SwfToken::Int(iv));
            } else if let Ok(fv) = num_str.parse::<f64>() {
                tokens.push(SwfToken::Float(fv));
            } else {
                return Err(format!("invalid numeric literal '{num_str}'"));
            }
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
            let start = i;
            i += 1;
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
            tokens.push(SwfToken::Ident(source[start..i].to_owned()));
            continue;
        }
        if b == b'<' && can_start_xml_literal(&tokens, source, i) {
            let (xml_tok, next_i) = scan_xml_literal(source, i)?;
            tokens.push(xml_tok);
            i = next_i;
            continue;
        }
        let rem = &source[i..];
        // Prefer the longest spelling so >>>= stays a single update of its left operand.
        const OPERATORS: &[(&str, SwfToken)] = &[
            (">>>=", SwfToken::CompoundAssign(SwfAssignOp::UShrAssign)),
            ("===", SwfToken::StrictEq),
            ("!==", SwfToken::StrictNe),
            (">>>", SwfToken::UShr),
            ("<<=", SwfToken::CompoundAssign(SwfAssignOp::ShlAssign)),
            (">>=", SwfToken::CompoundAssign(SwfAssignOp::ShrAssign)),
            ("==", SwfToken::Eq),
            ("=>", SwfToken::FatArrow),
            ("!=", SwfToken::Ne),
            ("<=", SwfToken::Le),
            (">=", SwfToken::Ge),
            ("&&", SwfToken::And),
            ("||", SwfToken::Or),
            ("++", SwfToken::PlusPlus),
            ("--", SwfToken::MinusMinus),
            ("+=", SwfToken::CompoundAssign(SwfAssignOp::AddAssign)),
            ("-=", SwfToken::CompoundAssign(SwfAssignOp::SubAssign)),
            ("*=", SwfToken::CompoundAssign(SwfAssignOp::MulAssign)),
            ("/=", SwfToken::CompoundAssign(SwfAssignOp::DivAssign)),
            ("%=", SwfToken::CompoundAssign(SwfAssignOp::ModAssign)),
            ("<<", SwfToken::Shl),
            (">>", SwfToken::Shr),
            ("&=", SwfToken::CompoundAssign(SwfAssignOp::BitAndAssign)),
            ("|=", SwfToken::CompoundAssign(SwfAssignOp::BitOrAssign)),
            ("^=", SwfToken::CompoundAssign(SwfAssignOp::BitXorAssign)),
            ("..", SwfToken::DotDot),
        ];
        if let Some((spelling, token)) = OPERATORS.iter().find(|(op, _)| rem.starts_with(op)) {
            tokens.push(token.clone());
            i += spelling.len();
        } else {
            let tok = match b {
                b'(' => SwfToken::LParen,
                b')' => SwfToken::RParen,
                b'{' => SwfToken::LBrace,
                b'}' => SwfToken::RBrace,
                b'[' => SwfToken::LBracket,
                b']' => SwfToken::RBracket,
                b';' => SwfToken::Semi,
                b',' => SwfToken::Comma,
                b'.' => SwfToken::Dot,
                b'@' => SwfToken::At,
                b':' => SwfToken::Colon,
                b'+' => SwfToken::Plus,
                b'-' => SwfToken::Minus,
                b'*' => SwfToken::Star,
                b'/' => SwfToken::Slash,
                b'%' => SwfToken::Percent,
                b'=' => SwfToken::Assign,
                b'<' => SwfToken::Lt,
                b'>' => SwfToken::Gt,
                b'!' => SwfToken::Not,
                b'&' => SwfToken::BitAnd,
                b'|' => SwfToken::BitOr,
                b'^' => SwfToken::BitXor,
                b'~' => SwfToken::Tilde,
                b'?' => SwfToken::Question,
                _ => {
                    let ch = source[i..].chars().next().unwrap_or('?');
                    return Err(format!(
                        "unsupported syntax token '{ch}' in bounded SWF compiler"
                    ));
                }
            };
            tokens.push(tok);
            i += 1;
        }
    }
    Ok(tokens)
}

fn can_start_xml_literal(tokens: &[SwfToken], source: &str, pos: usize) -> bool {
    let expr_start = match tokens.last() {
        None => true,
        Some(
            SwfToken::LParen
            | SwfToken::LBrace
            | SwfToken::LBracket
            | SwfToken::Semi
            | SwfToken::Comma
            | SwfToken::Colon
            | SwfToken::Question
            | SwfToken::Assign
            | SwfToken::FatArrow
            | SwfToken::CompoundAssign(_)
            | SwfToken::Eq
            | SwfToken::Ne
            | SwfToken::StrictEq
            | SwfToken::StrictNe
            | SwfToken::Lt
            | SwfToken::Le
            | SwfToken::Gt
            | SwfToken::Ge
            | SwfToken::And
            | SwfToken::Or
            | SwfToken::Not
            | SwfToken::BitAnd
            | SwfToken::BitOr
            | SwfToken::BitXor
            | SwfToken::Shl
            | SwfToken::Shr
            | SwfToken::UShr
            | SwfToken::Tilde
            | SwfToken::Plus
            | SwfToken::Minus
            | SwfToken::Star
            | SwfToken::Slash
            | SwfToken::Percent,
        ) => true,
        Some(SwfToken::Ident(kw)) => matches!(
            kw.as_str(),
            "return" | "throw" | "case" | "in" | "typeof" | "delete" | "new" | "else" | "do"
        ),
        _ => false,
    };
    if !expr_start {
        return false;
    }
    let after = &source[pos + 1..];
    after.starts_with('>')
        || after.starts_with("!--")
        || after.starts_with("![CDATA[")
        || after.starts_with('?')
        || after.starts_with('{')
        || after
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
}

fn push_xml_raw(pieces: &mut Vec<SwfXmlRawPiece>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(SwfXmlRawPiece::Raw(prev)) = pieces.last_mut() {
        prev.push_str(text);
    } else {
        pieces.push(SwfXmlRawPiece::Raw(text.to_owned()));
    }
}

fn scan_balanced_braces_expr(source: &str, brace_pos: usize) -> Result<(String, usize), String> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut p = brace_pos + 1;
    let mut depth = 1usize;
    while p < len && depth > 0 {
        let b = bytes[p];
        if b == b'"' || b == b'\'' {
            let q = b;
            p += 1;
            let mut closed = false;
            while p < len {
                let ch = source[p..].chars().next().unwrap();
                p += ch.len_utf8();
                if ch == '\\' {
                    if p >= len {
                        return Err(
                            "unterminated string escape inside E4X '{}' interpolation".to_owned()
                        );
                    }
                    let esc = source[p..].chars().next().unwrap();
                    p += esc.len_utf8();
                } else if ch as u32 == u32::from(q) {
                    closed = true;
                    break;
                }
            }
            if !closed {
                return Err("unterminated string literal inside E4X '{}' interpolation".to_owned());
            }
            continue;
        }
        if b == b'/' && p + 1 < len && bytes[p + 1] == b'/' {
            p += 2;
            while p < len && bytes[p] != b'\n' {
                p += 1;
            }
            continue;
        }
        if b == b'/' && p + 1 < len && bytes[p + 1] == b'*' {
            let Some(rel_end) = source[p + 2..].find("*/") else {
                return Err("unterminated block comment inside E4X '{}' interpolation".to_owned());
            };
            p += 2 + rel_end + 2;
            continue;
        }
        if b == b'{' {
            depth += 1;
            p += 1;
        } else if b == b'}' {
            depth -= 1;
            p += 1;
        } else {
            let ch = source[p..].chars().next().unwrap();
            p += ch.len_utf8();
        }
    }
    if depth != 0 {
        return Err("unterminated '{}' interpolation in E4X XML literal".to_owned());
    }
    let expr_src = source[brace_pos + 1..p - 1].trim().to_owned();
    if expr_src.is_empty() {
        return Err("empty '{}' interpolation in E4X XML literal".to_owned());
    }
    Ok((expr_src, p))
}

fn scan_xml_name(source: &str, pos: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    let first = *bytes.get(pos)?;
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b':') {
        return None;
    }
    let mut p = pos + 1;
    while p < bytes.len()
        && (bytes[p].is_ascii_alphanumeric() || matches!(bytes[p], b'_' | b':' | b'-' | b'.'))
    {
        p += 1;
    }
    Some((source[pos..p].to_owned(), p))
}

fn scan_xml_open_tag(
    source: &str,
    start: usize,
    pieces: &mut Vec<SwfXmlRawPiece>,
    tag_stack: &mut Vec<Option<String>>,
) -> Result<usize, String> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut p = start + 1;
    push_xml_raw(pieces, "<");

    let tag_name = if p < len && bytes[p] == b'{' {
        let (e, next_p) = scan_balanced_braces_expr(source, p)?;
        pieces.push(SwfXmlRawPiece::ElemExpr(e));
        p = next_p;
        None
    } else if let Some((tname, next_p)) = scan_xml_name(source, p) {
        push_xml_raw(pieces, &tname);
        p = next_p;
        Some(tname)
    } else {
        return Err("invalid XML element tag name in E4X literal".to_owned());
    };

    loop {
        let ws_start = p;
        while p < len && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        if p > ws_start {
            push_xml_raw(pieces, &source[ws_start..p]);
        }
        if p >= len {
            return Err("unterminated XML start tag in E4X literal".to_owned());
        }
        if source[p..].starts_with("/>") {
            push_xml_raw(pieces, "/>");
            return Ok(p + 2);
        }
        if bytes[p] == b'>' {
            push_xml_raw(pieces, ">");
            tag_stack.push(tag_name);
            return Ok(p + 1);
        }

        if bytes[p] == b'{' {
            let (e, next_p) = scan_balanced_braces_expr(source, p)?;
            pieces.push(SwfXmlRawPiece::ElemExpr(e));
            p = next_p;
        } else if let Some((aname, next_p)) = scan_xml_name(source, p) {
            push_xml_raw(pieces, &aname);
            p = next_p;
        } else {
            return Err("expected XML attribute name or '>' in E4X literal".to_owned());
        }

        while p < len && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= len || bytes[p] != b'=' {
            return Err("expected '=' after XML attribute name in E4X literal".to_owned());
        }
        push_xml_raw(pieces, "=");
        p += 1;
        while p < len && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= len {
            return Err("unterminated XML attribute in E4X literal".to_owned());
        }
        if bytes[p] == b'"' || bytes[p] == b'\'' {
            let q = bytes[p] as char;
            let Some(rel_end) = source[p + 1..].find(q) else {
                return Err("unterminated XML attribute string in E4X literal".to_owned());
            };
            let attr_end = p + 1 + rel_end + 1;
            push_xml_raw(pieces, &source[p..attr_end]);
            p = attr_end;
        } else if bytes[p] == b'{' {
            push_xml_raw(pieces, "\"");
            let (e, next_p) = scan_balanced_braces_expr(source, p)?;
            pieces.push(SwfXmlRawPiece::AttrExpr(e));
            push_xml_raw(pieces, "\"");
            p = next_p;
        } else {
            return Err(
                "expected quoted string or '{...}' for XML attribute value in E4X literal"
                    .to_owned(),
            );
        }
    }
}

fn scan_xml_literal(source: &str, start: usize) -> Result<(SwfToken, usize), String> {
    if source[start..].starts_with("<!--") {
        let Some(rel_end) = source[start + 4..].find("-->") else {
            return Err("unterminated XML comment in E4X literal".to_owned());
        };
        let end = start + 4 + rel_end + 3;
        return Ok((
            SwfToken::XmlLiteral {
                is_list: false,
                pieces: vec![SwfXmlRawPiece::Raw(source[start..end].to_owned())],
            },
            end,
        ));
    }
    if source[start..].starts_with("<![CDATA[") {
        let Some(rel_end) = source[start + 9..].find("]]>") else {
            return Err("unterminated XML CDATA section in E4X literal".to_owned());
        };
        let end = start + 9 + rel_end + 3;
        return Ok((
            SwfToken::XmlLiteral {
                is_list: false,
                pieces: vec![SwfXmlRawPiece::Raw(source[start..end].to_owned())],
            },
            end,
        ));
    }
    if source[start..].starts_with("<?") {
        let Some(rel_end) = source[start + 2..].find("?>") else {
            return Err("unterminated XML processing instruction in E4X literal".to_owned());
        };
        let end = start + 2 + rel_end + 2;
        return Ok((
            SwfToken::XmlLiteral {
                is_list: false,
                pieces: vec![SwfXmlRawPiece::Raw(source[start..end].to_owned())],
            },
            end,
        ));
    }

    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut pieces = Vec::new();
    let mut tag_stack: Vec<Option<String>> = Vec::new();
    let (is_list, mut p) = if source[start..].starts_with("<>") {
        tag_stack.push(Some(String::new()));
        (true, start + 2)
    } else {
        let next_p = scan_xml_open_tag(source, start, &mut pieces, &mut tag_stack)?;
        (false, next_p)
    };

    while !tag_stack.is_empty() {
        if p >= len {
            return Err("unterminated XML element in E4X literal".to_owned());
        }
        if bytes[p] == b'{' {
            let (e, next_p) = scan_balanced_braces_expr(source, p)?;
            pieces.push(SwfXmlRawPiece::ElemExpr(e));
            p = next_p;
        } else if source[p..].starts_with("<!--") {
            let Some(rel_end) = source[p + 4..].find("-->") else {
                return Err("unterminated XML comment in E4X literal".to_owned());
            };
            let end = p + 4 + rel_end + 3;
            push_xml_raw(&mut pieces, &source[p..end]);
            p = end;
        } else if source[p..].starts_with("<![CDATA[") {
            let Some(rel_end) = source[p + 9..].find("]]>") else {
                return Err("unterminated XML CDATA section in E4X literal".to_owned());
            };
            let end = p + 9 + rel_end + 3;
            push_xml_raw(&mut pieces, &source[p..end]);
            p = end;
        } else if source[p..].starts_with("<?") {
            let Some(rel_end) = source[p + 2..].find("?>") else {
                return Err("unterminated XML processing instruction in E4X literal".to_owned());
            };
            let end = p + 2 + rel_end + 2;
            push_xml_raw(&mut pieces, &source[p..end]);
            p = end;
        } else if source[p..].starts_with("</") {
            if is_list && tag_stack.len() == 1 && source[p..].starts_with("</>") {
                tag_stack.pop();
                p += 3;
                break;
            }
            p += 2;
            while p < len && bytes[p].is_ascii_whitespace() {
                p += 1;
            }
            push_xml_raw(&mut pieces, "</");
            let close_name = if p < len && bytes[p] == b'{' {
                let (e, next_p) = scan_balanced_braces_expr(source, p)?;
                pieces.push(SwfXmlRawPiece::ElemExpr(e));
                p = next_p;
                None
            } else if let Some((cname, next_p)) = scan_xml_name(source, p) {
                push_xml_raw(&mut pieces, &cname);
                p = next_p;
                Some(cname)
            } else {
                return Err("invalid XML closing tag in E4X literal".to_owned());
            };
            while p < len && bytes[p].is_ascii_whitespace() {
                p += 1;
            }
            if p >= len || bytes[p] != b'>' {
                return Err("expected '>' in XML closing tag in E4X literal".to_owned());
            }
            push_xml_raw(&mut pieces, ">");
            p += 1;
            let expected = tag_stack.pop().unwrap_or(None);
            if let (Some(open_n), Some(close_n)) = (expected, close_name)
                && open_n != close_n
            {
                return Err(format!(
                    "mismatched XML closing tag '</{close_n}>' for '<{open_n}>' in E4X literal"
                ));
            }
        } else if bytes[p] == b'<' {
            p = scan_xml_open_tag(source, p, &mut pieces, &mut tag_stack)?;
        } else {
            let text_start = p;
            while p < len && bytes[p] != b'<' && bytes[p] != b'{' {
                let ch = source[p..].chars().next().unwrap();
                p += ch.len_utf8();
            }
            push_xml_raw(&mut pieces, &source[text_start..p]);
        }
    }

    Ok((SwfToken::XmlLiteral { is_list, pieces }, p))
}

pub(super) struct SwfParser {
    pub(super) tokens: Vec<SwfToken>,
    pub(super) pos: usize,
    pub(super) is_avm2: bool,
    pub(super) function_depth: usize,
    pub(super) in_function_expression: bool,
    pub(super) allow_in: bool,
}

impl SwfParser {
    pub(super) fn new(tokens: Vec<SwfToken>, is_avm2: bool) -> Self {
        Self {
            tokens,
            pos: 0,
            is_avm2,
            function_depth: 0,
            in_function_expression: false,
            allow_in: true,
        }
    }

    pub(super) fn peek(&self) -> Option<&SwfToken> {
        self.tokens.get(self.pos)
    }

    pub(super) fn next(&mut self) -> Option<SwfToken> {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    pub(super) fn eat(&mut self, expected: &SwfToken) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub(super) fn expect(&mut self, expected: &SwfToken) -> Result<(), String> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(format!(
                "expected {expected:?}, found {:?} in bounded SWF compiler",
                self.peek()
            ))
        }
    }

    pub(super) fn expect_ident(&mut self) -> Result<String, String> {
        match self.next() {
            Some(SwfToken::Ident(s)) => Ok(s),
            other => Err(format!(
                "expected identifier, found {other:?} in bounded SWF compiler"
            )),
        }
    }

    pub(super) fn parse_optional_type_name(&mut self) -> Result<Option<String>, String> {
        if !self.eat(&SwfToken::Colon) {
            return Ok(None);
        }
        if self.eat(&SwfToken::Star) {
            return Ok(None);
        }
        let mut name = self.expect_ident()?;
        while self.eat(&SwfToken::Dot) {
            if self.eat(&SwfToken::Lt) {
                let mut depth = 1usize;
                while depth > 0 {
                    match self.next() {
                        Some(SwfToken::Lt) => depth += 1,
                        Some(SwfToken::Gt) => depth -= 1,
                        Some(SwfToken::Shr) => depth = depth.saturating_sub(2),
                        Some(SwfToken::UShr) => depth = depth.saturating_sub(3),
                        Some(_) => {}
                        None => return Err("unterminated generic type annotation".to_owned()),
                    }
                }
                break;
            }
            let next = self.expect_ident()?;
            name.push('.');
            name.push_str(&next);
        }
        Ok(Some(name))
    }

    pub(super) fn skip_type_annotation(&mut self) -> Result<(), String> {
        let _ = self.parse_optional_type_name()?;
        Ok(())
    }

    pub(super) fn parse_stmt_list(&mut self, stop_at_rbrace: bool) -> Result<Vec<SwfStmt>, String> {
        let mut stmts = Vec::new();
        while let Some(tok) = self.peek() {
            if stop_at_rbrace && *tok == SwfToken::RBrace {
                break;
            }
            if *tok == SwfToken::Semi {
                self.pos += 1;
                continue;
            }
            self.parse_stmt_into(&mut stmts)?;
        }
        Ok(stmts)
    }

    pub(super) fn parse_block_or_single(&mut self) -> Result<Vec<SwfStmt>, String> {
        if self.eat(&SwfToken::LBrace) {
            let stmts = self.parse_stmt_list(true)?;
            self.expect(&SwfToken::RBrace)?;
            Ok(stmts)
        } else {
            let mut stmts = Vec::new();
            self.parse_stmt_into(&mut stmts)?;
            Ok(stmts)
        }
    }

    pub(super) fn consume_optional_semi(&mut self) {
        while self.eat(&SwfToken::Semi) {}
    }

    pub(super) fn parse_stmt_into(&mut self, out: &mut Vec<SwfStmt>) -> Result<(), String> {
        while let Some(SwfToken::Ident(m)) = self.peek() {
            if matches!(
                m.as_str(),
                "public" | "private" | "protected" | "internal" | "static" | "override" | "final"
            ) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let Some(tok) = self.peek().cloned() else {
            return Ok(());
        };
        match tok {
            SwfToken::Semi => {
                self.pos += 1;
                Ok(())
            }
            SwfToken::LBrace => {
                self.pos += 1;
                let inner = self.parse_stmt_list(true)?;
                self.expect(&SwfToken::RBrace)?;
                out.push(SwfStmt::Block(inner));
                Ok(())
            }
            SwfToken::Ident(ref kw) => match kw.as_str() {
                "with" | "interface" => Err(format!(
                    "ActionScript '{kw}' is not supported by the bounded SWF compiler"
                )),
                "default"
                    if self.is_avm2
                        && self.tokens.get(self.pos + 1)
                            == Some(&SwfToken::Ident("xml".to_owned()))
                        && self.tokens.get(self.pos + 2)
                            == Some(&SwfToken::Ident("namespace".to_owned())) =>
                {
                    self.pos += 3;
                    self.expect(&SwfToken::Assign)?;
                    let ns_expr = self.parse_expr()?;
                    self.consume_optional_semi();
                    out.push(SwfStmt::DefaultXmlNamespace(ns_expr));
                    Ok(())
                }
                "catch" | "finally" | "case" | "default" => Err(format!(
                    "unexpected ActionScript '{kw}' outside its enclosing statement in bounded SWF compiler"
                )),
                "package" => Err(
                    "ActionScript 'package' declaration is not allowed inside a statement block"
                        .to_owned(),
                ),
                "class" => {
                    if self.is_avm2 || self.function_depth > 0 {
                        return Err(
                            "ActionScript 'class' declaration is not allowed inside a statement block"
                                .to_owned(),
                        );
                    }
                    let cls_stmt = self.parse_as2_class_decl()?;
                    out.push(cls_stmt);
                    Ok(())
                }
                "import" => {
                    self.pos += 1;
                    let _ = self.expect_ident()?;
                    while self.eat(&SwfToken::Dot) {
                        if !self.eat(&SwfToken::Star) {
                            let _ = self.expect_ident()?;
                        }
                    }
                    self.consume_optional_semi();
                    Ok(())
                }
                "break" => {
                    self.pos += 1;
                    self.consume_optional_semi();
                    out.push(SwfStmt::Break);
                    Ok(())
                }
                "continue" => {
                    self.pos += 1;
                    self.consume_optional_semi();
                    out.push(SwfStmt::Continue);
                    Ok(())
                }
                "throw" => {
                    self.pos += 1;
                    let expr = self.parse_expr()?;
                    self.consume_optional_semi();
                    out.push(SwfStmt::Throw(expr));
                    Ok(())
                }
                "switch" => {
                    self.pos += 1;
                    self.expect(&SwfToken::LParen)?;
                    let discriminant = self.parse_expr()?;
                    self.expect(&SwfToken::RParen)?;
                    self.expect(&SwfToken::LBrace)?;
                    let mut cases = Vec::new();
                    while self.peek().is_some() && self.peek() != Some(&SwfToken::RBrace) {
                        if self.eat(&SwfToken::Semi) {
                            continue;
                        }
                        let case_expr = if self.peek() == Some(&SwfToken::Ident("case".to_owned()))
                        {
                            self.pos += 1;
                            let ce = self.parse_expr()?;
                            self.expect(&SwfToken::Colon)?;
                            Some(ce)
                        } else if self.peek() == Some(&SwfToken::Ident("default".to_owned())) {
                            self.pos += 1;
                            self.expect(&SwfToken::Colon)?;
                            None
                        } else {
                            return Err(format!(
                                "expected 'case' or 'default' inside switch, found {:?} in bounded SWF compiler",
                                self.peek()
                            ));
                        };
                        let mut case_stmts = Vec::new();
                        while let Some(next_tok) = self.peek() {
                            if *next_tok == SwfToken::RBrace {
                                break;
                            }
                            if matches!(
                                next_tok,
                                SwfToken::Ident(id)
                                    if id == "case"
                                        || (id == "default"
                                            && self.tokens.get(self.pos + 1)
                                                != Some(&SwfToken::Ident("xml".to_owned())))
                            ) {
                                break;
                            }
                            if *next_tok == SwfToken::Semi {
                                self.pos += 1;
                                continue;
                            }
                            self.parse_stmt_into(&mut case_stmts)?;
                        }
                        cases.push((case_expr, case_stmts));
                    }
                    self.expect(&SwfToken::RBrace)?;
                    out.push(SwfStmt::Switch {
                        discriminant,
                        cases,
                    });
                    Ok(())
                }
                "try" => {
                    self.pos += 1;
                    self.expect(&SwfToken::LBrace)?;
                    let try_body = self.parse_stmt_list(true)?;
                    self.expect(&SwfToken::RBrace)?;
                    let mut catches = Vec::new();
                    while self.peek() == Some(&SwfToken::Ident("catch".to_owned())) {
                        self.pos += 1;
                        self.expect(&SwfToken::LParen)?;
                        let var_name = self.expect_ident()?;
                        let exc_type = self.parse_optional_type_name()?;
                        self.expect(&SwfToken::RParen)?;
                        self.expect(&SwfToken::LBrace)?;
                        let catch_body = self.parse_stmt_list(true)?;
                        self.expect(&SwfToken::RBrace)?;
                        catches.push((var_name, exc_type, catch_body));
                    }
                    let finally_body =
                        if self.peek() == Some(&SwfToken::Ident("finally".to_owned())) {
                            self.pos += 1;
                            self.expect(&SwfToken::LBrace)?;
                            let fin = self.parse_stmt_list(true)?;
                            self.expect(&SwfToken::RBrace)?;
                            Some(fin)
                        } else {
                            None
                        };
                    if catches.is_empty() && finally_body.is_none() {
                        return Err(
                            "expected 'catch' or 'finally' after 'try' block in bounded SWF compiler"
                                .to_owned(),
                        );
                    }
                    out.push(SwfStmt::Try {
                        try_body,
                        catches,
                        finally_body,
                    });
                    Ok(())
                }
                "var" | "const" => {
                    self.pos += 1;
                    loop {
                        let name = self.expect_ident()?;
                        self.skip_type_annotation()?;
                        let init = if self.eat(&SwfToken::Assign) {
                            Some(self.parse_expr()?)
                        } else {
                            None
                        };
                        out.push(SwfStmt::VarDecl { name, init });
                        if !self.eat(&SwfToken::Comma) {
                            break;
                        }
                    }
                    self.consume_optional_semi();
                    Ok(())
                }
                "if" => {
                    self.pos += 1;
                    self.expect(&SwfToken::LParen)?;
                    let cond = self.parse_expr()?;
                    self.expect(&SwfToken::RParen)?;
                    let then_branch = self.parse_block_or_single()?;
                    let else_branch = if self.peek() == Some(&SwfToken::Ident("else".to_owned())) {
                        self.pos += 1;
                        Some(self.parse_block_or_single()?)
                    } else {
                        None
                    };
                    out.push(SwfStmt::If {
                        cond,
                        then_branch,
                        else_branch,
                    });
                    Ok(())
                }
                "while" | "do" => {
                    let post_test = kw == "do";
                    self.pos += 1;
                    let first_body = if post_test {
                        let body = self.parse_block_or_single()?;
                        self.expect(&SwfToken::Ident("while".to_owned()))?;
                        Some(body)
                    } else {
                        None
                    };
                    self.expect(&SwfToken::LParen)?;
                    let cond = self.parse_expr()?;
                    self.expect(&SwfToken::RParen)?;
                    let body = match first_body {
                        Some(body) => {
                            self.consume_optional_semi();
                            body
                        }
                        None => self.parse_block_or_single()?,
                    };
                    out.push(SwfStmt::While {
                        post_test,
                        cond,
                        body,
                    });
                    Ok(())
                }
                "for" => {
                    self.pos += 1;
                    let is_each = if self.peek() == Some(&SwfToken::Ident("each".to_owned())) {
                        self.pos += 1;
                        true
                    } else {
                        false
                    };
                    self.expect(&SwfToken::LParen)?;
                    if is_each {
                        let is_var_decl = matches!(
                            self.peek(),
                            Some(SwfToken::Ident(k)) if k == "var" || k == "const"
                        );
                        if is_var_decl {
                            self.pos += 1;
                        }
                        let var_name = self.expect_ident()?;
                        self.skip_type_annotation()?;
                        if self.next() != Some(SwfToken::Ident("in".to_owned())) {
                            return Err("expected 'in' inside 'for each' loop header".to_owned());
                        }
                        let object = self.parse_expr()?;
                        self.expect(&SwfToken::RParen)?;
                        let body = self.parse_block_or_single()?;
                        out.push(SwfStmt::ForIn {
                            is_each: true,
                            is_var_decl,
                            var_name,
                            object,
                            body,
                        });
                        return Ok(());
                    }
                    let init = if self.eat(&SwfToken::Semi) {
                        None
                    } else {
                        let saved_allow_in = self.allow_in;
                        self.allow_in = false;
                        let mut init_stmts = Vec::new();
                        if self.peek() == Some(&SwfToken::Ident("var".to_owned()))
                            || self.peek() == Some(&SwfToken::Ident("const".to_owned()))
                        {
                            self.pos += 1;
                            let name = self.expect_ident()?;
                            self.skip_type_annotation()?;
                            if self.peek() == Some(&SwfToken::Ident("in".to_owned())) {
                                self.pos += 1;
                                self.allow_in = saved_allow_in;
                                let object = self.parse_expr()?;
                                self.expect(&SwfToken::RParen)?;
                                let body = self.parse_block_or_single()?;
                                out.push(SwfStmt::ForIn {
                                    is_each: false,
                                    is_var_decl: true,
                                    var_name: name,
                                    object,
                                    body,
                                });
                                return Ok(());
                            }
                            let init_val = if self.eat(&SwfToken::Assign) {
                                Some(self.parse_expr()?)
                            } else {
                                None
                            };
                            init_stmts.push(SwfStmt::VarDecl {
                                name,
                                init: init_val,
                            });
                        } else {
                            let saved_pos = self.pos;
                            if let Some(SwfToken::Ident(var_name)) = self.peek().cloned() {
                                self.pos += 1;
                                if self.peek() == Some(&SwfToken::Ident("in".to_owned())) {
                                    self.pos += 1;
                                    self.allow_in = saved_allow_in;
                                    let object = self.parse_expr()?;
                                    self.expect(&SwfToken::RParen)?;
                                    let body = self.parse_block_or_single()?;
                                    out.push(SwfStmt::ForIn {
                                        is_each: false,
                                        is_var_decl: false,
                                        var_name,
                                        object,
                                        body,
                                    });
                                    return Ok(());
                                }
                                self.pos = saved_pos;
                            }
                            let s = self.parse_simple_assign_or_expr_stmt();
                            self.allow_in = saved_allow_in;
                            init_stmts.push(s?);
                        }
                        self.allow_in = saved_allow_in;
                        self.expect(&SwfToken::Semi)?;
                        init_stmts.into_iter().next().map(Box::new)
                    };
                    let cond = if self.eat(&SwfToken::Semi) {
                        None
                    } else {
                        let c = self.parse_expr()?;
                        self.expect(&SwfToken::Semi)?;
                        Some(c)
                    };
                    let update = if self.eat(&SwfToken::RParen) {
                        None
                    } else {
                        let u = self.parse_simple_assign_or_expr_stmt()?;
                        self.expect(&SwfToken::RParen)?;
                        Some(Box::new(u))
                    };
                    let body = self.parse_block_or_single()?;
                    out.push(SwfStmt::For {
                        init,
                        cond,
                        update,
                        body,
                    });
                    Ok(())
                }
                "function" => {
                    self.pos += 1;
                    let name = self.expect_ident()?;
                    self.expect(&SwfToken::LParen)?;
                    let mut params = Vec::new();
                    if !self.eat(&SwfToken::RParen) {
                        loop {
                            let pname = self.expect_ident()?;
                            self.skip_type_annotation()?;
                            if self.eat(&SwfToken::Assign) {
                                let _ = self.parse_expr()?;
                            }
                            params.push(pname);
                            if self.eat(&SwfToken::Comma) {
                                continue;
                            }
                            self.expect(&SwfToken::RParen)?;
                            break;
                        }
                    }
                    self.skip_type_annotation()?;
                    self.expect(&SwfToken::LBrace)?;
                    self.function_depth += 1;
                    let body = self.parse_stmt_list(true)?;
                    self.function_depth -= 1;
                    self.expect(&SwfToken::RBrace)?;
                    if self.in_function_expression && find_event_handler_in_stmts(&body).is_some() {
                        return Err(
                            "AVM1 object event handler syntax is not allowed inside a function expression"
                                .to_owned(),
                        );
                    }
                    out.push(SwfStmt::FunctionDecl { name, params, body });
                    Ok(())
                }
                "return" => {
                    self.pos += 1;
                    if self.eat(&SwfToken::Semi)
                        || self.peek() == Some(&SwfToken::RBrace)
                        || self.peek().is_none()
                    {
                        out.push(SwfStmt::Return(None));
                    } else {
                        let expr = self.parse_expr()?;
                        self.consume_optional_semi();
                        out.push(SwfStmt::Return(Some(expr)));
                    }
                    Ok(())
                }
                "on" | "onClipEvent" => {
                    if self.is_avm2 {
                        return Err(format!(
                            "AVM1 '{kw}(...)' event handler syntax is not supported in ActionScript 3"
                        ));
                    }
                    let is_clip_event = kw == "onClipEvent";
                    self.pos += 1;
                    self.expect(&SwfToken::LParen)?;
                    let mut events = Vec::new();
                    loop {
                        let ev = self.expect_ident()?;
                        if ev == "keyPress" {
                            if let Some(SwfToken::Str(k)) = self.peek().cloned() {
                                self.pos += 1;
                                events.push(format!("keyPress \"{k}\""));
                            } else {
                                return Err(
                                    "expected string literal after 'keyPress' in event handler"
                                        .to_owned(),
                                );
                            }
                        } else {
                            events.push(ev);
                        }
                        if !self.eat(&SwfToken::Comma) {
                            break;
                        }
                    }
                    self.expect(&SwfToken::RParen)?;
                    self.expect(&SwfToken::LBrace)?;
                    let body = self.parse_stmt_list(true)?;
                    self.expect(&SwfToken::RBrace)?;
                    out.push(SwfStmt::EventHandler {
                        is_clip_event,
                        events,
                        body,
                    });
                    Ok(())
                }
                _ => {
                    let stmt = self.parse_simple_assign_or_expr_stmt()?;
                    self.consume_optional_semi();
                    out.push(stmt);
                    Ok(())
                }
            },
            SwfToken::PlusPlus | SwfToken::MinusMinus | SwfToken::LParen => {
                let stmt = self.parse_simple_assign_or_expr_stmt()?;
                self.consume_optional_semi();
                out.push(stmt);
                Ok(())
            }
            other => Err(format!(
                "unexpected token {other:?} at start of statement in bounded SWF compiler"
            )),
        }
    }

    pub(super) fn parse_as2_class_decl(&mut self) -> Result<SwfStmt, String> {
        self.expect(&SwfToken::Ident("class".to_owned()))?;
        let mut full_name = self.expect_ident()?;
        while self.eat(&SwfToken::Dot) {
            let seg = self.expect_ident()?;
            full_name.push('.');
            full_name.push_str(&seg);
        }
        let short_name = full_name
            .rsplit_once('.')
            .map_or(full_name.as_str(), |(_, s)| s)
            .to_owned();
        let super_name = if self.peek() == Some(&SwfToken::Ident("extends".to_owned())) {
            self.pos += 1;
            let mut sup = self.expect_ident()?;
            while self.eat(&SwfToken::Dot) {
                let seg = self.expect_ident()?;
                sup.push('.');
                sup.push_str(&seg);
            }
            Some(
                sup.rsplit_once('.')
                    .map_or(sup.as_str(), |(_, s)| s)
                    .to_owned(),
            )
        } else {
            None
        };
        if self.peek() == Some(&SwfToken::Ident("implements".to_owned())) {
            self.pos += 1;
            loop {
                let _ = self.expect_ident()?;
                while self.eat(&SwfToken::Dot) {
                    let _ = self.expect_ident()?;
                }
                if !self.eat(&SwfToken::Comma) {
                    break;
                }
            }
        }
        self.expect(&SwfToken::LBrace)?;
        let mut ctor = None;
        let mut methods = Vec::new();
        let mut fields = Vec::new();
        while self.peek().is_some() && self.peek() != Some(&SwfToken::RBrace) {
            if self.eat(&SwfToken::Semi) {
                continue;
            }
            let mut is_static = false;
            while let Some(SwfToken::Ident(m)) = self.peek() {
                if matches!(
                    m.as_str(),
                    "public" | "private" | "protected" | "internal" | "static" | "dynamic"
                ) {
                    if m == "static" {
                        is_static = true;
                    }
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.peek() == Some(&SwfToken::Ident("function".to_owned())) {
                self.pos += 1;
                if matches!(
                    self.peek(),
                    Some(SwfToken::Ident(acc)) if (acc == "get" || acc == "set")
                        && matches!(self.tokens.get(self.pos + 1), Some(SwfToken::Ident(_)))
                ) {
                    return Err(
                        "AS2 getter/setter methods are not supported by the bounded SWF compiler"
                            .to_owned(),
                    );
                }
                let fn_name = self.expect_ident()?;
                self.expect(&SwfToken::LParen)?;
                let mut params = Vec::new();
                if !self.eat(&SwfToken::RParen) {
                    loop {
                        let pname = self.expect_ident()?;
                        self.skip_type_annotation()?;
                        if self.eat(&SwfToken::Assign) {
                            let _ = self.parse_expr()?;
                        }
                        params.push(pname);
                        if self.eat(&SwfToken::Comma) {
                            continue;
                        }
                        self.expect(&SwfToken::RParen)?;
                        break;
                    }
                }
                self.skip_type_annotation()?;
                self.expect(&SwfToken::LBrace)?;
                self.function_depth += 1;
                let body_res = self.parse_stmt_list(true);
                self.function_depth -= 1;
                let body = body_res?;
                self.expect(&SwfToken::RBrace)?;
                if !is_static && fn_name == short_name {
                    ctor = Some((params, body));
                } else {
                    methods.push((is_static, fn_name, params, body));
                }
            } else if matches!(
                self.peek(),
                Some(SwfToken::Ident(k)) if k == "var" || k == "const"
            ) {
                self.pos += 1;
                loop {
                    let fname = self.expect_ident()?;
                    self.skip_type_annotation()?;
                    let init = if self.eat(&SwfToken::Assign) {
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    fields.push((is_static, fname, init));
                    if !self.eat(&SwfToken::Comma) {
                        break;
                    }
                }
                self.consume_optional_semi();
            } else {
                return Err(format!(
                    "unexpected member token {:?} inside AS2 class '{}'",
                    self.peek(),
                    short_name
                ));
            }
        }
        self.expect(&SwfToken::RBrace)?;
        Ok(SwfStmt::ClassDecl {
            name: short_name,
            super_name,
            ctor,
            methods,
            fields,
        })
    }

    pub(super) fn parse_simple_assign_or_expr_stmt(&mut self) -> Result<SwfStmt, String> {
        if self.eat(&SwfToken::PlusPlus) {
            let expr = self.parse_postfix()?;
            let target = expr_to_lvalue(expr)?;
            return Ok(SwfStmt::IncDec {
                target,
                increment: true,
            });
        }
        if self.eat(&SwfToken::MinusMinus) {
            let expr = self.parse_postfix()?;
            let target = expr_to_lvalue(expr)?;
            return Ok(SwfStmt::IncDec {
                target,
                increment: false,
            });
        }
        let lhs = self.parse_expr()?;
        if let Some(op) = match self.peek() {
            Some(SwfToken::Assign) => Some(SwfAssignOp::Assign),
            Some(SwfToken::CompoundAssign(op)) => Some(*op),
            _ => None,
        } {
            self.pos += 1;
            let target = expr_to_lvalue(lhs)?;
            let value = self.parse_expr()?;
            return Ok(SwfStmt::Assign { target, op, value });
        }
        if self.eat(&SwfToken::PlusPlus) {
            let target = expr_to_lvalue(lhs)?;
            return Ok(SwfStmt::IncDec {
                target,
                increment: true,
            });
        }
        if self.eat(&SwfToken::MinusMinus) {
            let target = expr_to_lvalue(lhs)?;
            return Ok(SwfStmt::IncDec {
                target,
                increment: false,
            });
        }
        if matches!(
            lhs,
            SwfExpr::Call { .. } | SwfExpr::New { .. } | SwfExpr::Delete(_)
        ) {
            Ok(SwfStmt::Expr(lhs))
        } else {
            Err(
                "standalone expression is not a function call or assignment in bounded SWF compiler"
                    .to_owned(),
            )
        }
    }

    pub(super) fn parse_expr(&mut self) -> Result<SwfExpr, String> {
        self.parse_ternary()
    }

    pub(super) fn parse_ternary(&mut self) -> Result<SwfExpr, String> {
        let cond = self.parse_logical_or()?;
        if self.eat(&SwfToken::Question) {
            let saved_in = self.allow_in;
            self.allow_in = true;
            let then_expr = self.parse_expr();
            self.allow_in = saved_in;
            let then_expr = then_expr?;
            self.expect(&SwfToken::Colon)?;
            let else_expr = self.parse_ternary()?;
            return Ok(SwfExpr::Ternary {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            });
        }
        Ok(cond)
    }

    pub(super) fn parse_logical_or(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_logical_and()?;
        while self.eat(&SwfToken::Or) {
            let rhs = self.parse_logical_and()?;
            lhs = SwfExpr::Binary {
                op: SwfBinaryOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_logical_and(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_bit_or()?;
        while self.eat(&SwfToken::And) {
            let rhs = self.parse_bit_or()?;
            lhs = SwfExpr::Binary {
                op: SwfBinaryOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_bit_or(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_bit_xor()?;
        while self.eat(&SwfToken::BitOr) {
            let rhs = self.parse_bit_xor()?;
            lhs = SwfExpr::Binary {
                op: SwfBinaryOp::BitOr,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_bit_xor(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_bit_and()?;
        while self.eat(&SwfToken::BitXor) {
            let rhs = self.parse_bit_and()?;
            lhs = SwfExpr::Binary {
                op: SwfBinaryOp::BitXor,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_bit_and(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_equality()?;
        while self.eat(&SwfToken::BitAnd) {
            let rhs = self.parse_equality()?;
            lhs = SwfExpr::Binary {
                op: SwfBinaryOp::BitAnd,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_equality(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_relational()?;
        while let Some(op) = match self.peek() {
            Some(SwfToken::Eq) => Some(SwfBinaryOp::Eq),
            Some(SwfToken::Ne) => Some(SwfBinaryOp::Ne),
            Some(SwfToken::StrictEq) => Some(SwfBinaryOp::StrictEq),
            Some(SwfToken::StrictNe) => Some(SwfBinaryOp::StrictNe),
            _ => None,
        } {
            self.pos += 1;
            let rhs = self.parse_relational()?;
            lhs = SwfExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_relational(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_shift()?;
        while let Some(op) = match self.peek() {
            Some(SwfToken::Lt) => Some(SwfBinaryOp::Lt),
            Some(SwfToken::Le) => Some(SwfBinaryOp::Le),
            Some(SwfToken::Gt) => Some(SwfBinaryOp::Gt),
            Some(SwfToken::Ge) => Some(SwfBinaryOp::Ge),
            Some(SwfToken::Ident(kw)) if kw == "instanceof" => Some(SwfBinaryOp::InstanceOf),
            Some(SwfToken::Ident(kw)) if self.is_avm2 && kw == "is" => Some(SwfBinaryOp::Is),
            Some(SwfToken::Ident(kw)) if self.is_avm2 && kw == "as" => Some(SwfBinaryOp::As),
            Some(SwfToken::Ident(kw)) if self.allow_in && kw == "in" => Some(SwfBinaryOp::In),
            _ => None,
        } {
            self.pos += 1;
            let rhs =
                if matches!(op, SwfBinaryOp::Is | SwfBinaryOp::As) && self.eat(&SwfToken::Star) {
                    SwfExpr::Ident("*".to_owned())
                } else {
                    self.parse_shift()?
                };
            lhs = SwfExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_shift(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_additive()?;
        while let Some(op) = match self.peek() {
            Some(SwfToken::Shl) => Some(SwfBinaryOp::Shl),
            Some(SwfToken::Shr) => Some(SwfBinaryOp::Shr),
            Some(SwfToken::UShr) => Some(SwfBinaryOp::UShr),
            _ => None,
        } {
            self.pos += 1;
            let rhs = self.parse_additive()?;
            lhs = SwfExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_additive(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_multiplicative()?;
        while let Some(op) = match self.peek() {
            Some(SwfToken::Plus) => Some(SwfBinaryOp::Add),
            Some(SwfToken::Minus) => Some(SwfBinaryOp::Sub),
            _ => None,
        } {
            self.pos += 1;
            let rhs = self.parse_multiplicative()?;
            lhs = SwfExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_multiplicative(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_unary()?;
        while let Some(op) = match self.peek() {
            Some(SwfToken::Star) => Some(SwfBinaryOp::Mul),
            Some(SwfToken::Slash) => Some(SwfBinaryOp::Div),
            Some(SwfToken::Percent) => Some(SwfBinaryOp::Mod),
            _ => None,
        } {
            self.pos += 1;
            let rhs = self.parse_unary()?;
            lhs = SwfExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_unary(&mut self) -> Result<SwfExpr, String> {
        if self.eat(&SwfToken::Not) {
            let expr = self.parse_unary()?;
            return Ok(SwfExpr::Unary {
                op: SwfUnaryOp::Not,
                expr: Box::new(expr),
            });
        }
        if self.eat(&SwfToken::Tilde) {
            let expr = self.parse_unary()?;
            return Ok(match expr {
                SwfExpr::Int(v) => SwfExpr::Int(!v),
                other => SwfExpr::Unary {
                    op: SwfUnaryOp::BitNot,
                    expr: Box::new(other),
                },
            });
        }
        if self.peek() == Some(&SwfToken::Ident("typeof".to_owned())) {
            self.pos += 1;
            let expr = self.parse_unary()?;
            return Ok(SwfExpr::Unary {
                op: SwfUnaryOp::Typeof,
                expr: Box::new(expr),
            });
        }
        if self.peek() == Some(&SwfToken::Ident("delete".to_owned())) {
            self.pos += 1;
            let expr = self.parse_unary()?;
            let target = expr_to_lvalue(expr)?;
            return Ok(SwfExpr::Delete(Box::new(target)));
        }
        if self.eat(&SwfToken::Minus) {
            let expr = self.parse_unary()?;
            return Ok(match expr {
                SwfExpr::Int(v) if v != i32::MIN => SwfExpr::Int(-v),
                SwfExpr::Float(2_147_483_648.0) => SwfExpr::Int(i32::MIN),
                SwfExpr::Float(v) => SwfExpr::Float(-v),
                other => SwfExpr::Unary {
                    op: SwfUnaryOp::Neg,
                    expr: Box::new(other),
                },
            });
        }
        if self.eat(&SwfToken::Plus) {
            let expr = self.parse_unary()?;
            return Ok(match expr {
                SwfExpr::Int(v) => SwfExpr::Int(v),
                SwfExpr::Float(v) => SwfExpr::Float(v),
                other => SwfExpr::Unary {
                    op: SwfUnaryOp::Pos,
                    expr: Box::new(other),
                },
            });
        }
        self.parse_postfix()
    }

    fn parse_xml_selector(&mut self) -> Result<SwfXmlSelector, String> {
        if self.eat(&SwfToken::LBracket) {
            let saved_in = self.allow_in;
            self.allow_in = true;
            let idx_res = self.parse_expr();
            self.allow_in = saved_in;
            let idx = idx_res?;
            self.expect(&SwfToken::RBracket)?;
            Ok(SwfXmlSelector::Computed(Box::new(idx)))
        } else if self.eat(&SwfToken::Star) {
            Ok(SwfXmlSelector::Named("*".to_owned()))
        } else {
            Ok(SwfXmlSelector::Named(self.expect_ident()?))
        }
    }

    pub(super) fn parse_postfix(&mut self) -> Result<SwfExpr, String> {
        let is_new = self.peek() == Some(&SwfToken::Ident("new".to_owned()));
        if is_new {
            self.pos += 1;
        }
        let mut expr = self.parse_primary()?;
        loop {
            if self.eat(&SwfToken::Dot) {
                if self.eat(&SwfToken::At) {
                    if !self.is_avm2 {
                        return Err(
                            "ActionScript 3 E4X '@' attribute access is not supported in AVM1"
                                .to_owned(),
                        );
                    }
                    let attr = self.parse_xml_selector()?;
                    expr = SwfExpr::XmlAttr {
                        object: Box::new(expr),
                        attr,
                    };
                } else if self.is_avm2 && self.eat(&SwfToken::Star) {
                    expr = SwfExpr::Member(Box::new(expr), "*".to_owned());
                } else if self.is_avm2
                    && let Some(&SwfToken::Int(iv)) = self.peek()
                    && iv >= 0
                {
                    self.pos += 1;
                    expr = SwfExpr::Index(Box::new(expr), Box::new(SwfExpr::Int(iv)));
                } else {
                    let prop = self.expect_ident()?;
                    expr = SwfExpr::Member(Box::new(expr), prop);
                }
            } else if self.eat(&SwfToken::DotDot) {
                if !self.is_avm2 {
                    return Err(
                        "ActionScript 3 E4X '..' descendant operator is not supported in AVM1"
                            .to_owned(),
                    );
                }
                let is_attr = self.eat(&SwfToken::At);
                let target = self.parse_xml_selector()?;
                expr = SwfExpr::XmlDescendants {
                    object: Box::new(expr),
                    target,
                    is_attr,
                };
            } else if self.eat(&SwfToken::LBracket) {
                let idx = self.parse_expr()?;
                self.expect(&SwfToken::RBracket)?;
                expr = SwfExpr::Index(Box::new(expr), Box::new(idx));
            } else if self.eat(&SwfToken::LParen) {
                let mut args = Vec::new();
                if !self.eat(&SwfToken::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.eat(&SwfToken::Comma) {
                            continue;
                        }
                        self.expect(&SwfToken::RParen)?;
                        break;
                    }
                }
                if is_new {
                    expr = SwfExpr::New {
                        callee: Box::new(expr),
                        args,
                    };
                    return Ok(expr);
                }
                expr = SwfExpr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else {
                break;
            }
        }
        if is_new {
            return Ok(SwfExpr::New {
                callee: Box::new(expr),
                args: Vec::new(),
            });
        }
        Ok(expr)
    }

    pub(super) fn parse_arrow_body(&mut self) -> Result<Vec<SwfStmt>, String> {
        let prev_in_fn_expr = self.in_function_expression;
        self.function_depth += 1;
        self.in_function_expression = true;
        let body_res: Result<Vec<SwfStmt>, String> = if self.eat(&SwfToken::LBrace) {
            let stmts = self.parse_stmt_list(true)?;
            self.expect(&SwfToken::RBrace)?;
            Ok(stmts)
        } else {
            let expr = self.parse_expr()?;
            Ok(vec![SwfStmt::Return(Some(expr))])
        };
        self.in_function_expression = prev_in_fn_expr;
        self.function_depth -= 1;
        let body = body_res?;
        if find_event_handler_in_stmts(&body).is_some() {
            return Err(
                "AVM1 object event handler syntax is not allowed inside a function expression"
                    .to_owned(),
            );
        }
        Ok(body)
    }

    pub(super) fn try_parse_arrow_after_lparen(&mut self) -> Result<Option<SwfExpr>, String> {
        let saved_pos = self.pos;
        let mut params = Vec::new();
        if !self.eat(&SwfToken::RParen) {
            loop {
                let Some(SwfToken::Ident(pname)) = self.next() else {
                    self.pos = saved_pos;
                    return Ok(None);
                };
                if self.skip_type_annotation().is_err() {
                    self.pos = saved_pos;
                    return Ok(None);
                }
                if self.eat(&SwfToken::Assign) && self.parse_expr().is_err() {
                    self.pos = saved_pos;
                    return Ok(None);
                }
                params.push(pname);
                if self.eat(&SwfToken::Comma) {
                    continue;
                }
                if !self.eat(&SwfToken::RParen) {
                    self.pos = saved_pos;
                    return Ok(None);
                }
                break;
            }
        }
        if self.peek() == Some(&SwfToken::Colon) && self.skip_type_annotation().is_err() {
            self.pos = saved_pos;
            return Ok(None);
        }
        if !self.eat(&SwfToken::FatArrow) {
            self.pos = saved_pos;
            return Ok(None);
        }
        let body = self.parse_arrow_body()?;
        Ok(Some(SwfExpr::Function {
            name: None,
            params,
            body,
        }))
    }

    pub(super) fn parse_primary(&mut self) -> Result<SwfExpr, String> {
        match self.next() {
            Some(SwfToken::Int(v)) => Ok(SwfExpr::Int(v)),
            Some(SwfToken::Float(v)) => Ok(SwfExpr::Float(v)),
            Some(SwfToken::Str(s)) => Ok(SwfExpr::Str(s)),
            Some(SwfToken::XmlLiteral { is_list, pieces }) => {
                if !self.is_avm2 {
                    return Err(
                        "ActionScript 3 E4X XML literals are not supported in AVM1".to_owned()
                    );
                }
                let mut compiled = Vec::with_capacity(pieces.len());
                for piece in pieces {
                    match piece {
                        SwfXmlRawPiece::Raw(s) => compiled.push(SwfXmlCompiledPiece::Raw(s)),
                        SwfXmlRawPiece::ElemExpr(src) => {
                            let e = parse_swf_expr_strict(&src, true)?;
                            compiled.push(SwfXmlCompiledPiece::ElemExpr(Box::new(e)));
                        }
                        SwfXmlRawPiece::AttrExpr(src) => {
                            let e = parse_swf_expr_strict(&src, true)?;
                            compiled.push(SwfXmlCompiledPiece::AttrExpr(Box::new(e)));
                        }
                    }
                }
                Ok(SwfExpr::XmlLit {
                    is_list,
                    pieces: compiled,
                })
            }
            Some(SwfToken::Ident(s)) => match s.as_str() {
                "true" => Ok(SwfExpr::Bool(true)),
                "false" => Ok(SwfExpr::Bool(false)),
                "null" => Ok(SwfExpr::Null),
                "undefined" => Ok(SwfExpr::Undefined),
                "function" => {
                    let fn_name = if matches!(self.peek(), Some(SwfToken::Ident(_))) {
                        Some(self.expect_ident()?)
                    } else {
                        None
                    };
                    self.expect(&SwfToken::LParen)?;
                    let mut params = Vec::new();
                    if !self.eat(&SwfToken::RParen) {
                        loop {
                            params.push(self.expect_ident()?);
                            self.skip_type_annotation()?;
                            if self.eat(&SwfToken::Assign) {
                                let _ = self.parse_expr()?;
                            }
                            if self.eat(&SwfToken::Comma) {
                                continue;
                            }
                            self.expect(&SwfToken::RParen)?;
                            break;
                        }
                    }
                    self.skip_type_annotation()?;
                    self.expect(&SwfToken::LBrace)?;
                    let prev_in_fn_expr = self.in_function_expression;
                    self.function_depth += 1;
                    self.in_function_expression = true;
                    let body_res = self.parse_stmt_list(true);
                    self.in_function_expression = prev_in_fn_expr;
                    self.function_depth -= 1;
                    let body = body_res?;
                    self.expect(&SwfToken::RBrace)?;
                    if find_event_handler_in_stmts(&body).is_some() {
                        return Err(
                            "AVM1 object event handler syntax is not allowed inside a function expression"
                                .to_owned(),
                        );
                    }
                    Ok(SwfExpr::Function {
                        name: fn_name,
                        params,
                        body,
                    })
                }
                _ => {
                    if self.eat(&SwfToken::FatArrow) {
                        let body = self.parse_arrow_body()?;
                        Ok(SwfExpr::Function {
                            name: None,
                            params: vec![s],
                            body,
                        })
                    } else {
                        Ok(SwfExpr::Ident(s))
                    }
                }
            },
            Some(SwfToken::LParen) => {
                let saved_in = self.allow_in;
                self.allow_in = true;
                let res = (|| {
                    if let Some(arrow) = self.try_parse_arrow_after_lparen()? {
                        return Ok(arrow);
                    }
                    let e = self.parse_expr()?;
                    self.expect(&SwfToken::RParen)?;
                    Ok(e)
                })();
                self.allow_in = saved_in;
                res
            }
            Some(SwfToken::LBrace) => {
                let mut props = Vec::new();
                if !self.eat(&SwfToken::RBrace) {
                    loop {
                        let key = match self.next() {
                            Some(SwfToken::Ident(k)) | Some(SwfToken::Str(k)) => k,
                            Some(SwfToken::Int(k)) => k.to_string(),
                            other => {
                                return Err(format!(
                                    "expected object property key, found {other:?} in bounded SWF compiler"
                                ));
                            }
                        };
                        self.expect(&SwfToken::Colon)?;
                        let val = self.parse_expr()?;
                        props.push((key, val));
                        if self.eat(&SwfToken::Comma) {
                            if self.eat(&SwfToken::RBrace) {
                                break;
                            }
                            continue;
                        }
                        self.expect(&SwfToken::RBrace)?;
                        break;
                    }
                }
                Ok(SwfExpr::ObjectLit(props))
            }
            Some(SwfToken::LBracket) => {
                let saved_in = self.allow_in;
                self.allow_in = true;
                let res = (|| {
                    let mut elems = Vec::new();
                    if !self.eat(&SwfToken::RBracket) {
                        loop {
                            elems.push(self.parse_expr()?);
                            if self.eat(&SwfToken::Comma) {
                                if self.eat(&SwfToken::RBracket) {
                                    break;
                                }
                                continue;
                            }
                            self.expect(&SwfToken::RBracket)?;
                            break;
                        }
                    }
                    Ok(SwfExpr::ArrayLit(elems))
                })();
                self.allow_in = saved_in;
                res
            }
            other => Err(format!(
                "unexpected token {other:?} in expression in bounded SWF compiler"
            )),
        }
    }
}

pub(super) fn expr_to_lvalue(expr: SwfExpr) -> Result<SwfLValue, String> {
    match expr {
        SwfExpr::Ident(name) => Ok(SwfLValue::Ident(name)),
        SwfExpr::Member(obj, prop) => Ok(SwfLValue::Member(obj, prop)),
        SwfExpr::Index(obj, idx) => Ok(SwfLValue::Index(obj, idx)),
        SwfExpr::XmlAttr { object, attr } => Ok(SwfLValue::XmlAttr { object, attr }),
        _ => Err("invalid assignment target in bounded SWF compiler".to_owned()),
    }
}

pub(super) fn parse_swf_expr_strict(source: &str, is_avm2: bool) -> Result<SwfExpr, String> {
    let tokens = tokenize_swf_script(source)?;
    let mut parser = SwfParser::new(tokens, is_avm2);
    let expr = parser.parse_expr()?;
    if parser.pos < parser.tokens.len() {
        return Err(format!(
            "unexpected trailing token {:?} inside expression in bounded SWF compiler",
            parser.peek()
        ));
    }
    Ok(expr)
}

pub(super) fn parse_swf_statements_strict(
    source: &str,
    is_avm2: bool,
) -> Result<Vec<SwfStmt>, String> {
    let tokens = tokenize_swf_script(source)?;
    let mut parser = SwfParser::new(tokens, is_avm2);
    let stmts = parser.parse_stmt_list(false)?;
    if parser.pos < parser.tokens.len() {
        return Err(format!(
            "unexpected trailing token {:?} in bounded SWF compiler",
            parser.peek()
        ));
    }
    Ok(stmts)
}

#[cfg(test)]
pub(super) fn unquote_literal(arg: &str) -> Option<String> {
    let s = arg.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        let delimiter = s.chars().next()?;
        let inner = &s[1..s.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(next) = chars.next() {
                    match next {
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        other => out.push(other),
                    }
                }
            } else if c == delimiter {
                return None;
            } else {
                out.push(c);
            }
        }
        Some(out)
    } else {
        None
    }
}
