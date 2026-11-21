//! AVM1 `DefineButton2` (`BUTTONCONDACTION`) and `PlaceObject2`/`PlaceObject3` (`CLIPACTIONS`) encoders.

use super::*;

pub(super) fn button_mouse_cond_mask(events: &[String]) -> Result<(u8, u8), String> {
    let mut b0 = 0u8;
    let mut b1 = 0u8;
    for ev in events {
        if let Some(rest) = ev.strip_prefix("keyPress") {
            let quoted = rest
                .trim()
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .ok_or_else(|| format!("Invalid button keyPress handler 'on({ev})'"))?;
            let kc = parse_clip_event_key_code(quoted)
                .ok_or_else(|| format!("Unsupported button keyPress key '{quoted}'"))?;
            if (b1 & 0xFE) != 0 {
                return Err(
                    "DefineButton2 BUTTONCONDACTION supports at most one keyPress key per handler"
                        .to_owned(),
                );
            }
            b1 |= (kc & 0x7F) << 1;
            continue;
        }
        match ev.as_str() {
            "release" => b0 |= 0x08,        // OverDownToOverUp
            "press" => b0 |= 0x04,          // OverUpToOverDown
            "rollOver" => b0 |= 0x01,       // IdleToOverUp
            "rollOut" => b0 |= 0x02,        // OverUpToIdle
            "dragOut" => b0 |= 0x10,        // OverDownToOutDown
            "dragOver" => b0 |= 0x20,       // OutDownToOverDown
            "releaseOutside" => b0 |= 0x40, // OutDownToIdle
            other => {
                return Err(format!(
                    "Button handler 'on({other})' is not supported by this SWF DefineButton2 writer"
                ));
            }
        }
    }
    if b0 == 0 && b1 == 0 {
        b0 = 0x08;
    }
    Ok((b0, b1))
}

pub(super) fn validate_button_script_syntax(
    project: &Project,
    os: &ObjectScript,
) -> Result<(), String> {
    if os.event != ObjectEvent::Click {
        return Err(format!(
            "Button instance does not support {:?} object events; only Click / 'on(...)' button handlers are valid in DefineButton2",
            os.event
        ));
    }
    let expanded = expand_swf_includes(project, os.script.language, "", &os.script.source)?;
    let stmts = parse_swf_statements_strict(&expanded, false)?;
    for stmt in &stmts {
        if let SwfStmt::EventHandler {
            is_clip_event,
            events,
            ..
        } = stmt
        {
            if *is_clip_event {
                return Err(
                    "Button instances only support 'on(...)' mouse handlers, not 'onClipEvent(...)'"
                        .to_owned(),
                );
            }
            let _ = button_mouse_cond_mask(events)?;
        } else if find_event_handler_in_stmts(std::slice::from_ref(stmt)).is_some() {
            return Err(
                "Nested 'on(...)' / 'onClipEvent(...)' handlers are not supported inside statements"
                    .to_owned(),
            );
        }
    }
    Ok(())
}

