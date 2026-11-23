use super::*;

impl Editor {
    pub(super) fn with_authoring_state<R>(
        &self,
        f: impl FnOnce(&DebuggerAuthoringState) -> R,
    ) -> R {
        let guard = DEBUGGER_AUTHORING_STATES
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let empty = DebuggerAuthoringState::default();
        let state = guard.get(&self.clipboard.session).unwrap_or(&empty);
        f(state)
    }

    pub(super) fn with_authoring_state_mut<R>(
        &mut self,
        f: impl FnOnce(&mut DebuggerAuthoringState) -> R,
    ) -> R {
        let mut guard = DEBUGGER_AUTHORING_STATES
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let state = guard.entry(self.clipboard.session.clone()).or_default();
        f(state)
    }

    pub(crate) fn remove_debugger_authoring_state(&self, session_key: &str) {
        let mut guard = DEBUGGER_AUTHORING_STATES
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.remove(session_key);
    }

    #[cfg(test)]
    pub(crate) fn has_authoring_state_for_session_key(&self, key: &str) -> bool {
        let guard = DEBUGGER_AUTHORING_STATES
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.contains_key(key)
    }

    pub(crate) fn current_source_tab_key(&self, fallback_tab: &str) -> String {
        if let Some(crate::panels::actions_sidebar::ScriptLocation::Source(id)) =
            self.effective_actions_location()
            && let Some(unit) = self.project.sources.iter().find(|s| s.id == id)
        {
            return unit.path.clone();
        }
        let trimmed = fallback_tab.trim();
        if trimmed.is_empty() {
            format!("Frame {}", self.frame + 1)
        } else {
            trimmed.to_owned()
        }
    }

