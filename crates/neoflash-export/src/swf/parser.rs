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
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SwfLValue {
    Ident(String),
    Member(Box<SwfExpr>, String),
    Index(Box<SwfExpr>, Box<SwfExpr>),
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
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semi,
    Comma,
    Dot,
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
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
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
            if i < len && bytes[i] == b'.' && (i + 1 >= len || bytes[i + 1].is_ascii_digit()) {
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
        let rem = &source[i..];
        if rem.starts_with("===") {
            tokens.push(SwfToken::StrictEq);
            i += 3;
        } else if rem.starts_with("!==") {
            tokens.push(SwfToken::StrictNe);
            i += 3;
        } else if rem.starts_with(">>>") {
            tokens.push(SwfToken::UShr);
            i += 3;
        } else if rem.starts_with("==") {
            tokens.push(SwfToken::Eq);
            i += 2;
        } else if rem.starts_with("=>") {
            tokens.push(SwfToken::FatArrow);
            i += 2;
        } else if rem.starts_with("!=") {
            tokens.push(SwfToken::Ne);
            i += 2;
        } else if rem.starts_with("<=") {
            tokens.push(SwfToken::Le);
            i += 2;
        } else if rem.starts_with(">=") {
            tokens.push(SwfToken::Ge);
            i += 2;
        } else if rem.starts_with("&&") {
            tokens.push(SwfToken::And);
            i += 2;
        } else if rem.starts_with("||") {
            tokens.push(SwfToken::Or);
            i += 2;
        } else if rem.starts_with("++") {
            tokens.push(SwfToken::PlusPlus);
            i += 2;
        } else if rem.starts_with("--") {
            tokens.push(SwfToken::MinusMinus);
            i += 2;
        } else if rem.starts_with("+=") {
            tokens.push(SwfToken::PlusAssign);
            i += 2;
        } else if rem.starts_with("-=") {
            tokens.push(SwfToken::MinusAssign);
            i += 2;
        } else if rem.starts_with("*=") {
            tokens.push(SwfToken::StarAssign);
            i += 2;
        } else if rem.starts_with("/=") {
            tokens.push(SwfToken::SlashAssign);
            i += 2;
        } else if rem.starts_with("%=") {
            tokens.push(SwfToken::PercentAssign);
            i += 2;
        } else if rem.starts_with("<<") {
            tokens.push(SwfToken::Shl);
            i += 2;
        } else if rem.starts_with(">>") {
            tokens.push(SwfToken::Shr);
            i += 2;
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
                "with" | "do" | "interface" => Err(format!(
                    "ActionScript '{kw}' is not supported by the bounded SWF compiler"
                )),
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
                                SwfToken::Ident(id) if id == "case" || id == "default"
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
                "while" => {
                    self.pos += 1;
                    self.expect(&SwfToken::LParen)?;
                    let cond = self.parse_expr()?;
                    self.expect(&SwfToken::RParen)?;
                    let body = self.parse_block_or_single()?;
                    out.push(SwfStmt::While { cond, body });
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
            Some(SwfToken::PlusAssign) => Some(SwfAssignOp::AddAssign),
            Some(SwfToken::MinusAssign) => Some(SwfAssignOp::SubAssign),
            Some(SwfToken::StarAssign) => Some(SwfAssignOp::MulAssign),
            Some(SwfToken::SlashAssign) => Some(SwfAssignOp::DivAssign),
            Some(SwfToken::PercentAssign) => Some(SwfAssignOp::ModAssign),
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

    pub(super) fn parse_postfix(&mut self) -> Result<SwfExpr, String> {
        let is_new = self.peek() == Some(&SwfToken::Ident("new".to_owned()));
        if is_new {
            self.pos += 1;
        }
        let mut expr = self.parse_primary()?;
        loop {
            if self.eat(&SwfToken::Dot) {
                let prop = self.expect_ident()?;
                expr = SwfExpr::Member(Box::new(expr), prop);
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
        _ => Err("invalid assignment target in bounded SWF compiler".to_owned()),
    }
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
