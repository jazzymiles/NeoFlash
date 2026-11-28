//! Deterministic Oxc AST lowerer (`JavaScript` & `TypeScript` -> NeoFlash `ParsedClass` / `ParsedScript`)
//! for the Experimental Xcode + Swift 6 (`@MainActor`) compiler (`T-499`).
//!
//! Parses `.js`, `.jsx`, `.ts`, and `.tsx` source modules, timeline frame scripts, and stage
//! instance handlers via `oxc_parser`, strips ES module `import` syntax, resolves TypeScript
//! structural `interface`s and literal `type` unions, and lowers classes, module-level constants,
//! module-level functions, and imperative statements into the shared AST consumed by `emit_swift.rs`.

use super::hir::{
    AccessorKind, MemberAccess, ParsedClass, ParsedField, ParsedMethod, ParsedScript,
};
use super::{ConversionIssue, ConversionIssueKind};
use crate::actionscript::ast::{Case, CatchClause, Expr, Function, PropertyKey as AsPropKey, Stmt};
use crate::byte_offset_to_line_col;
use neoflash_core::{Project, Script, ScriptLanguage, SourceUnit};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use std::collections::{BTreeMap, BTreeSet};

type LoweredFormalParams = (
    Vec<(String, Option<String>)>,
    Vec<Option<Expr>>,
    Option<String>,
    Vec<Stmt>,
);

#[derive(Default)]
pub(crate) struct JsTsProjectLowering {
    pub(crate) classes: Vec<(String, ParsedClass)>,
    pub(crate) module_fields: Vec<(String, ParsedField)>,
    pub(crate) module_functions: Vec<(String, Function)>,
    pub(crate) type_aliases: BTreeMap<String, String>,
}

pub(crate) fn is_js_or_ts(lang: ScriptLanguage) -> bool {
    matches!(
        lang,
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript
    )
}

pub(crate) fn lower_js_ts_source_units(
    project: &Project,
) -> (JsTsProjectLowering, Vec<ConversionIssue>) {
    let mut out = JsTsProjectLowering::default();
    let mut issues = Vec::new();

    let web_units: Vec<&SourceUnit> = project
        .sources
        .iter()
        .filter(|u| is_js_or_ts(u.language) && !u.source.trim().is_empty())
        .collect();
    if web_units.is_empty() {
        return (out, issues);
    }

    // Pass 1: discover declared class names vs structural `interface`s & `type` aliases across all modules.
    let mut declared_classes = BTreeSet::new();
    let mut raw_aliases: Vec<(String, String)> = Vec::new();

    for unit in &web_units {
        let allocator = Allocator::default();
        let st = oxc_source_type(unit.language);
        let parsed = Parser::new(&allocator, &unit.source, st).parse();
        if !parsed.errors.is_empty() {
            let first = &parsed.errors[0];
            issues.push(ConversionIssue {
                kind: ConversionIssueKind::ManualAttention,
                location: unit.path.clone(),
                line: 1,
                column: 1,
                message: format!("Failed to parse JS/TS module '{}': {first}", unit.path),
            });
            continue;
        }
        for stmt in &parsed.program.body {
            collect_top_level_types(stmt, &mut declared_classes, &mut raw_aliases);
        }
    }

    for (name, mapped) in raw_aliases {
        if !declared_classes.contains(&name) {
            out.type_aliases.insert(name, mapped);
        }
    }

    // Pass 2: lower classes, top-level constants/vars, and top-level helper functions in module order.
    // Order modules with `types` first when present so constants like `GRID_SIZE` initialize first.
    let mut ordered_units = web_units;
    ordered_units.sort_by_key(|u| {
        let p = u.path.to_ascii_lowercase();
        if p.contains("types.") { 0usize } else { 1usize }
    });

    let mut next_fn_id = 10_000usize;
    for unit in ordered_units {
        let allocator = Allocator::default();
        let st = oxc_source_type(unit.language);
        let parsed = Parser::new(&allocator, &unit.source, st).parse();
        if !parsed.errors.is_empty() {
            continue;
        }
        let mut lowerer = OxcSwiftLowerer {
            source: &unit.source,
            location: &unit.path,
            type_aliases: &out.type_aliases,
            declared_classes: &declared_classes,
            type_params_in_scope: BTreeSet::new(),
            next_fn_id: &mut next_fn_id,
            temp_counter: 0,
            issues: &mut issues,
        };
        lowerer.lower_module_program(
            &parsed.program,
            &unit.path,
            &mut out.classes,
            &mut out.module_fields,
            &mut out.module_functions,
        );
    }

    (out, issues)
}

pub(crate) fn parse_js_ts_script(
    script: &Script,
    location: &str,
    type_aliases: &BTreeMap<String, String>,
    declared_classes: &BTreeSet<String>,
) -> Result<(ParsedScript, Vec<ConversionIssue>), String> {
    let allocator = Allocator::default();
    let st = oxc_source_type(script.language);
    let parsed = Parser::new(&allocator, &script.source, st).parse();
    if let Some(first) = parsed.errors.first() {
        return Err(first.to_string());
    }
    let mut issues = Vec::new();
    let mut next_fn_id = 50_000usize;
    let mut lowerer = OxcSwiftLowerer {
        source: &script.source,
        location,
        type_aliases,
        declared_classes,
        type_params_in_scope: BTreeSet::new(),
        next_fn_id: &mut next_fn_id,
        temp_counter: 0,
        issues: &mut issues,
    };
    let mut statements = Vec::new();
    for stmt in &parsed.program.body {
        lowerer.lower_stmt_into(stmt, &mut statements);
    }
    Ok((
        ParsedScript {
            expanded_source: script.source.clone(),
            // Treat lowered JS/TS scripts with 0..1 modern scale/alpha semantics (same as AS3).
            language: ScriptLanguage::ActionScript3,
            imports: Vec::new(),
            statements,
        },
        issues,
    ))
}

fn oxc_source_type(lang: ScriptLanguage) -> SourceType {
    match lang {
        ScriptLanguage::TypeScript => SourceType::tsx().with_module(true),
        _ => SourceType::jsx().with_module(true),
    }
}

fn collect_top_level_types(
    stmt: &Statement<'_>,
    declared_classes: &mut BTreeSet<String>,
    raw_aliases: &mut Vec<(String, String)>,
) {
    match stmt {
        Statement::ClassDeclaration(cls) => {
            if let Some(id) = &cls.id {
                declared_classes.insert(id.name.as_str().to_owned());
            }
        }
        Statement::TSInterfaceDeclaration(iface) => {
            raw_aliases.push((iface.id.name.as_str().to_owned(), "Object".to_owned()));
        }
        Statement::TSTypeAliasDeclaration(alias) => {
            let mapped = classify_ts_type_alias(&alias.type_annotation);
            raw_aliases.push((alias.id.name.as_str().to_owned(), mapped));
        }
        Statement::ExportNamedDeclaration(exp) => {
            if let Some(decl) = &exp.declaration {
                match decl {
                    Declaration::ClassDeclaration(cls) => {
                        if let Some(id) = &cls.id {
                            declared_classes.insert(id.name.as_str().to_owned());
                        }
                    }
                    Declaration::TSInterfaceDeclaration(iface) => {
                        raw_aliases.push((iface.id.name.as_str().to_owned(), "Object".to_owned()));
                    }
                    Declaration::TSTypeAliasDeclaration(alias) => {
                        let mapped = classify_ts_type_alias(&alias.type_annotation);
                        raw_aliases.push((alias.id.name.as_str().to_owned(), mapped));
                    }
                    _ => {}
                }
            }
        }
        Statement::ExportDefaultDeclaration(exp) => {
            if let ExportDefaultDeclarationKind::ClassDeclaration(cls) = &exp.declaration
                && let Some(id) = &cls.id
            {
                declared_classes.insert(id.name.as_str().to_owned());
            }
        }
        _ => {}
    }
}

