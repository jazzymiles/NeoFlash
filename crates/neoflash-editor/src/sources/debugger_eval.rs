/// Conditional and data/property-change breakpoint metadata for a source line (1-based).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BreakpointSpec {
    pub line: usize,
    pub condition: Option<String>,
    pub hit_count: u32,
    pub hit_count_target: Option<u32>,
    pub enabled: bool,
    pub watch_property: Option<String>,
}

impl BreakpointSpec {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(line: usize) -> Self {
        Self {
            line: line.max(1),
            condition: None,
            hit_count: 0,
            hit_count_target: None,
            enabled: true,
            watch_property: None,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_condition(mut self, condition: impl Into<String>) -> Self {
        let condition = condition.into();
        self.condition = (!condition.trim().is_empty()).then(|| condition.trim().to_owned());
        self
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_hit_count_target(mut self, target: u32) -> Self {
        self.hit_count_target = (target > 0).then_some(target);
        self
    }

    #[allow(dead_code)]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_watch_property(mut self, watch_property: impl Into<String>) -> Self {
        let watch_property = watch_property.into();
        self.watch_property =
            (!watch_property.trim().is_empty()).then(|| watch_property.trim().to_owned());
        self
    }
}

/// Watched debugger expression evaluated against the paused stack frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchExpression {
    pub expression: String,
    pub last_value: String,
    pub is_error: bool,
}

/// Hierarchical variable node in a paused debugger scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebuggerVariable {
    pub name: String,
    pub value: String,
    pub kind: String,
    pub children: Vec<DebuggerVariable>,
}

impl DebuggerVariable {
    pub fn scalar(
        name: impl Into<String>,
        value: impl Into<String>,
        kind: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            kind: kind.into(),
            children: Vec::new(),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn object(
        name: impl Into<String>,
        class_name: impl Into<String>,
        children: Vec<DebuggerVariable>,
    ) -> Self {
        let class_str = class_name.into();
        Self {
            name: name.into(),
            value: format!("[object {class_str}]"),
            kind: class_str,
            children,
        }
    }
}

/// Named variable scope (`Local`, `Closure`, `Global`) in a paused stack frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebuggerScope {
    pub name: String,
    pub variables: Vec<DebuggerVariable>,
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct DebuggerAuthoringState {
    pub(super) breakpoints: std::collections::BTreeMap<String, Vec<BreakpointSpec>>,
    pub(super) last_watched_values: std::collections::BTreeMap<(String, usize, String), String>,
    pub(super) debugger_watches: Vec<WatchExpression>,
    pub(super) paused_scopes: Vec<DebuggerScope>,
    pub(super) folded_lines_by_tab:
        std::collections::BTreeMap<String, std::collections::BTreeSet<usize>>,
    pub(super) actions_debugger_open: bool,
    pub(super) watch_draft: String,
    pub(super) var_path_draft: String,
    pub(super) var_value_draft: String,
    pub(super) expanded_var_paths: std::collections::BTreeSet<String>,
}

pub(super) static DEBUGGER_AUTHORING_STATES: std::sync::LazyLock<
    std::sync::Mutex<std::collections::BTreeMap<String, DebuggerAuthoringState>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));

#[derive(Clone, Debug, PartialEq)]
pub(super) enum EvalValue {
    Number(f64),
    Bool(bool),
    Str(String),
    Null,
    Undefined,
}