/// Encode `BUTTONCONDACTION` records for a `DefineButton2` (tag 34).
pub(super) fn encode_button2_cond_actions(
    project: &Project,
    scripts: &[ObjectScript],
) -> Result<Vec<u8>, String> {
    let mut valid: Vec<(u8, u8, Vec<u8>)> = Vec::new();
    for os in scripts {
        if !matches!(
            os.script.language,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        ) {
            continue;
        }
        if os.event != ObjectEvent::Click {
            return Err(format!(
                "Button instance does not support {:?} object events; only Click / 'on(...)' handlers are valid in DefineButton2",
                os.event
            ));
        }
        let expanded = expand_swf_includes(project, os.script.language, "", &os.script.source)?;
        let stmts = parse_swf_statements_strict(&expanded, false)?;
        let mut top_stmts: Vec<SwfStmt> = Vec::new();
        for stmt in stmts {
            if let SwfStmt::EventHandler {
                is_clip_event,
                events,
                body,
            } = stmt
            {
                if is_clip_event {
                    return Err(
                        "Button instances only support 'on(...)' mouse/key handlers, not 'onClipEvent(...)'"
                            .to_owned(),
                    );
                }
                let (b0, b1) = button_mouse_cond_mask(&events)?;
                let mut h_bytes = Vec::new();
                avm1_compile_stmt_list(&mut h_bytes, &body, false, &HashMap::new())?;
                h_bytes.push(0x00);
                if h_bytes.len() > 1 {
                    valid.push((b0, b1, h_bytes));
                }
            } else {
                top_stmts.push(stmt);
            }
        }
        if !top_stmts.is_empty() {
            let mut t_bytes = Vec::new();
            avm1_compile_stmt_list(&mut t_bytes, &top_stmts, false, &HashMap::new())?;
            t_bytes.push(0x00);
            if t_bytes.len() > 1 {
                valid.push((0x08, 0x00, t_bytes));
            }
        }
    }
    let mut out = Vec::new();
    let count = valid.len();
    for (idx, (b0, b1, actions)) in valid.into_iter().enumerate() {
        let cond_size: u16 = if idx + 1 == count {
            0
        } else {
            actions
                .len()
                .checked_add(4)
                .and_then(|len| u16::try_from(len).ok())
                .ok_or_else(|| {
                    format!(
                        "AVM1 button conditional action encodes {} bytes; non-final BUTTONCONDACTION records support at most {} action bytes",
                        actions.len(),
                        usize::from(u16::MAX) - 4
                    )
                })?
        };
        out.extend_from_slice(&cond_size.to_le_bytes());
        out.push(b0);
        out.push(b1);
        out.extend_from_slice(&actions);
    }
    Ok(out)
}

/// Encode `CLIPACTIONS` for a `PlaceObject2` (tag 26) or `PlaceObject3` (tag 70) in SWF 6+.
pub(super) fn split_clip_event_header_items(inner: &str) -> Option<Vec<&str>> {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut in_quotes = false;
    let mut chars = inner.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek().is_some_and(|(_, next)| *next == '"') => {}
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                let item = inner[start..index].trim();
                if item.is_empty() {
                    return None;
                }
                items.push(item);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    if in_quotes {
        return None;
    }
    let tail = inner[start..].trim();
    if tail.is_empty() {
        return None;
    }
    items.push(tail);
    Some(items)
}

pub(super) fn parse_clip_event_key_code(raw: &str) -> Option<u8> {
    match raw {
        "<Left>" => Some(1),
        "<Right>" => Some(2),
        "<Home>" => Some(3),
        "<End>" => Some(4),
        "<Insert>" => Some(5),
        "<Delete>" => Some(6),
        "<Backspace>" => Some(8),
        "<Enter>" => Some(13),
        "<Up>" => Some(14),
        "<Down>" => Some(15),
        "<PageUp>" => Some(16),
        "<PageDown>" => Some(17),
        "<Tab>" => Some(18),
        "<Escape>" => Some(19),
        "<Space>" => Some(32),
        value if value.starts_with("key#") => value[4..].parse::<u8>().ok(),
        value if value.chars().count() == 1 => u8::try_from(u32::from(value.chars().next()?)).ok(),
        _ => None,
    }
}

