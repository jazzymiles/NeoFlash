use super::*;

// ---------------------------------------------------------------------------
// Lexer & Recursive-Descent Parser for JSFL
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Span {
    pub(super) line: u32,
    pub(super) column: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum TokenKind {
    Ident(String),
    Number(f64),
    Str(String),
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Dot,
    Comma,
    Semi,
    Colon,
    Question,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    PlusPlus,
    MinusMinus,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    EqEq,
    BangEq,
    EqEqEq,
    BangEqEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AndAnd,
    OrOr,
    Eof,
}

#[derive(Clone, Debug)]
pub(super) struct Token {
    pub(super) kind: TokenKind,
    pub(super) span: Span,
}

pub(super) fn tokenize(file: &str, src: &str) -> Result<Vec<Token>, JsflDiagnostic> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut col = 1u32;
    let mut tokens = Vec::new();

    while i < chars.len() {
        let c = chars[i];
        let span = Span { line, column: col };
        if c == '\n' {
            line += 1;
            col = 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            col += 1;
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
                col += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            col += 2;
            let mut closed = false;
            while i + 1 < chars.len() {
                if chars[i] == '*' && chars[i + 1] == '/' {
                    i += 2;
                    col += 2;
                    closed = true;
                    break;
                }
                if chars[i] == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                i += 1;
            }
            if !closed {
                return Err(JsflDiagnostic::new(
                    file,
                    span.line,
                    span.column,
                    "Unterminated block comment.",
                ));
            }
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            col += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < chars.len() {
                let ch = chars[i];
                if ch == quote {
                    i += 1;
                    col += 1;
                    closed = true;
                    break;
                }
                if ch == '\\' {
                    i += 1;
                    col += 1;
                    if i >= chars.len() {
                        break;
                    }
                    let esc = chars[i];
                    match esc {
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        '\'' => s.push('\''),
                        other => s.push(other),
                    }
                    i += 1;
                    col += 1;
                } else {
                    if ch == '\n' {
                        line += 1;
                        col = 1;
                    } else {
                        col += 1;
                    }
                    s.push(ch);
                    i += 1;
                }
            }
            if !closed {
                return Err(JsflDiagnostic::new(
                    file,
                    span.line,
                    span.column,
                    "Unterminated string literal.",
                ));
            }
            tokens.push(Token {
                kind: TokenKind::Str(s),
                span,
            });
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            if c == '0' && i + 1 < chars.len() && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
                i += 2;
                col += 2;
                let hex_start = i;
                while i < chars.len() && chars[i].is_ascii_hexdigit() {
                    i += 1;
                    col += 1;
                }
                let hex_str: String = chars[hex_start..i].iter().collect();
                let val = u64::from_str_radix(&hex_str, 16).map_err(|_| {
                    JsflDiagnostic::new(file, span.line, span.column, "Invalid hex literal.")
                })?;
                tokens.push(Token {
                    kind: TokenKind::Number(val as f64),
                    span,
                });
                continue;
            }
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
                col += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            let val = num_str.parse::<f64>().map_err(|_| {
                JsflDiagnostic::new(file, span.line, span.column, "Invalid numeric literal.")
            })?;
            tokens.push(Token {
                kind: TokenKind::Number(val),
                span,
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '$')
            {
                i += 1;
                col += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            tokens.push(Token {
                kind: TokenKind::Ident(ident),
                span,
            });
            continue;
        }

        let next = chars.get(i + 1).copied();
        let next2 = chars.get(i + 2).copied();
        let (kind, advance) = match (c, next, next2) {
            ('=', Some('='), Some('=')) => (TokenKind::EqEqEq, 3),
            ('!', Some('='), Some('=')) => (TokenKind::BangEqEq, 3),
            ('=', Some('='), _) => (TokenKind::EqEq, 2),
            ('!', Some('='), _) => (TokenKind::BangEq, 2),
            ('<', Some('='), _) => (TokenKind::LtEq, 2),
            ('>', Some('='), _) => (TokenKind::GtEq, 2),
            ('&', Some('&'), _) => (TokenKind::AndAnd, 2),
            ('|', Some('|'), _) => (TokenKind::OrOr, 2),
            ('+', Some('+'), _) => (TokenKind::PlusPlus, 2),
            ('-', Some('-'), _) => (TokenKind::MinusMinus, 2),
            ('+', Some('='), _) => (TokenKind::PlusEq, 2),
            ('-', Some('='), _) => (TokenKind::MinusEq, 2),
            ('*', Some('='), _) => (TokenKind::StarEq, 2),
            ('/', Some('='), _) => (TokenKind::SlashEq, 2),
            ('(', _, _) => (TokenKind::LParen, 1),
            (')', _, _) => (TokenKind::RParen, 1),
            ('{', _, _) => (TokenKind::LBrace, 1),
            ('}', _, _) => (TokenKind::RBrace, 1),
            ('[', _, _) => (TokenKind::LBracket, 1),
            (']', _, _) => (TokenKind::RBracket, 1),
            ('.', _, _) => (TokenKind::Dot, 1),
            (',', _, _) => (TokenKind::Comma, 1),
            (';', _, _) => (TokenKind::Semi, 1),
            (':', _, _) => (TokenKind::Colon, 1),
            ('?', _, _) => (TokenKind::Question, 1),
            ('+', _, _) => (TokenKind::Plus, 1),
            ('-', _, _) => (TokenKind::Minus, 1),
            ('*', _, _) => (TokenKind::Star, 1),
            ('/', _, _) => (TokenKind::Slash, 1),
            ('%', _, _) => (TokenKind::Percent, 1),
            ('!', _, _) => (TokenKind::Bang, 1),
            ('=', _, _) => (TokenKind::Eq, 1),
            ('<', _, _) => (TokenKind::Lt, 1),
            ('>', _, _) => (TokenKind::Gt, 1),
            _ => {
                return Err(JsflDiagnostic::new(
                    file,
                    span.line,
                    span.column,
                    format!("Unexpected character '{c}'."),
                ));
            }
        };
        tokens.push(Token { kind, span });
        i += advance;
        col += advance as u32;
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span { line, column: col },
    });
    Ok(tokens)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UnaryOp {
    Not,
    Neg,
    Pos,
    Typeof,
}

#[derive(Clone, Debug)]
pub(super) struct Expr {
    pub(super) kind: ExprKind,
    pub(super) span: Span,
}

#[derive(Clone, Debug)]
pub(super) enum ExprKind {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Ident(String),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Member {
        object: Box<Expr>,
        property: String,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Update {
        target: Box<Expr>,
        increment: bool,
        prefix: bool,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Assign {
        op: AssignOp,
        target: Box<Expr>,
        value: Box<Expr>,
    },
    Conditional {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
}

#[derive(Clone, Debug)]
pub(super) struct Stmt {
    pub(super) kind: StmtKind,
    pub(super) span: Span,
}

#[derive(Clone, Debug)]
pub(super) enum StmtKind {
    Block(Vec<Stmt>),
    VarDecl(Vec<(String, Option<Expr>)>),
    FuncDecl {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
    },
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    ForOf {
        var_name: String,
        iterable: Expr,
        body: Box<Stmt>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Throw(Expr),
    TryCatch {
        try_block: Vec<Stmt>,
        catch_param: Option<String>,
        catch_block: Vec<Stmt>,
    },
    Expr(Expr),
    Empty,
}

pub(super) struct Parser<'a> {
    pub(super) file: &'a str,
    pub(super) tokens: Vec<Token>,
    pub(super) pos: usize,
}

impl<'a> Parser<'a> {
    pub(super) fn new(file: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            file,
            tokens,
            pos: 0,
        }
    }

    pub(super) fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    pub(super) fn span(&self) -> Span {
        self.peek().span
    }

    pub(super) fn is_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    pub(super) fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    pub(super) fn check_ident(&self, kw: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(s) if s == kw)
    }

    pub(super) fn match_ident(&mut self, kw: &str) -> bool {
        if self.check_ident(kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub(super) fn match_kind(&mut self, expected: &TokenKind) -> bool {
        if &self.peek().kind == expected {
            self.advance();
            true
        } else {
            false
        }
    }

    pub(super) fn expect_kind(
        &mut self,
        expected: &TokenKind,
        msg: &str,
    ) -> Result<Token, JsflDiagnostic> {
        let tok = self.peek().clone();
        if &tok.kind == expected {
            Ok(self.advance())
        } else {
            Err(JsflDiagnostic::new(
                self.file,
                tok.span.line,
                tok.span.column,
                msg,
            ))
        }
    }

    pub(super) fn expect_ident(&mut self, msg: &str) -> Result<String, JsflDiagnostic> {
        let tok = self.peek().clone();
        if let TokenKind::Ident(name) = tok.kind {
            self.advance();
            Ok(name)
        } else {
            Err(JsflDiagnostic::new(
                self.file,
                tok.span.line,
                tok.span.column,
                msg,
            ))
        }
    }

    pub(super) fn consume_optional_semi(&mut self) {
        let _ = self.match_kind(&TokenKind::Semi);
    }

    pub(super) fn parse_program(&mut self) -> Result<Vec<Stmt>, JsflDiagnostic> {
        let mut stmts = Vec::new();
        while !self.is_eof() {
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    pub(super) fn parse_block(&mut self) -> Result<Vec<Stmt>, JsflDiagnostic> {
        self.expect_kind(&TokenKind::LBrace, "Expected '{'.")?;
        let mut stmts = Vec::new();
        while !self.is_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
            stmts.push(self.parse_stmt()?);
        }
        self.expect_kind(&TokenKind::RBrace, "Expected '}'.")?;
        Ok(stmts)
    }

    pub(super) fn parse_stmt(&mut self) -> Result<Stmt, JsflDiagnostic> {
        let span = self.span();
        if self.match_kind(&TokenKind::Semi) {
            return Ok(Stmt {
                kind: StmtKind::Empty,
                span,
            });
        }
        if matches!(self.peek().kind, TokenKind::LBrace) {
            return Ok(Stmt {
                kind: StmtKind::Block(self.parse_block()?),
                span,
            });
        }
        if self.match_ident("var") || self.match_ident("let") || self.match_ident("const") {
            let decl = self.parse_var_decl_list()?;
            self.consume_optional_semi();
            return Ok(Stmt {
                kind: StmtKind::VarDecl(decl),
                span,
            });
        }
        if self.match_ident("function") {
            let name = self.expect_ident("Expected function name.")?;
            self.expect_kind(&TokenKind::LParen, "Expected '(' after function name.")?;
            let mut params = Vec::new();
            if !matches!(self.peek().kind, TokenKind::RParen) {
                loop {
                    params.push(self.expect_ident("Expected parameter name.")?);
                    if !self.match_kind(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect_kind(&TokenKind::RParen, "Expected ')' after parameters.")?;
            let body = self.parse_block()?;
            return Ok(Stmt {
                kind: StmtKind::FuncDecl { name, params, body },
                span,
            });
        }
        if self.match_ident("if") {
            self.expect_kind(&TokenKind::LParen, "Expected '(' after 'if'.")?;
            let cond = self.parse_expr()?;
            self.expect_kind(&TokenKind::RParen, "Expected ')' after if condition.")?;
            let then_branch = Box::new(self.parse_stmt()?);
            let else_branch = if self.match_ident("else") {
                Some(Box::new(self.parse_stmt()?))
            } else {
                None
            };
            return Ok(Stmt {
                kind: StmtKind::If {
                    cond,
                    then_branch,
                    else_branch,
                },
                span,
            });
        }
        if self.match_ident("while") {
            self.expect_kind(&TokenKind::LParen, "Expected '(' after 'while'.")?;
            let cond = self.parse_expr()?;
            self.expect_kind(&TokenKind::RParen, "Expected ')' after while condition.")?;
            let body = Box::new(self.parse_stmt()?);
            return Ok(Stmt {
                kind: StmtKind::While { cond, body },
                span,
            });
        }
        if self.match_ident("for") {
            return self.parse_for_stmt(span);
        }
        if self.match_ident("return") {
            let expr = if matches!(
                self.peek().kind,
                TokenKind::Semi | TokenKind::RBrace | TokenKind::Eof
            ) {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.consume_optional_semi();
            return Ok(Stmt {
                kind: StmtKind::Return(expr),
                span,
            });
        }
        if self.match_ident("break") {
            self.consume_optional_semi();
            return Ok(Stmt {
                kind: StmtKind::Break,
                span,
            });
        }
        if self.match_ident("continue") {
            self.consume_optional_semi();
            return Ok(Stmt {
                kind: StmtKind::Continue,
                span,
            });
        }
        if self.match_ident("throw") {
            let expr = self.parse_expr()?;
            self.consume_optional_semi();
            return Ok(Stmt {
                kind: StmtKind::Throw(expr),
                span,
            });
        }
        if self.match_ident("try") {
            let try_block = self.parse_block()?;
            self.expect_ident("Expected 'catch' after try block.")?;
            let catch_param = if self.match_kind(&TokenKind::LParen) {
                let p = self.expect_ident("Expected catch parameter name.")?;
                self.expect_kind(&TokenKind::RParen, "Expected ')' after catch parameter.")?;
                Some(p)
            } else {
                None
            };
            let catch_block = self.parse_block()?;
            return Ok(Stmt {
                kind: StmtKind::TryCatch {
                    try_block,
                    catch_param,
                    catch_block,
                },
                span,
            });
        }

        let expr = self.parse_expr()?;
        self.consume_optional_semi();
        Ok(Stmt {
            kind: StmtKind::Expr(expr),
            span,
        })
    }

    pub(super) fn parse_var_decl_list(
        &mut self,
    ) -> Result<Vec<(String, Option<Expr>)>, JsflDiagnostic> {
        let mut list = Vec::new();
        loop {
            let name = self.expect_ident("Expected variable name.")?;
            let init = if self.match_kind(&TokenKind::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            list.push((name, init));
            if !self.match_kind(&TokenKind::Comma) {
                break;
            }
        }
        Ok(list)
    }

    pub(super) fn parse_for_stmt(&mut self, span: Span) -> Result<Stmt, JsflDiagnostic> {
        self.expect_kind(&TokenKind::LParen, "Expected '(' after 'for'.")?;
        let has_var =
            self.match_ident("var") || self.match_ident("let") || self.match_ident("const");
        if has_var {
            let first_name = self.expect_ident("Expected loop variable name.")?;
            if self.match_ident("of") || self.match_ident("in") {
                let iterable = self.parse_expr()?;
                self.expect_kind(&TokenKind::RParen, "Expected ')' after for-of header.")?;
                let body = Box::new(self.parse_stmt()?);
                return Ok(Stmt {
                    kind: StmtKind::ForOf {
                        var_name: first_name,
                        iterable,
                        body,
                    },
                    span,
                });
            }
            let first_init = if self.match_kind(&TokenKind::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            let mut decls = vec![(first_name, first_init)];
            while self.match_kind(&TokenKind::Comma) {
                let n = self.expect_ident("Expected variable name.")?;
                let v = if self.match_kind(&TokenKind::Eq) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                decls.push((n, v));
            }
            self.expect_kind(&TokenKind::Semi, "Expected ';' after for initializer.")?;
            let cond = if matches!(self.peek().kind, TokenKind::Semi) {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.expect_kind(&TokenKind::Semi, "Expected ';' after for condition.")?;
            let update = if matches!(self.peek().kind, TokenKind::RParen) {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.expect_kind(&TokenKind::RParen, "Expected ')' after for clauses.")?;
            let body = Box::new(self.parse_stmt()?);
            return Ok(Stmt {
                kind: StmtKind::For {
                    init: Some(Box::new(Stmt {
                        kind: StmtKind::VarDecl(decls),
                        span,
                    })),
                    cond,
                    update,
                    body,
                },
                span,
            });
        }

        let init = if self.match_kind(&TokenKind::Semi) {
            None
        } else {
            let e = self.parse_expr()?;
            self.expect_kind(&TokenKind::Semi, "Expected ';' after for initializer.")?;
            Some(Box::new(Stmt {
                kind: StmtKind::Expr(e),
                span,
            }))
        };
        let cond = if matches!(self.peek().kind, TokenKind::Semi) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect_kind(&TokenKind::Semi, "Expected ';' after for condition.")?;
        let update = if matches!(self.peek().kind, TokenKind::RParen) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect_kind(&TokenKind::RParen, "Expected ')' after for clauses.")?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt {
            kind: StmtKind::For {
                init,
                cond,
                update,
                body,
            },
            span,
        })
    }

    pub(super) fn parse_expr(&mut self) -> Result<Expr, JsflDiagnostic> {
        self.parse_assignment()
    }

    pub(super) fn parse_assignment(&mut self) -> Result<Expr, JsflDiagnostic> {
        let left = self.parse_conditional()?;
        let op = match self.peek().kind {
            TokenKind::Eq => Some(AssignOp::Assign),
            TokenKind::PlusEq => Some(AssignOp::AddAssign),
            TokenKind::MinusEq => Some(AssignOp::SubAssign),
            TokenKind::StarEq => Some(AssignOp::MulAssign),
            TokenKind::SlashEq => Some(AssignOp::DivAssign),
            _ => None,
        };
        if let Some(op) = op {
            let span = self.advance().span;
            let right = self.parse_assignment()?;
            return Ok(Expr {
                kind: ExprKind::Assign {
                    op,
                    target: Box::new(left),
                    value: Box::new(right),
                },
                span,
            });
        }
        Ok(left)
    }

    pub(super) fn parse_conditional(&mut self) -> Result<Expr, JsflDiagnostic> {
        let expr = self.parse_logical_or()?;
        if self.match_kind(&TokenKind::Question) {
            let span = expr.span;
            let then_expr = self.parse_assignment()?;
            self.expect_kind(&TokenKind::Colon, "Expected ':' in conditional expression.")?;
            let else_expr = self.parse_assignment()?;
            return Ok(Expr {
                kind: ExprKind::Conditional {
                    cond: Box::new(expr),
                    then_expr: Box::new(then_expr),
                    else_expr: Box::new(else_expr),
                },
                span,
            });
        }
        Ok(expr)
    }

    pub(super) fn parse_logical_or(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_logical_and()?;
        while self.match_kind(&TokenKind::OrOr) {
            let span = left.span;
            let right = self.parse_logical_and()?;
            left = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(left)
    }

    pub(super) fn parse_logical_and(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_equality()?;
        while self.match_kind(&TokenKind::AndAnd) {
            let span = left.span;
            let right = self.parse_equality()?;
            left = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(left)
    }

    pub(super) fn parse_equality(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_relational()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::EqEq => Some(BinaryOp::Eq),
                TokenKind::BangEq => Some(BinaryOp::Ne),
                TokenKind::EqEqEq => Some(BinaryOp::StrictEq),
                TokenKind::BangEqEq => Some(BinaryOp::StrictNe),
                _ => None,
            };
            if let Some(op) = op {
                let span = self.advance().span;
                let right = self.parse_relational()?;
                left = Expr {
                    kind: ExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    pub(super) fn parse_relational(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Lt => Some(BinaryOp::Lt),
                TokenKind::LtEq => Some(BinaryOp::Le),
                TokenKind::Gt => Some(BinaryOp::Gt),
                TokenKind::GtEq => Some(BinaryOp::Ge),
                _ => None,
            };
            if let Some(op) = op {
                let span = self.advance().span;
                let right = self.parse_additive()?;
                left = Expr {
                    kind: ExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    pub(super) fn parse_additive(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => Some(BinaryOp::Add),
                TokenKind::Minus => Some(BinaryOp::Sub),
                _ => None,
            };
            if let Some(op) = op {
                let span = self.advance().span;
                let right = self.parse_multiplicative()?;
                left = Expr {
                    kind: ExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    pub(super) fn parse_multiplicative(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Star => Some(BinaryOp::Mul),
                TokenKind::Slash => Some(BinaryOp::Div),
                TokenKind::Percent => Some(BinaryOp::Rem),
                _ => None,
            };
            if let Some(op) = op {
                let span = self.advance().span;
                let right = self.parse_unary()?;
                left = Expr {
                    kind: ExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    pub(super) fn parse_unary(&mut self) -> Result<Expr, JsflDiagnostic> {
        let span = self.span();
        if self.match_kind(&TokenKind::Bang) {
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(self.parse_unary()?),
                },
                span,
            });
        }
        if self.match_kind(&TokenKind::Minus) {
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(self.parse_unary()?),
                },
                span,
            });
        }
        if self.match_kind(&TokenKind::Plus) {
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(self.parse_unary()?),
                },
                span,
            });
        }
        if self.match_ident("typeof") {
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Typeof,
                    expr: Box::new(self.parse_unary()?),
                },
                span,
            });
        }
        if self.match_ident("new") {
            return self.parse_postfix();
        }
        if self.match_kind(&TokenKind::PlusPlus) {
            return Ok(Expr {
                kind: ExprKind::Update {
                    target: Box::new(self.parse_postfix()?),
                    increment: true,
                    prefix: true,
                },
                span,
            });
        }
        if self.match_kind(&TokenKind::MinusMinus) {
            return Ok(Expr {
                kind: ExprKind::Update {
                    target: Box::new(self.parse_postfix()?),
                    increment: false,
                    prefix: true,
                },
                span,
            });
        }
        self.parse_postfix()
    }

    pub(super) fn parse_postfix(&mut self) -> Result<Expr, JsflDiagnostic> {
        let mut expr = self.parse_primary()?;
        loop {
            let span = self.span();
            if self.match_kind(&TokenKind::Dot) {
                let property = self.expect_ident("Expected property name after '.'.")?;
                expr = Expr {
                    kind: ExprKind::Member {
                        object: Box::new(expr),
                        property,
                    },
                    span,
                };
            } else if self.match_kind(&TokenKind::LBracket) {
                let index = self.parse_expr()?;
                self.expect_kind(&TokenKind::RBracket, "Expected ']' after index expression.")?;
                expr = Expr {
                    kind: ExprKind::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                };
            } else if self.match_kind(&TokenKind::LParen) {
                let mut args = Vec::new();
                if !matches!(self.peek().kind, TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect_kind(&TokenKind::RParen, "Expected ')' after call arguments.")?;
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                };
            } else if self.match_kind(&TokenKind::PlusPlus) {
                expr = Expr {
                    kind: ExprKind::Update {
                        target: Box::new(expr),
                        increment: true,
                        prefix: false,
                    },
                    span,
                };
            } else if self.match_kind(&TokenKind::MinusMinus) {
                expr = Expr {
                    kind: ExprKind::Update {
                        target: Box::new(expr),
                        increment: false,
                        prefix: false,
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    pub(super) fn parse_primary(&mut self) -> Result<Expr, JsflDiagnostic> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Number(n) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Number(n),
                    span: tok.span,
                })
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Str(s),
                    span: tok.span,
                })
            }
            TokenKind::Ident(ident) => {
                self.advance();
                let kind = match ident.as_str() {
                    "true" => ExprKind::Bool(true),
                    "false" => ExprKind::Bool(false),
                    "null" => ExprKind::Null,
                    "undefined" => ExprKind::Undefined,
                    _ => ExprKind::Ident(ident),
                };
                Ok(Expr {
                    kind,
                    span: tok.span,
                })
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect_kind(&TokenKind::RParen, "Expected ')'.")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                while !self.is_eof() && !matches!(self.peek().kind, TokenKind::RBracket) {
                    items.push(self.parse_expr()?);
                    if !self.match_kind(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect_kind(&TokenKind::RBracket, "Expected ']'.")?;
                Ok(Expr {
                    kind: ExprKind::Array(items),
                    span: tok.span,
                })
            }
            TokenKind::LBrace => {
                self.advance();
                let mut entries = Vec::new();
                while !self.is_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
                    let key = match self.peek().kind.clone() {
                        TokenKind::Ident(k) | TokenKind::Str(k) => {
                            self.advance();
                            k
                        }
                        _ => {
                            return Err(JsflDiagnostic::new(
                                self.file,
                                self.span().line,
                                self.span().column,
                                "Expected object property key.",
                            ));
                        }
                    };
                    self.expect_kind(&TokenKind::Colon, "Expected ':' after object property key.")?;
                    let val = self.parse_expr()?;
                    entries.push((key, val));
                    if !self.match_kind(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect_kind(&TokenKind::RBrace, "Expected '}'.")?;
                Ok(Expr {
                    kind: ExprKind::Object(entries),
                    span: tok.span,
                })
            }
            _ => Err(JsflDiagnostic::new(
                self.file,
                tok.span.line,
                tok.span.column,
                format!("Unexpected token {:?} in expression.", tok.kind),
            )),
        }
    }
}