impl EvalValue {
    pub(super) fn from_variable(var: &DebuggerVariable) -> Self {
        let trimmed = var.value.trim();
        match var.kind.as_str() {
            "Number" | "int" | "uint" => {
                if let Ok(n) = trimmed.parse::<f64>() {
                    return Self::Number(n);
                }
            }
            "Boolean" => {
                if trimmed == "true" {
                    return Self::Bool(true);
                }
                if trimmed == "false" {
                    return Self::Bool(false);
                }
            }
            "Null" => return Self::Null,
            "Undefined" => return Self::Undefined,
            _ => {}
        }
        if trimmed == "true" {
            Self::Bool(true)
        } else if trimmed == "false" {
            Self::Bool(false)
        } else if trimmed == "null" {
            Self::Null
        } else if trimmed == "undefined" {
            Self::Undefined
        } else if let Ok(n) = trimmed.parse::<f64>() {
            Self::Number(n)
        } else {
            let unquoted = if trimmed.starts_with('"')
                && let Ok(parsed) = serde_json::from_str::<String>(trimmed)
            {
                return Self::Str(parsed);
            } else {
                trimmed
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .or_else(|| {
                        trimmed
                            .strip_prefix('\'')
                            .and_then(|s| s.strip_suffix('\''))
                    })
                    .unwrap_or(trimmed)
            };
            Self::Str(unquoted.to_owned())
        }
    }