pub(super) fn parse_clip_event_header_override(source: &str) -> Option<(u32, Option<u8>)> {
    let first_line = source.lines().next()?.trim();
    let inner = first_line
        .strip_prefix("// onClipEvent(")?
        .strip_suffix(')')?
        .trim();
    if inner.is_empty() {
        return None;
    }
    let mut flags = 0u32;
    let mut key_code = None;
    for tok in split_clip_event_header_items(inner)? {
        if let Some(rest) = tok.strip_prefix("keyPress") {
            if key_code.is_some() {
                return None;
            }
            let quoted = rest.trim().strip_prefix('"')?.strip_suffix('"')?;
            let kc = parse_clip_event_key_code(quoted)?;
            flags |= 0x0002_0000;
            key_code = Some(kc);
            continue;
        }
        let bit = match tok {
            "load" => 0x0000_0001,
            "enterFrame" => 0x0000_0002,
            "unload" => 0x0000_0004,
            "mouseMove" => 0x0000_0008,
            "mouseDown" => 0x0000_0010,
            "mouseUp" => 0x0000_0020,
            "keyDown" => 0x0000_0040,
            "keyUp" => 0x0000_0080,
            "data" => 0x0000_0100,
            "initialize" => 0x0000_0200,
            "press" => 0x0000_0400,
            "release" => 0x0000_0800,
            "releaseOutside" => 0x0000_1000,
            "rollOver" => 0x0000_2000,
            "rollOut" => 0x0000_4000,
            "dragOver" => 0x0000_8000,
            "dragOut" => 0x0001_0000,
            "construct" => 0x0004_0000,
            _ => return None,
        };
        flags |= bit;
    }
    (flags != 0).then_some((flags, key_code))
}

fn clip_event_handler_flags(
    is_clip_event: bool,
    events: &[String],
) -> Result<(u32, Option<u8>), String> {
    let mut flags = 0u32;
    let mut key_code = None;
    for ev in events {
        if let Some(rest) = ev.strip_prefix("keyPress") {
            if key_code.is_some() {
                return Err(
                    "CLIPACTIONRECORD supports at most one keyPress key per handler".to_owned(),
                );
            }
            let quoted = rest
                .trim()
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .ok_or_else(|| format!("Invalid keyPress event '{ev}'"))?;
            let kc = parse_clip_event_key_code(quoted)
                .ok_or_else(|| format!("Unsupported keyPress key '{quoted}'"))?;
            flags |= 0x0002_0000;
            key_code = Some(kc);
            continue;
        }
        let bit = if is_clip_event {
            match ev.as_str() {
                "load" | "LOAD" => 0x0000_0001,
                "enterFrame" | "ENTERFRAME" => 0x0000_0002,
                "unload" | "UNLOAD" => 0x0000_0004,
                "mouseMove" | "MOUSEMOVE" => 0x0000_0008,
                "mouseDown" | "MOUSEDOWN" => 0x0000_0010,
                "mouseUp" | "MOUSEUP" => 0x0000_0020,
                "keyDown" | "KEYDOWN" => 0x0000_0040,
                "keyUp" | "KEYUP" => 0x0000_0080,
                "data" | "DATA" => 0x0000_0100,
                "initialize" | "INITIALIZE" => 0x0000_0200,
                "construct" | "CONSTRUCT" => 0x0004_0000,
                other => {
                    return Err(format!(
                        "Unsupported MovieClip event 'onClipEvent({other})'"
                    ));
                }
            }
        } else {
            match ev.as_str() {
                "press" => 0x0000_0400,
                "release" => 0x0000_0800,
                "releaseOutside" => 0x0000_1000,
                "rollOver" => 0x0000_2000,
                "rollOut" => 0x0000_4000,
                "dragOver" => 0x0000_8000,
                "dragOut" => 0x0001_0000,
                other => {
                    return Err(format!("Unsupported MovieClip button event 'on({other})'"));
                }
            }
        };
        flags |= bit;
    }
    if flags == 0 {
        return Err("Empty MovieClip event handler list".to_owned());
    }
    Ok((flags, key_code))
}

