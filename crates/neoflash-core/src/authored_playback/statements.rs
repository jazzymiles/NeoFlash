use super::*;

impl AuthoredPlayback {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_block(
        &mut self,
        project: &Project,
        path: &[Id],
        current_event: &ScriptEvent,
        source: &str,
        origin: &str,
        base_line: usize,
        local_fns: &HashMap<String, (usize, String)>,
    ) {
        if self.call_depth >= MAX_PREVIEW_CALL_DEPTH {
            self.record_diagnostic(
                origin,
                base_line,
                &format!("preview script call depth exceeded maximum ({MAX_PREVIEW_CALL_DEPTH})"),
            );
            return;
        }
        self.call_depth += 1;
        self.execute_block_inner(
            project,
            path,
            current_event,
            source,
            origin,
            base_line,
            local_fns,
        );
        self.call_depth = self.call_depth.saturating_sub(1);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_block_inner(
        &mut self,
        project: &Project,
        path: &[Id],
        current_event: &ScriptEvent,
        source: &str,
        origin: &str,
        base_line: usize,
        local_fns: &HashMap<String, (usize, String)>,
    ) {
        for (rel_line, raw_stmt) in split_script_statements(source) {
            let line = base_line + rel_line.saturating_sub(1);
            if self.steps_remaining == 0 {
                self.record_diagnostic(
                    origin,
                    line,
                    &format!(
                        "preview script execution step budget ({MAX_PREVIEW_STEPS_PER_PASS}) exceeded"
                    ),
                );
                return;
            }
            self.steps_remaining -= 1;
            let stmt = strip_comments_and_trim(raw_stmt);
            let stmt = stmt.trim();
            if stmt.is_empty() || stmt == ";" || stmt == "\"use strict\"" || stmt == "'use strict'"
            {
                continue;
            }
            if stmt.starts_with("import ")
                || stmt == "#initclip"
                || stmt.starts_with("#initclip ")
                || stmt == "#endinitclip"
            {
                continue;
            }
            // Local named function declaration: collected in pass 1, skip at top level.
            if parse_named_function_decl(stmt).is_some() {
                continue;
            }
            // Bare `{ ... }` block
            if stmt.starts_with('{') && stmt.ends_with('}') && is_single_braced_group(stmt) {
                let inner = &stmt[1..stmt.len() - 1];
                self.execute_block(project, path, current_event, inner, origin, line, local_fns);
                continue;
            }
            // 0a. Classic Flash `on (release, press, ...) { ... }` wrapper
            if let Some((events_raw, body)) = parse_wrapper_block(stmt, "on") {
                let events: Vec<String> = events_raw
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                let is_click_like = events
                    .iter()
                    .any(|e| matches!(e.as_str(), "release" | "press" | "click"));
                if is_click_like {
                    match current_event {
                        ScriptEvent::Click => {
                            if !self.has_registered_handler(path, RegisteredEvent::Click, body) {
                                self.execute_block(
                                    project,
                                    path,
                                    current_event,
                                    body,
                                    origin,
                                    line,
                                    local_fns,
                                );
                            }
                        }
                        ScriptEvent::Load | ScriptEvent::Frame(_) => {
                            self.register_handler(
                                path.to_vec(),
                                RegisteredEvent::Click,
                                body.to_owned(),
                                origin.to_owned(),
                                line,
                                false,
                            );
                        }
                        ScriptEvent::EnterFrame => {}
                    }
                } else {
                    self.record_diagnostic(
                        origin,
                        line,
                        &format!(
                            "event wrapper on({}) is not simulated in native stage preview",
                            events_raw.trim()
                        ),
                    );
                }
                continue;
            }
            // 0b. Classic Flash `onClipEvent (load | enterFrame | ...) { ... }` wrapper
            if let Some((events_raw, body)) = parse_wrapper_block(stmt, "onClipEvent") {
                let events: Vec<String> = events_raw
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut handled_any = false;
                if events.iter().any(|e| e == "load") {
                    handled_any = true;
                    let lifetime = self.player.lifetime_id(path).unwrap_or(0);
                    if matches!(current_event, ScriptEvent::Load | ScriptEvent::Frame(_))
                        && self
                            .executed_load_blocks
                            .insert((path.to_vec(), lifetime, line))
                    {
                        self.execute_block(
                            project,
                            path,
                            &ScriptEvent::Load,
                            body,
                            origin,
                            line,
                            local_fns,
                        );
                    }
                }
                if events.iter().any(|e| e == "enterframe") {
                    handled_any = true;
                    match current_event {
                        ScriptEvent::EnterFrame => {
                            if !self.has_registered_handler(path, RegisteredEvent::EnterFrame, body)
                            {
                                self.execute_block(
                                    project,
                                    path,
                                    current_event,
                                    body,
                                    origin,
                                    line,
                                    local_fns,
                                );
                            }
                        }
                        ScriptEvent::Load | ScriptEvent::Frame(_) => {
                            self.register_handler(
                                path.to_vec(),
                                RegisteredEvent::EnterFrame,
                                body.to_owned(),
                                origin.to_owned(),
                                line,
                                false,
                            );
                        }
                        ScriptEvent::Click => {}
                    }
                }
                if events
                    .iter()
                    .any(|e| matches!(e.as_str(), "mousedown" | "mouseup"))
                {
                    handled_any = true;
                    if matches!(current_event, ScriptEvent::Load | ScriptEvent::Frame(_)) {
                        self.register_handler(
                            path.to_vec(),
                            RegisteredEvent::Click,
                            body.to_owned(),
                            origin.to_owned(),
                            line,
                            false,
                        );
                    } else if matches!(current_event, ScriptEvent::Click)
                        && !self.has_registered_handler(path, RegisteredEvent::Click, body)
                    {
                        self.execute_block(
                            project,
                            path,
                            current_event,
                            body,
                            origin,
                            line,
                            local_fns,
                        );
                    }
                }
                if !handled_any {
                    self.record_diagnostic(
                        origin,
                        line,
                        &format!(
                            "clip event onClipEvent({}) is not simulated in native stage preview",
                            events_raw.trim()
                        ),
                    );
                }
                continue;
            }
            // 0c. `if (...) { ... } [else ...]`
            if let Some((cond, then_body, else_part)) = parse_if_statement(stmt) {
                if self.eval_preview_condition(project, path, cond) {
                    self.execute_block(
                        project,
                        path,
                        current_event,
                        then_body,
                        origin,
                        line,
                        local_fns,
                    );
                } else if let Some(else_src) = else_part {
                    self.execute_block(
                        project,
                        path,
                        current_event,
                        else_src,
                        origin,
                        line,
                        local_fns,
                    );
                }
                continue;
            }
            // 0d. `delete <target>` (callbacks, array elements, object properties, dynamic variables)
            if let Some(rest) = stmt.strip_prefix("delete ") {
                let target_expr = rest.trim();
                if self
                    .try_apply_delete_target(project, path, target_expr)
                    .is_some()
                {
                    continue;
                }
            }
            // 0e. Callback assignment: `[receiver.]onRelease = function(...) { ... }` or `= null`
            if self
                .try_register_assigned_callback(project, path, stmt, origin, line, local_fns)
                .unwrap_or(false)
            {
                continue;
            }
            // 0f. `addEventListener(...)` / `removeEventListener(...)`
            if self
                .try_register_event_listener(project, path, stmt, origin, line, local_fns)
                .unwrap_or(false)
            {
                continue;
            }
            // 1. Variable declaration: var/let/const name[:Type] [= expr] (supports comma-separated declarations)
            if let Some(rest) = stmt
                .strip_prefix("var ")
                .or_else(|| stmt.strip_prefix("let "))
                .or_else(|| stmt.strip_prefix("const "))
            {
                let scope_path = self.effective_scope_path(project, path);
                for decl in split_top_level_comma(rest) {
                    if let Some((lhs, rhs)) = split_top_level_assign(decl) {
                        let name = lhs.split(':').next().unwrap_or(lhs).trim();
                        let rhs_trimmed = rhs.trim();
                        if let Some(del_target) = rhs_trimmed.strip_prefix("delete ")
                            && let Some(removed) = self.try_apply_delete_target(
                                project,
                                &scope_path,
                                del_target.trim(),
                            )
                        {
                            if !name.is_empty() {
                                self.vars.insert(
                                    (scope_path.clone(), name.to_owned()),
                                    PreviewValue::Bool(removed),
                                );
                            }
                            continue;
                        }
                        if !self.is_supported_rhs(rhs_trimmed) {
                            let snippet = truncate_snippet(stmt, 56);
                            self.record_diagnostic(
                                origin,
                                line,
                                &format!(
                                    "statement `{snippet}` is outside the bounded native stage preview subset"
                                ),
                            );
                        }
                        if !name.is_empty() {
                            let val = if self.is_supported_rhs(rhs_trimmed) {
                                self.eval_preview_value(project, &scope_path, rhs_trimmed)
                            } else {
                                PreviewValue::Undefined
                            };
                            self.vars.insert((scope_path.clone(), name.to_owned()), val);
                        }
                    } else {
                        let name = decl.split(':').next().unwrap_or(decl).trim();
                        if !name.is_empty() {
                            self.vars
                                .entry((scope_path.clone(), name.to_owned()))
                                .or_insert(PreviewValue::Undefined);
                        }
                    }
                }
                continue;
            }
            // 2. trace(...)
            if let Some(inner) = extract_call_arg(stmt, "trace") {
                let args = split_top_level_comma(inner);
                let mut rendered = Vec::with_capacity(args.len());
                for a in args {
                    if !self.is_supported_rhs(a) {
                        let snippet = truncate_snippet(a, 56);
                        self.record_diagnostic(
                            origin,
                            line,
                            &format!(
                                "expression `{snippet}` is outside the bounded native stage preview subset"
                            ),
                        );
                    }
                    rendered.push(self.eval_preview_expr(project, path, a));
                }
                self.traces.push(rendered.join(" "));
                continue;
            }
            // 2b. AS1/AS2 `set(path, value)` named assignment
            if let Some(inner) = extract_call_arg(stmt, "set")
                && !local_fns.contains_key("set")
            {
                if self.current_language == ScriptLanguage::ActionScript3 {
                    self.record_diagnostic(
                        origin,
                        line,
                        "ActionScript 3.0 removed set(); use property assignment instead",
                    );
                    continue;
                }
                if matches!(
                    self.current_language,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) {
                    let args = split_top_level_comma(inner);
                    if args.len() == 2
                        && self.is_supported_rhs(args[0])
                        && self.is_supported_rhs(args[1])
                    {
                        let scope_path = self.effective_scope_path(project, path);
                        let path_val = self.eval_preview_value(project, &scope_path, args[0]);
                        let path_str = path_val.to_es_string_for(self.current_language);
                        let rhs_val = self.eval_preview_value(project, &scope_path, args[1]);
                        if self.apply_eval_path_write(project, &scope_path, &path_str, rhs_val) {
                            continue;
                        }
                    }
                }
            }
            // 3. Timeline control functions (bare or receiver-qualified: `stop()`, `this.stop()`, `mc.gotoAndStop(2)`, `_root.play()`)
            if self.try_execute_timeline_call(project, path, stmt, origin, line) {
                continue;
            }
            // 4. Local named function call `helper()`
            if let Some((fn_name, arg_str)) = parse_simple_call(stmt)
                && arg_str.is_empty()
                && let Some((fn_line, fn_body)) = local_fns.get(fn_name)
            {
                self.execute_block(
                    project,
                    path,
                    current_event,
                    fn_body,
                    origin,
                    *fn_line,
                    local_fns,
                );
                continue;
            }
            // 4b. Container mutating method call (`arr.push(x, y)`)
            if self.try_apply_container_method_call(project, path, stmt) {
                continue;
            }
            // 5. Property or variable assignments on `this.`, named child, `_root.`, or bare `_prop` / `var`
            if self.try_apply_property_or_var_mutation(project, path, stmt) {
                continue;
            }
            // 6. Unrecognized construct -> emit deduplicated located diagnostic rather than silently dropping!
            let snippet = truncate_snippet(stmt, 56);
            self.record_diagnostic(
                origin,
                line,
                &format!(
                    "statement `{snippet}` is outside the bounded native stage preview subset"
                ),
            );
        }
    }

    pub(super) fn try_register_assigned_callback(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
        origin: &str,
        line: usize,
        local_fns: &HashMap<String, (usize, String)>,
    ) -> Option<bool> {
        let (lhs, rhs) = split_top_level_assign(stmt)?;
        let (recv, prop) = match split_receiver_member(lhs) {
            Some((r, p)) => (Some(r), p),
            None => (None, lhs.trim()),
        };
        let ev = callback_prop_to_event(prop)?;
        let Some(target_path) = self.resolve_receiver_path(project, path, recv.unwrap_or("this"))
        else {
            self.record_diagnostic(
                origin,
                line,
                &format!(
                    "cannot resolve instance `{}` for `.{prop}` in native preview",
                    recv.unwrap_or("this")
                ),
            );
            return Some(true);
        };
        let rhs_trimmed = rhs.trim();
        if matches!(rhs_trimmed, "null" | "undefined") {
            self.clear_registered_handler(&target_path, ev);
            return Some(true);
        }
        if let Some(body) = extract_function_literal_body(rhs_trimmed) {
            self.register_handler(
                target_path,
                ev,
                body.to_owned(),
                origin.to_owned(),
                line,
                true,
            );
            return Some(true);
        }
        if let Some((fn_line, fn_body)) = local_fns.get(rhs_trimmed) {
            self.register_handler(
                target_path,
                ev,
                fn_body.clone(),
                origin.to_owned(),
                *fn_line,
                true,
            );
            return Some(true);
        }
        None
    }

    pub(super) fn try_register_event_listener(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
        origin: &str,
        line: usize,
        local_fns: &HashMap<String, (usize, String)>,
    ) -> Option<bool> {
        let (recv, is_remove, args_raw) =
            if let Some(inner) = extract_call_arg(stmt, "addEventListener") {
                ("this", false, inner)
            } else if let Some(inner) = extract_call_arg(stmt, "removeEventListener") {
                ("this", true, inner)
            } else if let Some((call_head, call_args)) = parse_qualified_call(stmt) {
                if let Some(r) = call_head.strip_suffix(".addEventListener") {
                    (r.trim(), false, call_args)
                } else {
                    let r = call_head.strip_suffix(".removeEventListener")?;
                    (r.trim(), true, call_args)
                }
            } else {
                return None;
            };

        let args = split_top_level_comma(args_raw);
        if args.len() < 2 {
            return None;
        }
        let event_expr = args[0].trim();
        let listener_expr = args[1].trim();
        let ev = match event_expr {
            "\"click\""
            | "'click'"
            | "MouseEvent.CLICK"
            | "flash.events.MouseEvent.CLICK"
            | "\"mouseDown\""
            | "'mouseDown'"
            | "MouseEvent.MOUSE_DOWN" => RegisteredEvent::Click,
            "\"enterFrame\""
            | "'enterFrame'"
            | "Event.ENTER_FRAME"
            | "flash.events.Event.ENTER_FRAME" => RegisteredEvent::EnterFrame,
            other => {
                self.record_diagnostic(
                    origin,
                    line,
                    &format!("event listener for `{other}` requires Test Movie"),
                );
                return Some(true);
            }
        };
        let Some(target_path) = self.resolve_receiver_path(project, path, recv) else {
            self.record_diagnostic(
                origin,
                line,
                &format!("cannot resolve listener target `{recv}` in native preview"),
            );
            return Some(true);
        };
        if is_remove {
            self.clear_registered_handler(&target_path, ev);
            return Some(true);
        }
        if let Some(body) = extract_function_literal_body(listener_expr) {
            self.register_handler(
                target_path,
                ev,
                body.to_owned(),
                origin.to_owned(),
                line,
                false,
            );
            return Some(true);
        }
        if let Some((fn_line, fn_body)) = local_fns.get(listener_expr) {
            self.register_handler(
                target_path,
                ev,
                fn_body.clone(),
                origin.to_owned(),
                *fn_line,
                false,
            );
            return Some(true);
        }
        self.record_diagnostic(
            origin,
            line,
            &format!("listener `{listener_expr}` cannot be resolved in native preview"),
        );
        Some(true)
    }

    pub(super) fn try_execute_timeline_call(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
        origin: &str,
        line: usize,
    ) -> bool {
        if extract_call_arg(stmt, "stopAllSounds") == Some("")
            || extract_call_arg(stmt, "SoundMixer.stopAll") == Some("")
        {
            self.player.stop_all_sounds();
            return true;
        }
        let Some((head, arg)) = parse_qualified_call(stmt) else {
            return false;
        };
        let (recv_opt, method) = match split_receiver_member(head) {
            Some((r, m)) => (Some(r), m),
            None => (None, head),
        };
        if !matches!(
            method,
            "stop"
                | "play"
                | "nextFrame"
                | "prevFrame"
                | "nextScene"
                | "prevScene"
                | "gotoAndStop"
                | "gotoAndPlay"
        ) {
            return false;
        }
        let Some(target_path) =
            self.resolve_receiver_path(project, path, recv_opt.unwrap_or("this"))
        else {
            self.record_diagnostic(
                origin,
                line,
                &format!(
                    "cannot resolve timeline target `{}` in native preview",
                    recv_opt.unwrap_or("this")
                ),
            );
            return true;
        };
        // Classic Flash: unqualified `stop()` / `gotoAndPlay()` inside a Button's `on(release)` targets its parent timeline!
        let effective_path = if recv_opt.is_none() && self.is_button_instance(project, &target_path)
        {
            if target_path.is_empty() {
                Vec::new()
            } else {
                target_path[..target_path.len() - 1].to_vec()
            }
        } else {
            target_path
        };

        match method {
            "stop" if arg.is_empty() => {
                self.player
                    .control(project, &effective_path, PlaybackCommand::Stop);
                if effective_path.is_empty() {
                    self.script_halted_root = true;
                }
                true
            }
            "play" if arg.is_empty() => {
                self.player
                    .control(project, &effective_path, PlaybackCommand::Play);
                if effective_path.is_empty() {
                    self.script_halted_root = false;
                }
                true
            }
            "nextFrame" if arg.is_empty() => {
                let cur = self
                    .player
                    .clip_frame(&effective_path)
                    .unwrap_or_else(|| self.player.frame());
                self.player.control(
                    project,
                    &effective_path,
                    PlaybackCommand::GotoAndStop(cur + 1),
                );
                if effective_path.is_empty() {
                    self.script_halted_root = true;
                }
                true
            }
            "prevFrame" if arg.is_empty() => {
                let cur = self
                    .player
                    .clip_frame(&effective_path)
                    .unwrap_or_else(|| self.player.frame());
                self.player.control(
                    project,
                    &effective_path,
                    PlaybackCommand::GotoAndStop(cur.saturating_sub(1)),
                );
                if effective_path.is_empty() {
                    self.script_halted_root = true;
                }
                true
            }
            "nextScene" if arg.is_empty() => {
                if effective_path.is_empty() {
                    self.apply_scene_step(project, 1);
                }
                true
            }
            "prevScene" if arg.is_empty() => {
                if effective_path.is_empty() {
                    self.apply_scene_step(project, -1);
                }
                true
            }
            "gotoAndStop" => {
                self.apply_goto(project, path, &effective_path, arg, false);
                true
            }
            "gotoAndPlay" => {
                self.apply_goto(project, path, &effective_path, arg, true);
                true
            }
            _ => false,
        }
    }

    pub(super) fn is_button_instance(&self, project: &Project, path: &[Id]) -> bool {
        if path.is_empty() {
            return self.button_root;
        }
        self.player
            .previous
            .get(path)
            .and_then(|n| {
                n.symbol_kind.or_else(|| {
                    n.symbol_id
                        .as_deref()
                        .and_then(|id| project.symbol(id))
                        .map(|s| s.kind)
                })
            })
            .is_some_and(|kind| kind == SymbolKind::Button)
    }

    /// Classic Flash (`T-240`): `on(...)` on a `Button` executes in its parent MovieClip's timeline
    /// scope, whereas `on(...)` and `onClipEvent(...)` on a `MovieClip` execute in that MovieClip's
    /// own instance scope across frames.
    pub(super) fn effective_scope_path(&self, project: &Project, path: &[Id]) -> Vec<Id> {
        if !path.is_empty() && self.is_button_instance(project, path) {
            path[..path.len() - 1].to_vec()
        } else {
            path.to_vec()
        }
    }

    pub(super) fn resolve_receiver_path(
        &self,
        project: &Project,
        current_path: &[Id],
        recv: &str,
    ) -> Option<Vec<Id>> {
        let trimmed = recv.trim();
        if trimmed.is_empty() || trimmed == "this" {
            return Some(current_path.to_vec());
        }
        if matches!(trimmed, "_root" | "root" | "_level0" | "stage" | "Stage") {
            return Some(Vec::new());
        }
        if matches!(trimmed, "_parent" | "parent") {
            if current_path.is_empty() {
                return Some(Vec::new());
            }
            return Some(current_path[..current_path.len() - 1].to_vec());
        }
        let mut base: Vec<Id> = current_path.to_vec();
        let segments: Vec<&str> = trimmed.split('.').collect();
        let mut idx = 0;
        if segments[0] == "this" {
            idx = 1;
        } else if matches!(segments[0], "_root" | "root" | "_level0") {
            base.clear();
            idx = 1;
        } else if matches!(segments[0], "_parent" | "parent") {
            if !base.is_empty() {
                base.pop();
            }
            idx = 1;
        }
        while idx < segments.len() {
            let seg = segments[idx].trim();
            if matches!(seg, "_parent" | "parent") {
                if !base.is_empty() {
                    base.pop();
                }
            } else if seg != "this" {
                let child_id = self.find_named_child_id(project, &base, seg)?;
                base.push(child_id);
            }
            idx += 1;
        }
        Some(base)
    }

    pub(super) fn find_named_child_id(
        &self,
        project: &Project,
        parent_path: &[Id],
        instance_name: &str,
    ) -> Option<Id> {
        let sym_id = if parent_path.is_empty() {
            Some(self.symbol.as_str())
        } else {
            self.player
                .previous
                .get(parent_path)
                .and_then(|n| n.symbol_id.as_deref())
        }?;
        let sym = project.symbol(sym_id)?;
        let cur_frame = self
            .player
            .clip_frame(parent_path)
            .unwrap_or_else(|| self.player.frame());
        // Check elements sampled at the active frame first, then any element in the symbol timeline.
        for layer in sym.timeline.layers.iter().rev() {
            for el in crate::evaluate::sample_layer_in_at_fps(
                &sym.timeline,
                layer,
                cur_frame,
                f64::from(project.fps.max(1)),
            )
            .into_iter()
            .rev()
            {
                if matches!(el.kind, ElementKind::Instance { .. })
                    && (el.name == instance_name || el.id == instance_name)
                {
                    let mut candidate = parent_path.to_vec();
                    candidate.push(el.id.clone());
                    if self.player.previous.contains_key(&candidate) {
                        return Some(el.id);
                    }
                }
            }
        }
        None
    }

    pub(super) fn dispatch_switched_scene_scripts(&mut self, project: &Project) {
        if self.live_scripts && self.call_depth < MAX_PREVIEW_CALL_DEPTH && self.steps_remaining > 0
        {
            self.call_depth += 1;
            self.process_scripts(project);
            self.call_depth -= 1;
        } else {
            self.player.drain_scripts();
            self.player.runtime_scripts.clear();
        }
    }

    pub(super) fn apply_scene_step(&mut self, project: &Project, direction: i32) {
        let scenes = project.scene_ids();
        if scenes.len() > 1 {
            if let Some(idx) = scenes.iter().position(|id| id == &self.symbol) {
                let target_idx = if direction >= 0 {
                    idx.checked_add(1).filter(|&i| i < scenes.len())
                } else {
                    idx.checked_sub(1)
                };
                if let Some(next_idx) = target_idx {
                    let next_sym = scenes[next_idx].clone();
                    self.switch_root_scene(project, next_sym, 0, false);
                    self.dispatch_switched_scene_scripts(project);
                }
            }
            return;
        }
        if let Some(target_frame) = crate::scenes::scene_step(
            self.player.scenes(),
            self.player.frame(),
            if direction >= 0 { 1 } else { -1 },
        ) {
            self.player
                .control(project, &[], PlaybackCommand::GotoAndStop(target_frame));
            self.script_halted_root = true;
        }
    }

    pub(super) fn apply_goto(
        &mut self,
        project: &Project,
        scope_path: &[Id],
        effective_path: &[Id],
        arg: &str,
        play: bool,
    ) {
        let parts = split_top_level_comma(arg);
        if parts.is_empty() {
            return;
        }

        // Root timeline supports multi-scene navigation:
        // - AS1/AS2: `gotoAndPlay("Scene 2", 1)` (`(scene, frame)`)
        // - AS3/JS/TS: `gotoAndPlay(1, "Scene 2")` (`(frame, scene)`)
        // - Single-arg label: `gotoAndStop("intro_end")` across all project scenes
        if effective_path.is_empty() {
            let is_as2_order = matches!(
                self.current_language,
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
            );
            let (frame_val, explicit_scene): (String, Option<String>) = if parts.len() >= 2 {
                let first = self.eval_preview_expr(project, scope_path, parts[0]);
                let second = self.eval_preview_expr(project, scope_path, parts[1]);
                let clean_scene = |raw: String| -> Option<String> {
                    let trimmed = raw.trim();
                    if trimmed.is_empty() || trimmed == "null" || trimmed == "undefined" {
                        None
                    } else {
                        Some(trimmed.to_owned())
                    }
                };
                if is_as2_order {
                    // AS2 canonical order is `(scene, frame)`, but if arg 0 is numeric and arg 1 is
                    // a non-numeric scene name, also accept `(frame, scene)` cleanly.
                    if first.trim().parse::<f64>().is_ok() && second.trim().parse::<f64>().is_err()
                    {
                        (first, clean_scene(second))
                    } else {
                        (second, clean_scene(first))
                    }
                } else {
                    (first, clean_scene(second))
                }
            } else {
                (self.eval_preview_expr(project, scope_path, parts[0]), None)
            };

            let scenes = project.scene_ids();
            if scenes.len() > 1 {
                if let Some(scene_name) = explicit_scene {
                    let Some(target_sym_id) = scenes.iter().find(|sid| {
                        project
                            .symbol(sid)
                            .is_some_and(|s| s.name == scene_name || s.id == scene_name)
                    }) else {
                        return;
                    };
                    if let Some(target_frame) =
                        Self::resolve_frame_in_symbol(project, target_sym_id, &frame_val)
                    {
                        if target_sym_id == &self.symbol {
                            let cmd = if play {
                                PlaybackCommand::GotoAndPlay(target_frame)
                            } else {
                                PlaybackCommand::GotoAndStop(target_frame)
                            };
                            self.player.control(project, &[], cmd);
                            self.script_halted_root = !play;
                        } else {
                            let target_sym = target_sym_id.clone();
                            self.switch_root_scene(project, target_sym, target_frame, play);
                            self.dispatch_switched_scene_scripts(project);
                        }
                    }
                    return;
                }

                // Single-arg on multi-scene project: check current scene first, then cross-scene labels.
                if let Some(local_frame) =
                    Self::resolve_frame_in_symbol(project, &self.symbol, &frame_val)
                {
                    let cmd = if play {
                        PlaybackCommand::GotoAndPlay(local_frame)
                    } else {
                        PlaybackCommand::GotoAndStop(local_frame)
                    };
                    self.player.control(project, &[], cmd);
                    self.script_halted_root = !play;
                    return;
                }
                let label = frame_val.trim();
                if !label.is_empty() && label.parse::<f64>().is_err() {
                    for sid in &scenes {
                        if sid == &self.symbol {
                            continue;
                        }
                        if let Some(found_frame) = Self::find_label_in_symbol(project, sid, label) {
                            self.switch_root_scene(project, sid.clone(), found_frame, play);
                            self.dispatch_switched_scene_scripts(project);
                            return;
                        }
                    }
                }
                return;
            }

            // Flattened `SceneRange` fallback when `player.scenes()` is populated.
            if !self.player.scenes().is_empty()
                && let Some(root_sym) = project.symbol(&self.symbol)
            {
                let labels = root_sym.timeline.navigable_labels();
                let frame = match frame_val.trim().parse::<f64>() {
                    Ok(num) => crate::scenes::GotoFrame::Number(num),
                    Err(_) => crate::scenes::GotoFrame::Label(frame_val.trim().to_owned()),
                };
                let request = crate::scenes::GotoRequest {
                    frame,
                    scene: explicit_scene,
                    as2: is_as2_order,
                };
                if let Ok(Some(target_frame)) = crate::scenes::resolve_goto(
                    self.player.scenes(),
                    &labels,
                    self.player.frame(),
                    &request,
                ) {
                    let cmd = if play {
                        PlaybackCommand::GotoAndPlay(target_frame)
                    } else {
                        PlaybackCommand::GotoAndStop(target_frame)
                    };
                    self.player.control(project, &[], cmd);
                    self.script_halted_root = !play;
                }
                return;
            }

            if let Some(target_frame) =
                Self::resolve_frame_in_symbol(project, &self.symbol, &frame_val)
            {
                let cmd = if play {
                    PlaybackCommand::GotoAndPlay(target_frame)
                } else {
                    PlaybackCommand::GotoAndStop(target_frame)
                };
                self.player.control(project, &[], cmd);
                self.script_halted_root = !play;
            }
            return;
        }

        // Child MovieClip target (`effective_path` non-empty):
        let evaluated = self.eval_preview_expr(project, scope_path, parts[0]);
        let Some(symbol_id) = self
            .player
            .previous
            .get(effective_path)
            .and_then(|n| n.symbol_id.as_deref())
        else {
            return;
        };
        if let Some(target_frame) = Self::resolve_frame_in_symbol(project, symbol_id, &evaluated) {
            let cmd = if play {
                PlaybackCommand::GotoAndPlay(target_frame)
            } else {
                PlaybackCommand::GotoAndStop(target_frame)
            };
            self.player.control(project, effective_path, cmd);
        }
    }

    pub(super) fn find_label_in_symbol(
        project: &Project,
        symbol_id: &str,
        label: &str,
    ) -> Option<u32> {
        let symbol = project.symbol(symbol_id)?;
        symbol
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .find(|k| k.is_navigable_label() && k.label.as_str() == label)
            .map(|k| k.frame)
    }

    pub(super) fn resolve_frame_in_symbol(
        project: &Project,
        symbol_id: &str,
        evaluated: &str,
    ) -> Option<u32> {
        let trimmed = evaluated.trim();
        if let Ok(one_based) = trimmed.parse::<f64>() {
            if !one_based.is_finite() {
                return None;
            }
            let total = project
                .symbol(symbol_id)
                .map_or(1, |s| s.timeline.frame_count.max(1));
            let idx = (one_based.round() as i64 - 1).clamp(0, i64::from(total - 1)) as u32;
            return Some(idx);
        }
        Self::find_label_in_symbol(project, symbol_id, trimmed)
    }

    pub(super) fn try_apply_property_or_var_mutation(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
    ) -> bool {
        let scope_path = self.effective_scope_path(project, path);
        // Postfix and prefix increment / decrement shorthand: `x++`, `++x`, `this._x++`, `--count`
        if let Some(lhs) = stmt.strip_suffix("++").or_else(|| stmt.strip_prefix("++")) {
            let rewritten = format!("{} += 1", lhs.trim());
            return self.try_apply_property_or_var_mutation(project, &scope_path, &rewritten);
        }
        if let Some(lhs) = stmt.strip_suffix("--").or_else(|| stmt.strip_prefix("--")) {
            let rewritten = format!("{} -= 1", lhs.trim());
            return self.try_apply_property_or_var_mutation(project, &scope_path, &rewritten);
        }

        let (lhs, op, rhs) = if let Some((l, r)) = split_compound_op(stmt, ">>>=") {
            (l, ">>>=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "<<=") {
            (l, "<<=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, ">>=") {
            (l, ">>=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "&=") {
            (l, "&=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "|=") {
            (l, "|=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "^=") {
            (l, "^=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "+=") {
            (l, "+=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "-=") {
            (l, "-=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "*=") {
            (l, "*=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "/=") {
            (l, "/=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "%=") {
            (l, "%=", r)
        } else if let Some((l, r)) = split_top_level_assign(stmt) {
            (l, "=", r)
        } else {
            return false;
        };

        let (recv_opt, prop_raw) = match split_receiver_member(lhs) {
            Some((r, p)) => (Some(r), p.trim()),
            None => (None, lhs.trim()),
        };

        let is_as1 = self.is_as1();
        let lang = self.current_language;

        if op == "="
            && let Some(del_target) = rhs.trim().strip_prefix("delete ")
            && let Some(removed) =
                self.try_apply_delete_target(project, &scope_path, del_target.trim())
        {
            let bool_rhs = if removed { "true" } else { "false" };
            let rewritten = format!("{} = {bool_rhs}", lhs.trim());
            return self.try_apply_property_or_var_mutation(project, &scope_path, &rewritten);
        }

        // Check if mutating a nested array index or object property (`fig[1][2] = 9`, `state.score += 10`, `grid[r][c] |= 1`)
        if self.try_apply_nested_container_mutation(project, &scope_path, lhs, op, rhs, lang) {
            return true;
        }

        // Check if mutating an existing script variable (`score += 10`, `mask |= 4`, `count = 2`)
        if recv_opt.is_none()
            && self
                .vars
                .contains_key(&(scope_path.clone(), prop_raw.to_owned()))
        {
            if !self.is_supported_rhs(rhs) {
                return false;
            }
            let cur_val = self
                .vars
                .get(&(scope_path.clone(), prop_raw.to_owned()))
                .cloned()
                .unwrap_or(PreviewValue::Undefined);
            let rhs_val = self.eval_preview_value(project, &scope_path, rhs);
            let Some(next_val) = apply_compound_var_op(op, &cur_val, &rhs_val, lang) else {
                return false;
            };
            self.vars
                .insert((scope_path, prop_raw.to_owned()), next_val);
            return true;
        }

        let Some(target_path) =
            self.resolve_receiver_path(project, &scope_path, recv_opt.unwrap_or("this"))
        else {
            return false;
        };

        if matches!(prop_raw, "_visible" | "visible") && op == "=" {
            let val = self.eval_preview_value(project, &scope_path, rhs);
            let visible = val.is_truthy(is_as1);
            self.player.set_visible(&target_path, visible);
            return true;
        }
        let (prop_name, scale_factor) = match prop_raw {
            "x" | "_x" => ("x", 1.0),
            "y" | "_y" => ("y", 1.0),
            "rotation" | "_rotation" => ("rotation", 1.0),
            "scaleX" | "scale_x" => ("scale_x", 1.0),
            "_xscale" => ("scale_x", 0.01),
            "scaleY" | "scale_y" => ("scale_y", 1.0),
            "_yscale" => ("scale_y", 0.01),
            "alpha" => ("alpha", 1.0),
            "_alpha" => ("alpha", 0.01),
            _ => {
                // Allow dynamic custom field/variable storage on target MovieClip scope
                // (`this.vy = 4`, `vy = 4`, `hero.hp += 10`, `flags |= 2`)
                if is_valid_identifier(prop_raw) && self.is_supported_rhs(rhs) {
                    let cur_val = self
                        .vars
                        .get(&(target_path.clone(), prop_raw.to_owned()))
                        .cloned()
                        .unwrap_or(PreviewValue::Undefined);
                    let rhs_val = self.eval_preview_value(project, &scope_path, rhs);
                    let Some(next_val) = apply_compound_var_op(op, &cur_val, &rhs_val, lang) else {
                        return false;
                    };
                    self.vars
                        .insert((target_path, prop_raw.to_owned()), next_val);
                    return true;
                }
                return false;
            }
        };
        let rhs_eval = self.eval_preview_value(project, &scope_path, rhs);
        let rhs_num = rhs_eval.to_compound_lhs_number(is_as1);
        let cur = self
            .player
            .property(project, &target_path, prop_name)
            .unwrap_or(match prop_name {
                "scale_x" | "scale_y" | "alpha" => 1.0,
                _ => 0.0,
            })
            / scale_factor;
        let cur_pv = PreviewValue::Number(cur);
        let next = match op {
            "+=" => cur + rhs_num,
            "-=" => cur - rhs_num,
            "*=" => cur * rhs_num,
            "/=" => {
                if rhs_num.abs() < 1e-12 {
                    cur
                } else {
                    cur / rhs_num
                }
            }
            "%=" => {
                if rhs_num.abs() < 1e-12 {
                    0.0
                } else {
                    cur % rhs_num
                }
            }
            "&=" => f64::from(cur_pv.to_i32(is_as1) & rhs_eval.to_i32(is_as1)),
            "|=" => f64::from(cur_pv.to_i32(is_as1) | rhs_eval.to_i32(is_as1)),
            "^=" => f64::from(cur_pv.to_i32(is_as1) ^ rhs_eval.to_i32(is_as1)),
            "<<=" => f64::from(
                cur_pv
                    .to_i32(is_as1)
                    .wrapping_shl(rhs_eval.to_u32(is_as1) & 0x1f),
            ),
            ">>=" => f64::from(
                cur_pv
                    .to_i32(is_as1)
                    .wrapping_shr(rhs_eval.to_u32(is_as1) & 0x1f),
            ),
            ">>>=" => f64::from(
                cur_pv
                    .to_u32(is_as1)
                    .wrapping_shr(rhs_eval.to_u32(is_as1) & 0x1f),
            ),
            "=" => rhs_num,
            _ => return false,
        } * scale_factor;
        let next = if prop_name == "alpha" {
            next.clamp(0.0, 1.0)
        } else {
            next
        };
        self.player.set_property(&target_path, prop_name, next);
        true
    }
}
