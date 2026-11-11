use super::*;

// ============================================================================
// Script & PropertyExpression -> After Effects JavaScript Expression Compiler
// ============================================================================

/// Translates a NeoFlash [`PropertyExpression`] into a valid After Effects JavaScript expression
/// snippet evaluating the scalar result for `base_var` (e.g. `"value"` or `"value[0]"`).
pub fn compile_property_expression_to_ae_js(expr: &PropertyExpression, base_var: &str) -> String {
    let raw = expr.expression.trim();
    if raw.is_empty() || raw == "value" {
        return base_var.to_owned();
    }
    let mentions_value = contains_ident(raw, "value");
    let rewritten = rewrite_neoflash_expr_tokens(raw, base_var, expr.seed);
    let mut out = String::new();
    let _ = write!(
        out,
        "(() => {{ const amp = {:.6}; const freq = {:.6}; const decay = {:.6}; const phase = {:.6}; \
         const fps = 1.0 / thisComp.frameDuration; const t = Math.max(0, time - inPoint); \
         const frame = timeToFrames(t, fps); const pi = Math.PI; const tau = Math.PI * 2; \
         const lerp = (a, b, u) => a + (b - a) * u; \
         const pingpong = (x) => 1 - Math.abs((((x) % 2) + 2) % 2 - 1); \
         const saw = (x) => (((x) % 1) + 1) % 1; \
         const square = (x) => ((((x) % 1) + 1) % 1) < 0.5 ? 1 : -1; \
         const triangle = (x) => 1 - 4 * Math.abs(((((x) % 1) + 1) % 1) - 0.5); \
         const bounce = (x) => Math.abs(Math.sin(x * Math.PI)) * Math.exp(-decay * Math.max(0, x)); \
         const elastic = (x) => (x <= 0 ? 0 : Math.sin(x * tau * freq) * Math.exp(-decay * x)); \
         const noise = (x) => {{ seedRandom({}, true); return (wiggle(1, 1, 1, 0.5, x) - {base_var}); }}; \
         const _res = ({rewritten}); return {}; }})()",
        expr.amplitude,
        expr.frequency,
        expr.decay,
        expr.phase,
        expr.seed,
        if mentions_value {
            "_res".to_owned()
        } else {
            format!("{base_var} + _res")
        }
    );
    out
}

fn contains_ident(src: &str, needle: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if &src[start..i] == needle {
                return true;
            }
        } else {
            i += 1;
        }
    }
    false
}

fn rewrite_neoflash_expr_tokens(src: &str, base_var: &str, seed: u32) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() + 32);
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            // Peek next non-whitespace char to distinguish function calls from variables
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let is_call = j < chars.len() && chars[j] == '(';
            let lower = ident.to_ascii_lowercase();
            if is_call {
                match lower.as_str() {
                    "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "atan2" | "abs" | "min"
                    | "max" | "floor" | "ceil" | "round" | "sqrt" | "pow" | "exp" => {
                        out.push_str("Math.");
                        out.push_str(&lower);
                    }
                    "wiggle" => {
                        // In AE, `wiggle(freq, amp)` returns `value + delta`. Return delta when
                        // combined with `value + wiggle(...)` or when `mentions_value` is false.
                        let _ = write!(
                            out,
                            "((f, a) => {{ seedRandom({seed}, true); const w = wiggle(f, a); return Array.isArray(w) ? (w[0] - {base_var}) : (w - {base_var}); }})"
                        );
                    }
                    _ => out.push_str(&ident),
                }
            } else {
                match lower.as_str() {
                    "value" => out.push_str(base_var),
                    "time" => out.push('t'),
                    _ => out.push_str(&ident),
                }
            }
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

#[derive(Clone, Debug, Default)]
pub(super) struct ScriptPropertyDrivers {
    pub(super) dx_per_frame: f64,
    pub(super) dy_per_frame: f64,
    pub(super) x_formula: Option<String>,
    pub(super) y_formula: Option<String>,
    pub(super) dsx_pct_per_frame: f64,
    pub(super) dsy_pct_per_frame: f64,
    pub(super) sx_formula: Option<String>,
    pub(super) sy_formula: Option<String>,
    pub(super) drot_per_frame: f64,
    pub(super) rot_formula: Option<String>,
    pub(super) dalpha_pct_per_frame: f64,
    pub(super) alpha_formula: Option<String>,
    pub(super) text_formula: Option<String>,
}