pub(super) fn encode_clip_actions(
    project: &Project,
    scripts: &[ObjectScript],
) -> Result<Option<Vec<u8>>, String> {
    let mut records: Vec<(u32, Option<u8>, Vec<u8>)> = Vec::new();
    let mut all_flags = 0u32;
    for os in scripts {
        if !matches!(
            os.script.language,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        ) {
            continue;
        }
        let expanded = expand_swf_includes(project, os.script.language, "", &os.script.source)?;
        if let Some((flag, key)) = parse_clip_event_header_override(&os.script.source) {
            let actions = compile_avm1_actions_mode(&expanded, true)?;
            if actions.len() > 1 {
                all_flags |= flag;
                records.push((flag, key, actions));
            }
            continue;
        }
        let stmts = parse_swf_statements_strict(&expanded, false)?;
        let mut top_stmts: Vec<SwfStmt> = Vec::new();
        for stmt in stmts {
            if let SwfStmt::EventHandler {
                is_clip_event,
                events,
                body,
            } = stmt
            {
                let (flag, key) = clip_event_handler_flags(is_clip_event, &events)?;
                let mut h_bytes = Vec::new();
                avm1_compile_stmt_list(&mut h_bytes, &body, false, &HashMap::new())?;
                h_bytes.push(0x00);
                if h_bytes.len() > 1 {
                    all_flags |= flag;
                    records.push((flag, key, h_bytes));
                }
            } else {
                top_stmts.push(stmt);
            }
        }
        if !top_stmts.is_empty() {
            let mut t_bytes = Vec::new();
            avm1_compile_stmt_list(&mut t_bytes, &top_stmts, false, &HashMap::new())?;
            t_bytes.push(0x00);
            if t_bytes.len() > 1 {
                let flag = match os.event {
                    ObjectEvent::Load => 0x0000_0001,       // ClipEventLoad
                    ObjectEvent::EnterFrame => 0x0000_0002, // ClipEventEnterFrame
                    ObjectEvent::Click => 0x0000_0800,      // ClipEventRelease
                };
                all_flags |= flag;
                records.push((flag, None, t_bytes));
            }
        }
    }
    if records.is_empty() {
        return Ok(None);
    }
    let mut w = SwfBitWriter::new();
    w.u16(0); // reserved
    w.u32(all_flags);
    for (flag, key, actions) in records {
        w.u32(flag);
        let record_size = actions.len() + usize::from(key.is_some());
        w.u32(record_size as u32);
        if let Some(k) = key {
            w.u8(k);
        }
        w.bytes(&actions);
    }
    w.u32(0); // end of clip action records
    Ok(Some(w.buf))
}

pub(super) fn find_event_handler_in_expr(expr: &SwfExpr) -> Option<&SwfStmt> {
    match expr {
        SwfExpr::Function { body, .. } => find_event_handler_in_stmts(body),
        SwfExpr::ArrayLit(elems) => elems.iter().find_map(find_event_handler_in_expr),
        SwfExpr::ObjectLit(props) => props
            .iter()
            .find_map(|(_, v)| find_event_handler_in_expr(v)),
        SwfExpr::Member(obj, _) => find_event_handler_in_expr(obj),
        SwfExpr::Index(obj, idx) => {
            find_event_handler_in_expr(obj).or_else(|| find_event_handler_in_expr(idx))
        }
        SwfExpr::Unary { expr, .. } => find_event_handler_in_expr(expr),
        SwfExpr::Binary { lhs, rhs, .. } => {
            find_event_handler_in_expr(lhs).or_else(|| find_event_handler_in_expr(rhs))
        }
        SwfExpr::Call { callee, args } | SwfExpr::New { callee, args } => {
            find_event_handler_in_expr(callee)
                .or_else(|| args.iter().find_map(find_event_handler_in_expr))
        }
        SwfExpr::Delete(lv) => match lv.as_ref() {
            SwfLValue::Ident(_) => None,
            SwfLValue::Member(obj, _) => find_event_handler_in_expr(obj),
            SwfLValue::Index(obj, idx) => {
                find_event_handler_in_expr(obj).or_else(|| find_event_handler_in_expr(idx))
            }
        },
        SwfExpr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => find_event_handler_in_expr(cond)
            .or_else(|| find_event_handler_in_expr(then_expr))
            .or_else(|| find_event_handler_in_expr(else_expr)),
        SwfExpr::Null
        | SwfExpr::Undefined
        | SwfExpr::Bool(_)
        | SwfExpr::Int(_)
        | SwfExpr::Float(_)
        | SwfExpr::Str(_)
        | SwfExpr::Ident(_) => None,
    }
}

