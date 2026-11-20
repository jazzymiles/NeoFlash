//! Pratt expression, E4X selector/literal, RegExp, and primary parser.

use super::*;

impl<'a> Frontend<'a> {
    pub(super) fn is_grouped_qualifier(
        &self,
        open_pos: usize,
        open: &'static str,
        close: &'static str,
    ) -> bool {
        if !self
            .peek(open_pos)
            .is_some_and(|t| t.kind == Kind::Punct(open))
        {
            return false;
        }
        let mut depth = 1usize;
        let mut idx = open_pos + 1;
        while let Some(tok) = self.peek(idx) {
            if tok.kind == Kind::End {
                return false;
            }
            if tok.kind == Kind::Punct(open) {
                depth += 1;
            } else if tok.kind == Kind::Punct(close) {
                depth -= 1;
                if depth == 0 {
                    return self
                        .peek(idx + 1)
                        .is_some_and(|next| next.kind == Kind::Punct("::"));
                }
            }
            idx += 1;
        }
        false
    }
    pub(super) fn xml_part(&mut self, allow_parens: bool) -> Result<(Expr, bool), Diagnostic> {
        if allow_parens && self.eat("(") {
            let prev = self.no_in;
            self.no_in = false;
            let expr = self.sequence_expression()?;
            self.no_in = prev;
            self.expect(")")?;
            Ok((expr, true))
        } else if self.eat("[") {
            let prev = self.no_in;
            self.no_in = false;
            let name = self.sequence_expression()?;
            self.no_in = prev;
            self.expect("]")?;
            Ok((name, true))
        } else if self.eat("*") {
            Ok((Expr::Literal("\"*\"".into()), false))
        } else {
            Ok((Expr::Literal(format!("{:?}", self.ident()?)), false))
        }
    }
    pub(super) fn xml_name(&mut self) -> Result<Expr, Diagnostic> {
        Ok(self.xml_part(false)?.0)
    }
    pub(super) fn xml_qualified_parts(&mut self) -> Result<(Expr, Option<Box<Expr>>), Diagnostic> {
        let (first, computed) = self.xml_part(true)?;
        if self.eat("::") {
            let namespace = if computed {
                first
            } else {
                match &first {
                    Expr::Literal(value) if value == "\"*\"" => Expr::Literal("null".into()),
                    Expr::Literal(value) => Expr::Name(
                        serde_json::from_str(value)
                            .map_err(|_| self.error("Invalid namespace identifier."))?,
                    ),
                    _ => first,
                }
            };
            Ok((self.xml_name()?, Some(Box::new(namespace))))
        } else {
            Ok((first, None))
        }
    }
    pub(super) fn xml_selector(
        &mut self,
        base: Expr,
        descendant: bool,
    ) -> Result<Expr, Diagnostic> {
        if self.script.language != ScriptLanguage::ActionScript3 {
            return Err(self.error("E4X selectors require AS3; use AS2 XMLNode DOM methods."));
        }
        let attribute = self.eat("@");
        let (local, namespace) = self.xml_qualified_parts()?;
        Ok(Expr::XmlSelect {
            base: Box::new(base),
            local: Box::new(local),
            namespace,
            attribute,
            descendant,
        })
    }
    pub(super) fn xml_literal(&mut self) -> Result<Expr, Diagnostic> {
        if self.script.language != ScriptLanguage::ActionScript3 {
            return Err(self.error("XML literals require AS3; use the AS2 XML constructor."));
        }
        let start = self.token().offset;
        let position = self.pos;
        let script = self.script;
        let mut holes = Vec::new();
        let literal = xml_literals::scan(script, start, |at| {
            self.tokens.jump(self.pos, at);
            let outer_no_in = self.no_in;
            self.no_in = false;
            let value = self.expression(0)?;
            self.no_in = outer_no_in;
            let close = self.token();
            self.expect("}")?;
            holes.push(value);
            Ok(close.end)
        })?;
        let encoded = serde_json::to_string(&literal)
            .map_err(|_| self.error("XML literal template could not be encoded."))?;
        self.tokens.literal(position, start, literal.end);
        self.pos = position + 1;
        Ok(Expr::XmlLiteral(encoded, holes))
    }
    pub(super) fn regexp_literal(&mut self) -> Result<Expr, Diagnostic> {
        if self.script.language != ScriptLanguage::ActionScript3 {
            return Err(self.error(
                "Regular expression literals require AS3; AS2 has no native RegExp family.",
            ));
        }
        let start = self.token().offset;
        let source = &self.script.source;
        let mut escaped = false;
        let mut in_class = false;
        let mut close = None;
        for (offset, ch) in source[start + 1..].char_indices() {
            if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                return Err(self.error(
                    "Unterminated regular expression literal; escape line breaks in the pattern.",
                ));
            }
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '[' => in_class = true,
                ']' => in_class = false,
                '/' if !in_class => {
                    close = Some(start + 1 + offset);
                    break;
                }
                _ => {}
            }
        }
        let Some(close) = close else {
            return Err(
                self.error("Unterminated regular expression literal; expected a closing '/'.")
            );
        };
        let mut end = close + 1;
        while source[end..].starts_with(|ch: char| ch.is_alphanumeric() || ch == '_' || ch == '$') {
            end += source[end..].chars().next().unwrap().len_utf8();
        }
        let flags = &source[close + 1..end];
        if flags
            .chars()
            .any(|ch| !matches!(ch, 'g' | 'i' | 'm' | 's' | 'x'))
        {
            return Err(Diagnostic::at(
                self.script,
                close + 1,
                "Unsupported RegExp literal flag; this profile supports g, i, m, s and x.",
            ));
        }
        let pattern = serde_json::to_string(&source[start + 1..close]).unwrap();
        let flags = serde_json::to_string(flags).unwrap();
        self.tokens.literal(self.pos, start, end);
        self.pos += 1;
        Ok(Expr::RegExpLiteral(pattern, flags))
    }
    pub(super) fn sequence_expression(&mut self) -> Result<Expr, Diagnostic> {
        let mut value = self.expression(0)?;
        while self.eat(",") {
            if value.depth() >= 128 {
                return Err(self.error(
                    "Expression exceeds 128 nested operations; split it into smaller expressions.",
                ));
            }
            value = Expr::Binary(",".into(), Box::new(value), Box::new(self.expression(0)?));
        }
        Ok(value)
    }
    pub(super) fn expression(&mut self, min_bp: u8) -> Result<Expr, Diagnostic> {
        self.enter()?;
        let result = self.expression_inner(min_bp)?;
        self.depth -= 1;
        Ok(result)
    }
    pub(super) fn expression_inner(&mut self, min_bp: u8) -> Result<Expr, Diagnostic> {
        let mut lhs = if self.is("!")
            || self.is("~")
            || self.is("-")
            || self.is("+")
            || self.is("typeof")
            || self.is("void")
            || (self.script.language == ScriptLanguage::ActionScript2 && self.is("not"))
        {
            let operator = self.token_text();
            self.pos += 1;
            Expr::Unary(operator, Box::new(self.expression(25)?))
        } else if self.eat("delete") {
            let offset = self.peek(self.pos - 1).unwrap().offset;
            let operand = self.expression(25)?;
            if !matches!(
                operand,
                Expr::Member(_, _)
                    | Expr::Index(_, _)
                    | Expr::XmlSelect { .. }
                    | Expr::XmlWithAttr { .. }
            ) && !((self.with_depth > 0
                || self.script.language == ScriptLanguage::ActionScript2)
                && matches!(operand, Expr::Name(_)))
            {
                return Err(self.error("delete requires a property or indexed element; deleting lexical bindings is unsupported."));
            }
            Expr::Delete(Box::new(operand), offset)
        } else if self.is("++") || self.is("--") {
            let operator = self.token_text();
            self.pos += 1;
            let operand = self.expression(25)?;
            if !operand.assignable() {
                return Err(self.error("Increment/decrement requires a variable or property."));
            }
            Expr::Update(operator, Box::new(operand), true)
        } else {
            self.primary()?
        };
        loop {
            if lhs.depth() > 128 {
                return Err(self.error(
                    "Expression exceeds 128 nested operations; split it into smaller expressions.",
                ));
            }
            if min_bp <= 30
                && self.script.language == ScriptLanguage::ActionScript3
                && self.eat("::")
            {
                let local = self.xml_name()?;
                lhs = if self.with_depth > 0 {
                    Expr::XmlWithAttr {
                        local: Box::new(local),
                        namespace: Some(Box::new(lhs)),
                        attribute: false,
                        descendant: false,
                    }
                } else {
                    Expr::XmlSelect {
                        base: Box::new(Expr::Name("this".into())),
                        local: Box::new(local),
                        namespace: Some(Box::new(lhs)),
                        attribute: false,
                        descendant: false,
                    }
                };
                continue;
            }
            if min_bp <= 30 && self.eat("..") {
                lhs = self.xml_selector(lhs, true)?;
                continue;
            }
            if min_bp <= 30 && self.eat(".") {
                if self.eat("<") {
                    if !matches!(&lhs,Expr::Name(name) if name=="Vector" && self.context.intrinsic_type(name,self.script.language==ScriptLanguage::ActionScript3))
                    {
                        return Err(self.error("Only the unshadowed AS3 Vector.<T> native type supports type parameters."));
                    }
                    let offset = self.peek(self.pos - 1).unwrap().offset;
                    let element = self.type_name()?;
                    if !self.supported_type(&element, false) {
                        return Err(
                            self.error(format!("Unsupported Vector element type '{element}'."))
                        );
                    }
                    self.type_close()?;
                    lhs = Expr::TypeValue(format!("Vector.<{element}>"), offset);
                    continue;
                }
                if !self.is_grouped_qualifier(self.pos, "(", ")") && self.eat("(") {
                    if self.script.language != ScriptLanguage::ActionScript3 {
                        return Err(self.error("E4X filters require AS3."));
                    }
                    self.with_depth += 1;
                    let outer_no_in = self.no_in;
                    self.no_in = false;
                    let predicate = self.sequence_expression()?;
                    self.no_in = outer_no_in;
                    self.with_depth -= 1;
                    self.expect(")")?;
                    if !scope::with_body_supported(&Stmt::Expr(predicate.clone())) {
                        return Err(self.error(
                            "Function definitions inside E4X filter predicates are not supported.",
                        ));
                    }
                    let id = self.next_function;
                    self.next_function += 1;
                    lhs = Expr::XmlFilter(Box::new(lhs), Box::new(predicate), id);
                    continue;
                }
                if self.is("@")
                    || self.is("*")
                    || self.is_grouped_qualifier(self.pos, "(", ")")
                    || self.is_grouped_qualifier(self.pos, "[", "]")
                    || self
                        .peek(self.pos + 1)
                        .is_some_and(|t| t.kind == Kind::Punct("::"))
                {
                    lhs = self.xml_selector(lhs, false)?;
                    continue;
                }
                lhs = Expr::Member(Box::new(lhs), self.ident()?);
                continue;
            }
            if min_bp <= 30 && self.eat("[") {
                let index = self.sequence_expression()?;
                self.expect("]")?;
                lhs = Expr::Index(Box::new(lhs), Box::new(index));
                continue;
            }
            if min_bp <= 30 && self.is("(") {
                let offset = self.token().offset;
                self.pos += 1;
                let mut args = Vec::new();
                if !self.is(")") {
                    loop {
                        args.push(self.expression(0)?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                }
                self.expect(")")?;
                lhs = Expr::Call(Box::new(lhs), args, offset);
                continue;
            }
            if min_bp <= 27 && (self.is("++") || self.is("--")) {
                // Postfix operators have a restricted production: a line
                // terminator starts a separate prefix-expression statement.
                if self.line_break_before_token() {
                    break;
                }
                if !lhs.assignable() {
                    return Err(self.error("Increment/decrement requires a variable or property."));
                }
                let operator = self.token_text();
                self.pos += 1;
                lhs = Expr::Update(operator, Box::new(lhs), false);
                continue;
            }
            if min_bp <= 2 && self.eat("?") {
                let yes = self.expression(0)?;
                self.expect(":")?;
                let no = self.expression(1)?;
                lhs = Expr::Conditional(Box::new(lhs), Box::new(yes), Box::new(no));
                continue;
            }
            let operator = self.token_text();
            if matches!(operator.as_str(), "is" | "as") {
                if min_bp > 15 {
                    break;
                }
                if self.script.language != ScriptLanguage::ActionScript3 {
                    return Err(self.error("AS2 does not support the AS3 is/as type operators."));
                }
                let offset = self.token().offset;
                self.pos += 1;
                let name = self.type_name()?;
                if (matches!(name.as_str(), "JSON" | "Vector")
                    && self.context.intrinsic_type(&name, true))
                    || (!self.context.intrinsic_type(&name, true)
                        && !self.context.core_type(&name)
                        && !self.context.types.contains_key(&name)
                        && !(!self.context.project && is_as3_wave7_type(&name))
                        && vector_element(&name).is_none())
                {
                    return Err(self.error("is/as currently requires a supported native type or a resolved project/native type; dynamic class expressions are unsupported."));
                }
                lhs = Expr::TypeTest(operator, Box::new(lhs), name, offset);
                continue;
            }
            if operator == "instanceof" && self.script.language == ScriptLanguage::ActionScript2 {
                if min_bp > 15 {
                    break;
                }
                let offset = self.token().offset;
                self.pos += 1;
                let name = self.type_name()?;
                if !self.context.legacy_case
                    && !self.context.core_type(&name)
                    && !self.context.intrinsic_type(&name, false)
                    && !self.context.types.contains_key(&name)
                    && !is_as2_legacy_instanceof_type(&name)
                {
                    return Err(self.error("AS2 instanceof requires a resolved class or supported native constructor; dynamic constructor expressions are outside this profile."));
                }
                lhs = Expr::TypeTest(operator, Box::new(lhs), name, offset);
                continue;
            }
            if operator == "instanceof" && self.script.language == ScriptLanguage::ActionScript3 {
                if min_bp > 15 {
                    break;
                }
                self.pos += 1;
                let rhs = self.expression(16)?;
                lhs = Expr::InstanceOf(Box::new(lhs), Box::new(rhs));
                continue;
            }
            if operator == "in" && self.no_in {
                break;
            }
            if matches!(operator.as_str(), "instanceof" | "::" | ".." | "@") {
                return Err(self.error(format!("ActionScript operator '{operator}' requires compatibility semantics not implemented yet.")));
            }
            if self.script.language == ScriptLanguage::ActionScript3
                && matches!(
                    operator.as_str(),
                    "add" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "<>" | "and" | "or"
                )
            {
                return Err(self
                    .error("Legacy string operators belong to AS2; use AS3 symbolic operators."));
            }
            let Some((left_bp, right_bp, assignment)) = binding_power(&operator) else {
                break;
            };
            if left_bp < min_bp {
                break;
            }
            if assignment && !lhs.assignable() {
                return Err(
                    self.error("The left side of an assignment must be a variable or property.")
                );
            }
            self.pos += 1;
            let rhs = self.expression(right_bp)?;
            lhs = if assignment {
                Expr::Assign(operator, Box::new(lhs), Box::new(rhs))
            } else {
                Expr::Binary(operator, Box::new(lhs), Box::new(rhs))
            };
        }
        Ok(lhs)
    }
    pub(super) fn token_text(&self) -> String {
        match &self.token().kind {
            Kind::Ident(s) | Kind::Number(s) | Kind::String(s) => s.clone(),
            Kind::Punct(s) => s.to_string(),
            Kind::End | Kind::Invalid(_) => String::new(),
        }
    }
    pub(super) fn primary(&mut self) -> Result<Expr, Diagnostic> {
        if self.is("/") || self.is("/=") {
            return self.regexp_literal();
        }
        if self.is("<") {
            return self.xml_literal();
        }
        if self.eat("new") {
            let offset = self.token().offset;
            let name = self.type_name()?;
            if name == "Vector"
                && self
                    .context
                    .intrinsic_type(&name, self.script.language == ScriptLanguage::ActionScript3)
            {
                return Err(self.error(
                    "Vector construction requires an element type: new Vector.<T>(length, fixed).",
                ));
            }
            let class = self.context.types.get(&name).cloned();
            let builtin = matches!(name.as_str(), "Array" | "String")
                || (matches!(
                    name.as_str(),
                    "Object" | "Number" | "Boolean" | "int" | "uint"
                ) && self.context.core_type(&name))
                || self
                    .context
                    .intrinsic_type(&name, self.script.language == ScriptLanguage::ActionScript3)
                || (self.script.language == ScriptLanguage::ActionScript2
                    && matches!(name.as_str(), "LoadVars" | "TextFormat" | "Sound"))
                || (self.script.language == ScriptLanguage::ActionScript2
                    && class.is_none()
                    && matches!(
                        name.as_str(),
                        "MovieClipLoader"
                            | "TextField.StyleSheet"
                            | "Color"
                            | "SharedObject"
                            | "Selection"
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
                    ))
                || (self.script.language == ScriptLanguage::ActionScript3
                    && !self.context.project
                    && class.is_none()
                    && is_as3_wave7_constructible(&name));
            if !builtin
                && vector_element(&name).is_none()
                && class.is_none()
                && (!is_error_type(&name)
                    || (self.script.language == ScriptLanguage::ActionScript2 && name != "Error"))
                && !self.context.legacy_case
                && (is_error_type(&name) || name.contains('.'))
            {
                return Err(self.error("This constructor is not a supported native type in this language/profile; user classes require project type resolution."));
            }
            let mut args = Vec::new();
            if self.eat("(") {
                if !self.is(")") {
                    loop {
                        args.push(self.expression(0)?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                }
                self.expect(")")?;
            }
            if vector_element(&name).is_some() {
                return Ok(Expr::NewVector(name, args, offset));
            }
            if builtin {
                return Ok(Expr::NewBuiltin(name, args, offset));
            }
            if let Some(name) = class {
                return Ok(Expr::NewClass(name, args, offset));
            }
            if !is_error_type(&name) {
                return Ok(Expr::NewValue(name, args, offset));
            }
            let maximum = if self.script.language == ScriptLanguage::ActionScript3 {
                2
            } else {
                1
            };
            if args.len() > maximum {
                return Err(
                    self.error("Error constructor has too many arguments for this language.")
                );
            }
            return Ok(Expr::NewError(name, args, offset));
        }
        if self.is("function") {
            return Ok(Expr::Function(Box::new(self.function(false)?)));
        }
        if self.script.language == ScriptLanguage::ActionScript3 && self.eat("@") {
            let (local, namespace) = self.xml_qualified_parts()?;
            return Ok(Expr::XmlWithAttr {
                local: Box::new(local),
                namespace,
                attribute: true,
                descendant: false,
            });
        }
        if self.script.language == ScriptLanguage::ActionScript3 && self.eat("..") {
            let attribute = self.eat("@");
            let (local, namespace) = self.xml_qualified_parts()?;
            return Ok(Expr::XmlWithAttr {
                local: Box::new(local),
                namespace,
                attribute,
                descendant: true,
            });
        }
        if self.script.language == ScriptLanguage::ActionScript3
            && self.is("*")
            && self
                .peek(self.pos + 1)
                .is_some_and(|t| t.kind == Kind::Punct("::"))
        {
            self.pos += 2;
            let local = self.xml_name()?;
            return Ok(Expr::XmlWithAttr {
                local: Box::new(local),
                namespace: Some(Box::new(Expr::Literal("null".into()))),
                attribute: false,
                descendant: false,
            });
        }
        let token = self.token().clone();
        match token.kind {
            Kind::Number(value) | Kind::String(value) => { self.pos += 1; Ok(Expr::Literal(value)) }
            Kind::Ident(name) => {
                if (is_unsupported_keyword(&name) && !(name == "super" && (self.context.class_name.is_some() || self.script.language == ScriptLanguage::ActionScript2)) && !(name == "set" && self.script.language == ScriptLanguage::ActionScript2)) || matches!(name.as_str(), "if" | "else" | "while" | "for" | "var" | "break" | "continue" | "return") {
                    return Err(self.error(format!("ActionScript construct '{name}' is not supported in this expression.")));
                }
                self.pos += 1;
                if matches!(name.as_str(), "true" | "false" | "null" | "undefined" | "NaN" | "Infinity") {
                    Ok(Expr::Literal(name))
                } else if self.script.language == ScriptLanguage::ActionScript3 && self.eat("::") {
                    let local = self.xml_name()?;
                    if self.with_depth > 0 {
                        Ok(Expr::XmlWithAttr {
                            local: Box::new(local),
                            namespace: Some(Box::new(Expr::Name(name))),
                            attribute: false,
                            descendant: false,
                        })
                    } else {
                        Ok(Expr::XmlSelect {
                            base: Box::new(Expr::Name("this".into())),
                            local: Box::new(local),
                            namespace: Some(Box::new(Expr::Name(name))),
                            attribute: false,
                            descendant: false,
                        })
                    }
                } else { Ok(Expr::Name(name)) }
            }
            Kind::Punct("(") => { self.pos += 1; let previous=self.no_in;self.no_in=false; let expr = self.sequence_expression()?;self.no_in=previous; self.expect(")")?; Ok(expr) }
            Kind::Punct("[") => {
                self.pos += 1; let mut values = Vec::new();
                while !self.is("]") {
                    if self.eat(",") { values.push(None); continue; }
                    values.push(Some(self.expression(0)?));
                    if !self.eat(",") { break; }
                }
                self.expect("]")?; Ok(Expr::Array(values))
            }
            Kind::Punct("{") => {
                self.pos += 1; let mut values = Vec::new();
                if !self.is("}") { loop {
                    let key = match self.token().kind.clone() {
                        Kind::Ident(s) | Kind::Number(s) => PropertyKey { source:s.clone(), name:s },
                        Kind::String(s) => self.string_key(s)?,
                        _ => return Err(self.error("Expected a property name in object literal.")),
                    };
                    self.pos += 1; self.expect(":")?; values.push((key, self.expression(0)?));
                    if !self.eat(",") || self.is("}") { break; }
                } }
                self.expect("}")?; Ok(Expr::Object(values))
            }
            Kind::Punct("<" | "@" | "::" | "..") => Err(self.error("XML literals and E4X namespace/attribute operators are unsupported; use XML(string) and explicit traversal methods.")),
            _ => Err(self.error("Expected an expression.")),
        }
    }
    pub(super) fn string_key(&self, source: String) -> Result<PropertyKey, Diagnostic> {
        // Use the same real parser as output validation, including escaped
        // quotes, hexadecimal and Unicode escapes. Never strip quotes by hand.
        let allocator = oxc_allocator::Allocator::default();
        let value = oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::cjs())
            .parse_expression()
            .map_err(|_| self.error("Invalid escaped string property name."))?;
        let oxc_ast::ast::Expression::StringLiteral(literal) = value else {
            return Err(self.error("Expected a string property name."));
        };
        if literal.lone_surrogates {
            return Err(self
                .error("Property names containing lone UTF-16 surrogates are not supported yet."));
        }
        let name = literal.value.to_string();
        Ok(PropertyKey { source, name })
    }
    pub(super) fn builtin_call(callee: &Expr) -> bool {
        match callee {
            Expr::Name(name) => {
                matches!(
                    name.as_str(),
                    "trace"
                        | "Object"
                        | "Number"
                        | "String"
                        | "Array"
                        | "Boolean"
                        | "parseInt"
                        | "parseFloat"
                        | "escape"
                        | "unescape"
                        | "isXMLName"
                        | "setInterval"
                        | "setTimeout"
                        | "clearInterval"
                        | "clearTimeout"
                        | "chr"
                        | "ord"
                        | "length"
                        | "substring"
                        | "mbchr"
                        | "mbord"
                        | "mblength"
                        | "mbsubstring"
                        | "isNaN"
                        | "isFinite"
                        | "Date"
                        | "RegExp"
                        | "XML"
                        | "XMLList"
                        | "XMLNode"
                        | "Namespace"
                        | "QName"
                ) || is_timeline_method(name)
                    || is_as3_wave7_type(name)
            }
            Expr::Member(base, name) if matches!(base.as_ref(), Expr::Name(n) if n == "Math") => {
                matches!(
                    name.as_str(),
                    "abs"
                        | "acos"
                        | "asin"
                        | "atan"
                        | "atan2"
                        | "ceil"
                        | "cos"
                        | "exp"
                        | "floor"
                        | "log"
                        | "max"
                        | "min"
                        | "pow"
                        | "random"
                        | "round"
                        | "sin"
                        | "sqrt"
                        | "tan"
                )
            }
            Expr::Member(base, name)
                if name == "get"
                    && matches!(
                        base.as_ref(),
                        Expr::Name(n) if matches!(n.as_str(), "Camera" | "Microphone")
                    ) =>
            {
                true
            }
            Expr::Member(_, name) => {
                is_timeline_method(name)
                    || is_family_method(name)
                    || matches!(
                        name.as_str(),
                        "trace"
                            | "appendText"
                            | "replaceText"
                            | "replaceSelectedText"
                            | "setSelection"
                            | "getLineText"
                            | "getLineLength"
                            | "getLineOffset"
                            | "getLineIndexOfChar"
                            | "getTextFormat"
                            | "setTextFormat"
                            | "getNewTextFormat"
                            | "setNewTextFormat"
                            | "createTextField"
                            | "hitTestObject"
                            | "hitTestPoint"
                            | "hitTest"
                            | "startDrag"
                            | "stopDrag"
                            | "getURL"
                            | "loadMovie"
                            | "unloadMovie"
                            | "loadVariables"
                            | "getSWFVersion"
                            | "addCallback"
                            | "send"
                            | "setMode"
                            | "setMotionLevel"
                            | "setRate"
                            | "setGain"
                            | "setSilenceLevel"
                            | "setUseEchoSuppression"
                            | "setBufferTime"
                            | "receiveAudio"
                            | "receiveVideo"
                            | "browse"
                            | "upload"
                            | "download"
                            | "cancel"
                            | "hasMultiChannelAudio"
                            | "createVideoTexture"
                            | "uploadCompressedTextureFromByteArray"
                            | "attachNetStream"
                            | "setProfileLevel"
                            | "setKeyFrameInterval"
                            | "setQuality"
                            | "setDirty"
                            | "setProperty"
                            | "getLocal"
                            | "getRemote"
                            | "connect"
                            | "close"
                            | "setFps"
                            | "flush"
                            | "clear"
                            | "dispatchEvent"
                            | "writeUTFBytes"
                            | "writeByte"
                            | "writeUnsignedInt"
                            | "dispose"
                            | "play2"
                            | "createTexture"
                            | "createCubeTexture"
                            | "clone"
                    )
            }
            Expr::Index(base, index) => literal_property_name(index)
                .is_some_and(|name| Self::builtin_call(&Expr::Member(base.clone(), name))),
            _ => false,
        }
    }
}
