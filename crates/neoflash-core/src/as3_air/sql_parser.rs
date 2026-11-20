use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k", content = "v")]
pub enum SqlValue {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
    Blob(String), // base64
}

impl SqlValue {
    pub(super) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub(super) fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Int(n) => *n != 0,
            Self::Real(f) => *f != 0.0 && !f.is_nan(),
            Self::Text(s) => {
                let t = s.trim();
                !t.is_empty() && t != "0" && !t.eq_ignore_ascii_case("false")
            }
            Self::Blob(b) => !b.is_empty(),
        }
    }

    pub(super) fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Real(f) => Some(*f),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Self::Text(s) => s.trim().parse::<f64>().ok(),
            Self::Null | Self::Blob(_) => None,
        }
    }

    pub(super) fn as_text(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Int(n) => n.to_string(),
            Self::Real(f) => {
                if f.fract() == 0.0 && f.is_finite() {
                    format!("{f:.1}")
                } else {
                    f.to_string()
                }
            }
            Self::Text(s) => s.clone(),
            Self::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
            Self::Blob(b) => b.clone(),
        }
    }

    pub(super) fn cmp_sql(&self, other: &Self, nocase: bool) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Null, _) => Ordering::Less,
            (_, Self::Null) => Ordering::Greater,
            (Self::Int(a), Self::Int(b)) => a.cmp(b),
            (Self::Int(a), Self::Real(b)) => (*a as f64).partial_cmp(b).unwrap_or(Ordering::Equal),
            (Self::Real(a), Self::Int(b)) => a.partial_cmp(&(*b as f64)).unwrap_or(Ordering::Equal),
            (Self::Real(a), Self::Real(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Bool(a), Self::Int(b)) => i64::from(*a).cmp(b),
            (Self::Int(a), Self::Bool(b)) => a.cmp(&i64::from(*b)),
            (Self::Text(a), Self::Text(b)) => {
                if nocase {
                    a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
                } else {
                    a.cmp(b)
                }
            }
            (a, b) => {
                if let (Some(na), Some(nb)) = (a.as_f64(), b.as_f64())
                    && (matches!(a, Self::Int(_) | Self::Real(_) | Self::Bool(_))
                        || matches!(b, Self::Int(_) | Self::Real(_) | Self::Bool(_)))
                {
                    return na.partial_cmp(&nb).unwrap_or(Ordering::Equal);
                }
                let sa = a.as_text();
                let sb = b.as_text();
                if nocase {
                    sa.to_ascii_lowercase().cmp(&sb.to_ascii_lowercase())
                } else {
                    sa.cmp(&sb)
                }
            }
        }
    }

    pub(super) fn coerce_affinity(&self, data_type: &str) -> Self {
        if self.is_null() {
            return Self::Null;
        }
        let upper = data_type.to_ascii_uppercase();
        if upper.contains("INT") {
            match self {
                Self::Int(n) => Self::Int(*n),
                Self::Real(f) if f.is_finite() && f.fract() == 0.0 => Self::Int(*f as i64),
                Self::Bool(b) => Self::Int(i64::from(*b)),
                Self::Text(s) => s
                    .trim()
                    .parse::<i64>()
                    .map_or_else(|_| self.clone(), Self::Int),
                _ => self.clone(),
            }
        } else if upper.contains("BOOL") {
            match self {
                Self::Bool(b) => Self::Bool(*b),
                Self::Int(n) => Self::Bool(*n != 0),
                _ => self.clone(),
            }
        } else if upper.contains("CHAR") || upper.contains("CLOB") || upper.contains("TEXT") {
            match self {
                Self::Text(_) | Self::Blob(_) => self.clone(),
                other => Self::Text(other.as_text()),
            }
        } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
            self.as_f64().map_or_else(|| self.clone(), Self::Real)
        } else {
            self.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlColumnDef {
    pub name: String,
    pub data_type: String,
    pub allow_null: bool,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub unique: bool,
    pub default_collation_type: String,
    pub default_value: Option<SqlValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SqlRow {
    pub(super) rowid: i64,
    pub(super) values: Vec<SqlValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SqlTable {
    pub(super) name: String,
    pub(super) sql: String,
    pub(super) columns: Vec<SqlColumnDef>,
    pub(super) rows: Vec<SqlRow>,
    pub(super) next_rowid: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SqlIndexMeta {
    pub(super) name: String,
    pub(super) table: String,
    pub(super) sql: String,
    pub(super) unique: bool,
    pub(super) columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SqlViewMeta {
    pub(super) name: String,
    pub(super) sql: String,
    pub(super) select_sql: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SqlTriggerMeta {
    pub(super) name: String,
    pub(super) table: String,
    pub(super) sql: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct SqlDatabase {
    pub(super) tables: BTreeMap<String, SqlTable>,
    pub(super) indices: BTreeMap<String, SqlIndexMeta>,
    pub(super) views: BTreeMap<String, SqlViewMeta>,
    pub(super) triggers: BTreeMap<String, SqlTriggerMeta>,
}

#[derive(Debug, Clone)]
pub(super) struct AttachedDb {
    pub(super) vfs_path: Option<String>,
    pub(super) read_only: bool,
    pub(super) encryption_key: Option<[u8; 16]>,
    pub(super) db: SqlDatabase,
}

#[derive(Debug, Clone)]
pub(super) struct SavepointSnap {
    pub(super) name: String,
    pub(super) dbs: BTreeMap<String, SqlDatabase>,
}

#[derive(Debug, Clone)]
pub(super) struct SqlConnState {
    pub(super) connected: bool,
    pub(super) auto_compact: bool,
    pub(super) page_size: u32,
    pub(super) cache_size: u32,
    pub(super) column_name_style: String,
    pub(super) last_insert_rowid: i64,
    pub(super) total_changes: u64,
    pub(super) in_transaction: bool,
    pub(super) tx_base: Option<BTreeMap<String, SqlDatabase>>,
    pub(super) savepoints: Vec<SavepointSnap>,
    pub(super) sp_counter: u64,
    pub(super) attached: BTreeMap<String, AttachedDb>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlUpdateRecord {
    pub action: String,
    pub table: String,
    pub row_id: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Tok {
    Word(String),
    Ident(String), // quoted identifier
    Str(String),
    Int(i64),
    Real(f64),
    Param(String), // ":id", "@id", "$id" or "?"
    LParen,
    RParen,
    Comma,
    Semi,
    Dot,
    Star,
    Plus,
    Minus,
    Slash,
    Percent,
    Concat, // ||
    Eq,     // = or ==
    Ne,     // != or <>
    Lt,
    Le,
    Gt,
    Ge,
}

pub(super) fn tokenize_sql(sql: &str) -> Result<Vec<Tok>, AirSqlError> {
    if sql.len() > MAX_AIR_SQL_BYTES {
        return Err(AirSqlError::new(
            "execute",
            "SQL text exceeds 256 KiB limit.",
            3115,
        ));
    }
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    let mut pos_param = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return Err(AirSqlError::new(
                    "execute",
                    "Unterminated SQL block comment.",
                    3115,
                ));
            }
            i += 2;
            continue;
        }
        match b {
            b'(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            b',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            b';' => {
                out.push(Tok::Semi);
                i += 1;
            }
            b'.' if !(i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()) => {
                out.push(Tok::Dot);
                i += 1;
            }
            b'*' => {
                out.push(Tok::Star);
                i += 1;
            }
            b'+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            b'-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            b'/' => {
                out.push(Tok::Slash);
                i += 1;
            }
            b'%' => {
                out.push(Tok::Percent);
                i += 1;
            }
            b'|' if i + 1 < bytes.len() && bytes[i + 1] == b'|' => {
                out.push(Tok::Concat);
                i += 2;
            }
            b'=' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    i += 2;
                } else {
                    i += 1;
                }
                out.push(Tok::Eq);
            }
            b'!' if i + 1 < bytes.len() && bytes[i + 1] == b'=' => {
                out.push(Tok::Ne);
                i += 2;
            }
            b'<' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    out.push(Tok::Le);
                    i += 2;
                } else if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                    out.push(Tok::Ne);
                    i += 2;
                } else {
                    out.push(Tok::Lt);
                    i += 1;
                }
            }
            b'>' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    out.push(Tok::Ge);
                    i += 2;
                } else {
                    out.push(Tok::Gt);
                    i += 1;
                }
            }
            b'?' => {
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i > start {
                    let n: usize = sql[start..i].parse().unwrap_or(1);
                    out.push(Tok::Param(n.saturating_sub(1).to_string()));
                } else {
                    out.push(Tok::Param(pos_param.to_string()));
                    pos_param += 1;
                }
            }
            b':' | b'@' | b'$' => {
                let prefix = b as char;
                i += 1;
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if i == start {
                    return Err(AirSqlError::new(
                        "execute",
                        "Invalid SQL parameter name.",
                        3115,
                    ));
                }
                out.push(Tok::Param(format!("{prefix}{}", &sql[start..i])));
            }
            b'\'' => {
                i += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < sql.len() {
                    let ch = sql[i..].chars().next().unwrap();
                    i += ch.len_utf8();
                    if ch == '\'' {
                        if i < bytes.len() && bytes[i] == b'\'' {
                            s.push('\'');
                            i += 1;
                        } else {
                            closed = true;
                            break;
                        }
                    } else {
                        s.push(ch);
                    }
                }
                if !closed {
                    return Err(AirSqlError::new(
                        "execute",
                        "Unterminated SQL string literal.",
                        3115,
                    ));
                }
                out.push(Tok::Str(s));
            }
            b'"' | b'`' | b'[' => {
                let closer = if b == b'[' { ']' } else { b as char };
                i += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < sql.len() {
                    let ch = sql[i..].chars().next().unwrap();
                    i += ch.len_utf8();
                    if ch == closer {
                        if closer != ']' && i < bytes.len() && bytes[i] == closer as u8 {
                            s.push(closer);
                            i += 1;
                        } else {
                            closed = true;
                            break;
                        }
                    } else {
                        s.push(ch);
                    }
                }
                if !closed {
                    return Err(AirSqlError::new(
                        "execute",
                        "Unterminated quoted SQL identifier.",
                        3115,
                    ));
                }
                out.push(Tok::Ident(s));
            }
            _ if b.is_ascii_digit()
                || (b == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()) =>
            {
                let start = i;
                let mut is_float = false;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'.' {
                    is_float = true;
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                    is_float = true;
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let raw = &sql[start..i];
                if is_float {
                    let v = raw.parse::<f64>().map_err(|_| {
                        AirSqlError::new("execute", format!("Invalid number '{raw}'."), 3115)
                    })?;
                    out.push(Tok::Real(v));
                } else {
                    let v = raw.parse::<i64>().map_err(|_| {
                        AirSqlError::new("execute", format!("Invalid integer '{raw}'."), 3115)
                    })?;
                    out.push(Tok::Int(v));
                }
            }
            _ if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(Tok::Word(sql[start..i].to_owned()));
            }
            _ => {
                return Err(AirSqlError::new(
                    "execute",
                    format!("Unexpected character '{}' in SQL statement.", b as char),
                    3115,
                ));
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub(super) enum SqlExpr {
    Lit(SqlValue),
    Param(String),
    Col {
        table: Option<String>,
        name: String,
    },
    UnaryMinus(Box<SqlExpr>),
    Not(Box<SqlExpr>),
    Binary {
        op: BinOp,
        left: Box<SqlExpr>,
        right: Box<SqlExpr>,
    },
    IsNull {
        expr: Box<SqlExpr>,
        negated: bool,
    },
    InList {
        expr: Box<SqlExpr>,
        list: Vec<SqlExpr>,
        negated: bool,
    },
    Between {
        expr: Box<SqlExpr>,
        low: Box<SqlExpr>,
        high: Box<SqlExpr>,
        negated: bool,
    },
    Like {
        expr: Box<SqlExpr>,
        pattern: Box<SqlExpr>,
        negated: bool,
    },
    Call {
        name: String,
        distinct: bool,
        star: bool,
        args: Vec<SqlExpr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Concat,
}

#[derive(Debug, Clone)]
pub(super) enum SelectItem {
    All(Option<String>),
    Expr {
        expr: SqlExpr,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub(super) struct JoinClause {
    pub(super) left_outer: bool,
    pub(super) db: Option<String>,
    pub(super) table: String,
    pub(super) alias: Option<String>,
    pub(super) on: Option<SqlExpr>,
}

#[derive(Debug, Clone)]
pub(super) struct OrderItem {
    pub(super) expr: SqlExpr,
    pub(super) desc: bool,
}

#[derive(Debug, Clone)]
pub(super) struct SelectPlan {
    pub(super) distinct: bool,
    pub(super) items: Vec<SelectItem>,
    pub(super) from_db: Option<String>,
    pub(super) from_table: Option<String>,
    pub(super) from_alias: Option<String>,
    pub(super) joins: Vec<JoinClause>,
    pub(super) where_expr: Option<SqlExpr>,
    pub(super) group_by: Vec<SqlExpr>,
    pub(super) having: Option<SqlExpr>,
    pub(super) order_by: Vec<OrderItem>,
    pub(super) limit: Option<usize>,
    pub(super) offset: usize,
}

pub(super) struct SqlParser<'a> {
    pub(super) toks: &'a [Tok],
    pub(super) pos: usize,
}

impl<'a> SqlParser<'a> {
    pub(super) fn new(toks: &'a [Tok]) -> Self {
        Self { toks, pos: 0 }
    }

    pub(super) fn peek(&self) -> Option<&'a Tok> {
        self.toks.get(self.pos)
    }

    pub(super) fn at_end(&self) -> bool {
        matches!(self.peek(), None | Some(Tok::Semi))
    }

    pub(super) fn kw(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(w)) if w.eq_ignore_ascii_case(word))
    }

    pub(super) fn eat_kw(&mut self, word: &str) -> bool {
        if self.kw(word) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub(super) fn expect_kw(&mut self, word: &str) -> Result<(), AirSqlError> {
        if self.eat_kw(word) {
            Ok(())
        } else {
            Err(AirSqlError::new(
                "execute",
                format!("Expected SQL keyword '{word}'."),
                3115,
            ))
        }
    }

    pub(super) fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == Some(tok) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub(super) fn expect(&mut self, tok: &Tok) -> Result<(), AirSqlError> {
        if self.eat(tok) {
            Ok(())
        } else {
            Err(AirSqlError::new(
                "execute",
                format!("Unexpected SQL syntax near token #{}.", self.pos + 1),
                3115,
            ))
        }
    }

    pub(super) fn ident(&mut self) -> Result<String, AirSqlError> {
        match self.peek() {
            Some(Tok::Word(w)) | Some(Tok::Ident(w)) => {
                let s = w.clone();
                self.pos += 1;
                Ok(s)
            }
            Some(Tok::Str(s)) => {
                let v = s.clone();
                self.pos += 1;
                Ok(v)
            }
            _ => Err(AirSqlError::new(
                "execute",
                "Expected SQL identifier.",
                3115,
            )),
        }
    }

    pub(super) fn qualified_name(&mut self) -> Result<(Option<String>, String), AirSqlError> {
        let first = self.ident()?;
        if self.eat(&Tok::Dot) {
            let second = self.ident()?;
            Ok((Some(first), second))
        } else {
            Ok((None, first))
        }
    }

    pub(super) fn expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        self.or_expr()
    }

    pub(super) fn or_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        let mut left = self.and_expr()?;
        while self.eat_kw("OR") {
            let right = self.and_expr()?;
            left = SqlExpr::Binary {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn and_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        let mut left = self.not_expr()?;
        while self.eat_kw("AND") {
            let right = self.not_expr()?;
            left = SqlExpr::Binary {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    pub(super) fn not_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        if self.eat_kw("NOT") {
            Ok(SqlExpr::Not(Box::new(self.not_expr()?)))
        } else {
            self.cmp_expr()
        }
    }

    pub(super) fn cmp_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        let mut left = self.add_expr()?;
        loop {
            if self.eat_kw("IS") {
                let negated = self.eat_kw("NOT");
                if self.eat_kw("NULL") {
                    left = SqlExpr::IsNull {
                        expr: Box::new(left),
                        negated,
                    };
                    continue;
                }
                let right = self.add_expr()?;
                left = SqlExpr::Binary {
                    op: if negated { BinOp::Ne } else { BinOp::Eq },
                    left: Box::new(left),
                    right: Box::new(right),
                };
                continue;
            }
            let negated_op = if self.kw("NOT") {
                let saved = self.pos;
                self.pos += 1;
                if self.kw("IN") || self.kw("LIKE") || self.kw("BETWEEN") {
                    true
                } else {
                    self.pos = saved;
                    false
                }
            } else {
                false
            };
            if self.eat_kw("IN") {
                self.expect(&Tok::LParen)?;
                let mut list = Vec::new();
                if !self.eat(&Tok::RParen) {
                    loop {
                        list.push(self.expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(&Tok::RParen)?;
                }
                left = SqlExpr::InList {
                    expr: Box::new(left),
                    list,
                    negated: negated_op,
                };
                continue;
            }
            if self.eat_kw("BETWEEN") {
                let low = self.add_expr()?;
                self.expect_kw("AND")?;
                let high = self.add_expr()?;
                left = SqlExpr::Between {
                    expr: Box::new(left),
                    low: Box::new(low),
                    high: Box::new(high),
                    negated: negated_op,
                };
                continue;
            }
            if self.eat_kw("LIKE") {
                let pattern = self.add_expr()?;
                left = SqlExpr::Like {
                    expr: Box::new(left),
                    pattern: Box::new(pattern),
                    negated: negated_op,
                };
                continue;
            }
            let op = match self.peek() {
                Some(Tok::Eq) => Some(BinOp::Eq),
                Some(Tok::Ne) => Some(BinOp::Ne),
                Some(Tok::Lt) => Some(BinOp::Lt),
                Some(Tok::Le) => Some(BinOp::Le),
                Some(Tok::Gt) => Some(BinOp::Gt),
                Some(Tok::Ge) => Some(BinOp::Ge),
                _ => None,
            };
            if let Some(op) = op {
                self.pos += 1;
                let right = self.add_expr()?;
                left = SqlExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    pub(super) fn add_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        let mut left = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => Some(BinOp::Add),
                Some(Tok::Minus) => Some(BinOp::Sub),
                Some(Tok::Concat) => Some(BinOp::Concat),
                _ => None,
            };
            if let Some(op) = op {
                self.pos += 1;
                let right = self.mul_expr()?;
                left = SqlExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    pub(super) fn mul_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        let mut left = self.unary_expr()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => Some(BinOp::Mul),
                Some(Tok::Slash) => Some(BinOp::Div),
                Some(Tok::Percent) => Some(BinOp::Rem),
                _ => None,
            };
            if let Some(op) = op {
                self.pos += 1;
                let right = self.unary_expr()?;
                left = SqlExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    pub(super) fn unary_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        if self.eat(&Tok::Plus) {
            return self.unary_expr();
        }
        if self.eat(&Tok::Minus) {
            return Ok(SqlExpr::UnaryMinus(Box::new(self.unary_expr()?)));
        }
        self.primary_expr()
    }

    pub(super) fn primary_expr(&mut self) -> Result<SqlExpr, AirSqlError> {
        if self.eat(&Tok::LParen) {
            let inner = self.expr()?;
            self.expect(&Tok::RParen)?;
            return Ok(inner);
        }
        match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Int(n)))
            }
            Some(Tok::Real(f)) => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Real(f)))
            }
            Some(Tok::Str(s)) => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Text(s)))
            }
            Some(Tok::Param(p)) => {
                self.pos += 1;
                Ok(SqlExpr::Param(p))
            }
            Some(Tok::Word(w)) if w.eq_ignore_ascii_case("NULL") => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Null))
            }
            Some(Tok::Word(w)) if w.eq_ignore_ascii_case("TRUE") => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Bool(true)))
            }
            Some(Tok::Word(w)) if w.eq_ignore_ascii_case("FALSE") => {
                self.pos += 1;
                Ok(SqlExpr::Lit(SqlValue::Bool(false)))
            }
            Some(Tok::Word(w)) | Some(Tok::Ident(w)) => {
                let is_word = matches!(self.peek(), Some(Tok::Word(_)));
                self.pos += 1;
                if is_word && self.eat(&Tok::LParen) {
                    if self.eat(&Tok::Star) {
                        self.expect(&Tok::RParen)?;
                        return Ok(SqlExpr::Call {
                            name: w.to_ascii_uppercase(),
                            distinct: false,
                            star: true,
                            args: Vec::new(),
                        });
                    }
                    let distinct = self.eat_kw("DISTINCT");
                    let mut args = Vec::new();
                    if !self.eat(&Tok::RParen) {
                        loop {
                            args.push(self.expr()?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                        self.expect(&Tok::RParen)?;
                    }
                    return Ok(SqlExpr::Call {
                        name: w.to_ascii_uppercase(),
                        distinct,
                        star: false,
                        args,
                    });
                }
                if self.eat(&Tok::Dot) {
                    let col = self.ident()?;
                    Ok(SqlExpr::Col {
                        table: Some(w),
                        name: col,
                    })
                } else {
                    Ok(SqlExpr::Col {
                        table: None,
                        name: w,
                    })
                }
            }
            _ => Err(AirSqlError::new(
                "execute",
                "Expected SQL expression.",
                3115,
            )),
        }
    }

    pub(super) fn optional_table_alias(&mut self) -> Option<String> {
        let has_as = self.eat_kw("AS");
        match self.peek() {
            Some(Tok::Word(w)) => {
                if !has_as
                    && matches!(
                        w.to_ascii_uppercase().as_str(),
                        "WHERE"
                            | "GROUP"
                            | "HAVING"
                            | "ORDER"
                            | "LIMIT"
                            | "OFFSET"
                            | "JOIN"
                            | "INNER"
                            | "LEFT"
                            | "CROSS"
                            | "ON"
                    )
                {
                    return None;
                }
                let s = w.clone();
                self.pos += 1;
                Some(s)
            }
            Some(Tok::Ident(w)) => {
                let s = w.clone();
                self.pos += 1;
                Some(s)
            }
            _ => None,
        }
    }

    pub(super) fn parse_select(&mut self) -> Result<SelectPlan, AirSqlError> {
        self.expect_kw("SELECT")?;
        let distinct = if self.eat_kw("DISTINCT") {
            true
        } else {
            self.eat_kw("ALL");
            false
        };
        let mut items = Vec::new();
        loop {
            if self.eat(&Tok::Star) {
                items.push(SelectItem::All(None));
            } else if matches!(
                (
                    self.toks.get(self.pos),
                    self.toks.get(self.pos + 1),
                    self.toks.get(self.pos + 2)
                ),
                (
                    Some(Tok::Word(_) | Tok::Ident(_)),
                    Some(Tok::Dot),
                    Some(Tok::Star)
                )
            ) {
                let tbl = self.ident()?;
                self.pos += 2;
                items.push(SelectItem::All(Some(tbl)));
            } else {
                let expr = self.expr()?;
                let alias = if self.eat_kw("AS") {
                    Some(self.ident()?)
                } else if let Some(Tok::Word(w) | Tok::Ident(w)) = self.peek() {
                    if !matches!(
                        w.to_ascii_uppercase().as_str(),
                        "FROM" | "WHERE" | "GROUP" | "HAVING" | "ORDER" | "LIMIT"
                    ) {
                        let a = w.clone();
                        self.pos += 1;
                        Some(a)
                    } else {
                        None
                    }
                } else {
                    None
                };
                items.push(SelectItem::Expr { expr, alias });
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        let (mut from_db, mut from_table, mut from_alias) = (None, None, None);
        let mut joins = Vec::new();
        if self.eat_kw("FROM") {
            let (db, tbl) = self.qualified_name()?;
            from_db = db;
            from_table = Some(tbl);
            from_alias = self.optional_table_alias();
            loop {
                let mut left_outer = false;
                if self.eat_kw("LEFT") {
                    self.eat_kw("OUTER");
                    self.expect_kw("JOIN")?;
                    left_outer = true;
                } else if self.eat_kw("INNER") || self.eat_kw("CROSS") {
                    self.expect_kw("JOIN")?;
                } else if self.eat_kw("JOIN") || self.eat(&Tok::Comma) {
                    // plain JOIN / comma join
                } else {
                    break;
                }
                let (jdb, jtbl) = self.qualified_name()?;
                let jalias = self.optional_table_alias();
                let on = if self.eat_kw("ON") {
                    Some(self.expr()?)
                } else {
                    None
                };
                joins.push(JoinClause {
                    left_outer,
                    db: jdb,
                    table: jtbl,
                    alias: jalias,
                    on,
                });
            }
        }
        let where_expr = if self.eat_kw("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        let mut group_by = Vec::new();
        if self.eat_kw("GROUP") {
            self.expect_kw("BY")?;
            loop {
                group_by.push(self.expr()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        let having = if self.eat_kw("HAVING") {
            Some(self.expr()?)
        } else {
            None
        };
        let mut order_by = Vec::new();
        if self.eat_kw("ORDER") {
            self.expect_kw("BY")?;
            loop {
                let expr = self.expr()?;
                self.eat_kw("COLLATE");
                if matches!(self.peek(), Some(Tok::Word(w)) if w.eq_ignore_ascii_case("NOCASE") || w.eq_ignore_ascii_case("BINARY"))
                {
                    self.pos += 1;
                }
                let desc = if self.eat_kw("DESC") {
                    true
                } else {
                    self.eat_kw("ASC");
                    false
                };
                order_by.push(OrderItem { expr, desc });
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        let mut limit = None;
        let mut offset = 0usize;
        if self.eat_kw("LIMIT") {
            let lim_val = self.parse_nonneg_int()?;
            if self.eat_kw("OFFSET") {
                offset = self.parse_nonneg_int()?;
                limit = Some(lim_val);
            } else if self.eat(&Tok::Comma) {
                offset = lim_val;
                limit = Some(self.parse_nonneg_int()?);
            } else {
                limit = Some(lim_val);
            }
        }
        Ok(SelectPlan {
            distinct,
            items,
            from_db,
            from_table,
            from_alias,
            joins,
            where_expr,
            group_by,
            having,
            order_by,
            limit,
            offset,
        })
    }

    pub(super) fn parse_nonneg_int(&mut self) -> Result<usize, AirSqlError> {
        match self.peek() {
            Some(Tok::Int(n)) if *n >= 0 => {
                let v = *n as usize;
                self.pos += 1;
                Ok(v)
            }
            _ => Err(AirSqlError::new(
                "execute",
                "Expected non-negative integer in LIMIT/OFFSET.",
                3115,
            )),
        }
    }

    pub(super) fn parse_column_def(&mut self) -> Result<SqlColumnDef, AirSqlError> {
        let name = self.ident()?;
        let mut type_words = Vec::new();
        while let Some(Tok::Word(w)) = self.peek() {
            if matches!(
                w.to_ascii_uppercase().as_str(),
                "PRIMARY"
                    | "NOT"
                    | "NULL"
                    | "UNIQUE"
                    | "DEFAULT"
                    | "COLLATE"
                    | "AUTOINCREMENT"
                    | "CHECK"
                    | "REFERENCES"
                    | "CONSTRAINT"
            ) {
                break;
            }
            type_words.push(w.clone());
            self.pos += 1;
            if self.eat(&Tok::LParen) {
                let mut depth = 1;
                while depth > 0 && self.pos < self.toks.len() {
                    if self.eat(&Tok::LParen) {
                        depth += 1;
                    } else if self.eat(&Tok::RParen) {
                        depth -= 1;
                    } else {
                        self.pos += 1;
                    }
                }
            }
        }
        let data_type = if type_words.is_empty() {
            "TEXT".to_owned()
        } else {
            type_words.join(" ").to_ascii_uppercase()
        };
        let mut allow_null = true;
        let mut primary_key = false;
        let mut auto_increment = false;
        let mut unique = false;
        let mut default_collation_type = "BINARY".to_owned();
        let mut default_value = None;

        while !self.at_end() && !matches!(self.peek(), Some(Tok::Comma | Tok::RParen)) {
            if self.eat_kw("CONSTRAINT") {
                let _ = self.ident()?;
            } else if self.eat_kw("PRIMARY") {
                self.expect_kw("KEY")?;
                self.eat_kw("ASC");
                self.eat_kw("DESC");
                primary_key = true;
                if self.eat_kw("AUTOINCREMENT") {
                    auto_increment = true;
                }
            } else if self.eat_kw("AUTOINCREMENT") {
                auto_increment = true;
            } else if self.eat_kw("NOT") {
                self.expect_kw("NULL")?;
                allow_null = false;
            } else if self.eat_kw("NULL") {
                allow_null = true;
            } else if self.eat_kw("UNIQUE") {
                unique = true;
            } else if self.eat_kw("COLLATE") {
                let c = self.ident()?.to_ascii_uppercase();
                default_collation_type = if c == "NOCASE" {
                    "NO_CASE".to_owned()
                } else {
                    "BINARY".to_owned()
                };
            } else if self.eat_kw("DEFAULT") {
                let expr = self.unary_expr()?;
                default_value = Some(eval_const_expr(&expr)?);
            } else if self.eat_kw("CHECK") {
                self.expect(&Tok::LParen)?;
                let mut depth = 1;
                while depth > 0 && self.pos < self.toks.len() {
                    if self.eat(&Tok::LParen) {
                        depth += 1;
                    } else if self.eat(&Tok::RParen) {
                        depth -= 1;
                    } else {
                        self.pos += 1;
                    }
                }
            } else if self.eat_kw("REFERENCES") {
                let _ = self.ident()?;
                if self.eat(&Tok::LParen) {
                    while !self.eat(&Tok::RParen) && self.pos < self.toks.len() {
                        self.pos += 1;
                    }
                }
            } else {
                break;
            }
        }
        if primary_key && data_type == "INTEGER" {
            auto_increment = true;
        }
        Ok(SqlColumnDef {
            name,
            data_type,
            allow_null,
            primary_key,
            auto_increment,
            unique,
            default_collation_type,
            default_value,
        })
    }
}

pub(super) fn eval_const_expr(expr: &SqlExpr) -> Result<SqlValue, AirSqlError> {
    match expr {
        SqlExpr::Lit(v) => Ok(v.clone()),
        SqlExpr::UnaryMinus(inner) => match eval_const_expr(inner)? {
            SqlValue::Int(n) => Ok(SqlValue::Int(-n)),
            SqlValue::Real(f) => Ok(SqlValue::Real(-f)),
            _ => Err(AirSqlError::new(
                "execute",
                "Invalid DEFAULT numeric literal.",
                3115,
            )),
        },
        _ => Err(AirSqlError::new(
            "execute",
            "Column DEFAULT requires a literal value in this bounded SQL profile.",
            3115,
        )),
    }
}

pub(super) fn sql_like_match(value: &str, pattern: &str) -> bool {
    let v: Vec<char> = value.chars().map(|c| c.to_ascii_lowercase()).collect();
    let p: Vec<char> = pattern.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (mut vi, mut pi) = (0usize, 0usize);
    let (mut star_pi, mut star_vi) = (None::<usize>, 0usize);
    while vi < v.len() {
        if pi < p.len() && (p[pi] == '_' || p[pi] == v[vi]) {
            vi += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == '%' {
            star_pi = Some(pi);
            pi += 1;
            star_vi = vi;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_vi += 1;
            vi = star_vi;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '%' {
        pi += 1;
    }
    pi == p.len()
}

#[derive(Debug, Clone)]
pub(super) struct EvalColMeta {
    pub(super) table_name: String,
    pub(super) table_alias: String,
    pub(super) col_name: String,
    pub(super) nocase: bool,
}

#[derive(Debug, Clone)]
pub(super) struct EvalRow {
    pub(super) rowid: Option<i64>,
    pub(super) values: Vec<SqlValue>,
}

pub(super) fn resolve_param(
    params: &BTreeMap<String, SqlValue>,
    key: &str,
) -> Result<SqlValue, AirSqlError> {
    if let Some(v) = params.get(key) {
        return Ok(v.clone());
    }
    let stripped = key.trim_start_matches([':', '@', '$']);
    if let Some(v) = params.get(stripped) {
        return Ok(v.clone());
    }
    for prefix in [":", "@", "$"] {
        if let Some(v) = params.get(&format!("{prefix}{stripped}")) {
            return Ok(v.clone());
        }
    }
    Err(AirSqlError::new(
        "execute",
        format!("Missing bound value for SQL parameter '{key}'."),
        3115,
    ))
}

pub(super) fn expr_contains_aggregate(expr: &SqlExpr) -> bool {
    match expr {
        SqlExpr::Call { name, args, .. } => {
            matches!(
                name.as_str(),
                "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" | "TOTAL"
            ) || args.iter().any(expr_contains_aggregate)
        }
        SqlExpr::UnaryMinus(e) | SqlExpr::Not(e) | SqlExpr::IsNull { expr: e, .. } => {
            expr_contains_aggregate(e)
        }
        SqlExpr::Binary { left, right, .. } => {
            expr_contains_aggregate(left) || expr_contains_aggregate(right)
        }
        SqlExpr::InList { expr, list, .. } => {
            expr_contains_aggregate(expr) || list.iter().any(expr_contains_aggregate)
        }
        SqlExpr::Between {
            expr, low, high, ..
        } => {
            expr_contains_aggregate(expr)
                || expr_contains_aggregate(low)
                || expr_contains_aggregate(high)
        }
        SqlExpr::Like { expr, pattern, .. } => {
            expr_contains_aggregate(expr) || expr_contains_aggregate(pattern)
        }
        SqlExpr::Lit(_) | SqlExpr::Param(_) | SqlExpr::Col { .. } => false,
    }
}

pub(super) type ProjectedRowCtx = (Vec<SqlValue>, Option<EvalRow>, Option<Vec<EvalRow>>);
pub(super) type OrderKeyedRow = (Vec<(SqlValue, bool)>, Vec<SqlValue>);

pub(super) fn default_expr_column_name(expr: &SqlExpr, long_style: bool) -> String {
    match expr {
        SqlExpr::Col { table, name } => {
            if long_style && let Some(t) = table {
                return format!("{t}_{name}");
            }
            name.clone()
        }
        SqlExpr::Call {
            name, star: true, ..
        } => format!("{name}(*)"),
        SqlExpr::Call { name, args, .. } => {
            let inner: Vec<String> = args
                .iter()
                .map(|a| default_expr_column_name(a, false))
                .collect();
            format!("{name}({})", inner.join(","))
        }
        _ => "column".to_owned(),
    }
}

pub(super) fn eval_sql_expr(
    expr: &SqlExpr,
    cols: &[EvalColMeta],
    row: Option<&EvalRow>,
    group: Option<&[EvalRow]>,
    params: &BTreeMap<String, SqlValue>,
    extra_aliases: Option<&[(&str, SqlValue)]>,
) -> Result<SqlValue, AirSqlError> {
    let active_row = row.or_else(|| group.and_then(|g| g.first()));
    match expr {
        SqlExpr::Lit(v) => Ok(v.clone()),
        SqlExpr::Param(p) => resolve_param(params, p),
        SqlExpr::Col { table, name } => {
            if table.is_none()
                && let Some(aliases) = extra_aliases
                && let Some((_, val)) = aliases.iter().find(|(a, _)| a.eq_ignore_ascii_case(name))
            {
                return Ok(val.clone());
            }
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "rowid" | "_rowid_" | "oid"
            ) && let Some(r) = active_row
                && let Some(id) = r.rowid
            {
                let has_explicit = cols.iter().any(|c| c.col_name.eq_ignore_ascii_case(name));
                if !has_explicit {
                    return Ok(SqlValue::Int(id));
                }
            }
            let mut matched: Option<(usize, &EvalColMeta)> = None;
            for (idx, c) in cols.iter().enumerate() {
                let table_ok = match table {
                    Some(t) => {
                        c.table_alias.eq_ignore_ascii_case(t)
                            || c.table_name.eq_ignore_ascii_case(t)
                    }
                    None => true,
                };
                if table_ok && c.col_name.eq_ignore_ascii_case(name) {
                    matched = Some((idx, c));
                    break;
                }
            }
            let (idx, _) = matched.ok_or_else(|| {
                AirSqlError::new("execute", format!("No such column '{name}'."), 3115)
            })?;
            Ok(active_row
                .and_then(|r| r.values.get(idx).cloned())
                .unwrap_or(SqlValue::Null))
        }
        SqlExpr::UnaryMinus(inner) => {
            let v = eval_sql_expr(inner, cols, row, group, params, extra_aliases)?;
            match v {
                SqlValue::Null => Ok(SqlValue::Null),
                SqlValue::Int(n) => Ok(SqlValue::Int(-n)),
                SqlValue::Real(f) => Ok(SqlValue::Real(-f)),
                other => Ok(SqlValue::Real(-other.as_f64().unwrap_or(0.0))),
            }
        }
        SqlExpr::Not(inner) => {
            let v = eval_sql_expr(inner, cols, row, group, params, extra_aliases)?;
            Ok(if v.is_null() {
                SqlValue::Null
            } else {
                SqlValue::Bool(!v.truthy())
            })
        }
        SqlExpr::IsNull { expr, negated } => {
            let v = eval_sql_expr(expr, cols, row, group, params, extra_aliases)?;
            let is_n = v.is_null();
            Ok(SqlValue::Bool(if *negated { !is_n } else { is_n }))
        }
        SqlExpr::InList {
            expr,
            list,
            negated,
        } => {
            let lhs = eval_sql_expr(expr, cols, row, group, params, extra_aliases)?;
            if lhs.is_null() {
                return Ok(SqlValue::Null);
            }
            let mut found = false;
            for item in list {
                let rhs = eval_sql_expr(item, cols, row, group, params, extra_aliases)?;
                if !rhs.is_null() && lhs.cmp_sql(&rhs, false) == std::cmp::Ordering::Equal {
                    found = true;
                    break;
                }
            }
            Ok(SqlValue::Bool(if *negated { !found } else { found }))
        }
        SqlExpr::Between {
            expr,
            low,
            high,
            negated,
        } => {
            let v = eval_sql_expr(expr, cols, row, group, params, extra_aliases)?;
            let lo = eval_sql_expr(low, cols, row, group, params, extra_aliases)?;
            let hi = eval_sql_expr(high, cols, row, group, params, extra_aliases)?;
            if v.is_null() || lo.is_null() || hi.is_null() {
                return Ok(SqlValue::Null);
            }
            let inside = v.cmp_sql(&lo, false) != std::cmp::Ordering::Less
                && v.cmp_sql(&hi, false) != std::cmp::Ordering::Greater;
            Ok(SqlValue::Bool(if *negated { !inside } else { inside }))
        }
        SqlExpr::Like {
            expr,
            pattern,
            negated,
        } => {
            let v = eval_sql_expr(expr, cols, row, group, params, extra_aliases)?;
            let p = eval_sql_expr(pattern, cols, row, group, params, extra_aliases)?;
            if v.is_null() || p.is_null() {
                return Ok(SqlValue::Null);
            }
            let m = sql_like_match(&v.as_text(), &p.as_text());
            Ok(SqlValue::Bool(if *negated { !m } else { m }))
        }
        SqlExpr::Binary { op, left, right } => match op {
            BinOp::And => {
                let l = eval_sql_expr(left, cols, row, group, params, extra_aliases)?;
                if !l.is_null() && !l.truthy() {
                    return Ok(SqlValue::Bool(false));
                }
                let r = eval_sql_expr(right, cols, row, group, params, extra_aliases)?;
                if !r.is_null() && !r.truthy() {
                    return Ok(SqlValue::Bool(false));
                }
                if l.is_null() || r.is_null() {
                    Ok(SqlValue::Null)
                } else {
                    Ok(SqlValue::Bool(true))
                }
            }
            BinOp::Or => {
                let l = eval_sql_expr(left, cols, row, group, params, extra_aliases)?;
                if l.truthy() {
                    return Ok(SqlValue::Bool(true));
                }
                let r = eval_sql_expr(right, cols, row, group, params, extra_aliases)?;
                if r.truthy() {
                    return Ok(SqlValue::Bool(true));
                }
                if l.is_null() || r.is_null() {
                    Ok(SqlValue::Null)
                } else {
                    Ok(SqlValue::Bool(false))
                }
            }
            _ => {
                let l = eval_sql_expr(left, cols, row, group, params, extra_aliases)?;
                let r = eval_sql_expr(right, cols, row, group, params, extra_aliases)?;
                if l.is_null() || r.is_null() {
                    return Ok(SqlValue::Null);
                }
                let nocase = col_uses_nocase(left, cols) || col_uses_nocase(right, cols);
                match op {
                    BinOp::Eq => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) == std::cmp::Ordering::Equal,
                    )),
                    BinOp::Ne => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) != std::cmp::Ordering::Equal,
                    )),
                    BinOp::Lt => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) == std::cmp::Ordering::Less,
                    )),
                    BinOp::Le => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) != std::cmp::Ordering::Greater,
                    )),
                    BinOp::Gt => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) == std::cmp::Ordering::Greater,
                    )),
                    BinOp::Ge => Ok(SqlValue::Bool(
                        l.cmp_sql(&r, nocase) != std::cmp::Ordering::Less,
                    )),
                    BinOp::Concat => Ok(SqlValue::Text(format!("{}{}", l.as_text(), r.as_text()))),
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                        if let (SqlValue::Int(a), SqlValue::Int(b)) = (&l, &r) {
                            return match op {
                                BinOp::Add => Ok(SqlValue::Int(a.saturating_add(*b))),
                                BinOp::Sub => Ok(SqlValue::Int(a.saturating_sub(*b))),
                                BinOp::Mul => Ok(SqlValue::Int(a.saturating_mul(*b))),
                                BinOp::Div => Ok(if *b == 0 {
                                    SqlValue::Null
                                } else {
                                    SqlValue::Int(a / b)
                                }),
                                BinOp::Rem => Ok(if *b == 0 {
                                    SqlValue::Null
                                } else {
                                    SqlValue::Int(a % b)
                                }),
                                _ => unreachable!(),
                            };
                        }
                        let a = l.as_f64().unwrap_or(0.0);
                        let b = r.as_f64().unwrap_or(0.0);
                        match op {
                            BinOp::Add => Ok(SqlValue::Real(a + b)),
                            BinOp::Sub => Ok(SqlValue::Real(a - b)),
                            BinOp::Mul => Ok(SqlValue::Real(a * b)),
                            BinOp::Div => Ok(if b == 0.0 {
                                SqlValue::Null
                            } else {
                                SqlValue::Real(a / b)
                            }),
                            BinOp::Rem => Ok(if b == 0.0 {
                                SqlValue::Null
                            } else {
                                SqlValue::Real(a % b)
                            }),
                            _ => unreachable!(),
                        }
                    }
                    BinOp::And | BinOp::Or => unreachable!(),
                }
            }
        },
        SqlExpr::Call {
            name,
            distinct,
            star,
            args,
        } => {
            if matches!(
                name.as_str(),
                "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" | "TOTAL"
            ) {
                let rows_slice: &[EvalRow] = if let Some(g) = group {
                    g
                } else if let Some(r) = row {
                    std::slice::from_ref(r)
                } else {
                    &[]
                };
                if name == "COUNT" && *star {
                    return Ok(SqlValue::Int(rows_slice.len() as i64));
                }
                let arg = args.first().ok_or_else(|| {
                    AirSqlError::new("execute", format!("{name}() requires an argument."), 3115)
                })?;
                let mut vals = Vec::new();
                for r in rows_slice {
                    let v = eval_sql_expr(arg, cols, Some(r), None, params, None)?;
                    if !v.is_null() {
                        if *distinct
                            && vals.iter().any(|old: &SqlValue| {
                                old.cmp_sql(&v, false) == std::cmp::Ordering::Equal
                            })
                        {
                            continue;
                        }
                        vals.push(v);
                    }
                }
                return match name.as_str() {
                    "COUNT" => Ok(SqlValue::Int(vals.len() as i64)),
                    "SUM" | "TOTAL" => {
                        if vals.is_empty() {
                            return Ok(if name == "TOTAL" {
                                SqlValue::Real(0.0)
                            } else {
                                SqlValue::Null
                            });
                        }
                        if name == "SUM" && vals.iter().all(|v| matches!(v, SqlValue::Int(_))) {
                            let s: i64 = vals
                                .iter()
                                .map(|v| match v {
                                    SqlValue::Int(n) => *n,
                                    _ => 0,
                                })
                                .sum();
                            Ok(SqlValue::Int(s))
                        } else {
                            let s: f64 = vals.iter().filter_map(SqlValue::as_f64).sum();
                            Ok(SqlValue::Real(s))
                        }
                    }
                    "AVG" => {
                        if vals.is_empty() {
                            Ok(SqlValue::Null)
                        } else {
                            let s: f64 = vals.iter().filter_map(SqlValue::as_f64).sum();
                            Ok(SqlValue::Real(s / (vals.len() as f64)))
                        }
                    }
                    "MIN" => Ok(vals
                        .into_iter()
                        .min_by(|a, b| a.cmp_sql(b, false))
                        .unwrap_or(SqlValue::Null)),
                    "MAX" => Ok(vals
                        .into_iter()
                        .max_by(|a, b| a.cmp_sql(b, false))
                        .unwrap_or(SqlValue::Null)),
                    _ => unreachable!(),
                };
            }
            let mut evaluated = Vec::with_capacity(args.len());
            for a in args {
                evaluated.push(eval_sql_expr(a, cols, row, group, params, extra_aliases)?);
            }
            match name.as_str() {
                "COALESCE" | "IFNULL" => Ok(evaluated
                    .into_iter()
                    .find(|v| !v.is_null())
                    .unwrap_or(SqlValue::Null)),
                "NULLIF" => {
                    if evaluated.len() == 2
                        && evaluated[0].cmp_sql(&evaluated[1], false) == std::cmp::Ordering::Equal
                    {
                        Ok(SqlValue::Null)
                    } else {
                        Ok(evaluated.into_iter().next().unwrap_or(SqlValue::Null))
                    }
                }
                "UPPER" => Ok(match evaluated.first() {
                    None | Some(SqlValue::Null) => SqlValue::Null,
                    Some(v) => SqlValue::Text(v.as_text().to_uppercase()),
                }),
                "LOWER" => Ok(match evaluated.first() {
                    None | Some(SqlValue::Null) => SqlValue::Null,
                    Some(v) => SqlValue::Text(v.as_text().to_lowercase()),
                }),
                "TRIM" => Ok(match evaluated.first() {
                    None | Some(SqlValue::Null) => SqlValue::Null,
                    Some(v) => SqlValue::Text(v.as_text().trim().to_owned()),
                }),
                "LENGTH" => Ok(match evaluated.first() {
                    None | Some(SqlValue::Null) => SqlValue::Null,
                    Some(SqlValue::Blob(b)) => {
                        let decoded = BASE64.decode(b).unwrap_or_default();
                        SqlValue::Int(decoded.len() as i64)
                    }
                    Some(v) => SqlValue::Int(v.as_text().chars().count() as i64),
                }),
                "ABS" => Ok(match evaluated.first() {
                    None | Some(SqlValue::Null) => SqlValue::Null,
                    Some(SqlValue::Int(n)) => SqlValue::Int(n.abs()),
                    Some(v) => SqlValue::Real(v.as_f64().unwrap_or(0.0).abs()),
                }),
                "ROUND" => match evaluated.first() {
                    None | Some(SqlValue::Null) => Ok(SqlValue::Null),
                    Some(v) => {
                        let f = v.as_f64().unwrap_or(0.0);
                        let places =
                            evaluated.get(1).and_then(SqlValue::as_f64).unwrap_or(0.0) as i32;
                        let factor = 10_f64.powi(places.clamp(0, 12));
                        Ok(SqlValue::Real((f * factor).round() / factor))
                    }
                },
                "SUBSTR" | "SUBSTRING" => match evaluated.first() {
                    None | Some(SqlValue::Null) => Ok(SqlValue::Null),
                    Some(v) => {
                        let chars: Vec<char> = v.as_text().chars().collect();
                        let start_1 =
                            evaluated.get(1).and_then(SqlValue::as_f64).unwrap_or(1.0) as isize;
                        let idx = if start_1 > 0 {
                            (start_1 as usize).saturating_sub(1).min(chars.len())
                        } else if start_1 < 0 {
                            chars.len().saturating_sub((-start_1) as usize)
                        } else {
                            0
                        };
                        let take = evaluated
                            .get(2)
                            .and_then(SqlValue::as_f64)
                            .map_or(chars.len(), |n| n.max(0.0) as usize);
                        let s: String = chars.into_iter().skip(idx).take(take).collect();
                        Ok(SqlValue::Text(s))
                    }
                },
                "TYPEOF" => Ok(SqlValue::Text(
                    match evaluated.first().unwrap_or(&SqlValue::Null) {
                        SqlValue::Null => "null",
                        SqlValue::Int(_) | SqlValue::Bool(_) => "integer",
                        SqlValue::Real(_) => "real",
                        SqlValue::Text(_) => "text",
                        SqlValue::Blob(_) => "blob",
                    }
                    .to_owned(),
                )),
                other => Err(AirSqlError::new(
                    "execute",
                    format!("Unsupported SQL function '{other}'."),
                    3115,
                )),
            }
        }
    }
}

pub(super) fn col_uses_nocase(expr: &SqlExpr, cols: &[EvalColMeta]) -> bool {
    if let SqlExpr::Col { table, name } = expr {
        cols.iter().any(|c| {
            let t_ok = table.as_ref().is_none_or(|t| {
                c.table_alias.eq_ignore_ascii_case(t) || c.table_name.eq_ignore_ascii_case(t)
            });
            t_ok && c.col_name.eq_ignore_ascii_case(name) && c.nocase
        })
    } else {
        false
    }
}