pub(super) fn find_event_handler_in_stmts(stmts: &[SwfStmt]) -> Option<&SwfStmt> {
    for stmt in stmts {
        match stmt {
            SwfStmt::EventHandler { .. } => return Some(stmt),
            SwfStmt::VarDecl { init: Some(e), .. }
            | SwfStmt::Assign { value: e, .. }
            | SwfStmt::Expr(e)
            | SwfStmt::Return(Some(e))
            | SwfStmt::Throw(e) => {
                if let Some(found) = find_event_handler_in_expr(e) {
                    return Some(found);
                }
            }
            SwfStmt::Block(body) | SwfStmt::FunctionDecl { body, .. } => {
                if let Some(found) = find_event_handler_in_stmts(body) {
                    return Some(found);
                }
            }
            SwfStmt::While { cond, body }
            | SwfStmt::ForIn {
                object: cond, body, ..
            } => {
                if let Some(found) = find_event_handler_in_expr(cond) {
                    return Some(found);
                }
                if let Some(found) = find_event_handler_in_stmts(body) {
                    return Some(found);
                }
            }
            SwfStmt::Switch {
                discriminant,
                cases,
            } => {
                if let Some(found) = find_event_handler_in_expr(discriminant) {
                    return Some(found);
                }
                for (ce, cb) in cases {
                    if let Some(e) = ce
                        && let Some(found) = find_event_handler_in_expr(e)
                    {
                        return Some(found);
                    }
                    if let Some(found) = find_event_handler_in_stmts(cb) {
                        return Some(found);
                    }
                }
            }
            SwfStmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                if let Some(found) = find_event_handler_in_stmts(try_body) {
                    return Some(found);
                }
                for (_, _, cb) in catches {
                    if let Some(found) = find_event_handler_in_stmts(cb) {
                        return Some(found);
                    }
                }
                if let Some(fb) = finally_body
                    && let Some(found) = find_event_handler_in_stmts(fb)
                {
                    return Some(found);
                }
            }
            SwfStmt::ClassDecl {
                ctor,
                methods,
                fields,
                ..
            } => {
                if let Some((_, cb)) = ctor
                    && let Some(found) = find_event_handler_in_stmts(cb)
                {
                    return Some(found);
                }
                for (_, _, _, mb) in methods {
                    if let Some(found) = find_event_handler_in_stmts(mb) {
                        return Some(found);
                    }
                }
                for (_, _, fe_opt) in fields {
                    if let Some(fe) = fe_opt
                        && let Some(found) = find_event_handler_in_expr(fe)
                    {
                        return Some(found);
                    }
                }
            }
            SwfStmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                if let Some(found) = find_event_handler_in_expr(cond) {
                    return Some(found);
                }
                if let Some(found) = find_event_handler_in_stmts(then_branch) {
                    return Some(found);
                }
                if let Some(eb) = else_branch
                    && let Some(found) = find_event_handler_in_stmts(eb)
                {
                    return Some(found);
                }
            }
            SwfStmt::For {
                init,
                cond,
                update,
                body,
            } => {
                if let Some(i) = init
                    && let Some(found) =
                        find_event_handler_in_stmts(std::slice::from_ref(i.as_ref()))
                {
                    return Some(found);
                }
                if let Some(c) = cond
                    && let Some(found) = find_event_handler_in_expr(c)
                {
                    return Some(found);
                }
                if let Some(u) = update
                    && let Some(found) =
                        find_event_handler_in_stmts(std::slice::from_ref(u.as_ref()))
                {
                    return Some(found);
                }
                if let Some(found) = find_event_handler_in_stmts(body) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}