fn classify_ts_type_alias(ts_ty: &TSType<'_>) -> String {
    match ts_ty {
        TSType::TSStringKeyword(_) => "String".into(),
        TSType::TSNumberKeyword(_) => "Number".into(),
        TSType::TSBooleanKeyword(_) => "Boolean".into(),
        TSType::TSVoidKeyword(_) => "void".into(),
        TSType::TSLiteralType(lit) => match &lit.literal {
            TSLiteral::StringLiteral(_) => "String".into(),
            TSLiteral::NumericLiteral(n) => {
                if n.value.fract() == 0.0 {
                    "int".into()
                } else {
                    "Number".into()
                }
            }
            TSLiteral::BooleanLiteral(_) => "Boolean".into(),
            _ => "*".into(),
        },
        TSType::TSUnionType(u) => {
            let non_null: Vec<&TSType<'_>> = u
                .types
                .iter()
                .filter(|t| !matches!(t, TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_)))
                .collect();
            if non_null.is_empty() {
                return "*".into();
            }
            if non_null.iter().all(|t| match t {
                TSType::TSStringKeyword(_) => true,
                TSType::TSLiteralType(lt) => matches!(
                    lt.as_ref(),
                    TSLiteralType {
                        literal: TSLiteral::StringLiteral(_),
                        ..
                    }
                ),
                _ => false,
            }) {
                return "String".into();
            }
            if non_null.iter().all(|t| match t {
                TSType::TSNumberKeyword(_) => true,
                TSType::TSLiteralType(lt) => matches!(
                    lt.as_ref(),
                    TSLiteralType {
                        literal: TSLiteral::NumericLiteral(_),
                        ..
                    }
                ),
                _ => false,
            }) {
                return "Number".into();
            }
            if non_null
                .iter()
                .all(|t| matches!(t, TSType::TSBooleanKeyword(_)))
            {
                return "Boolean".into();
            }
            "Object".into()
        }
        TSType::TSFunctionType(_) => "Function".into(),
        TSType::TSTypeLiteral(_) | TSType::TSObjectKeyword(_) => "Object".into(),
        _ => "*".into(),
    }
}

struct OxcSwiftLowerer<'a> {
    source: &'a str,
    location: &'a str,
    type_aliases: &'a BTreeMap<String, String>,
    declared_classes: &'a BTreeSet<String>,
    type_params_in_scope: BTreeSet<String>,
    next_fn_id: &'a mut usize,
    temp_counter: usize,
    issues: &'a mut Vec<ConversionIssue>,
}

impl<'a> OxcSwiftLowerer<'a> {
    fn alloc_fn_id(&mut self) -> usize {
        let id = *self.next_fn_id;
        *self.next_fn_id += 1;
        id
    }

    fn alloc_temp(&mut self, prefix: &str) -> String {
        self.temp_counter += 1;
        format!("_nfJsTmp_{prefix}_{}", self.temp_counter)
    }

    fn note_issue(&mut self, kind: ConversionIssueKind, offset: u32, msg: impl Into<String>) {
        let (line, column) = byte_offset_to_line_col(self.source, offset as usize);
        self.issues.push(ConversionIssue {
            kind,
            location: self.location.to_owned(),
            line,
            column,
            message: msg.into(),
        });
    }

    fn lower_module_program(
        &mut self,
        program: &Program<'_>,
        path: &str,
        classes_out: &mut Vec<(String, ParsedClass)>,
        fields_out: &mut Vec<(String, ParsedField)>,
        funcs_out: &mut Vec<(String, Function)>,
    ) {
        for stmt in &program.body {
            self.lower_module_item(stmt, path, classes_out, fields_out, funcs_out);
        }
    }

