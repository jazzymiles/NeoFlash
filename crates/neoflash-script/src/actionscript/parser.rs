//! Recursive-descent statement, declaration, type, and function parser.

use super::*;

pub(crate) struct Frontend<'a> {
    pub(crate) script: &'a Script,
    pub(crate) tokens: TokenCursor,
    pub(crate) pos: usize,
    pub(crate) depth: usize,
    pub(crate) controls: Vec<ControlTarget>,
    pub(crate) function_depth: usize,
    pub(crate) next_function: usize,
    pub(crate) context: CompileContext,
    pub(crate) no_in: bool,
    pub(crate) with_depth: usize,
    pub(crate) in_initclip: bool,
}

impl<'a> Frontend<'a> {
    pub(crate) fn peek(&self, index: usize) -> Option<Token> {
        self.tokens.get(self.script, index)
    }
    pub(crate) fn token(&self) -> Token {
        self.peek(self.pos).expect("parser does not consume EOF")
    }
    pub(crate) fn is(&self, text: &str) -> bool {
        match &self.token().kind {
            Kind::Ident(s) => s == text,
            Kind::Punct(s) => *s == text,
            _ => false,
        }
    }
    pub(crate) fn eat(&mut self, text: &str) -> bool {
        if self.is(text) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    pub(crate) fn expect(&mut self, text: &str) -> Result<(), Diagnostic> {
        if self.eat(text) {
            Ok(())
        } else {
            Err(self.error(format!("Expected '{text}'.")))
        }
    }
    pub(crate) fn error(&self, message: impl Into<String>) -> Diagnostic {
        let token = self.token();
        if let Kind::Invalid(error) = token.kind {
            *error
        } else {
            Diagnostic::at(self.script, token.offset, message)
        }
    }
    pub(crate) fn ident(&mut self) -> Result<String, Diagnostic> {
        if let Kind::Ident(name) = &self.token().kind {
            let name = name.clone();
            self.pos += 1;
            Ok(name)
        } else {
            Err(self.error("Expected an identifier."))
        }
    }
    pub(crate) fn enter(&mut self) -> Result<(), Diagnostic> {
        self.depth += 1;
        if self.depth > 48 {
            return Err(self.error("ActionScript nesting exceeds 48 levels."));
        }
        Ok(())
    }
    pub(crate) fn semicolon(&mut self) -> Result<(), Diagnostic> {
        if self.eat(";") || self.is("}") || self.token().kind == Kind::End {
            return Ok(());
        }
        if self.line_break_before_token() {
            return Ok(());
        }
        Err(self.error("Expected ';' or a line break after the statement."))
    }
    pub(crate) fn line_break_before_token(&self) -> bool {
        self.pos > 0
            && self.script.source[self.peek(self.pos - 1).unwrap().end..self.token().offset]
                .contains(['\n', '\r', '\u{2028}', '\u{2029}'])
    }
    pub(crate) fn statements(&mut self, block: bool) -> Result<Vec<Stmt>, Diagnostic> {
        let mut statements = Vec::new();
        while self.token().kind != Kind::End && !(block && self.is("}")) {
            statements.push(self.statement()?);
        }
        if block {
            self.expect("}")?;
        }
        Ok(statements)
    }
    pub(crate) fn statement(&mut self) -> Result<Stmt, Diagnostic> {
        self.enter()?;
        let offset = self.token().offset;
        let statement = if self.eat("if") {
            self.parse_if()?
        } else if self.eat("{") {
            Stmt::Block(self.statements(true)?)
        } else if self.eat(";") {
            Stmt::Empty
        } else if matches!(self.token().kind, Kind::Ident(_))
            && self
                .peek(self.pos + 1)
                .is_some_and(|t| t.kind == Kind::Punct(":"))
        {
            self.parse_label()?
        } else {
            self.statement_inner()?
        };
        self.depth -= 1;
        Ok(Stmt::Located(offset, Box::new(statement)))
    }
    #[inline(never)]
    fn parse_label(&mut self) -> Result<Stmt, Diagnostic> {
        if self.script.language != ScriptLanguage::ActionScript3 {
            return Err(self
                .error("Labels require ActionScript 3; AS2 label semantics are not implemented."));
        }
        let name = self.ident()?;
        if invalid_binding(&name, true)
            || self
                .controls
                .iter()
                .any(|c| c.label.as_ref() == Some(&name))
        {
            return Err(self.error("Invalid or duplicate active label."));
        }
        self.expect(":")?;
        let mut at = self.pos;
        while matches!(self.peek(at).map(|t| t.kind), Some(Kind::Ident(_)))
            && self
                .peek(at + 1)
                .is_some_and(|t| t.kind == Kind::Punct(":"))
        {
            at += 2;
        }
        let iteration = matches!(self.peek(at).map(|t| t.kind), Some(Kind::Ident(s)) if matches!(s.as_str(), "for" | "while" | "do"));
        self.controls.push(ControlTarget {
            label: Some(name.clone()),
            iteration,
            breakable: false,
        });
        let body = self.statement()?;
        self.controls.pop();
        Ok(Stmt::Label(name, Box::new(body)))
    }
    #[inline(never)]
    pub(crate) fn parse_if(&mut self) -> Result<Stmt, Diagnostic> {
        let mut branches = Vec::new();
        let mut tail = None;
        let mut opened_brace = false;
        loop {
            if branches.len() >= 1024 {
                return Err(self.error("ActionScript if/else-if chain exceeds 1024 branches."));
            }
            let offset = self.peek(self.pos - 1).unwrap().offset;
            self.expect("(")?;
            let condition = self.sequence_expression()?;
            self.expect(")")?;
            let yes = Box::new(self.statement()?);
            branches.push((offset, condition, yes, opened_brace));
            opened_brace = false;
            if self.eat("else") {
                if self.eat("if") {
                    continue;
                }
                let saved_pos = self.pos;
                if self.eat("{") {
                    if self.eat("if") {
                        opened_brace = true;
                        continue;
                    }
                    self.pos = saved_pos;
                }
                tail = Some(Box::new(self.statement()?));
            }
            break;
        }
        let mut current = tail;
        for (offset, condition, yes, close_brace) in branches.into_iter().rev() {
            let mut node = Stmt::Located(offset, Box::new(Stmt::If(condition, yes, current)));
            if close_brace {
                if !self.is("}") {
                    let mut stmts = vec![node];
                    while self.token().kind != Kind::End && !self.is("}") {
                        stmts.push(self.statement()?);
                    }
                    node = Stmt::Block(stmts);
                }
                self.expect("}")?;
            }
            current = Some(Box::new(node));
        }
        Ok(*current.unwrap())
    }
    #[inline(never)]
    pub(crate) fn parse_initclip(&mut self) -> Result<Stmt, Diagnostic> {
        let offset = self.token().offset;
        if self.script.language != ScriptLanguage::ActionScript2 {
            return Err(self.error("#initclip is supported only in ActionScript 2."));
        }
        if self.function_depth != 0
            || self.context.class_name.is_some()
            || self.depth != 1
            || self.with_depth != 0
            || self.in_initclip
        {
            return Err(
                self.error("#initclip is allowed only at the top level of an AS2 frame script.")
            );
        }
        self.expect("#initclip")?;
        let order = if !self.line_break_before_token()
            && let Kind::Number(raw) = &self.token().kind
        {
            let raw = raw.clone();
            let order = raw.parse::<u32>().map_err(|_| {
                self.error("#initclip order requires a non-negative 32-bit integer.")
            })?;
            self.pos += 1;
            Some(order)
        } else {
            None
        };
        if !self.eat(";") && !self.is("#endinitclip") && !self.line_break_before_token() {
            return Err(self.error("Expected a line break or ';' after #initclip."));
        }
        self.in_initclip = true;
        let mut body = Vec::new();
        while self.token().kind != Kind::End && !self.is("#endinitclip") {
            body.push(self.statement()?);
        }
        self.in_initclip = false;
        if !self.eat("#endinitclip") {
            return Err(Diagnostic::at(
                self.script,
                offset,
                "Unterminated #initclip block; expected #endinitclip.",
            ));
        }
        let _ = self.eat(";");
        Ok(Stmt::InitClip(order, body, offset))
    }
    #[inline(never)]
    pub(crate) fn parse_tell_target(&mut self) -> Result<Stmt, Diagnostic> {
        let offset = self.token().offset;
        self.expect("tellTarget")?;
        self.expect("(")?;
        let target = self.expression(0)?;
        self.expect(")")?;
        self.with_depth += 1;
        let body = self.statement()?;
        self.with_depth -= 1;
        if !scope::tell_target_body_supported(&body) {
            return Err(self.error("Function definitions and legacy event blocks inside tellTarget require captured scope-chain semantics outside this profile."));
        }
        Ok(Stmt::TellTarget(target, Box::new(body), offset))
    }
    #[inline(never)]
    pub(crate) fn parse_if_frame_loaded(&mut self) -> Result<Stmt, Diagnostic> {
        self.expect("ifFrameLoaded")?;
        self.expect("(")?;
        if self.is(")") {
            return Err(self.error("ifFrameLoaded requires (frame) or (scene, frame)."));
        }
        let mut args = vec![self.expression(0)?];
        if self.eat(",") {
            args.push(self.expression(0)?);
        }
        if self.eat(",") {
            return Err(self.error("ifFrameLoaded accepts (frame) or (scene, frame)."));
        }
        self.expect(")")?;
        let body = self.statement()?;
        Ok(Stmt::IfFrameLoaded(args, Box::new(body)))
    }
    #[inline(never)]
    pub(crate) fn parse_as3_directive(&mut self) -> Result<Option<Stmt>, Diagnostic> {
        if self.is("default")
            && self
                .peek(self.pos + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "xml"))
            && self
                .peek(self.pos + 2)
                .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "namespace"))
        {
            self.expect("default")?;
            self.expect("xml")?;
            self.expect("namespace")?;
            self.expect("=")?;
            let expr = self.expression(0)?;
            self.semicolon()?;
            return Ok(Some(Stmt::DefaultXmlNamespace(expr)));
        }
        if self.is("use")
            && self
                .peek(self.pos + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "namespace"))
        {
            self.expect("use")?;
            self.expect("namespace")?;
            loop {
                let mut ns = self.ident()?;
                while self.eat(".") {
                    ns.push('.');
                    ns.push_str(&self.ident()?);
                }
                let uri = self
                    .context
                    .namespaces
                    .get(&ns)
                    .cloned()
                    .or_else(|| match ns.as_str() {
                        "flash_proxy" | "flash.utils.flash_proxy" => {
                            Some("http://www.adobe.com/2006/actionscript/flash/proxy".to_owned())
                        }
                        "object_proxy" | "flash.utils.object_proxy" | "mx.utils.object_proxy" => {
                            Some("http://www.adobe.com/2006/flex/mx/internal".to_owned())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| ns.clone());
                if !self.context.open_namespaces.contains(&uri) {
                    self.context.open_namespaces.push(uri);
                }
                if !self.eat(",") {
                    break;
                }
            }
            self.semicolon()?;
            return Ok(Some(Stmt::Empty));
        }
        if self.is("namespace")
            || ((self.is("public") || self.is("internal"))
                && self
                    .peek(self.pos + 1)
                    .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "namespace")))
        {
            let _ = self.eat("public") || self.eat("internal");
            let offset = self.token().offset;
            self.expect("namespace")?;
            let name = self.ident()?;
            let uri = if self.eat("=") {
                let expr = self.expression(0)?;
                let Some(s) = literal_property_name(&expr) else {
                    return Err(self.error("namespace URI requires a string literal."));
                };
                s
            } else {
                format!("neoflash://namespace/{name}")
            };
            self.semicolon()?;
            self.context.namespaces.insert(name.clone(), uri.clone());
            let uri_lit = serde_json::to_string(&uri).unwrap();
            let init = Expr::Call(
                Box::new(Expr::Name("Namespace".into())),
                vec![Expr::Literal(uri_lit)],
                offset,
            );
            return Ok(Some(Stmt::Var(vec![(name, None, Some(init))])));
        }
        {
            let mut look = self.pos;
            let mut saw_native = false;
            while let Some(t) = self.peek(look) {
                if let Kind::Ident(s) = &t.kind {
                    if s == "native" {
                        saw_native = true;
                        look += 1;
                    } else if matches!(
                        s.as_str(),
                        "public" | "internal" | "private" | "protected" | "static" | "final"
                    ) {
                        look += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            if saw_native
                && self
                    .peek(look)
                    .is_some_and(|t| matches!(&t.kind, Kind::Ident(s) if s == "function"))
            {
                self.pos = look;
                let (func, _) = self.project_function(true, false, false, true)?;
                return Ok(Some(Stmt::Function(Box::new(func))));
            }
        }
        if self.is("include")
            && self
                .peek(self.pos + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::String(_)))
        {
            return Err(self.error(
                "include requires compile_with_program and bundled .as project resources.",
            ));
        }
        Ok(None)
    }
    #[inline(never)]
    pub(crate) fn statement_inner(&mut self) -> Result<Stmt, Diagnostic> {
        if self.is("#endinitclip") {
            return Err(self.error("Unexpected #endinitclip without a matching #initclip."));
        }
        if self.is("#initclip") {
            return self.parse_initclip();
        }
        if self.script.language == ScriptLanguage::ActionScript2
            && self.is("tellTarget")
            && matches!(
                self.peek(self.pos + 1).map(|t| t.kind),
                Some(Kind::Punct("("))
            )
        {
            return self.parse_tell_target();
        }
        if self.script.language == ScriptLanguage::ActionScript2
            && self.is("ifFrameLoaded")
            && matches!(
                self.peek(self.pos + 1).map(|t| t.kind),
                Some(Kind::Punct("("))
            )
        {
            return self.parse_if_frame_loaded();
        }
        if self.is("with") {
            let offset = self.token().offset;
            self.expect("with")?;
            self.expect("(")?;
            let target = self.expression(0)?;
            self.expect(")")?;
            self.with_depth += 1;
            let body = self.statement()?;
            self.with_depth -= 1;
            let supported = if self.script.language == ScriptLanguage::ActionScript3 {
                scope::with_body_supported(&body)
            } else {
                scope::as2_with_body_supported(&body)
            };
            if !supported {
                return Err(self.error("Function definitions and eval/set calls inside with require captured/dynamic scope-chain semantics outside this profile. Define callbacks and resolve named paths outside the with block."));
            }
            return Ok(Stmt::With(target, Box::new(body), offset));
        }
        if self.script.language == ScriptLanguage::ActionScript3
            && let Some(stmt) = self.parse_as3_directive()?
        {
            return Ok(stmt);
        }
        if self.is("on") || self.is("onClipEvent") {
            if self.script.language != ScriptLanguage::ActionScript2
                || self.function_depth != 0
                || self.context.class_name.is_some()
                || self.in_initclip
            {
                return Err(self.error(
                    "Legacy on/onClipEvent blocks belong at the top level of an AS1/AS2 object script.",
                ));
            }
            let clip_event = self.eat("onClipEvent");
            if !clip_event {
                self.expect("on")?;
            }
            let offset = self.token().offset;
            self.expect("(")?;
            let mut triggers = Vec::new();
            loop {
                let name = self.ident()?;
                let Some(event) =
                    resolve_legacy_event_name(clip_event, &name, self.context.legacy_case)
                else {
                    return Err(self.error(format!(
                        "Legacy event '{name}' is not a supported {} event.",
                        if clip_event { "onClipEvent" } else { "on" }
                    )));
                };
                let key_press = if !clip_event && event == "keyPress" {
                    let Kind::String(raw) = self.token().kind.clone() else {
                        return Err(self.error(
                            "Legacy on(keyPress \"<key>\") requires a key string literal immediately after keyPress without a comma (for example on(keyPress \"<Enter>\") or on(keyPress \"a\")).",
                        ));
                    };
                    self.pos += 1;
                    let Some(decoded) = literal_property_name(&Expr::Literal(raw)) else {
                        return Err(self.error("Invalid string literal in on(keyPress)."));
                    };
                    let Some(valid_key) =
                        validate_flash_key_press(&decoded, self.context.legacy_case)
                    else {
                        return Err(self.error(format!(
                            "Invalid on(keyPress) key specifier {decoded:?}; expected a single character or a Flash key token (<Left>, <Right>, <Up>, <Down>, <Enter>, <Space>, <Tab>, <Escape>, <Backspace>, <Delete>, <Home>, <End>, <PageUp>, <PageDown>, <Insert>)."
                        )));
                    };
                    Some(valid_key)
                } else {
                    None
                };
                let trigger = LegacyTrigger {
                    clip_event,
                    event,
                    key_press,
                };
                if !triggers.contains(&trigger) {
                    triggers.push(trigger);
                }
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(")")?;
            self.expect("{")?;
            let id = self.next_function;
            self.next_function += 1;
            let outer_controls = std::mem::take(&mut self.controls);
            let outer_no_in = self.no_in;
            self.no_in = false;
            self.function_depth += 1;
            let body = self.statements(true)?;
            self.function_depth -= 1;
            self.controls = outer_controls;
            self.no_in = outer_no_in;
            let func = Function {
                id,
                offset,
                name: None,
                params: Vec::new(),
                defaults: Vec::new(),
                default_offsets: Vec::new(),
                rest: None,
                returns: None,
                body,
                declaration: false,
            };
            return Ok(Stmt::LegacyEvent(triggers, Box::new(func)));
        }
        if matches!(self.token().kind, Kind::Ident(_))
            && self
                .peek(self.pos + 1)
                .is_some_and(|t| t.kind == Kind::Punct(":"))
        {
            if self.script.language != ScriptLanguage::ActionScript3 {
                return Err(self.error(
                    "Labels require ActionScript 3; AS2 label semantics are not implemented.",
                ));
            }
            let name = self.ident()?;
            if invalid_binding(&name, true)
                || self
                    .controls
                    .iter()
                    .any(|c| c.label.as_ref() == Some(&name))
            {
                return Err(self.error("Invalid or duplicate active label."));
            }
            self.expect(":")?;
            let mut at = self.pos;
            while matches!(self.peek(at).map(|t| t.kind), Some(Kind::Ident(_)))
                && self
                    .peek(at + 1)
                    .is_some_and(|t| t.kind == Kind::Punct(":"))
            {
                at += 2;
            }
            let iteration = matches!(self.peek(at).map(|t| t.kind), Some(Kind::Ident(s)) if matches!(s.as_str(), "for" | "while" | "do"));
            self.controls.push(ControlTarget {
                label: Some(name.clone()),
                iteration,
                breakable: false,
            });
            let body = self.statement()?;
            self.controls.pop();
            return Ok(Stmt::Label(name, Box::new(body)));
        }
        if self.eat("throw") {
            if self.line_break_before_token()
                || self.is(";")
                || self.is("}")
                || self.token().kind == Kind::End
            {
                return Err(self.error("throw requires an expression on the same line."));
            }
            let value = self.sequence_expression()?;
            self.semicolon()?;
            return Ok(Stmt::Throw(value));
        }
        if self.eat("try") {
            self.expect("{")?;
            let body = self.statements(true)?;
            let mut catches = Vec::new();
            let mut catch_all = false;
            while self.eat("catch") {
                let offset = self.token().offset;
                if catch_all {
                    return Err(self.error("A catch-all clause must be last."));
                }
                self.expect("(")?;
                let name = self.ident()?;
                if invalid_binding(&name, true) {
                    return Err(self.error("Invalid catch parameter name."));
                }
                let ty = if self.eat(":") {
                    let ty = if self.eat("*") {
                        "*".into()
                    } else {
                        let mut name = self.ident()?;
                        while self.script.language == ScriptLanguage::ActionScript3 && self.eat(".")
                        {
                            name.push('.');
                            name.push_str(&self.ident()?);
                        }
                        name
                    };
                    let supported = if self.script.language == ScriptLanguage::ActionScript3 {
                        is_error_type(&ty)
                            || matches!(
                                ty.as_str(),
                                "*" | "Object"
                                    | "Number"
                                    | "String"
                                    | "Boolean"
                                    | "Function"
                                    | "int"
                                    | "uint"
                            )
                            || (self.context.project && self.context.types.contains_key(&ty))
                    } else {
                        ty == "Error"
                    };
                    if !supported {
                        return Err(self.error("Unsupported catch type. AS2 currently accepts Error or an untyped catch; AS3 accepts core error and primitive types, Object, Function or *. Custom classes require project type resolution."));
                    }
                    Some(ty)
                } else {
                    None
                };
                catch_all = ty.as_deref().is_none_or(|t| t == "*");
                self.expect(")")?;
                self.expect("{")?;
                let id = self.next_function;
                self.next_function += 1;
                catches.push(CatchClause {
                    id,
                    offset,
                    name,
                    ty,
                    body: self.statements(true)?,
                });
            }
            let finally = if self.eat("finally") {
                self.expect("{")?;
                Some(self.statements(true)?)
            } else {
                None
            };
            if catches.is_empty() && finally.is_none() {
                return Err(self.error("try requires catch or finally."));
            }
            return Ok(Stmt::Try(body, catches, finally));
        }
        if self.eat("switch") {
            self.expect("(")?;
            let value = self.sequence_expression()?;
            self.expect(")")?;
            self.expect("{")?;
            self.controls.push(ControlTarget {
                label: None,
                iteration: false,
                breakable: true,
            });
            let mut cases = Vec::new();
            let mut default = false;
            while !self.eat("}") {
                let test = if self.eat("case") {
                    Some(self.sequence_expression()?)
                } else if self.eat("default") {
                    if default {
                        return Err(self.error("A switch cannot have multiple default clauses."));
                    }
                    default = true;
                    None
                } else {
                    return Err(self.error("Expected case or default in switch."));
                };
                self.expect(":")?;
                let mut body = Vec::new();
                while !self.is("case")
                    && !self.is("default")
                    && !self.is("}")
                    && self.token().kind != Kind::End
                {
                    body.push(self.statement()?);
                }
                cases.push(Case { test, body });
            }
            self.controls.pop();
            return Ok(Stmt::Switch(value, cases));
        }
        if self.eat("do") {
            self.controls.push(ControlTarget {
                label: None,
                iteration: true,
                breakable: true,
            });
            let body = self.statement()?;
            self.controls.pop();
            self.expect("while")?;
            self.expect("(")?;
            let test = self.sequence_expression()?;
            self.expect(")")?;
            self.eat(";");
            return Ok(Stmt::DoWhile(Box::new(body), test));
        }
        if self.is("function") {
            return Ok(Stmt::Function(Box::new(self.function(true)?)));
        }
        if self.is("return") {
            let offset = self.token().offset;
            if self.function_depth == 0 {
                return Err(self.error("ActionScript return requires a function body."));
            }
            self.pos += 1;
            let value = if self.is(";")
                || self.is("}")
                || self.token().kind == Kind::End
                || self.script.source[offset..self.token().offset]
                    .contains(['\n', '\r', '\u{2028}', '\u{2029}'])
            {
                None
            } else {
                Some(self.sequence_expression()?)
            };
            self.semicolon()?;
            return Ok(Stmt::Return(value, offset));
        }
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        if self.eat("{") {
            return Ok(Stmt::Block(self.statements(true)?));
        }
        if self.eat("var") {
            let value = self.variables()?;
            self.semicolon()?;
            return Ok(value);
        }
        if self.eat("if") {
            return self.parse_if();
        }
        if self.eat("while") {
            self.expect("(")?;
            let condition = self.sequence_expression()?;
            self.expect(")")?;
            self.controls.push(ControlTarget {
                label: None,
                iteration: true,
                breakable: true,
            });
            let body = self.statement()?;
            self.controls.pop();
            return Ok(Stmt::While(condition, Box::new(body)));
        }
        if self.eat("for") {
            let values = self.eat("each");
            if values && self.script.language != ScriptLanguage::ActionScript3 {
                return Err(self.error(
                    "for each..in is an AS3 collection statement; use for..in keys in AS2.",
                ));
            }
            self.expect("(")?;
            let previous_no_in = self.no_in;
            self.no_in = true;
            let init = if self.is(";") {
                Stmt::Empty
            } else if self.eat("var") {
                self.variables()?
            } else {
                Stmt::Expr(self.sequence_expression()?)
            };
            self.no_in = previous_no_in;
            if self.eat("in") {
                let valid = match &init {
                    Stmt::Var(vars) => vars.len() == 1 && vars[0].2.is_none(),
                    Stmt::Expr(value) => value.assignable(),
                    _ => false,
                };
                if !valid {
                    return Err(self.error("for..in requires one uninitialized variable or assignable property target."));
                }
                let collection = self.sequence_expression()?;
                self.expect(")")?;
                self.controls.push(ControlTarget {
                    label: None,
                    iteration: true,
                    breakable: true,
                });
                let body = self.statement()?;
                self.controls.pop();
                return Ok(Stmt::ForIn(
                    Box::new(init),
                    collection,
                    values,
                    Box::new(body),
                ));
            }
            if values {
                return Err(self.error("for each requires an 'in' collection."));
            }
            self.expect(";")?;
            let condition = if self.is(";") {
                None
            } else {
                Some(self.sequence_expression()?)
            };
            self.expect(";")?;
            let update = if self.is(")") {
                None
            } else {
                Some(self.sequence_expression()?)
            };
            self.expect(")")?;
            self.controls.push(ControlTarget {
                label: None,
                iteration: true,
                breakable: true,
            });
            let body = self.statement()?;
            self.controls.pop();
            return Ok(Stmt::For(Box::new(init), condition, update, Box::new(body)));
        }
        if self.is("break") || self.is("continue") {
            let is_break = self.eat("break");
            if !is_break {
                self.expect("continue")?;
            }
            let label =
                if !self.line_break_before_token() && matches!(self.token().kind, Kind::Ident(_)) {
                    Some(self.ident()?)
                } else {
                    None
                };
            if label.is_some() && self.script.language != ScriptLanguage::ActionScript3 {
                return Err(self.error("Labelled jumps require ActionScript 3."));
            }
            if !self.controls.iter().rev().any(|target| match &label {
                Some(name) => target.label.as_ref() == Some(name) && (is_break || target.iteration),
                None => {
                    if is_break {
                        target.breakable
                    } else {
                        target.iteration && target.label.is_none()
                    }
                }
            }) {
                return Err(self.error("Jump has no valid enclosing loop, switch or label target."));
            }
            self.semicolon()?;
            return Ok(if is_break {
                Stmt::Break(label)
            } else {
                Stmt::Continue(label)
            });
        }
        if let Kind::Ident(word) = &self.token().kind
            && self.context.legacy_case
            && matches!(
                word.as_str(),
                "class" | "interface" | "extends" | "implements" | "package" | "import"
            )
        {
            return Err(self.error(format!(
                "ActionScript 1.0 does not support '{word}' declarations; use ActionScript 2.0/3.0 for classes and interfaces, or prototype functions in AS1."
            )));
        }
        if let Kind::Ident(word) = &self.token().kind
            && is_unsupported_keyword(word)
            && !(word == "set" && self.script.language == ScriptLanguage::ActionScript2)
            && !matches!(word.as_str(), "new" | "delete" | "void")
            && !(((self.context.project && self.context.class_name.is_some())
                || self.script.language == ScriptLanguage::ActionScript2)
                && word == "super")
        {
            return Err(self.error(format!("ActionScript '{word}' is not implemented yet. See docs/COMPATIBILITY.md; use a JavaScript/TypeScript handler for this feature.")));
        }
        let expression = self.sequence_expression()?;
        self.semicolon()?;
        Ok(Stmt::Expr(expression))
    }
    pub(crate) fn variables(&mut self) -> Result<Stmt, Diagnostic> {
        let mut variables = Vec::new();
        loop {
            let offset = self.token().offset;
            let name = self.ident()?;
            if invalid_binding(&name, self.function_depth > 0) {
                return Err(Diagnostic::at(
                    self.script,
                    offset,
                    format!("'{name}' cannot be declared in the current ActionScript subset."),
                ));
            }
            let annotation = self.annotation(false)?;
            let value = if self.eat("=") {
                Some(self.expression(0)?)
            } else {
                None
            };
            variables.push((name, annotation, value));
            if !self.eat(",") {
                break;
            }
        }
        Ok(Stmt::Var(variables))
    }
    pub(crate) fn annotation(&mut self, returns: bool) -> Result<Option<String>, Diagnostic> {
        if !self.eat(":") {
            return Ok(None);
        }
        if self.context.legacy_case {
            return Err(self.error("Type annotations require ActionScript 2.0 or 3.0."));
        }
        let start = self.token().offset;
        let name = self.type_name()?;
        if !self.supported_type(&name, returns) {
            let end = self
                .peek(self.pos.saturating_sub(1))
                .map_or(start, |token| token.end);
            let mut error = Diagnostic::at_span(
                self.script,
                (start, end),
                format!(
                    "Type '{name}' is not supported in this ActionScript annotation. Use a supported primitive, resolved project type or AS3 Vector.<T>; void/Void is return-only."
                ),
            );
            error.code = Some("1046".into());
            return Err(error);
        }
        Ok(Some(name))
    }
    pub(crate) fn type_name(&mut self) -> Result<String, Diagnostic> {
        self.enter()?;
        let mut name = if self.is("*=") {
            // In an annotation, `value:*=...` is wildcard plus initializer,
            // not multiplication assignment. Leave '=' for its caller.
            self.tokens.split(self.pos, Kind::Punct("="));
            "*".into()
        } else if self.eat("*") {
            "*".into()
        } else {
            self.ident()?
        };
        while self.is(".")
            && (self.context.project
                || self.context.legacy_case
                || (self.script.language == ScriptLanguage::ActionScript2 && name == "TextField")
                || name == "flash"
                || name.starts_with("flash.")
                || matches!(
                    self.peek(self.pos + 1).map(|t| t.kind),
                    Some(Kind::Punct("<"))
                ))
        {
            self.pos += 1;
            if self.eat("<") {
                if name != "Vector"
                    || !self.context.intrinsic_type(
                        "Vector",
                        self.script.language == ScriptLanguage::ActionScript3,
                    )
                {
                    return Err(self.error("Only the unshadowed AS3 Vector.<T> native type supports type parameters in this profile."));
                }
                let element = self.type_name()?;
                if !self.supported_type(&element, false) {
                    return Err(self.error(format!("Unsupported Vector element type '{element}'.")));
                }
                self.type_close()?;
                name = format!("Vector.<{element}>");
                break;
            }
            name.push('.');
            name.push_str(&self.ident()?);
        }
        self.depth -= 1;
        Ok(name)
    }
    pub(crate) fn type_close(&mut self) -> Result<(), Diagnostic> {
        if self.eat(">") {
            return Ok(());
        }
        // Split only while consuming a generic closer; expression shifts and
        // >= remain untouched outside this type grammar.
        let rest = match self.token().kind {
            Kind::Punct(">>") => ">",
            Kind::Punct(">>>") => ">>",
            Kind::Punct(">=") => "=",
            Kind::Punct(">>=") => ">=",
            Kind::Punct(">>>=") => ">>=",
            _ => return Err(self.error("Expected '>' after the Vector element type.")),
        };
        self.tokens.split(self.pos, Kind::Punct(rest));
        Ok(())
    }
    pub(crate) fn supported_type(&self, name: &str, returns: bool) -> bool {
        let as3 = self.script.language == ScriptLanguage::ActionScript3;
        if let Some(element) = vector_element(name) {
            return as3
                && self.context.intrinsic_type("Vector", true)
                && self.supported_type(element, false);
        }
        let supported = matches!(
            name,
            "Number" | "String" | "Boolean" | "Function" | "Array" | "Object"
        ) || (as3 && matches!(name, "int" | "uint" | "Class" | "*"))
            || self.context.intrinsic_type(name, as3)
            || (!as3
                && matches!(
                    name,
                    "MovieClip"
                        | "Button"
                        | "TextField"
                        | "TextField.StyleSheet"
                        | "TextSnapshot"
                        | "LoadVars"
                        | "Color"
                        | "TextFormat"
                        | "Sound"
                        | "MovieClipLoader"
                        | "SharedObject"
                        | "Camera"
                        | "Microphone"
                        | "ContextMenu"
                        | "ContextMenuItem"
                        | "PrintJob"
                        | "LocalConnection"
                        | "XMLSocket"
                        | "NetConnection"
                        | "NetStream"
                        | "Video"
                        | "FileReference"
                        | "FileReferenceList"
                        | "flash.net.FileReference"
                        | "flash.net.FileReferenceList"
                        | "TextRenderer"
                        | "flash.text.TextRenderer"
                ))
            || (as3 && !self.context.project && is_as3_wave7_type(name))
            || (self.context.project && self.context.types.contains_key(name))
            || (is_error_type(name) && (as3 || name == "Error"))
            || (returns && name == if as3 { "void" } else { "Void" });
        supported && !(matches!(name, "JSON" | "Vector") && self.context.intrinsic_type(name, as3))
    }
    pub(crate) fn function(&mut self, declaration: bool) -> Result<Function, Diagnostic> {
        self.project_function(declaration, false, false, false)
            .map(|(function, _)| function)
    }
    pub(crate) fn project_function(
        &mut self,
        declaration: bool,
        member: bool,
        signature_only: bool,
        allow_as2_signature_accessor: bool,
    ) -> Result<(Function, Option<String>), Diagnostic> {
        let offset = self.token().offset;
        self.expect("function")?;
        let accessor = if member
            && (self.is("get") || self.is("set"))
            && matches!(
                self.peek(self.pos + 1).map(|token| token.kind),
                Some(Kind::Ident(_))
            ) {
            let kind = self.ident()?;
            if signature_only
                && !allow_as2_signature_accessor
                && self.script.language == ScriptLanguage::ActionScript2
            {
                return Err(self.error("AS2 interface accessor signatures are outside this profile; declare get/set accessors in a concrete source class."));
            }
            Some(kind)
        } else {
            None
        };
        let name = if declaration || !self.is("(") {
            let name = self.ident()?;
            if invalid_binding(&name, !declaration || self.function_depth > 0) {
                return Err(self.error("Invalid or reserved function name."));
            }
            Some(name)
        } else {
            None
        };
        self.expect("(")?;
        let mut params = Vec::new();
        let mut defaults: Vec<Option<Expr>> = Vec::new();
        let mut default_offsets: Vec<usize> = Vec::new();
        let mut rest = None;
        if !self.is(")") {
            loop {
                if self.eat("...") {
                    if self.script.language != ScriptLanguage::ActionScript3 {
                        return Err(self.error("Rest parameter syntax requires AS3. AS2 functions receive extra values through arguments."));
                    }
                    let name = self.ident()?;
                    if invalid_binding(&name, true) {
                        return Err(self.error("Invalid or reserved rest parameter name."));
                    }
                    let ty = self.annotation(false)?;
                    if ty.as_deref().is_some_and(|ty| ty != "Array") {
                        return Err(
                            self.error("A rest parameter may be unannotated or typed Array.")
                        );
                    }
                    if !self.is(")") {
                        return Err(self.error(
                            "The rest parameter must be last and cannot have a default value.",
                        ));
                    }
                    rest = Some(name);
                    break;
                }
                let param_offset = self.token().offset;
                let name = self.ident()?;
                if invalid_binding(&name, true) {
                    return Err(self.error("Invalid or reserved parameter name."));
                }
                let ty = self.annotation(false)?;
                let default = if self.eat("=") {
                    if self.script.language != ScriptLanguage::ActionScript3 {
                        return Err(Diagnostic::at(
                            self.script,
                            param_offset,
                            "Default parameter syntax requires AS3. AS2 functions can test omitted values explicitly.",
                        ));
                    }
                    let value = self.expression(0)?;
                    if !constant_default(&value) && super::ast::dotted_const_ref(&value).is_none() {
                        return Err(Diagnostic::at(
                            self.script,
                            param_offset,
                            "Optional parameters require primitive literals, undefined/NaN/Infinity, signed numeric literals, or AS3 static const references.",
                        ));
                    }
                    Some(value)
                } else {
                    if defaults.iter().any(Option::is_some) {
                        return Err(
                            self.error("Required parameters cannot follow optional parameters.")
                        );
                    }
                    None
                };
                params.push((name, ty));
                defaults.push(default);
                default_offsets.push(param_offset);
                if params.len() > 256 {
                    return Err(
                        self.error("A source function supports at most 256 normal parameters.")
                    );
                }
                if !self.eat(",") {
                    break;
                }
            }
        }
        self.expect(")")?;
        let returns = self.annotation(true)?;
        let id = self.next_function;
        self.next_function += 1;
        let body = if signature_only {
            self.expect(";")?;
            Vec::new()
        } else if allow_as2_signature_accessor
            && self.script.language == ScriptLanguage::ActionScript3
            && self.eat(";")
        {
            let class_key =
                serde_json::to_string(&self.context.class_name.clone().unwrap_or_default())
                    .unwrap();
            let raw_name = name.clone().unwrap_or_default();
            let full_name = match accessor.as_deref() {
                Some("get") => format!("get:{raw_name}"),
                Some("set") => format!("set:{raw_name}"),
                _ => raw_name,
            };
            let fn_key = serde_json::to_string(&full_name).unwrap();
            let mut arg_parts: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
            if let Some(r) = &rest {
                arg_parts.push(format!("...{r}"));
            }
            let args_js = if arg_parts.is_empty() {
                "__nfActivation.values".to_owned()
            } else {
                format!("[{}]", arg_parts.join(", "))
            };
            let call_js = format!(
                "__nfInvokeNative(typeof __nfFrame !== 'undefined' ? __nfFrame : this, {class_key}, {fn_key}, {args_js})"
            );
            if returns.as_deref() == Some("void") {
                vec![Stmt::Expr(Expr::Literal(call_js))]
            } else {
                vec![Stmt::Return(Some(Expr::Literal(call_js)), offset)]
            }
        } else {
            self.expect("{")?;
            let outer_controls = std::mem::take(&mut self.controls);
            let outer_no_in = self.no_in;
            self.no_in = false;
            self.function_depth += 1;
            let body = self.statements(true)?;
            self.function_depth -= 1;
            self.controls = outer_controls;
            self.no_in = outer_no_in;
            body
        };
        Ok((
            Function {
                id,
                offset,
                name,
                params,
                defaults,
                default_offsets,
                rest,
                returns,
                body,
                declaration,
            },
            accessor,
        ))
    }
}