impl ScriptPropertyDrivers {
    pub(super) fn is_empty(&self) -> bool {
        self.dx_per_frame.abs() <= 1e-9
            && self.dy_per_frame.abs() <= 1e-9
            && self.x_formula.is_none()
            && self.y_formula.is_none()
            && self.dsx_pct_per_frame.abs() <= 1e-9
            && self.dsy_pct_per_frame.abs() <= 1e-9
            && self.sx_formula.is_none()
            && self.sy_formula.is_none()
            && self.drot_per_frame.abs() <= 1e-9
            && self.rot_formula.is_none()
            && self.dalpha_pct_per_frame.abs() <= 1e-9
            && self.alpha_formula.is_none()
            && self.text_formula.is_none()
    }

    pub(super) fn apply_to_expressions(&self, exprs: &mut AeLayerExpressions, origin: [f64; 2]) {
        if self.dx_per_frame.abs() > 1e-9
            || self.dy_per_frame.abs() > 1e-9
            || self.x_formula.is_some()
            || self.y_formula.is_some()
        {
            let x_part = if let Some(fx) = &self.x_formula {
                format!("({fx}) + {:.4}", origin[0])
            } else if self.dx_per_frame.abs() > 1e-9 {
                format!("value[0] + _f * ({:.6})", self.dx_per_frame)
            } else {
                "value[0]".to_owned()
            };
            let y_part = if let Some(fy) = &self.y_formula {
                format!("({fy}) + {:.4}", origin[1])
            } else if self.dy_per_frame.abs() > 1e-9 {
                format!("value[1] + _f * ({:.6})", self.dy_per_frame)
            } else {
                "value[1]".to_owned()
            };
            exprs.position = Some(format!(
                "(() => {{ const _t = Math.max(0, time - inPoint); const _fps = 1.0 / thisComp.frameDuration; const _f = timeToFrames(_t, _fps); return [{x_part}, {y_part}]; }})()"
            ));
        }
        if self.dsx_pct_per_frame.abs() > 1e-9
            || self.dsy_pct_per_frame.abs() > 1e-9
            || self.sx_formula.is_some()
            || self.sy_formula.is_some()
        {
            let sx_part = if let Some(fsx) = &self.sx_formula {
                fsx.clone()
            } else if self.dsx_pct_per_frame.abs() > 1e-9 {
                format!("value[0] + _f * ({:.6})", self.dsx_pct_per_frame)
            } else {
                "value[0]".to_owned()
            };
            let sy_part = if let Some(fsy) = &self.sy_formula {
                fsy.clone()
            } else if self.dsy_pct_per_frame.abs() > 1e-9 {
                format!("value[1] + _f * ({:.6})", self.dsy_pct_per_frame)
            } else {
                "value[1]".to_owned()
            };
            exprs.scale = Some(format!(
                "(() => {{ const _t = Math.max(0, time - inPoint); const _fps = 1.0 / thisComp.frameDuration; const _f = timeToFrames(_t, _fps); return [{sx_part}, {sy_part}]; }})()"
            ));
        }
        if let Some(frot) = &self.rot_formula {
            exprs.rotation = Some(format!(
                "(() => {{ const _t = Math.max(0, time - inPoint); const _fps = 1.0 / thisComp.frameDuration; const _f = timeToFrames(_t, _fps); return ({frot}); }})()"
            ));
        } else if self.drot_per_frame.abs() > 1e-9 {
            exprs.rotation = Some(format!(
                "value + timeToFrames(Math.max(0, time - inPoint), 1.0 / thisComp.frameDuration) * ({:.6})",
                self.drot_per_frame
            ));
        }
        if let Some(fa) = &self.alpha_formula {
            exprs.opacity = Some(format!(
                "(() => {{ const _t = Math.max(0, time - inPoint); const _fps = 1.0 / thisComp.frameDuration; const _f = timeToFrames(_t, _fps); return clamp(({fa}), 0, 100); }})()"
            ));
        } else if self.dalpha_pct_per_frame.abs() > 1e-9 {
            exprs.opacity = Some(format!(
                "clamp(value + timeToFrames(Math.max(0, time - inPoint), 1.0 / thisComp.frameDuration) * ({:.6}), 0, 100)",
                self.dalpha_pct_per_frame
            ));
        }
        if let Some(ft) = &self.text_formula {
            exprs.source_text = Some(format!(
                "(() => {{ const _t = Math.max(0, time - inPoint); const _fps = 1.0 / thisComp.frameDuration; const _f = timeToFrames(_t, _fps); return String({ft}); }})()"
            ));
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum TimelineFlowRule {
    Stop { at_frame: u32 },
    GotoAndPlay { at_frame: u32, target_frame: u32 },
    GotoAndStop { at_frame: u32, target_frame: u32 },
}

/// Analyzes timeline frame scripts (`Timeline.scripts`) and instance scripts (`Element.scripts`)
/// to extract After Effects property expressions (`Position`, `Scale`, `Rotation`, `Opacity`,
/// `Source Text`) and `Time Remap` timeline transport expressions (`stop`, `gotoAndPlay`, `gotoAndStop`).
pub(super) fn analyze_symbol_scripts_for_ae(
    symbol: &Symbol,
    label_map: &BTreeMap<String, u32>,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> (
    BTreeMap<String, ScriptPropertyDrivers>,
    Option<String>,
    Vec<AeMarker>,
) {
    let mut named_drivers: BTreeMap<String, ScriptPropertyDrivers> = BTreeMap::new();
    let mut flow_rules: Vec<TimelineFlowRule> = Vec::new();
    let mut comp_markers: Vec<AeMarker> = Vec::new();

    for fs in &symbol.timeline.scripts {
        let src = fs.script.source.trim();
        if src.is_empty() {
            continue;
        }
        let loc = format!("symbol '{}' frame {}", symbol.name, fs.frame + 1);
        let mut translated_any = false;
        let mut untranslated_lines = Vec::new();

        for stmt in split_script_statements(src) {
            if let Some(rule) = parse_timeline_flow_stmt(&stmt, fs.frame, label_map) {
                flow_rules.push(rule);
                translated_any = true;
            } else if let Some((target, prop, op, rhs)) = parse_property_assignment(&stmt) {
                if !target.is_empty() && target != "this" && target != "_root" {
                    let drv = named_drivers.entry(target.clone()).or_default();
                    if apply_parsed_property_driver(drv, &prop, &op, &rhs, fs.script.language) {
                        translated_any = true;
                    } else {
                        untranslated_lines.push(stmt);
                    }
                } else {
                    untranslated_lines.push(stmt);
                }
            } else if stmt.contains("onEnterFrame") || stmt.contains("ENTER_FRAME") {
                if let Some(body) = extract_brace_body(&stmt) {
                    let mut body_ok = false;
                    for inner in split_script_statements(&body) {
                        if let Some((target, prop, op, rhs)) = parse_property_assignment(&inner)
                            && !target.is_empty()
                            && target != "this"
                            && target != "_root"
                        {
                            let drv = named_drivers.entry(target).or_default();
                            if apply_parsed_property_driver(
                                drv,
                                &prop,
                                &op,
                                &rhs,
                                fs.script.language,
                            ) {
                                body_ok = true;
                            }
                        }
                    }
                    if body_ok {
                        translated_any = true;
                    } else {
                        untranslated_lines.push(stmt);
                    }
                } else {
                    untranslated_lines.push(stmt);
                }
            } else {
                untranslated_lines.push(stmt);
            }
        }

        comp_markers.push(AeMarker {
            frame: fs.frame,
            duration_frames: 0,
            label: format!("Action f{}", fs.frame + 1),
            comment: src.to_owned(),
        });

        if translated_any {
            diagnostics.push(AepDiagnostic {
                code: "AE_SCRIPT_TRANSLATED",
                kind: AepDiagnosticKind::TranslatedExpression,
                location: loc.clone(),
                message: "Translated timeline script actions into After Effects JavaScript Expressions and Comp markers.".into(),
            });
        }
        if !untranslated_lines.is_empty() {
            diagnostics.push(AepDiagnostic {
                code: "AE_SCRIPT_MARKER_ONLY",
                kind: AepDiagnosticKind::MarkerPreserved,
                location: loc,
                message: format!(
                    "Preserved non-declarative script statements in Composition Marker comment ({} statement(s)).",
                    untranslated_lines.len()
                ),
            });
        }
    }

    let time_remap_expr = compile_flow_rules_to_time_remap_expr(&flow_rules);
    (named_drivers, time_remap_expr, comp_markers)
}

pub(super) fn analyze_element_scripts_for_ae(
    symbol_name: &str,
    frame: u32,
    el: &Element,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> (ScriptPropertyDrivers, Vec<AeMarker>) {
    let mut drivers = ScriptPropertyDrivers::default();
    let mut markers = Vec::new();
    for os in &el.scripts {
        let src = os.script.source.trim();
        if src.is_empty() {
            continue;
        }
        let loc = format!(
            "symbol '{}' frame {} element '{}'",
            symbol_name,
            frame + 1,
            if el.name.is_empty() { &el.id } else { &el.name }
        );
        markers.push(AeMarker {
            frame,
            duration_frames: 0,
            label: format!("{:?} script", os.event),
            comment: src.to_owned(),
        });

        if matches!(os.event, ObjectEvent::EnterFrame | ObjectEvent::Load) {
            let body = extract_enter_frame_or_plain_body(src);
            let mut any_ok = false;
            for stmt in split_script_statements(&body) {
                if let Some((target, prop, op, rhs)) = parse_property_assignment(&stmt)
                    && (target.is_empty() || target == "this")
                    && apply_parsed_property_driver(
                        &mut drivers,
                        &prop,
                        &op,
                        &rhs,
                        os.script.language,
                    )
                {
                    any_ok = true;
                }
            }
            if any_ok {
                diagnostics.push(AepDiagnostic {
                    code: "AE_INSTANCE_EXPR_TRANSLATED",
                    kind: AepDiagnosticKind::TranslatedExpression,
                    location: loc,
                    message: "Translated instance script into After Effects layer JavaScript Expression(s).".into(),
                });
            } else {
                diagnostics.push(AepDiagnostic {
                    code: "AE_INSTANCE_SCRIPT_MARKER",
                    kind: AepDiagnosticKind::MarkerPreserved,
                    location: loc,
                    message: "Preserved instance script on After Effects Layer Marker.".into(),
                });
            }
        } else {
            diagnostics.push(AepDiagnostic {
                code: "AE_INTERACTIVE_HANDLER_MARKER",
                kind: AepDiagnosticKind::MarkerPreserved,
                location: loc,
                message: "Interactive click/button handler preserved on After Effects Layer Marker (AE playback is non-interactive).".into(),
            });
        }
    }
    (drivers, markers)
}

pub(super) fn compile_flow_rules_to_time_remap_expr(rules: &[TimelineFlowRule]) -> Option<String> {
    if rules.is_empty() {
        return None;
    }
    // Emit a deterministic AE JS expression for `ADBE Time Remapping`
    let mut branches = String::new();
    for rule in rules {
        match *rule {
            TimelineFlowRule::Stop { at_frame } => {
                let _ = write!(branches, "if (f >= {at_frame}) return {at_frame} / fps; ");
            }
            TimelineFlowRule::GotoAndStop {
                at_frame,
                target_frame,
            } => {
                let _ = write!(
                    branches,
                    "if (f >= {at_frame}) return {target_frame} / fps; "
                );
            }
            TimelineFlowRule::GotoAndPlay {
                at_frame,
                target_frame,
            } => {
                if target_frame < at_frame {
                    let span = (at_frame - target_frame).max(1);
                    let _ = write!(
                        branches,
                        "if (f >= {at_frame}) return ({target_frame} + ((f - {target_frame}) % {span})) / fps; "
                    );
                } else {
                    let _ = write!(
                        branches,
                        "if (f >= {at_frame}) return ({target_frame} + (f - {at_frame})) / fps; "
                    );
                }
            }
        }
    }
    Some(format!(
        "(() => {{ const fps = 1.0 / thisComp.frameDuration; const f = timeToFrames(Math.max(0, time - inPoint), fps); {branches}return Math.max(0, time - inPoint); }})()"
    ))
}

fn extract_enter_frame_or_plain_body(src: &str) -> String {
    let trimmed = src.trim();
    if (trimmed.starts_with("onClipEvent")
        || trimmed.contains("onEnterFrame")
        || trimmed.contains("ENTER_FRAME"))
        && let Some(body) = extract_brace_body(trimmed)
    {
        return body;
    }
    trimmed.to_owned()
}

fn extract_brace_body(src: &str) -> Option<String> {
    let open = src.find('{')?;
    let close = src.rfind('}')?;
    (close > open).then(|| src[open + 1..close].trim().to_owned())
}

fn split_script_statements(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_str: Option<char> = None;
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if let Some(q) = in_str {
            cur.push(ch);
            if ch == '\\' && i + 1 < chars.len() {
                i += 1;
                cur.push(chars[i]);
            } else if ch == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        // Skip `//` line comments
        if ch == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                in_str = Some(ch);
                cur.push(ch);
            }
            '{' | '(' | '[' => {
                depth += 1;
                cur.push(ch);
            }
            '}' | ')' | ']' => {
                depth = (depth - 1).max(0);
                cur.push(ch);
            }
            ';' | '\n' if depth == 0 => {
                let s = cur.trim();
                if !s.is_empty() {
                    out.push(s.to_owned());
                }
                cur.clear();
            }
            _ => cur.push(ch),
        }
        i += 1;
    }
    let tail = cur.trim();
    if !tail.is_empty() {
        out.push(tail.to_owned());
    }
    out
}

fn parse_timeline_flow_stmt(
    stmt: &str,
    at_frame: u32,
    label_map: &BTreeMap<String, u32>,
) -> Option<TimelineFlowRule> {
    let s = stmt
        .trim()
        .trim_start_matches("this.")
        .trim_start_matches("_root.")
        .trim();
    if s == "stop()" {
        return Some(TimelineFlowRule::Stop { at_frame });
    }
    for (prefix, is_play) in [("gotoAndPlay(", true), ("gotoAndStop(", false)] {
        if let Some(rest) = s.strip_prefix(prefix)
            && let Some(arg_raw) = rest.strip_suffix(')')
        {
            let arg = arg_raw.trim();
            let target = if let Some(lbl) = arg
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| arg.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            {
                *label_map.get(lbl)?
            } else if let Ok(one_based) = arg.parse::<u32>() {
                one_based.saturating_sub(1)
            } else {
                return None;
            };
            return Some(if is_play {
                TimelineFlowRule::GotoAndPlay {
                    at_frame,
                    target_frame: target,
                }
            } else {
                TimelineFlowRule::GotoAndStop {
                    at_frame,
                    target_frame: target,
                }
            });
        }
    }
    None
}

fn parse_property_assignment(stmt: &str) -> Option<(String, String, String, String)> {
    for op in ["+=", "-=", "="] {
        if let Some(idx) = stmt.find(op) {
            if op == "=" {
                let prev = idx
                    .checked_sub(1)
                    .and_then(|k| stmt.as_bytes().get(k))
                    .copied();
                let next = stmt.as_bytes().get(idx + 1).copied();
                if matches!(
                    prev,
                    Some(b'=' | b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/')
                ) || next == Some(b'=')
                {
                    continue;
                }
            }
            let lhs_raw = stmt[..idx].trim();
            let lhs = lhs_raw
                .strip_prefix("var ")
                .or_else(|| lhs_raw.strip_prefix("let "))
                .or_else(|| lhs_raw.strip_prefix("const "))
                .map(str::trim)
                .unwrap_or(lhs_raw);
            let rhs = stmt[idx + op.len()..].trim();
            if lhs.is_empty() || rhs.is_empty() {
                return None;
            }
            let (target, prop) = if let Some(dot) = lhs.rfind('.') {
                (
                    lhs[..dot].trim().to_owned(),
                    lhs[dot + 1..].trim().to_owned(),
                )
            } else {
                (String::new(), lhs.to_owned())
            };
            if prop.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Some((target, prop, op.to_owned(), rhs.to_owned()));
            }
        }
    }
    None
}

fn apply_parsed_property_driver(
    drv: &mut ScriptPropertyDrivers,
    prop: &str,
    op: &str,
    rhs: &str,
    lang: ScriptLanguage,
) -> bool {
    let is_as2 = matches!(
        lang,
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
    );
    let sign = if op == "-=" { -1.0 } else { 1.0 };

    if matches!(op, "+=" | "-=") {
        let Ok(num) = rhs.parse::<f64>() else {
            return false;
        };
        let delta = sign * num;
        match prop {
            "_x" | "x" => drv.dx_per_frame += delta,
            "_y" | "y" => drv.dy_per_frame += delta,
            "_xscale" => drv.dsx_pct_per_frame += delta,
            "scaleX" => drv.dsx_pct_per_frame += delta * 100.0,
            "_yscale" => drv.dsy_pct_per_frame += delta,
            "scaleY" => drv.dsy_pct_per_frame += delta * 100.0,
            "_rotation" | "rotation" => drv.drot_per_frame += delta,
            "_alpha" => drv.dalpha_pct_per_frame += delta,
            "alpha" => drv.dalpha_pct_per_frame += if is_as2 { delta } else { delta * 100.0 },
            _ => return false,
        }
        return true;
    }

    // Direct `=` formula assignment
    if !is_safe_ae_js_formula(rhs) {
        return false;
    }
    let rewritten = rewrite_script_rhs_for_ae(rhs);
    match prop {
        "_x" | "x" => drv.x_formula = Some(rewritten),
        "_y" | "y" => drv.y_formula = Some(rewritten),
        "_xscale" => drv.sx_formula = Some(rewritten),
        "scaleX" => drv.sx_formula = Some(format!("({rewritten}) * 100.0")),
        "_yscale" => drv.sy_formula = Some(rewritten),
        "scaleY" => drv.sy_formula = Some(format!("({rewritten}) * 100.0")),
        "_rotation" | "rotation" => drv.rot_formula = Some(rewritten),
        "_alpha" => drv.alpha_formula = Some(rewritten),
        "alpha" => {
            drv.alpha_formula = Some(if is_as2 {
                rewritten
            } else {
                format!("({rewritten}) * 100.0")
            });
        }
        "text" => drv.text_formula = Some(rewritten),
        _ => return false,
    }
    true
}

fn is_safe_ae_js_formula(rhs: &str) -> bool {
    if rhs.is_empty() || rhs.len() > 1024 {
        return false;
    }
    // Reject function definitions, loops, assignments or arbitrary host calls
    for forbidden in [
        "function",
        "=>",
        "while",
        "for(",
        "for (",
        "new ",
        "loadMovie",
        "URLRequest",
    ] {
        if rhs.contains(forbidden) {
            return false;
        }
    }
    true
}

fn rewrite_script_rhs_for_ae(rhs: &str) -> String {
    let mut s = rhs
        .replace("getTimer()", "(_t * 1000.0)")
        .replace("this._currentframe", "(_f + 1)")
        .replace("this.currentFrame", "(_f + 1)")
        .replace("Stage.width", "thisComp.width")
        .replace("Stage.height", "thisComp.height")
        .replace("stage.stageWidth", "thisComp.width")
        .replace("stage.stageHeight", "thisComp.height");

    // Identifier-boundary replacement for bare `_currentframe` / `currentFrame`
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 16);
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() || chars[i] == '_' {
            let prev_is_prop = i > 0 && chars[i - 1] == '.';
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            if !prev_is_prop && (ident == "_currentframe" || ident == "currentFrame") {
                out.push_str("(_f + 1)");
            } else {
                out.push_str(&ident);
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    s = out;
    s
}