    pub(super) fn format_display(&self) -> String {
        match self {
            Self::Number(n) => {
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            Self::Bool(b) => b.to_string(),
            Self::Str(s) => s.clone(),
            Self::Null => "null".to_owned(),
            Self::Undefined => "undefined".to_owned(),
        }
    }

    pub(super) fn is_truthy(&self) -> bool {
        match self {
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::Bool(b) => *b,
            Self::Str(s) => !s.is_empty(),
            Self::Null | Self::Undefined => false,
        }
    }

    pub(super) fn as_f64(&self) -> Result<f64, String> {
        match self {
            Self::Number(n) => Ok(*n),
            Self::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Self::Str(s) => s
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("TypeError: cannot convert '{s}' to Number")),
            Self::Null => Ok(0.0),
            Self::Undefined => Err("TypeError: undefined is not a number".to_owned()),
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn split_var_path(path: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    for ch in path.trim().chars() {
        match ch {
            '.' | '[' | ']' => {
                let seg = current.trim().trim_matches('"').trim_matches('\'');
                if !seg.is_empty() {
                    segments.push(seg.to_owned());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let tail = current.trim().trim_matches('"').trim_matches('\'');
    if !tail.is_empty() {
        segments.push(tail.to_owned());
    }
    segments
}

pub(super) fn find_variable_in_slice<'a>(
    vars: &'a [DebuggerVariable],
    segments: &[String],
) -> Option<&'a DebuggerVariable> {
    let (first, rest) = segments.split_first()?;
    let bracketed = format!("[{first}]");
    let found = vars
        .iter()
        .find(|v| v.name == *first || v.name == bracketed)
        .or_else(|| first.parse::<usize>().ok().and_then(|idx| vars.get(idx)))?;
    if rest.is_empty() {
        Some(found)
    } else {
        find_variable_in_slice(&found.children, rest)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn find_variable_in_slice_mut<'a>(
    vars: &'a mut [DebuggerVariable],
    segments: &[String],
) -> Option<&'a mut DebuggerVariable> {
    let (first, rest) = segments.split_first()?;
    let bracketed = format!("[{first}]");
    let pos = vars
        .iter()
        .position(|v| v.name == *first || v.name == bracketed)
        .or_else(|| first.parse::<usize>().ok().filter(|&idx| idx < vars.len()))?;
    let found = &mut vars[pos];
    if rest.is_empty() {
        Some(found)
    } else {
        find_variable_in_slice_mut(&mut found.children, rest)
    }
}

pub(super) struct ExprEvaluator<'a> {
    pub(super) scopes: &'a [DebuggerScope],
}

impl<'a> ExprEvaluator<'a> {
    pub(super) fn eval(&self, input: &str) -> Result<EvalValue, String> {
        let s = input.trim();
        if s.is_empty() {
            return Err("Empty expression".to_owned());
        }
        self.eval_or(s)
    }

    fn split_top_level<'b>(s: &'b str, ops: &[&'b str]) -> Option<(&'b str, &'b str, &'b str)> {
        let bytes = s.as_bytes();
        let mut depth_paren = 0i32;
        let mut depth_bracket = 0i32;
        let mut in_quote: Option<u8> = None;
        let mut i = bytes.len();
        while i > 0 {
            i -= 1;
            let b = bytes[i];
            if let Some(q) = in_quote {
                if b == q && (i == 0 || bytes[i - 1] != b'\\') {
                    in_quote = None;
                }
                continue;
            }
            match b {
                b'"' | b'\'' => in_quote = Some(b),
                b')' => depth_paren += 1,
                b'(' => depth_paren -= 1,
                b']' => depth_bracket += 1,
                b'[' => depth_bracket -= 1,
                _ if depth_paren == 0 && depth_bracket == 0 => {
                    for &op in ops {
                        let op_len = op.len();
                        if i + op_len <= bytes.len() && &s[i..i + op_len] == op {
                            if (op == "+" || op == "-") && i == 0 {
                                continue;
                            }
                            if (op == "<" || op == ">")
                                && i + 1 < bytes.len()
                                && bytes[i + 1] == b'='
                            {
                                continue;
                            }
                            if op == "="
                                && (i > 0 && matches!(bytes[i - 1], b'=' | b'!' | b'<' | b'>')
                                    || (i + 1 < bytes.len() && bytes[i + 1] == b'='))
                            {
                                continue;
                            }
                            let left = s[..i].trim();
                            let right = s[i + op_len..].trim();
                            if !left.is_empty() && !right.is_empty() {
                                return Some((left, op, right));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn eval_or(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, _, r)) = Self::split_top_level(s, &["||"]) {
            let lv = self.eval_or(l)?;
            if lv.is_truthy() {
                return Ok(EvalValue::Bool(true));
            }
            let rv = self.eval_and(r)?;
            return Ok(EvalValue::Bool(rv.is_truthy()));
        }
        self.eval_and(s)
    }

    fn eval_and(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, _, r)) = Self::split_top_level(s, &["&&"]) {
            let lv = self.eval_and(l)?;
            if !lv.is_truthy() {
                return Ok(EvalValue::Bool(false));
            }
            let rv = self.eval_equality(r)?;
            return Ok(EvalValue::Bool(rv.is_truthy()));
        }
        self.eval_equality(s)
    }

    fn eval_equality(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, op, r)) = Self::split_top_level(s, &["===", "!==", "==", "!="]) {
            let lv = self.eval_equality(l)?;
            let rv = self.eval_relational(r)?;
            let eq = match (&lv, &rv) {
                (EvalValue::Number(a), EvalValue::Number(b)) => (a - b).abs() < 1e-9,
                _ => lv.format_display() == rv.format_display(),
            };
            let res = if op.starts_with('!') { !eq } else { eq };
            return Ok(EvalValue::Bool(res));
        }
        self.eval_relational(s)
    }

    fn eval_relational(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, op, r)) = Self::split_top_level(s, &["<=", ">=", "<", ">"]) {
            let lv = self.eval_relational(l)?.as_f64()?;
            let rv = self.eval_additive(r)?.as_f64()?;
            let res = match op {
                "<=" => lv <= rv,
                ">=" => lv >= rv,
                "<" => lv < rv,
                ">" => lv > rv,
                _ => unreachable!(),
            };
            return Ok(EvalValue::Bool(res));
        }
        self.eval_additive(s)
    }

    fn eval_additive(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, op, r)) = Self::split_top_level(s, &["+", "-"]) {
            let lv = self.eval_additive(l)?;
            let rv = self.eval_multiplicative(r)?;
            if op == "+" && (matches!(lv, EvalValue::Str(_)) || matches!(rv, EvalValue::Str(_))) {
                return Ok(EvalValue::Str(format!(
                    "{}{}",
                    lv.format_display(),
                    rv.format_display()
                )));
            }
            let a = lv.as_f64()?;
            let b = rv.as_f64()?;
            return Ok(EvalValue::Number(if op == "+" { a + b } else { a - b }));
        }
        self.eval_multiplicative(s)
    }

    fn eval_multiplicative(&self, s: &str) -> Result<EvalValue, String> {
        if let Some((l, op, r)) = Self::split_top_level(s, &["*", "/", "%"]) {
            let a = self.eval_multiplicative(l)?.as_f64()?;
            let b = self.eval_unary(r)?.as_f64()?;
            let val = match op {
                "*" => a * b,
                "/" => {
                    if b == 0.0 {
                        return Err("RangeError: division by zero".to_owned());
                    }
                    a / b
                }
                "%" => {
                    if b == 0.0 {
                        return Err("RangeError: modulo by zero".to_owned());
                    }
                    a % b
                }
                _ => unreachable!(),
            };
            return Ok(EvalValue::Number(val));
        }
        self.eval_unary(s)
    }

    fn eval_unary(&self, s: &str) -> Result<EvalValue, String> {
        let s = s.trim();
        if let Some(rest) = s.strip_prefix('!') {
            let v = self.eval_unary(rest)?;
            return Ok(EvalValue::Bool(!v.is_truthy()));
        }
        if let Some(rest) = s.strip_prefix('-') {
            let v = self.eval_unary(rest)?.as_f64()?;
            return Ok(EvalValue::Number(-v));
        }
        if let Some(rest) = s.strip_prefix('+') {
            let v = self.eval_unary(rest)?.as_f64()?;
            return Ok(EvalValue::Number(v));
        }
        self.eval_primary(s)
    }

    fn eval_primary(&self, s: &str) -> Result<EvalValue, String> {
        let s = s.trim();
        if s.starts_with('(') && s.ends_with(')') {
            return self.eval(&s[1..s.len() - 1]);
        }
        if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
            return Ok(EvalValue::Str(s[1..s.len() - 1].to_owned()));
        }
        if s == "true" {
            return Ok(EvalValue::Bool(true));
        }
        if s == "false" {
            return Ok(EvalValue::Bool(false));
        }
        if s == "null" {
            return Ok(EvalValue::Null);
        }
        if s == "undefined" {
            return Ok(EvalValue::Undefined);
        }
        if let Ok(n) = s.parse::<f64>() {
            return Ok(EvalValue::Number(n));
        }
        if s.contains('(') || s.contains(')') {
            return Err(format!(
                "Unsupported expression '{s}': debugger evaluation is bounded to pure member paths, indexing, and arithmetic/comparison operators"
            ));
        }

        // Resolve member/index path (supporting expressions inside `[...]`)
        let mut resolved_segments = Vec::new();
        let mut current = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '.' => {
                    if !current.trim().is_empty() {
                        resolved_segments.push(current.trim().to_owned());
                        current.clear();
                    }
                    i += 1;
                }
                '[' => {
                    if !current.trim().is_empty() {
                        resolved_segments.push(current.trim().to_owned());
                        current.clear();
                    }
                    let mut depth = 1;
                    i += 1;
                    let start = i;
                    while i < chars.len() && depth > 0 {
                        if chars[i] == '[' {
                            depth += 1;
                        } else if chars[i] == ']' {
                            depth -= 1;
                        }
                        if depth > 0 {
                            i += 1;
                        }
                    }
                    let inner: String = chars[start..i].iter().collect();
                    let idx_val = self.eval(&inner)?;
                    resolved_segments.push(idx_val.format_display());
                    if i < chars.len() && chars[i] == ']' {
                        i += 1;
                    }
                }
                ch => {
                    current.push(ch);
                    i += 1;
                }
            }
        }
        if !current.trim().is_empty() {
            resolved_segments.push(current.trim().to_owned());
        }

        for scope in self.scopes {
            if let Some(var) = find_variable_in_slice(&scope.variables, &resolved_segments) {
                return Ok(EvalValue::from_variable(var));
            }
        }

        Err(format!(
            "ReferenceError: '{s}' is not defined in the paused stack frame"
        ))
    }
}