    pub(super) fn normalize_breakpoint_key(path: &str) -> String {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            "Frame 1".to_owned()
        } else {
            trimmed.to_owned()
        }
    }

    /// Toggle an authored breakpoint on `line` (1-based) in `path`. Returns `true` if added, `false` if removed.
    pub fn toggle_breakpoint(&mut self, path: &str, line: usize) -> bool {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        let added = self.with_authoring_state_mut(|state| {
            let list = state.breakpoints.entry(key.clone()).or_default();
            if let Some(pos) = list.iter().position(|bp| bp.line == line) {
                list.remove(pos);
                false
            } else {
                list.push(BreakpointSpec::new(line));
                list.sort_by_key(|bp| bp.line);
                true
            }
        });
        if added {
            if !self
                .debugger
                .breakpoints
                .iter()
                .any(|bp| bp.source == key && bp.line == line)
            {
                self.debugger
                    .breakpoints
                    .push(crate::debugger_protocol::AuthoredBreakpoint {
                        source: key,
                        line,
                        column: None,
                    });
            }
        } else {
            self.debugger
                .breakpoints
                .retain(|bp| !(bp.source == key && bp.line == line));
        }
        added
    }

    pub(crate) fn clear_all_session_breakpoints(&mut self) {
        self.with_authoring_state_mut(|state| {
            state.breakpoints.clear();
            state.last_watched_values.clear();
        });
        self.debugger.breakpoints.clear();
    }

    pub(crate) fn remove_session_breakpoint(&mut self, path: &str, line: usize) {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        self.with_authoring_state_mut(|state| {
            if let Some(list) = state.breakpoints.get_mut(&key) {
                list.retain(|bp| bp.line != line);
                if list.is_empty() {
                    state.breakpoints.remove(&key);
                }
            }
            state
                .last_watched_values
                .retain(|(src, ln, _), _| !(src == &key && *ln == line));
        });
        self.debugger
            .breakpoints
            .retain(|bp| !(bp.source == key && bp.line == line));
    }

    pub(crate) fn sync_runtime_breakpoints_from_session(&mut self) {
        let mut points = Vec::new();
        self.with_authoring_state(|state| {
            for (source, list) in &state.breakpoints {
                for bp in list {
                    if bp.enabled {
                        points.push(crate::debugger_protocol::AuthoredBreakpoint {
                            source: source.clone(),
                            line: bp.line,
                            column: None,
                        });
                    }
                }
            }
        });
        points.sort();
        points.dedup();
        self.debugger.breakpoints = points;
    }

    /// Returns `true` if `path` has a breakpoint at `line` (1-based).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn has_breakpoint(&self, path: &str, line: usize) -> bool {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        self.with_authoring_state(|state| {
            state
                .breakpoints
                .get(&key)
                .is_some_and(|list| list.iter().any(|bp| bp.line == line))
        }) || self
            .debugger
            .breakpoints
            .iter()
            .any(|bp| bp.source == key && bp.line == line)
    }

    /// Configure a conditional or data/property-change breakpoint on `spec.line` in `path`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_breakpoint_spec(&mut self, path: &str, mut spec: BreakpointSpec) {
        let key = Self::normalize_breakpoint_key(path);
        spec.line = spec.line.max(1);
        let line = spec.line;
        let baseline = spec.watch_property.as_deref().and_then(|prop| {
            self.evaluate_debugger_expression(prop)
                .ok()
                .map(|v| (prop.to_owned(), v))
        });
        self.with_authoring_state_mut(|state| {
            if let Some((prop, val)) = baseline {
                state
                    .last_watched_values
                    .insert((key.clone(), line, prop), val);
            }
            let list = state.breakpoints.entry(key.clone()).or_default();
            if let Some(existing) = list.iter_mut().find(|bp| bp.line == line) {
                *existing = spec;
            } else {
                list.push(spec);
                list.sort_by_key(|bp| bp.line);
            }
        });
        if !self
            .debugger
            .breakpoints
            .iter()
            .any(|bp| bp.source == key && bp.line == line)
        {
            self.debugger
                .breakpoints
                .push(crate::debugger_protocol::AuthoredBreakpoint {
                    source: key,
                    line,
                    column: None,
                });
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn breakpoint_spec(&self, path: &str, line: usize) -> Option<BreakpointSpec> {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        self.with_authoring_state(|state| {
            state
                .breakpoints
                .get(&key)
                .and_then(|list| list.iter().find(|bp| bp.line == line).cloned())
        })
    }

    pub fn breakpoints_for(&self, path: &str) -> Vec<BreakpointSpec> {
        let key = Self::normalize_breakpoint_key(path);
        self.with_authoring_state(|state| state.breakpoints.get(&key).cloned().unwrap_or_default())
    }

    pub fn set_breakpoint_condition(
        &mut self,
        path: &str,
        line: usize,
        condition: Option<String>,
    ) -> bool {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        let normalized = condition
            .map(|c| c.trim().to_owned())
            .filter(|c| !c.is_empty());
        self.with_authoring_state_mut(|state| {
            let Some(list) = state.breakpoints.get_mut(&key) else {
                return false;
            };
            let Some(bp) = list.iter_mut().find(|bp| bp.line == line) else {
                return false;
            };
            bp.condition = normalized;
            true
        })
    }

    pub fn set_breakpoint_hit_count_target(
        &mut self,
        path: &str,
        line: usize,
        target: Option<u32>,
    ) -> bool {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        let normalized = target.filter(|&t| t > 0);
        self.with_authoring_state_mut(|state| {
            let Some(list) = state.breakpoints.get_mut(&key) else {
                return false;
            };
            let Some(bp) = list.iter_mut().find(|bp| bp.line == line) else {
                return false;
            };
            bp.hit_count_target = normalized;
            true
        })
    }

    pub fn set_breakpoint_enabled(&mut self, path: &str, line: usize, enabled: bool) -> bool {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        let updated = self.with_authoring_state_mut(|state| {
            let Some(list) = state.breakpoints.get_mut(&key) else {
                return false;
            };
            let Some(bp) = list.iter_mut().find(|bp| bp.line == line) else {
                return false;
            };
            bp.enabled = enabled;
            true
        });
        if updated {
            if enabled {
                if !self
                    .debugger
                    .breakpoints
                    .iter()
                    .any(|bp| bp.source == key && bp.line == line)
                {
                    self.debugger
                        .breakpoints
                        .push(crate::debugger_protocol::AuthoredBreakpoint {
                            source: key,
                            line,
                            column: None,
                        });
                }
            } else {
                self.debugger
                    .breakpoints
                    .retain(|bp| !(bp.source == key && bp.line == line));
            }
        }
        updated
    }

    pub fn actions_debugger_open(&self) -> bool {
        self.with_authoring_state(|state| state.actions_debugger_open)
    }

    pub fn set_actions_debugger_open(&mut self, open: bool) {
        self.with_authoring_state_mut(|state| {
            state.actions_debugger_open = open;
        });
    }

    pub fn actions_debugger_inspector_visible(&self, _tab_key: &str) -> bool {
        self.actions_debugger_open()
    }

    /// Evaluate whether execution should pause at `(path, line)`, checking breakpoint enabled status,
    /// condition, data/property-change watch, and hit-count target, and incrementing `hit_count`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn should_pause_at_breakpoint(&mut self, path: &str, line: usize) -> bool {
        let key = Self::normalize_breakpoint_key(path);
        let line = line.max(1);
        let Some(spec) = self.breakpoint_spec(&key, line) else {
            return false;
        };
        if !spec.enabled {
            return false;
        }

        if let Some(cond) = spec.condition.as_deref() {
            let scopes = self.effective_debugger_scopes();
            let evaluator = ExprEvaluator { scopes: &scopes };
            match evaluator.eval(cond) {
                Ok(v) if v.is_truthy() => {}
                _ => return false,
            }
        }

        if let Some(prop) = spec.watch_property.as_deref() {
            let Ok(current_val) = self.evaluate_debugger_expression(prop) else {
                return false;
            };
            let changed = self.with_authoring_state_mut(|state| {
                let watch_key = (key.clone(), line, prop.to_owned());
                match state
                    .last_watched_values
                    .insert(watch_key, current_val.clone())
                {
                    Some(prev) => prev != current_val,
                    None => false,
                }
            });
            if !changed {
                return false;
            }
        }

        let new_hits = self.with_authoring_state_mut(|state| {
            if let Some(list) = state.breakpoints.get_mut(&key)
                && let Some(bp) = list.iter_mut().find(|bp| bp.line == line)
            {
                bp.hit_count = bp.hit_count.saturating_add(1);
                bp.hit_count
            } else {
                1
            }
        });
        if let Some(target) = spec.hit_count_target.filter(|&t| t > 0) {
            return new_hits >= target;
        }
        true
    }

    pub(super) fn effective_debugger_scopes(&self) -> Vec<DebuggerScope> {
        let authored = self.with_authoring_state(|state| state.paused_scopes.clone());
        if !authored.is_empty() {
            return authored;
        }
        if let Some(snapshot) = self.debugger.snapshot() {
            let inspection = self.debugger.inspection();
            let variables = if !inspection.locals.is_empty() {
                inspection
                    .locals
                    .iter()
                    .map(|v| {
                        let children = if let Some(oid) = v.object_id.as_deref()
                            && let Some(props) = inspection.object_properties.get(oid)
                        {
                            props
                                .iter()
                                .map(|p| DebuggerVariable::scalar(&p.name, &p.value, &p.kind))
                                .collect()
                        } else if let Some(preview) = inspection.preview_properties.get(&v.name) {
                            preview
                                .iter()
                                .map(|(name, val, kind)| DebuggerVariable::scalar(name, val, kind))
                                .collect()
                        } else {
                            Vec::new()
                        };
                        DebuggerVariable {
                            name: v.name.clone(),
                            value: v.value.clone(),
                            kind: v.kind.clone(),
                            children,
                        }
                    })
                    .collect()
            } else {
                snapshot
                    .locals
                    .iter()
                    .map(|v| DebuggerVariable::scalar(&v.name, &v.value, &v.kind))
                    .collect()
            };
            vec![DebuggerScope {
                name: "Local".to_owned(),
                variables,
            }]
        } else {
            Vec::new()
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_paused_debugger_scopes(&mut self, scopes: Vec<DebuggerScope>) {
        self.with_authoring_state_mut(|state| {
            state.paused_scopes = scopes;
        });
        self.refresh_debugger_watches();
    }

    pub fn paused_debugger_scopes(&self) -> Vec<DebuggerScope> {
        self.effective_debugger_scopes()
    }

    /// Inspect a potentially nested `DebuggerVariable` by path (e.g. `"player.stats.hp"` or `"items[0]"`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn inspect_debugger_variable_path(
        &self,
        scope_idx: usize,
        var_path: &str,
    ) -> Option<DebuggerVariable> {
        let segments = split_var_path(var_path);
        if segments.is_empty() {
            return None;
        }
        let scopes = self.effective_debugger_scopes();
        let scope = scopes.get(scope_idx)?;
        find_variable_in_slice(&scope.variables, &segments).cloned()
    }

    /// Edit a variable or nested object property in the paused debugger scope and refresh watches.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_debugger_variable_value(
        &mut self,
        scope_idx: usize,
        var_path: &str,
        new_value: &str,
    ) -> Result<String, String> {
        let segments = split_var_path(var_path);
        if segments.is_empty() {
            return Err("Variable path must not be empty".to_owned());
        }
        let result = self.with_authoring_state_mut(|state| {
            let scope = state
                .paused_scopes
                .get_mut(scope_idx)
                .ok_or_else(|| format!("Scope index {scope_idx} is out of bounds"))?;
            let var =
                find_variable_in_slice_mut(&mut scope.variables, &segments).ok_or_else(|| {
                    format!("Variable '{var_path}' not found in scope '{}'", scope.name)
                })?;
            let trimmed = new_value.trim();
            var.value = trimmed.to_owned();
            if var.children.is_empty() {
                if trimmed == "true" || trimmed == "false" {
                    var.kind = "Boolean".to_owned();
                } else if trimmed.parse::<f64>().is_ok() {
                    var.kind = "Number".to_owned();
                } else if trimmed == "null" {
                    var.kind = "Null".to_owned();
                } else if trimmed == "undefined" {
                    var.kind = "Undefined".to_owned();
                } else {
                    var.kind = "String".to_owned();
                }
            }
            Ok(var.value.clone())
        });
        if result.is_ok() {
            self.refresh_debugger_watches();
        }
        result
    }

    /// Evaluate an expression (identifiers, dotted member paths, array indexing, arithmetic, comparisons,
    /// and boolean conditions) against the current paused stack frame's `DebuggerVariable` tree.
    pub fn evaluate_debugger_expression(&self, expr: &str) -> Result<String, String> {
        let scopes = self.effective_debugger_scopes();
        let evaluator = ExprEvaluator { scopes: &scopes };
        evaluator.eval(expr).map(|v| v.format_display())
    }

    pub(crate) fn raw_debugger_watch_expressions(&self) -> Vec<String> {
        self.with_authoring_state(|state| {
            state
                .debugger_watches
                .iter()
                .map(|w| w.expression.clone())
                .collect()
        })
    }

    pub(crate) fn debugger_watch_draft(&self) -> String {
        self.with_authoring_state(|state| state.watch_draft.clone())
    }

    pub(crate) fn sync_watch_values_from_cdp(
        &mut self,
        results: &[crate::debugger_protocol::DebugWatchResult],
    ) {
        if results.is_empty() {
            return;
        }
        self.with_authoring_state_mut(|state| {
            for watch in &mut state.debugger_watches {
                if let Some(cdp) = results.iter().find(|r| r.expression == watch.expression)
                    && cdp.value != "<not paused>"
                    && cdp.value != "…"
                {
                    watch.last_value = cdp.value.clone();
                    watch.is_error = cdp.is_error;
                }
            }
        });
    }

    pub fn debugger_watches(&self) -> Vec<WatchExpression> {
        let mut watches = self.with_authoring_state(|state| state.debugger_watches.clone());
        if let Some(snapshot) = self.debugger.snapshot()
            && snapshot.state == crate::debugger_protocol::DebugState::Paused
        {
            let cdp_watches = self.debugger.watch_results();
            for watch in &mut watches {
                if let Some(cdp) = cdp_watches
                    .iter()
                    .find(|r| r.expression == watch.expression)
                    && cdp.value != "<not paused>"
                {
                    watch.last_value = cdp.value.clone();
                    watch.is_error = cdp.is_error;
                }
            }
        }
        watches
    }

    pub fn add_debugger_watch(&mut self, expr: impl Into<String>) -> usize {
        let expression = expr.into().trim().to_owned();
        let eval_res = self.evaluate_debugger_expression(&expression);
        let (last_value, is_error) = match eval_res {
            Ok(val) => (val, false),
            Err(err) => (err, true),
        };
        let idx = self.with_authoring_state_mut(|state| {
            let idx = state.debugger_watches.len();
            state.debugger_watches.push(WatchExpression {
                expression,
                last_value,
                is_error,
            });
            idx
        });
        self.sync_debug_watches();
        idx
    }

    pub fn remove_debugger_watch(&mut self, index: usize) -> bool {
        let removed = self.with_authoring_state_mut(|state| {
            if index < state.debugger_watches.len() {
                state.debugger_watches.remove(index);
                true
            } else {
                false
            }
        });
        if removed {
            self.sync_debug_watches();
        }
        removed
    }

    pub(crate) fn refresh_debugger_watches_local(&mut self) {
        let expressions: Vec<String> = self.raw_debugger_watch_expressions();
        let evaluated: Vec<(String, bool)> = expressions
            .iter()
            .map(|expr| match self.evaluate_debugger_expression(expr) {
                Ok(v) => (v, false),
                Err(e) => (e, true),
            })
            .collect();
        self.with_authoring_state_mut(|state| {
            for (watch, (val, is_err)) in state.debugger_watches.iter_mut().zip(evaluated) {
                watch.last_value = val;
                watch.is_error = is_err;
            }
        });
        if let Some(snapshot) = self.debugger.snapshot()
            && snapshot.state == crate::debugger_protocol::DebugState::Paused
        {
            let cdp_watches = self.debugger.watch_results();
            self.sync_watch_values_from_cdp(&cdp_watches);
        }
    }

    pub fn refresh_debugger_watches(&mut self) {
        self.refresh_debugger_watches_local();
        self.evaluate_active_debug_watches();
    }

    #[allow(dead_code)]
    pub fn set_debugger_watch_draft(&mut self, draft: impl Into<String>) {
        let text = draft.into();
        self.with_authoring_state_mut(|state| {
            state.watch_draft = text;
        });
    }

    #[allow(dead_code)]
    pub fn set_debugger_variable_edit_draft(
        &mut self,
        var_path: impl Into<String>,
        var_value: impl Into<String>,
    ) {
        let path = var_path.into();
        let val = var_value.into();
        self.with_authoring_state_mut(|state| {
            state.var_path_draft = path;
            state.var_value_draft = val;
        });
    }

    pub(crate) fn actions_debugger_inspector_ui(&mut self, ui: &mut egui::Ui, tab_key: &str) {
        use neoflash_ui::icons::i16;
        let p = neoflash_ui::palette(ui.ctx());
        let mut close_drawer = false;
        ui.horizontal(|ui| {
            ui.label(
                neoflash_ui::typography::rich(
                    "Breakpoints & Watch",
                    neoflash_ui::tokens::TextStyle::Overline,
                )
                .color(p.text_secondary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(nf::IconButton::new(i16::X, "Close debugger inspector"))
                    .clicked()
                {
                    close_drawer = true;
                }
            });
        });
        if close_drawer {
            self.set_actions_debugger_open(false);
            return;
        }
        let session = self.clipboard.session.clone();
        egui::ScrollArea::vertical()
            .id_salt(("actions-debugger-inspector", &session, tab_key))
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = space::s1();

                // 1. Conditional & hit-count breakpoints for the current script tab
                let breakpoints = self.breakpoints_for(tab_key);
                nf::SectionHeader::new("Breakpoints")
                    .id_salt(("actions-dbg-bps", &session, tab_key))
                    .show(ui, |ui| {
                        for bp in breakpoints {
                            let mut enabled = bp.enabled;
                            let mut cond_text = bp.condition.clone().unwrap_or_default();
                            let mut hit_target = bp.hit_count_target.unwrap_or(0) as f64;
                            let mut remove_clicked = false;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::s2();
                                let label = format!("Line {}", bp.line);
                                let hits_badge = format!("Hits: {}", bp.hit_count);
                                // One row per breakpoint: the condition takes what the fixed
                                // controls leave, down to a numeric field.
                                let text_width = |text: &str, style| {
                                    ui.painter()
                                        .layout_no_wrap(
                                            text.to_owned(),
                                            neoflash_ui::typography::font(style),
                                            egui::Color32::PLACEHOLDER,
                                        )
                                        .size()
                                        .x
                                };
                                let fixed = neoflash_ui::metrics::check_box()
                                    + space::s2()
                                    + text_width(&label, neoflash_ui::TextStyle::Body)
                                    + neoflash_ui::metrics::field_s()
                                    + text_width(&hits_badge, neoflash_ui::TextStyle::Small)
                                    + 12.
                                    + neoflash_ui::metrics::control()
                                    + 4. * space::s2();
                                let condition_width = (ui.available_width() - fixed).clamp(
                                    neoflash_ui::metrics::field_num(),
                                    neoflash_ui::metrics::field_m(),
                                );
                                if ui.add(nf::Checkbox::new(&mut enabled, &label)).changed() {
                                    self.set_breakpoint_enabled(tab_key, bp.line, enabled);
                                }
                                if ui
                                    .add(
                                        nf::TextField::new(&mut cond_text, "Breakpoint condition")
                                            .prefix("if")
                                            .hint("Condition")
                                            .mono()
                                            .width(condition_width),
                                    )
                                    .changed()
                                {
                                    let cond = (!cond_text.trim().is_empty())
                                        .then(|| cond_text.trim().to_owned());
                                    self.set_breakpoint_condition(tab_key, bp.line, cond);
                                }
                                if ui
                                    .add(
                                        nf::Field::new(&mut hit_target, "Hit count target")
                                            .prefix("Hit >=")
                                            .range(0.0..=10_000.0)
                                            .step(1.0)
                                            .decimals(0)
                                            .width(neoflash_ui::metrics::field_s()),
                                    )
                                    .changed()
                                {
                                    let target_u32 = hit_target.round().max(0.0) as u32;
                                    self.set_breakpoint_hit_count_target(
                                        tab_key,
                                        bp.line,
                                        (target_u32 > 0).then_some(target_u32),
                                    );
                                }
                                ui.add(nf::Badge::new(&hits_badge, nf::BadgeType::Neutral));
                                if ui
                                    .add(nf::IconButton::new(i16::TRASH, "Remove breakpoint"))
                                    .clicked()
                                {
                                    remove_clicked = true;
                                }
                            });
                            if remove_clicked {
                                self.toggle_breakpoint(tab_key, bp.line);
                            }
                        }
                    });

                // 2. Watch expressions (add / remove / evaluate)
                let mut watch_draft = self.debugger_watch_draft();
                let watches = self.debugger_watches();
                nf::SectionHeader::new("Watch")
                    .id_salt(("actions-dbg-watch", &session, tab_key))
                    .show(ui, |ui| {
                        let mut add_watch_now = false;
                        let mut refresh_now = false;
                        ui.horizontal(|ui| {
                            let gap = space::s2();
                            let control = neoflash_ui::metrics::control();
                            ui.spacing_mut().item_spacing.x = gap;
                            let add_label_w = ui
                                .painter()
                                .layout_no_wrap(
                                    "Add watch".to_owned(),
                                    neoflash_ui::typography::font(neoflash_ui::TextStyle::Body),
                                    egui::Color32::PLACEHOLDER,
                                )
                                .size()
                                .x;
                            let add_btn_w = nf::button::button_padding()
                                + nf::button::button_padding()
                                + neoflash_ui::metrics::ICON_UI
                                + gap
                                + add_label_w;
                            let max_expr_w =
                                (ui.available_width() - add_btn_w - control - gap - gap).max(0.0);
                            let expr_w = max_expr_w.min(neoflash_ui::metrics::field_m());
                            let tf = ui.add(
                                nf::TextField::new(&mut watch_draft, "Watch expression")
                                    .prefix("Expr")
                                    .hint("Expression")
                                    .mono()
                                    .width(expr_w),
                            );
                            if tf.changed() {
                                self.set_debugger_watch_draft(watch_draft.clone());
                            }
                            let mut tf_edit = tf.clone();
                            tf_edit.id = nf::TextField::edit_id(tf.id);
                            if (tf_edit.has_focus() || tf_edit.lost_focus())
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                && !watch_draft.trim().is_empty()
                            {
                                add_watch_now = true;
                            }
                            if ui
                                .add_enabled(
                                    !watch_draft.trim().is_empty(),
                                    nf::Button::new("Add watch").icon(i16::PLUS),
                                )
                                .clicked()
                            {
                                add_watch_now = true;
                            }
                            if ui
                                .add(nf::IconButton::new(i16::REFRESH, "Evaluate watches"))
                                .clicked()
                            {
                                refresh_now = true;
                            }
                        });
                        if add_watch_now {
                            let expr = watch_draft.trim().to_owned();
                            if !expr.is_empty() {
                                self.add_debugger_watch(expr);
                                self.set_debugger_watch_draft(String::new());
                            }
                        }
                        if refresh_now {
                            self.refresh_debugger_watches();
                        }
                        let mut remove_watch_idx = None;
                        let cols = &[0.0, 0.45];
                        let row_h = neoflash_ui::metrics::row();
                        let row_gap = space::s2();
                        for (idx, watch) in watches.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.set_height(row_h);
                                ui.spacing_mut().item_spacing.x = row_gap;
                                let table_w = (ui.available_width() - row_h - row_gap).max(0.0);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(table_w, row_h),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_width(table_w);
                                        ui.add(nf::TableRow {
                                            cells: &[&watch.expression, &watch.last_value],
                                            kind: nf::TableRowKind::Row,
                                            columns: cols,
                                        });
                                    },
                                );
                                if ui
                                    .add(
                                        nf::IconButton::new(i16::TRASH, "Remove watch").side(row_h),
                                    )
                                    .clicked()
                                {
                                    remove_watch_idx = Some(idx);
                                }
                            });
                        }
                        if let Some(idx) = remove_watch_idx {
                            self.remove_debugger_watch(idx);
                        }
                    });

                // 3. Paused scope variables (nested inspection + live value editing)
                let scopes = self.paused_debugger_scopes();
                if !scopes.is_empty() {
                    let (mut var_path_draft, mut var_value_draft, expanded) = self
                        .with_authoring_state(|state| {
                            (
                                state.var_path_draft.clone(),
                                state.var_value_draft.clone(),
                                state.expanded_var_paths.clone(),
                            )
                        });
                    nf::SectionHeader::new("Variables")
                        .id_salt(("actions-dbg-vars", &session, tab_key))
                        .show(ui, |ui| {
                            let mut apply_var_edit = false;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::s2();
                                // The path takes what the value field and the button leave.
                                let set_value_width = ui
                                    .painter()
                                    .layout_no_wrap(
                                        "Set value".to_owned(),
                                        neoflash_ui::typography::font(neoflash_ui::TextStyle::Body),
                                        egui::Color32::PLACEHOLDER,
                                    )
                                    .size()
                                    .x
                                    + 2. * nf::button::button_padding();
                                let path_width = (ui.available_width()
                                    - neoflash_ui::metrics::field_s()
                                    - set_value_width
                                    - 2. * space::s2())
                                .clamp(
                                    neoflash_ui::metrics::field_num(),
                                    neoflash_ui::metrics::field_m(),
                                );
                                if ui
                                    .add(
                                        nf::TextField::new(&mut var_path_draft, "Variable path")
                                            .prefix("Path")
                                            .hint("player.stats.hp")
                                            .mono()
                                            .width(path_width),
                                    )
                                    .changed()
                                {
                                    self.with_authoring_state_mut(|state| {
                                        state.var_path_draft = var_path_draft.clone();
                                    });
                                }
                                if ui
                                    .add(
                                        nf::TextField::new(&mut var_value_draft, "Variable value")
                                            .prefix("=")
                                            .hint("Value")
                                            .mono()
                                            .width(neoflash_ui::metrics::field_s()),
                                    )
                                    .changed()
                                {
                                    self.with_authoring_state_mut(|state| {
                                        state.var_value_draft = var_value_draft.clone();
                                    });
                                }
                                if ui
                                    .add_enabled(
                                        !var_path_draft.trim().is_empty(),
                                        nf::Button::new("Set value"),
                                    )
                                    .clicked()
                                {
                                    apply_var_edit = true;
                                }
                            });
                            if apply_var_edit {
                                let _ = self.set_debugger_variable_value(
                                    0,
                                    var_path_draft.trim(),
                                    &var_value_draft,
                                );
                            }
                            for (scope_idx, scope) in scopes.iter().enumerate() {
                                self.render_debugger_variable_nodes(
                                    ui,
                                    scope_idx,
                                    "",
                                    &scope.variables,
                                    0,
                                    &expanded,
                                );
                            }
                        });
                }
            });
    }

    pub(super) fn render_debugger_variable_nodes(
        &mut self,
        ui: &mut egui::Ui,
        scope_idx: usize,
        parent_path: &str,
        variables: &[DebuggerVariable],
        depth: usize,
        expanded: &std::collections::BTreeSet<String>,
    ) {
        use neoflash_ui::icons::i16;
        for var in variables {
            let full_path = if parent_path.is_empty() {
                var.name.clone()
            } else {
                format!("{parent_path}.{}", var.name)
            };
            let has_children = !var.children.is_empty();
            let is_open = expanded.contains(&full_path);
            let kind = if has_children {
                if is_open {
                    nf::TreeItemType::FolderOpen
                } else {
                    nf::TreeItemType::FolderClosed
                }
            } else {
                nf::TreeItemType::Symbol(i16::VARIABLE)
            };
            let label = format!("{} = {}", var.name, var.value);
            let row = nf::TreeItem::new(&label, kind)
                .depth(depth)
                .meta(&var.kind)
                .show(ui);
            if row.toggle || row.row.clicked() {
                let inspected = self
                    .inspect_debugger_variable_path(scope_idx, &full_path)
                    .unwrap_or_else(|| var.clone());
                self.with_authoring_state_mut(|state| {
                    state.var_path_draft = full_path.clone();
                    state.var_value_draft = inspected.value.clone();
                    if has_children {
                        if state.expanded_var_paths.contains(&full_path) {
                            state.expanded_var_paths.remove(&full_path);
                        } else {
                            state.expanded_var_paths.insert(full_path.clone());
                        }
                    }
                });
            }
            if has_children && is_open {
                self.render_debugger_variable_nodes(
                    ui,
                    scope_idx,
                    &full_path,
                    &var.children,
                    depth + 1,
                    expanded,
                );
            }
        }
    }
}