    fn lower_module_item(
        &mut self,
        stmt: &Statement<'_>,
        path: &str,
        classes_out: &mut Vec<(String, ParsedClass)>,
        fields_out: &mut Vec<(String, ParsedField)>,
        funcs_out: &mut Vec<(String, Function)>,
    ) {
        match stmt {
            Statement::ImportDeclaration(_)
            | Statement::TSInterfaceDeclaration(_)
            | Statement::TSTypeAliasDeclaration(_)
            | Statement::EmptyStatement(_) => {}
            Statement::ClassDeclaration(cls) => {
                if let Some(parsed) = self.lower_class_decl(cls) {
                    classes_out.push((path.to_owned(), parsed));
                }
            }
            Statement::FunctionDeclaration(fn_decl) => {
                if let Some(func) = self.lower_function(fn_decl, true)
                    && func.name.is_some()
                {
                    funcs_out.push((path.to_owned(), func));
                }
            }
            Statement::VariableDeclaration(var_decl) => {
                let is_const = var_decl.kind == VariableDeclarationKind::Const;
                for d in &var_decl.declarations {
                    if let BindingPatternKind::BindingIdentifier(id) = &d.id.kind {
                        let ty =
                            d.id.type_annotation
                                .as_ref()
                                .and_then(|ann| self.map_ts_type(&ann.type_annotation));
                        let value = d.init.as_ref().map(|e| self.lower_expr(e));
                        fields_out.push((
                            path.to_owned(),
                            ParsedField {
                                offset: d.span.start as usize,
                                name: id.name.as_str().to_owned(),
                                ty,
                                value,
                                is_const,
                                is_static: true,
                                access: MemberAccess::Public,
                            },
                        ));
                    }
                }
            }
            Statement::ExportNamedDeclaration(exp) => {
                if let Some(decl) = &exp.declaration {
                    match decl {
                        Declaration::ClassDeclaration(cls) => {
                            if let Some(parsed) = self.lower_class_decl(cls) {
                                classes_out.push((path.to_owned(), parsed));
                            }
                        }
                        Declaration::FunctionDeclaration(fn_decl) => {
                            if let Some(func) = self.lower_function(fn_decl, true)
                                && func.name.is_some()
                            {
                                funcs_out.push((path.to_owned(), func));
                            }
                        }
                        Declaration::VariableDeclaration(var_decl) => {
                            let is_const = var_decl.kind == VariableDeclarationKind::Const;
                            for d in &var_decl.declarations {
                                if let BindingPatternKind::BindingIdentifier(id) = &d.id.kind {
                                    let ty =
                                        d.id.type_annotation
                                            .as_ref()
                                            .and_then(|ann| self.map_ts_type(&ann.type_annotation));
                                    let value = d.init.as_ref().map(|e| self.lower_expr(e));
                                    fields_out.push((
                                        path.to_owned(),
                                        ParsedField {
                                            offset: d.span.start as usize,
                                            name: id.name.as_str().to_owned(),
                                            ty,
                                            value,
                                            is_const,
                                            is_static: true,
                                            access: MemberAccess::Public,
                                        },
                                    ));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Statement::ExportDefaultDeclaration(exp) => match &exp.declaration {
                ExportDefaultDeclarationKind::ClassDeclaration(cls) => {
                    if let Some(parsed) = self.lower_class_decl(cls) {
                        classes_out.push((path.to_owned(), parsed));
                    }
                }
                ExportDefaultDeclarationKind::FunctionDeclaration(fn_decl) => {
                    if let Some(func) = self.lower_function(fn_decl, true)
                        && func.name.is_some()
                    {
                        funcs_out.push((path.to_owned(), func));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn lower_class_decl(&mut self, cls: &Class<'_>) -> Option<ParsedClass> {
        let short_name = cls.id.as_ref()?.name.as_str().to_owned();
        let saved_tparams = self.type_params_in_scope.clone();
        if let Some(tparams) = &cls.type_parameters {
            for p in &tparams.params {
                self.type_params_in_scope
                    .insert(p.name.name.as_str().to_owned());
            }
        }

        let base_class = cls.super_class.as_ref().and_then(|sc| match sc {
            Expression::Identifier(id) => Some(id.name.as_str().to_owned()),
            Expression::StaticMemberExpression(mem) => Some(mem.property.name.as_str().to_owned()),
            _ => None,
        });

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut declared_field_names = BTreeSet::new();

        for elem in &cls.body.body {
            match elem {
                ClassElement::PropertyDefinition(prop) => {
                    let Some(name) = prop_key_to_name(&prop.key) else {
                        continue;
                    };
                    declared_field_names.insert(name.clone());
                    let ty = prop
                        .type_annotation
                        .as_ref()
                        .and_then(|ann| self.map_ts_type(&ann.type_annotation));
                    let value = prop.value.as_ref().map(|v| self.lower_expr(v));
                    fields.push(ParsedField {
                        offset: prop.span.start as usize,
                        name,
                        ty,
                        value,
                        is_const: prop.readonly && prop.r#static && prop.value.is_some(),
                        is_static: prop.r#static,
                        access: MemberAccess::Public,
                    });
                }
                ClassElement::MethodDefinition(m) => {
                    let raw_key = prop_key_to_name(&m.key).unwrap_or_default();
                    let is_ctor = m.kind == MethodDefinitionKind::Constructor;
                    // Handle TS constructor parameter properties (`constructor(public x: number)`)
                    if is_ctor {
                        for p in &m.value.params.items {
                            if (p.accessibility.is_some() || p.readonly)
                                && let BindingPatternKind::BindingIdentifier(id) = &p.pattern.kind
                            {
                                let pname = id.name.as_str().to_owned();
                                if declared_field_names.insert(pname.clone()) {
                                    let pty = p
                                        .pattern
                                        .type_annotation
                                        .as_ref()
                                        .and_then(|a| self.map_ts_type(&a.type_annotation));
                                    fields.push(ParsedField {
                                        offset: p.span.start as usize,
                                        name: pname,
                                        ty: pty,
                                        value: None,
                                        is_const: false,
                                        is_static: false,
                                        access: MemberAccess::Public,
                                    });
                                }
                            }
                        }
                    }

                    let Some(mut func) = self.lower_function(&m.value, false) else {
                        continue;
                    };
                    if is_ctor {
                        func.name = Some(short_name.clone());
                        func.returns = None;
                        // Inject `this.<prop> = <prop>` for TS parameter properties
                        let mut prop_assigns = Vec::new();
                        for p in &m.value.params.items {
                            if (p.accessibility.is_some() || p.readonly)
                                && let BindingPatternKind::BindingIdentifier(id) = &p.pattern.kind
                            {
                                let pname = id.name.as_str().to_owned();
                                prop_assigns.push(Stmt::Expr(Expr::Assign(
                                    "=".into(),
                                    Box::new(Expr::Member(
                                        Box::new(Expr::Name("this".into())),
                                        pname.clone(),
                                    )),
                                    Box::new(Expr::Name(pname)),
                                )));
                            }
                        }
                        if !prop_assigns.is_empty() {
                            prop_assigns.extend(func.body);
                            func.body = prop_assigns;
                        }
                    } else {
                        func.name = Some(raw_key);
                    }

                    let accessor = match m.kind {
                        MethodDefinitionKind::Get => Some(AccessorKind::Get),
                        MethodDefinitionKind::Set => Some(AccessorKind::Set),
                        _ => None,
                    };
                    methods.push(ParsedMethod {
                        function: func,
                        is_static: m.r#static,
                        is_override: m.r#override,
                        access: MemberAccess::Public,
                        accessor,
                        param_type_spans: Vec::new(),
                        return_type_span: None,
                        name_span: (m.key.span().start as usize, m.key.span().end as usize),
                    });
                }
                _ => {}
            }
        }

        // In JS classes without TS property declarations, scan constructor `this.<prop> = ...` assignments!
        for m in &methods {
            if m.function.name.as_deref() == Some(short_name.as_str()) {
                for stmt in &m.function.body {
                    if let Stmt::Expr(Expr::Assign(op, lhs, _)) = stmt.unlocated()
                        && op == "="
                        && let Expr::Member(base, prop) = lhs.as_ref()
                        && matches!(base.as_ref(), Expr::Name(n) if n == "this")
                        && declared_field_names.insert(prop.clone())
                    {
                        fields.push(ParsedField {
                            offset: 0,
                            name: prop.clone(),
                            ty: Some("*".into()),
                            value: None,
                            is_const: false,
                            is_static: false,
                            access: MemberAccess::Public,
                        });
                    }
                }
            }
        }

        self.type_params_in_scope = saved_tparams;

        Some(ParsedClass {
            source: self.source.to_owned(),
            language: ScriptLanguage::ActionScript3,
            package: String::new(),
            qualified_name: short_name.clone(),
            short_name,
            base_class,
            interfaces: Vec::new(),
            imports: Vec::new(),
            is_interface: false,
            is_dynamic: true,
            is_public: true,
            fields,
            methods,
        })
    }

    fn lower_function(
        &mut self,
        fn_node: &oxc_ast::ast::Function<'_>,
        declaration: bool,
    ) -> Option<Function> {
        let body_ast = fn_node.body.as_ref()?;
        let saved_tparams = self.type_params_in_scope.clone();
        if let Some(tparams) = &fn_node.type_parameters {
            for p in &tparams.params {
                self.type_params_in_scope
                    .insert(p.name.name.as_str().to_owned());
            }
        }

        let (params, defaults, rest, destructure_prelude) =
            self.lower_formal_params(&fn_node.params);
        let returns = fn_node
            .return_type
            .as_ref()
            .and_then(|rt| self.map_ts_type(&rt.type_annotation))
            .or_else(|| {
                if body_has_value_return(&body_ast.statements) {
                    Some("*".into())
                } else {
                    Some("void".into())
                }
            });

        let mut body = destructure_prelude;
        for s in &body_ast.statements {
            self.lower_stmt_into(s, &mut body);
        }

        self.type_params_in_scope = saved_tparams;

        let id = self.alloc_fn_id();
        Some(Function {
            id,
            offset: fn_node.span.start as usize,
            name: fn_node.id.as_ref().map(|i| i.name.as_str().to_owned()),
            params,
            defaults,
            default_offsets: Vec::new(),
            rest,
            returns,
            body,
            declaration,
        })
    }

    fn lower_formal_params(&mut self, formal: &FormalParameters<'_>) -> LoweredFormalParams {
        let mut params = Vec::new();
        let mut defaults = Vec::new();
        let mut prelude = Vec::new();

        for item in &formal.items {
            let explicit_ty = item
                .pattern
                .type_annotation
                .as_ref()
                .and_then(|ann| self.map_ts_type(&ann.type_annotation));
            match &item.pattern.kind {
                BindingPatternKind::BindingIdentifier(id) => {
                    params.push((id.name.as_str().to_owned(), explicit_ty));
                    defaults.push(None);
                }
                BindingPatternKind::AssignmentPattern(assign) => {
                    let def_expr = self.lower_expr(&assign.right);
                    let ty = explicit_ty.or_else(|| {
                        assign
                            .left
                            .type_annotation
                            .as_ref()
                            .and_then(|ann| self.map_ts_type(&ann.type_annotation))
                    });
                    if let BindingPatternKind::BindingIdentifier(id) = &assign.left.kind {
                        params.push((id.name.as_str().to_owned(), ty));
                        defaults.push(Some(def_expr));
                    } else {
                        let tmp = self.alloc_temp("param");
                        params.push((tmp.clone(), Some("*".into())));
                        defaults.push(Some(def_expr));
                        self.lower_destructure_pattern(&assign.left, Expr::Name(tmp), &mut prelude);
                    }
                }
                _ => {
                    let tmp = self.alloc_temp("param");
                    params.push((tmp.clone(), Some("*".into())));
                    defaults.push(None);
                    self.lower_destructure_pattern(&item.pattern, Expr::Name(tmp), &mut prelude);
                }
            }
        }

        let rest = formal.rest.as_ref().and_then(|r| match &r.argument.kind {
            BindingPatternKind::BindingIdentifier(id) => Some(id.name.as_str().to_owned()),
            _ => None,
        });

        (params, defaults, rest, prelude)
    }

    fn lower_destructure_pattern(
        &mut self,
        pat: &BindingPattern<'_>,
        rhs_expr: Expr,
        out: &mut Vec<Stmt>,
    ) {
        match &pat.kind {
            BindingPatternKind::BindingIdentifier(id) => {
                out.push(Stmt::Var(vec![(
                    id.name.as_str().to_owned(),
                    Some("*".into()),
                    Some(rhs_expr),
                )]));
            }
            BindingPatternKind::AssignmentPattern(assign) => {
                let tmp = self.alloc_temp("dflt");
                out.push(Stmt::Var(vec![(
                    tmp.clone(),
                    Some("*".into()),
                    Some(rhs_expr),
                )]));
                let def_val = self.lower_expr(&assign.right);
                let chosen = Expr::Conditional(
                    Box::new(Expr::Binary(
                        "===".into(),
                        Box::new(Expr::Name(tmp.clone())),
                        Box::new(Expr::Literal("undefined".into())),
                    )),
                    Box::new(def_val),
                    Box::new(Expr::Name(tmp)),
                );
                self.lower_destructure_pattern(&assign.left, chosen, out);
            }
            BindingPatternKind::ObjectPattern(obj) => {
                let base_tmp = self.alloc_temp("obj");
                out.push(Stmt::Var(vec![(
                    base_tmp.clone(),
                    Some("*".into()),
                    Some(rhs_expr),
                )]));
                for p in &obj.properties {
                    if let Some(kname) = prop_key_to_name(&p.key) {
                        let access = Expr::Member(Box::new(Expr::Name(base_tmp.clone())), kname);
                        self.lower_destructure_pattern(&p.value, access, out);
                    }
                }
            }
            BindingPatternKind::ArrayPattern(arr) => {
                let base_tmp = self.alloc_temp("arr");
                out.push(Stmt::Var(vec![(
                    base_tmp.clone(),
                    Some("*".into()),
                    Some(rhs_expr),
                )]));
                for (idx, elem_opt) in arr.elements.iter().enumerate() {
                    if let Some(elem_pat) = elem_opt {
                        let access = Expr::Index(
                            Box::new(Expr::Name(base_tmp.clone())),
                            Box::new(Expr::Literal(idx.to_string())),
                        );
                        self.lower_destructure_pattern(elem_pat, access, out);
                    }
                }
            }
        }
    }

    fn map_ts_type(&self, ts_ty: &TSType<'_>) -> Option<String> {
        match ts_ty {
            TSType::TSNumberKeyword(_) => Some("Number".into()),
            TSType::TSStringKeyword(_) => Some("String".into()),
            TSType::TSBooleanKeyword(_) => Some("Boolean".into()),
            TSType::TSVoidKeyword(_) => Some("void".into()),
            TSType::TSAnyKeyword(_)
            | TSType::TSUnknownKeyword(_)
            | TSType::TSNullKeyword(_)
            | TSType::TSUndefinedKeyword(_) => Some("*".into()),
            TSType::TSObjectKeyword(_) | TSType::TSTypeLiteral(_) => Some("Object".into()),
            TSType::TSFunctionType(_) => Some("Function".into()),
            TSType::TSParenthesizedType(p) => self.map_ts_type(&p.type_annotation),
            TSType::TSLiteralType(lit) => match &lit.literal {
                TSLiteral::StringLiteral(_) => Some("String".into()),
                TSLiteral::NumericLiteral(_) => Some("Number".into()),
                TSLiteral::BooleanLiteral(_) => Some("Boolean".into()),
                _ => Some("*".into()),
            },
            TSType::TSArrayType(arr) => {
                let elem = self
                    .map_ts_type(&arr.element_type)
                    .unwrap_or_else(|| "*".into());
                match elem.as_str() {
                    "Number" | "int" | "uint" | "String" | "Boolean" => {
                        Some(format!("Vector.<{elem}>"))
                    }
                    other if self.declared_classes.contains(other) => {
                        Some(format!("Vector.<{other}>"))
                    }
                    _ => Some("Array".into()),
                }
            }
            TSType::TSUnionType(u) => {
                let non_null: Vec<&TSType<'_>> = u
                    .types
                    .iter()
                    .filter(|t| {
                        !matches!(t, TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_))
                    })
                    .collect();
                if non_null.len() == 1 {
                    let inner = self.map_ts_type(non_null[0]).unwrap_or_else(|| "*".into());
                    // Nullable primitive/structural value (`GridCoord | null`, `number | null`) -> dynamic `NFValue` (`*`)
                    if matches!(
                        inner.as_str(),
                        "Number" | "int" | "uint" | "Boolean" | "String" | "Object"
                    ) && u.types.len() > 1
                    {
                        return Some("*".into());
                    }
                    return Some(inner);
                }
                Some(classify_ts_type_alias(ts_ty))
            }
            TSType::TSTypeReference(tref) => {
                let name = ts_type_name_str(&tref.type_name);
                if self.type_params_in_scope.contains(&name) {
                    return Some("*".into());
                }
                match name.as_str() {
                    "Array" | "ReadonlyArray" => {
                        let elem = tref
                            .type_arguments
                            .as_ref()
                            .and_then(|ta| ta.params.first())
                            .and_then(|t| self.map_ts_type(t))
                            .unwrap_or_else(|| "*".into());
                        if matches!(
                            elem.as_str(),
                            "Number" | "int" | "uint" | "String" | "Boolean"
                        ) || self.declared_classes.contains(&elem)
                        {
                            Some(format!("Vector.<{elem}>"))
                        } else {
                            Some("Array".into())
                        }
                    }
                    "Record" | "Map" | "Set" => Some("Object".into()),
                    _ => {
                        if let Some(mapped) = self.type_aliases.get(&name) {
                            Some(mapped.clone())
                        } else {
                            Some(name)
                        }
                    }
                }
            }
            _ => Some("*".into()),
        }
    }

    fn lower_stmt_into(&mut self, stmt: &Statement<'_>, out: &mut Vec<Stmt>) {
        let off = stmt.span().start as usize;
        match stmt {
            Statement::EmptyStatement(_)
            | Statement::ImportDeclaration(_)
            | Statement::TSInterfaceDeclaration(_)
            | Statement::TSTypeAliasDeclaration(_) => {}
            Statement::BlockStatement(blk) => {
                let mut inner = Vec::new();
                for s in &blk.body {
                    self.lower_stmt_into(s, &mut inner);
                }
                out.push(Stmt::Located(off, Box::new(Stmt::Block(inner))));
            }
            Statement::VariableDeclaration(vd) => {
                self.lower_var_decl_into(vd, out);
            }
            Statement::FunctionDeclaration(fd) => {
                if let Some(f) = self.lower_function(fd, true) {
                    out.push(Stmt::Located(off, Box::new(Stmt::Function(Box::new(f)))));
                }
            }
            Statement::ExpressionStatement(es) => {
                if let Expression::AssignmentExpression(assign) = &es.expression
                    && assign.left.as_simple_assignment_target().is_none()
                {
                    let tmp = self.alloc_temp("destr");
                    let rhs = self.lower_expr(&assign.right);
                    out.push(Stmt::Located(
                        off,
                        Box::new(Stmt::Var(vec![(tmp.clone(), Some("*".into()), Some(rhs))])),
                    ));
                    self.lower_assignment_target_pattern(&assign.left, Expr::Name(tmp), out);
                    return;
                }
                let e = self.lower_expr(&es.expression);
                out.push(Stmt::Located(off, Box::new(Stmt::Expr(e))));
            }
            Statement::IfStatement(ifs) => {
                // Browser-DOM feature guard (`if (typeof document !== "undefined") { ... }`):
                // on the native Apple target `document` and `window` are undefined, so evaluate alternate only!
                if is_browser_dom_typeof_guard(&ifs.test) {
                    self.note_issue(
                        ConversionIssueKind::Approximated,
                        ifs.span.start,
                        "Skipped browser-only DOM guard (`typeof document/window !== 'undefined'`) on native Apple target.",
                    );
                    if let Some(alt) = &ifs.alternate {
                        self.lower_stmt_into(alt, out);
                    }
                    return;
                }
                let cond = self.lower_expr(&ifs.test);
                let yes = self.lower_single_stmt(&ifs.consequent);
                let no = ifs
                    .alternate
                    .as_ref()
                    .map(|a| Box::new(self.lower_single_stmt(a)));
                out.push(Stmt::Located(
                    off,
                    Box::new(Stmt::If(cond, Box::new(yes), no)),
                ));
            }
            Statement::WhileStatement(ws) => {
                let cond = self.lower_expr(&ws.test);
                let body = self.lower_single_stmt(&ws.body);
                out.push(Stmt::Located(
                    off,
                    Box::new(Stmt::While(cond, Box::new(body))),
                ));
            }
            Statement::DoWhileStatement(dw) => {
                let body = self.lower_single_stmt(&dw.body);
                let cond = self.lower_expr(&dw.test);
                out.push(Stmt::Located(
                    off,
                    Box::new(Stmt::DoWhile(Box::new(body), cond)),
                ));
            }
            Statement::ForStatement(fs) => {
                let init_stmt = match &fs.init {
                    None => Stmt::Empty,
                    Some(ForStatementInit::VariableDeclaration(vd)) => {
                        let mut vstmts = Vec::new();
                        self.lower_var_decl_into(vd, &mut vstmts);
                        if vstmts.len() == 1 {
                            vstmts.remove(0)
                        } else {
                            Stmt::Block(vstmts)
                        }
                    }
                    Some(other) => {
                        if let Some(e) = other.as_expression() {
                            Stmt::Expr(self.lower_expr(e))
                        } else {
                            Stmt::Empty
                        }
                    }
                };
                let cond = fs.test.as_ref().map(|t| self.lower_expr(t));
                let upd = fs.update.as_ref().map(|u| self.lower_expr(u));
                let body = self.lower_single_stmt(&fs.body);
                out.push(Stmt::Located(
                    off,
                    Box::new(Stmt::For(Box::new(init_stmt), cond, upd, Box::new(body))),
                ));
            }
            Statement::ForInStatement(fi) => {
                let (init_stmt, pre_body) = self.lower_for_left(&fi.left);
                let coll = self.lower_expr(&fi.right);
                let lowered_body = self.lower_single_stmt(&fi.body);
                let body = self.prepend_to_stmt(pre_body, lowered_body);
                out.push(Stmt::Located(
                    off,
                    Box::new(Stmt::ForIn(
                        Box::new(init_stmt),
                        coll,
                        false,
                        Box::new(body),
                    )),
                ));
            }
            Statement::ForOfStatement(fo) => {
                let (init_stmt, pre_body) = self.lower_for_left(&fo.left);
                let coll = self.lower_expr(&fo.right);
                let lowered_body = self.lower_single_stmt(&fo.body);
                let body = self.prepend_to_stmt(pre_body, lowered_body);
                out.push(Stmt::Located(
                    off,
                    Box::new(Stmt::ForIn(Box::new(init_stmt), coll, true, Box::new(body))),
                ));
            }
            Statement::SwitchStatement(sw) => {
                let disc = self.lower_expr(&sw.discriminant);
                let mut cases = Vec::new();
                for c in &sw.cases {
                    let test = c.test.as_ref().map(|t| self.lower_expr(t));
                    let mut cbody = Vec::new();
                    for s in &c.consequent {
                        self.lower_stmt_into(s, &mut cbody);
                    }
                    cases.push(Case { test, body: cbody });
                }
                out.push(Stmt::Located(off, Box::new(Stmt::Switch(disc, cases))));
            }
            Statement::ReturnStatement(ret) => {
                let val = ret.argument.as_ref().map(|a| self.lower_expr(a));
                out.push(Stmt::Located(off, Box::new(Stmt::Return(val, off))));
            }
            Statement::BreakStatement(brk) => {
                let lbl = brk.label.as_ref().map(|l| l.name.as_str().to_owned());
                out.push(Stmt::Located(off, Box::new(Stmt::Break(lbl))));
            }
            Statement::ContinueStatement(cnt) => {
                let lbl = cnt.label.as_ref().map(|l| l.name.as_str().to_owned());
                out.push(Stmt::Located(off, Box::new(Stmt::Continue(lbl))));
            }
            Statement::ThrowStatement(thr) => {
                let val = self.lower_expr(&thr.argument);
                out.push(Stmt::Located(off, Box::new(Stmt::Throw(val))));
            }
            Statement::TryStatement(tr) => {
                let mut body = Vec::new();
                for s in &tr.block.body {
                    self.lower_stmt_into(s, &mut body);
                }
                let mut catches = Vec::new();
                if let Some(h) = &tr.handler {
                    let cname = h
                        .param
                        .as_ref()
                        .and_then(|p| match &p.pattern.kind {
                            BindingPatternKind::BindingIdentifier(id) => {
                                Some(id.name.as_str().to_owned())
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| "_err".into());
                    let mut cbody = Vec::new();
                    for s in &h.body.body {
                        self.lower_stmt_into(s, &mut cbody);
                    }
                    catches.push(CatchClause {
                        id: self.alloc_fn_id(),
                        offset: h.span.start as usize,
                        name: cname,
                        ty: Some("Error".into()),
                        body: cbody,
                    });
                }
                let fin = tr.finalizer.as_ref().map(|f| {
                    let mut fbody = Vec::new();
                    for s in &f.body {
                        self.lower_stmt_into(s, &mut fbody);
                    }
                    fbody
                });
                out.push(Stmt::Located(off, Box::new(Stmt::Try(body, catches, fin))));
            }
            Statement::LabeledStatement(lbl) => {
                let inner = self.lower_single_stmt(&lbl.body);
                out.push(Stmt::Located(
                    off,
                    Box::new(Stmt::Label(
                        lbl.label.name.as_str().to_owned(),
                        Box::new(inner),
                    )),
                ));
            }
            Statement::ExportNamedDeclaration(exp) => {
                if let Some(Declaration::VariableDeclaration(vd)) = &exp.declaration {
                    self.lower_var_decl_into(vd, out);
                } else if let Some(Declaration::FunctionDeclaration(fd)) = &exp.declaration
                    && let Some(f) = self.lower_function(fd, true)
                {
                    out.push(Stmt::Located(off, Box::new(Stmt::Function(Box::new(f)))));
                }
            }
            _ => {}
        }
    }

    fn lower_single_stmt(&mut self, stmt: &Statement<'_>) -> Stmt {
        let mut buf = Vec::new();
        self.lower_stmt_into(stmt, &mut buf);
        match buf.len() {
            0 => Stmt::Empty,
            1 => buf.remove(0),
            _ => Stmt::Block(buf),
        }
    }

    fn prepend_to_stmt(&self, mut prefix: Vec<Stmt>, body: Stmt) -> Stmt {
        if prefix.is_empty() {
            return body;
        }
        match body {
            Stmt::Block(b) => {
                prefix.extend(b);
                Stmt::Block(prefix)
            }
            other => {
                prefix.push(other);
                Stmt::Block(prefix)
            }
        }
    }

    fn lower_for_left(&mut self, left: &ForStatementLeft<'_>) -> (Stmt, Vec<Stmt>) {
        let mut pre = Vec::new();
        match left {
            ForStatementLeft::VariableDeclaration(vd) => {
                if let Some(decl) = vd.declarations.first() {
                    let ty = decl
                        .id
                        .type_annotation
                        .as_ref()
                        .and_then(|ann| self.map_ts_type(&ann.type_annotation));
                    if let BindingPatternKind::BindingIdentifier(id) = &decl.id.kind {
                        return (
                            Stmt::Var(vec![(id.name.as_str().to_owned(), ty, None)]),
                            pre,
                        );
                    }
                    let tmp = self.alloc_temp("iter");
                    self.lower_destructure_pattern(&decl.id, Expr::Name(tmp.clone()), &mut pre);
                    return (Stmt::Var(vec![(tmp, Some("*".into()), None)]), pre);
                }
                (Stmt::Var(vec![("_item".into(), None, None)]), pre)
            }
            ForStatementLeft::AssignmentTargetIdentifier(id) => {
                (Stmt::Expr(Expr::Name(id.name.as_str().to_owned())), pre)
            }
            _ => (Stmt::Var(vec![("_item".into(), None, None)]), pre),
        }
    }

    fn lower_var_decl_into(&mut self, vd: &VariableDeclaration<'_>, out: &mut Vec<Stmt>) {
        let off = vd.span.start as usize;
        for decl in &vd.declarations {
            let ty = decl
                .id
                .type_annotation
                .as_ref()
                .and_then(|ann| self.map_ts_type(&ann.type_annotation));
            match &decl.id.kind {
                BindingPatternKind::BindingIdentifier(id) => {
                    let init = decl.init.as_ref().map(|e| self.lower_expr(e));
                    out.push(Stmt::Located(
                        off,
                        Box::new(Stmt::Var(vec![(id.name.as_str().to_owned(), ty, init)])),
                    ));
                }
                _ => {
                    let rhs = decl
                        .init
                        .as_ref()
                        .map(|e| self.lower_expr(e))
                        .unwrap_or_else(|| Expr::Literal("undefined".into()));
                    let mut expanded = Vec::new();
                    self.lower_destructure_pattern(&decl.id, rhs, &mut expanded);
                    out.extend(expanded);
                }
            }
        }
    }

    fn lower_expr(&mut self, expr: &Expression<'_>) -> Expr {
        let off = expr.span().start as usize;
        match expr {
            Expression::BooleanLiteral(b) => {
                Expr::Literal(if b.value { "true" } else { "false" }.into())
            }
            Expression::NullLiteral(_) => Expr::Literal("null".into()),
            Expression::NumericLiteral(n) => {
                let raw = n.raw.as_ref().map(|r| r.as_str()).unwrap_or("");
                if raw.starts_with("0x") || raw.starts_with("0X") {
                    Expr::Literal(raw.replace('_', ""))
                } else if n.value.is_finite() {
                    if n.value.fract() == 0.0 && n.value.abs() < 9_007_199_254_740_991.0 {
                        Expr::Literal(format!("{}", n.value as i64))
                    } else {
                        Expr::Literal(format!("{}", n.value))
                    }
                } else {
                    Expr::Name("NaN".into())
                }
            }
            Expression::StringLiteral(s) => Expr::Literal(
                serde_json::to_string(s.value.as_str()).unwrap_or_else(|_| "\"\"".into()),
            ),
            Expression::RegExpLiteral(rx) => Expr::RegExpLiteral(
                rx.regex.pattern.text.as_str().to_owned(),
                rx.regex.flags.to_string(),
            ),
            Expression::TemplateLiteral(tpl) => {
                let mut acc: Option<Expr> = None;
                for (idx, quasi) in tpl.quasis.iter().enumerate() {
                    let cooked = quasi
                        .value
                        .cooked
                        .as_ref()
                        .map(|c| c.as_str())
                        .unwrap_or_else(|| quasi.value.raw.as_str());
                    if !cooked.is_empty() || acc.is_none() {
                        let str_lit = Expr::Literal(
                            serde_json::to_string(cooked).unwrap_or_else(|_| "\"\"".into()),
                        );
                        acc = Some(match acc {
                            None => str_lit,
                            Some(prev) => {
                                Expr::Binary("+".into(), Box::new(prev), Box::new(str_lit))
                            }
                        });
                    }
                    if let Some(interp) = tpl.expressions.get(idx) {
                        let lowered = self.lower_expr(interp);
                        let prev = acc.unwrap_or_else(|| Expr::Literal("\"\"".into()));
                        acc = Some(Expr::Binary("+".into(), Box::new(prev), Box::new(lowered)));
                    }
                }
                acc.unwrap_or_else(|| Expr::Literal("\"\"".into()))
            }
            Expression::Identifier(id) => {
                let name = id.name.as_str();
                if name == "undefined" {
                    Expr::Literal("undefined".into())
                } else if name == "globalThis" {
                    Expr::Name("_global".into())
                } else {
                    Expr::Name(name.to_owned())
                }
            }
            Expression::ThisExpression(_) => Expr::Name("this".into()),
            Expression::Super(_) => Expr::Name("super".into()),
            Expression::ParenthesizedExpression(p) => self.lower_expr(&p.expression),
            Expression::TSAsExpression(a) => self.lower_expr(&a.expression),
            Expression::TSTypeAssertion(a) => self.lower_expr(&a.expression),
            Expression::TSNonNullExpression(a) => self.lower_expr(&a.expression),
            Expression::TSSatisfiesExpression(a) => self.lower_expr(&a.expression),
            Expression::TSInstantiationExpression(a) => self.lower_expr(&a.expression),
            Expression::SequenceExpression(seq) => seq
                .expressions
                .last()
                .map(|e| self.lower_expr(e))
                .unwrap_or_else(|| Expr::Literal("undefined".into())),
            Expression::ChainExpression(ch) => match &ch.expression {
                ChainElement::CallExpression(call) => self.lower_call_expr(call),
                ChainElement::TSNonNullExpression(nn) => self.lower_expr(&nn.expression),
                ChainElement::StaticMemberExpression(mem) => Expr::Member(
                    Box::new(self.lower_expr(&mem.object)),
                    mem.property.name.as_str().to_owned(),
                ),
                ChainElement::ComputedMemberExpression(mem) => Expr::Index(
                    Box::new(self.lower_expr(&mem.object)),
                    Box::new(self.lower_expr(&mem.expression)),
                ),
                ChainElement::PrivateFieldExpression(pf) => Expr::Member(
                    Box::new(self.lower_expr(&pf.object)),
                    pf.field.name.as_str().to_owned(),
                ),
            },
            Expression::StaticMemberExpression(mem) => {
                let prop = mem.property.name.as_str();
                // Normalize `NeoFlashEvents.Event` -> `Event`, `console.log` -> `trace`
                if let Expression::Identifier(obj_id) = &mem.object {
                    match obj_id.name.as_str() {
                        "NeoFlashEvents" | "NeoFlashDisplay" | "NeoFlashGeom" | "NeoFlashText"
                        | "NeoFlashMedia" | "NeoFlashNet" | "NeoFlashUtils" => {
                            return Expr::Name(prop.to_owned());
                        }
                        "console" if matches!(prop, "log" | "info" | "warn" | "error") => {
                            return Expr::Name("trace".into());
                        }
                        "Number" if prop == "isNaN" => return Expr::Name("isNaN".into()),
                        "Number" if prop == "isFinite" => return Expr::Name("isFinite".into()),
                        "Math" if prop == "trunc" => {
                            return Expr::Name("int".into());
                        }
                        _ => {}
                    }
                }
                Expr::Member(Box::new(self.lower_expr(&mem.object)), prop.to_owned())
            }
            Expression::ComputedMemberExpression(mem) => Expr::Index(
                Box::new(self.lower_expr(&mem.object)),
                Box::new(self.lower_expr(&mem.expression)),
            ),
            Expression::PrivateFieldExpression(pf) => Expr::Member(
                Box::new(self.lower_expr(&pf.object)),
                pf.field.name.as_str().to_owned(),
            ),
            Expression::ArrayExpression(arr) => {
                let items = arr
                    .elements
                    .iter()
                    .map(|el| match el {
                        ArrayExpressionElement::Elision(_) => None,
                        ArrayExpressionElement::SpreadElement(sp) => {
                            Some(self.lower_expr(&sp.argument))
                        }
                        other => other.as_expression().map(|e| self.lower_expr(e)),
                    })
                    .collect();
                Expr::Array(items)
            }
            Expression::ObjectExpression(obj) => {
                let mut props = Vec::new();
                for p in &obj.properties {
                    if let ObjectPropertyKind::ObjectProperty(prop) = p
                        && let Some(kname) = prop_key_to_name(&prop.key)
                    {
                        let val = if prop.shorthand {
                            Expr::Name(kname.clone())
                        } else {
                            self.lower_expr(&prop.value)
                        };
                        props.push((
                            AsPropKey {
                                source: serde_json::to_string(&kname)
                                    .unwrap_or_else(|_| format!("\"{kname}\"")),
                                name: kname,
                            },
                            val,
                        ));
                    }
                }
                Expr::Object(props)
            }
            Expression::UnaryExpression(un) => match un.operator {
                UnaryOperator::Typeof => {
                    Expr::Unary("typeof".into(), Box::new(self.lower_expr(&un.argument)))
                }
                UnaryOperator::Void => {
                    Expr::Unary("void".into(), Box::new(self.lower_expr(&un.argument)))
                }
                UnaryOperator::Delete => Expr::Delete(Box::new(self.lower_expr(&un.argument)), off),
                UnaryOperator::LogicalNot => {
                    Expr::Unary("!".into(), Box::new(self.lower_expr(&un.argument)))
                }
                UnaryOperator::UnaryNegation => {
                    Expr::Unary("-".into(), Box::new(self.lower_expr(&un.argument)))
                }
                UnaryOperator::UnaryPlus => self.lower_expr(&un.argument),
                UnaryOperator::BitwiseNot => {
                    Expr::Unary("~".into(), Box::new(self.lower_expr(&un.argument)))
                }
            },
            Expression::UpdateExpression(up) => {
                let op = match up.operator {
                    UpdateOperator::Increment => "++",
                    UpdateOperator::Decrement => "--",
                };
                let tgt = self.lower_simple_assignment_target(&up.argument);
                Expr::Update(op.into(), Box::new(tgt), up.prefix)
            }
            Expression::BinaryExpression(bin) => {
                let lhs = self.lower_expr(&bin.left);
                let rhs = self.lower_expr(&bin.right);
                match bin.operator {
                    BinaryOperator::Exponential => Expr::Call(
                        Box::new(Expr::Member(
                            Box::new(Expr::Name("Math".into())),
                            "pow".into(),
                        )),
                        vec![lhs, rhs],
                        off,
                    ),
                    BinaryOperator::Instanceof => Expr::InstanceOf(Box::new(lhs), Box::new(rhs)),
                    other => Expr::Binary(other.as_str().to_owned(), Box::new(lhs), Box::new(rhs)),
                }
            }
            Expression::LogicalExpression(log) => {
                let lhs = self.lower_expr(&log.left);
                let rhs = self.lower_expr(&log.right);
                match log.operator {
                    LogicalOperator::And => Expr::Binary("&&".into(), Box::new(lhs), Box::new(rhs)),
                    LogicalOperator::Or => Expr::Binary("||".into(), Box::new(lhs), Box::new(rhs)),
                    LogicalOperator::Coalesce => {
                        // `a ?? b` -> `(a != null ? a : b)`
                        Expr::Conditional(
                            Box::new(Expr::Binary(
                                "!=".into(),
                                Box::new(lhs.clone()),
                                Box::new(Expr::Literal("null".into())),
                            )),
                            Box::new(lhs),
                            Box::new(rhs),
                        )
                    }
                }
            }
            Expression::ConditionalExpression(cond) => Expr::Conditional(
                Box::new(self.lower_expr(&cond.test)),
                Box::new(self.lower_expr(&cond.consequent)),
                Box::new(self.lower_expr(&cond.alternate)),
            ),
            Expression::AssignmentExpression(assign) => {
                if assign.left.as_simple_assignment_target().is_none() {
                    let tmp = self.alloc_temp("destr");
                    let rhs = self.lower_expr(&assign.right);
                    let mut stmts =
                        vec![Stmt::Var(vec![(tmp.clone(), Some("*".into()), Some(rhs))])];
                    self.lower_assignment_target_pattern(
                        &assign.left,
                        Expr::Name(tmp.clone()),
                        &mut stmts,
                    );
                    stmts.push(Stmt::Return(Some(Expr::Name(tmp)), off));
                    let id = self.alloc_fn_id();
                    return Expr::Call(
                        Box::new(Expr::Function(Box::new(Function {
                            id,
                            offset: off,
                            name: None,
                            params: Vec::new(),
                            defaults: Vec::new(),
                            default_offsets: Vec::new(),
                            rest: None,
                            returns: Some("*".into()),
                            body: stmts,
                            declaration: false,
                        }))),
                        Vec::new(),
                        off,
                    );
                }
                let lhs = self.lower_assignment_target(&assign.left);
                let rhs = self.lower_expr(&assign.right);
                let op_str = assign.operator.as_str();
                match op_str {
                    "??=" => Expr::Assign(
                        "=".into(),
                        Box::new(lhs.clone()),
                        Box::new(Expr::Conditional(
                            Box::new(Expr::Binary(
                                "!=".into(),
                                Box::new(lhs.clone()),
                                Box::new(Expr::Literal("null".into())),
                            )),
                            Box::new(lhs),
                            Box::new(rhs),
                        )),
                    ),
                    "||=" => Expr::Assign(
                        "=".into(),
                        Box::new(lhs.clone()),
                        Box::new(Expr::Conditional(
                            Box::new(lhs.clone()),
                            Box::new(lhs),
                            Box::new(rhs),
                        )),
                    ),
                    "&&=" => Expr::Assign(
                        "=".into(),
                        Box::new(lhs.clone()),
                        Box::new(Expr::Conditional(
                            Box::new(lhs.clone()),
                            Box::new(rhs),
                            Box::new(lhs),
                        )),
                    ),
                    "**=" => Expr::Assign(
                        "=".into(),
                        Box::new(lhs.clone()),
                        Box::new(Expr::Call(
                            Box::new(Expr::Member(
                                Box::new(Expr::Name("Math".into())),
                                "pow".into(),
                            )),
                            vec![lhs, rhs],
                            off,
                        )),
                    ),
                    other => Expr::Assign(other.to_owned(), Box::new(lhs), Box::new(rhs)),
                }
            }
            Expression::CallExpression(call) => self.lower_call_expr(call),
            Expression::NewExpression(ne) => {
                let args: Vec<Expr> = ne
                    .arguments
                    .iter()
                    .filter_map(|a| a.as_expression().map(|e| self.lower_expr(e)))
                    .collect();
                let ty_name = match &ne.callee {
                    Expression::Identifier(id) => id.name.as_str().to_owned(),
                    Expression::StaticMemberExpression(mem) => {
                        mem.property.name.as_str().to_owned()
                    }
                    other => {
                        let c = self.lower_expr(other);
                        return Expr::Call(Box::new(c), args, off);
                    }
                };
                Expr::NewClass(ty_name, args, off)
            }
            Expression::ArrowFunctionExpression(arrow) => {
                let (params, defaults, rest, mut body) = self.lower_formal_params(&arrow.params);
                let returns = arrow
                    .return_type
                    .as_ref()
                    .and_then(|rt| self.map_ts_type(&rt.type_annotation));
                if arrow.expression
                    && let Some(Statement::ExpressionStatement(es)) = arrow.body.statements.first()
                {
                    let val = self.lower_expr(&es.expression);
                    body.push(Stmt::Return(Some(val), off));
                } else {
                    for s in &arrow.body.statements {
                        self.lower_stmt_into(s, &mut body);
                    }
                }
                let id = self.alloc_fn_id();
                Expr::Function(Box::new(Function {
                    id,
                    offset: off,
                    name: None,
                    params,
                    defaults,
                    default_offsets: Vec::new(),
                    rest,
                    returns,
                    body,
                    declaration: false,
                }))
            }
            Expression::FunctionExpression(fe) => {
                if let Some(f) = self.lower_function(fe, false) {
                    Expr::Function(Box::new(f))
                } else {
                    Expr::Literal("null".into())
                }
            }
            _ => Expr::Literal("undefined".into()),
        }
    }

    fn lower_call_expr(&mut self, call: &CallExpression<'_>) -> Expr {
        let off = call.span.start as usize;
        let args: Vec<Expr> = call
            .arguments
            .iter()
            .filter_map(|a| match a {
                Argument::SpreadElement(sp) => Some(self.lower_expr(&sp.argument)),
                other => other.as_expression().map(|e| self.lower_expr(e)),
            })
            .collect();

        // Special JS/TS helpers (`Math.imul(a, b)` -> `(a * b) | 0`, `Math.sign`, `Object.keys`, `Array.isArray`)
        if let Expression::StaticMemberExpression(mem) = &call.callee
            && let Expression::Identifier(obj_id) = &mem.object
        {
            match (obj_id.name.as_str(), mem.property.name.as_str()) {
                ("Math", "imul") if args.len() == 2 => {
                    return Expr::Call(
                        Box::new(Expr::Member(
                            Box::new(Expr::Name("NFRuntime".into())),
                            "imul".into(),
                        )),
                        args,
                        off,
                    );
                }
                ("Math", "trunc") if args.len() == 1 => {
                    return Expr::Call(Box::new(Expr::Name("int".into())), args, off);
                }
                _ => {}
            }
        }

        let callee = self.lower_expr(&call.callee);
        Expr::Call(Box::new(callee), args, off)
    }

    fn lower_assignment_target_pattern(
        &mut self,
        tgt: &AssignmentTarget<'_>,
        src: Expr,
        out: &mut Vec<Stmt>,
    ) {
        if let Some(simple) = tgt.as_simple_assignment_target() {
            let lhs = self.lower_simple_assignment_target(simple);
            out.push(Stmt::Expr(Expr::Assign(
                "=".into(),
                Box::new(lhs),
                Box::new(src),
            )));
            return;
        }
        match tgt {
            AssignmentTarget::ArrayAssignmentTarget(arr) => {
                for (idx, el_opt) in arr.elements.iter().enumerate() {
                    let Some(el) = el_opt else {
                        continue;
                    };
                    let item_expr = Expr::Index(
                        Box::new(src.clone()),
                        Box::new(Expr::Literal(idx.to_string())),
                    );
                    self.lower_assignment_maybe_default(el, item_expr, out);
                }
            }
            AssignmentTarget::ObjectAssignmentTarget(obj) => {
                for prop in &obj.properties {
                    match prop {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(id) => {
                            let name = id.binding.name.as_str().to_owned();
                            let raw_val = Expr::Member(Box::new(src.clone()), name.clone());
                            let val_expr = if let Some(def) = &id.init {
                                let d = self.lower_expr(def);
                                Expr::Conditional(
                                    Box::new(Expr::Binary(
                                        "!=".into(),
                                        Box::new(raw_val.clone()),
                                        Box::new(Expr::Literal("null".into())),
                                    )),
                                    Box::new(raw_val),
                                    Box::new(d),
                                )
                            } else {
                                raw_val
                            };
                            out.push(Stmt::Expr(Expr::Assign(
                                "=".into(),
                                Box::new(Expr::Name(name)),
                                Box::new(val_expr),
                            )));
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                            if let Some(kname) = prop_key_to_name(&p.name) {
                                let raw_val = Expr::Member(Box::new(src.clone()), kname);
                                self.lower_assignment_maybe_default(&p.binding, raw_val, out);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn lower_assignment_maybe_default(
        &mut self,
        target: &AssignmentTargetMaybeDefault<'_>,
        src: Expr,
        out: &mut Vec<Stmt>,
    ) {
        match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(wd) => {
                let def = self.lower_expr(&wd.init);
                let val_expr = Expr::Conditional(
                    Box::new(Expr::Binary(
                        "!=".into(),
                        Box::new(src.clone()),
                        Box::new(Expr::Literal("null".into())),
                    )),
                    Box::new(src),
                    Box::new(def),
                );
                self.lower_assignment_target_pattern(&wd.binding, val_expr, out);
            }
            other => {
                if let Some(at) = other.as_assignment_target() {
                    self.lower_assignment_target_pattern(at, src, out);
                }
            }
        }
    }

    fn lower_assignment_target(&mut self, tgt: &AssignmentTarget<'_>) -> Expr {
        if let Some(simple) = tgt.as_simple_assignment_target() {
            return self.lower_simple_assignment_target(simple);
        }
        Expr::Literal("undefined".into())
    }

    fn lower_simple_assignment_target(&mut self, tgt: &SimpleAssignmentTarget<'_>) -> Expr {
        match tgt {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                Expr::Name(id.name.as_str().to_owned())
            }
            SimpleAssignmentTarget::StaticMemberExpression(mem) => Expr::Member(
                Box::new(self.lower_expr(&mem.object)),
                mem.property.name.as_str().to_owned(),
            ),
            SimpleAssignmentTarget::ComputedMemberExpression(mem) => Expr::Index(
                Box::new(self.lower_expr(&mem.object)),
                Box::new(self.lower_expr(&mem.expression)),
            ),
            SimpleAssignmentTarget::PrivateFieldExpression(pf) => Expr::Member(
                Box::new(self.lower_expr(&pf.object)),
                pf.field.name.as_str().to_owned(),
            ),
            SimpleAssignmentTarget::TSAsExpression(a) => self.lower_expr(&a.expression),
            SimpleAssignmentTarget::TSTypeAssertion(a) => self.lower_expr(&a.expression),
            SimpleAssignmentTarget::TSNonNullExpression(a) => self.lower_expr(&a.expression),
            SimpleAssignmentTarget::TSSatisfiesExpression(a) => self.lower_expr(&a.expression),
        }
    }
}

fn prop_key_to_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str().to_owned()),
        PropertyKey::PrivateIdentifier(id) => Some(id.name.as_str().to_owned()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str().to_owned()),
        PropertyKey::NumericLiteral(n) => Some(format!("{}", n.value)),
        _ => None,
    }
}

fn ts_type_name_str(name: &TSTypeName<'_>) -> String {
    match name {
        TSTypeName::IdentifierReference(id) => id.name.as_str().to_owned(),
        TSTypeName::QualifiedName(qn) => {
            format!("{}.{}", ts_type_name_str(&qn.left), qn.right.name.as_str())
        }
    }
}

fn is_browser_dom_typeof_guard(test: &Expression<'_>) -> bool {
    let Expression::BinaryExpression(bin) = test else {
        return false;
    };
    if !matches!(
        bin.operator,
        BinaryOperator::StrictInequality | BinaryOperator::Inequality
    ) {
        return false;
    }
    let check_pair = |typeof_side: &Expression<'_>, str_side: &Expression<'_>| -> bool {
        if let Expression::UnaryExpression(un) = typeof_side
            && un.operator == UnaryOperator::Typeof
            && let Expression::Identifier(id) = &un.argument
            && matches!(id.name.as_str(), "document" | "window")
            && let Expression::StringLiteral(s) = str_side
            && s.value.as_str() == "undefined"
        {
            return true;
        }
        false
    };
    check_pair(&bin.left, &bin.right) || check_pair(&bin.right, &bin.left)
}

fn body_has_value_return(stmts: &[Statement<'_>]) -> bool {
    for s in stmts {
        match s {
            Statement::ReturnStatement(r) if r.argument.is_some() => return true,
            Statement::BlockStatement(b) if body_has_value_return(&b.body) => return true,
            Statement::IfStatement(i) => {
                if body_has_value_return(std::slice::from_ref(&i.consequent))
                    || i.alternate
                        .as_ref()
                        .is_some_and(|a| body_has_value_return(std::slice::from_ref(a)))
                {
                    return true;
                }
            }
            Statement::WhileStatement(w)
                if body_has_value_return(std::slice::from_ref(&w.body)) =>
            {
                return true;
            }
            Statement::ForStatement(f) if body_has_value_return(std::slice::from_ref(&f.body)) => {
                return true;
            }
            Statement::ForInStatement(f)
                if body_has_value_return(std::slice::from_ref(&f.body)) =>
            {
                return true;
            }
            Statement::ForOfStatement(f)
                if body_has_value_return(std::slice::from_ref(&f.body)) =>
            {
                return true;
            }
            Statement::SwitchStatement(sw)
                if sw
                    .cases
                    .iter()
                    .any(|c| body_has_value_return(&c.consequent)) =>
            {
                return true;
            }
            Statement::TryStatement(tr)
                if body_has_value_return(&tr.block.body)
                    || tr
                        .handler
                        .as_ref()
                        .is_some_and(|h| body_has_value_return(&h.body.body)) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}
