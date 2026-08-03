use super::{
    looks_like_sql, parameters, BootstrapCandidate, ImportReference, MigrationCandidate,
    MigrationCandidateSource, ModuleFacts, SqlExpression, StaticSql, StaticTemplate,
    StaticTemplateExpression,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use swc_core::common::{sync::Lrc, SourceMap, SourceMapper, Span, Spanned};
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};

pub(super) struct ModuleCollector {
    path: String,
    source_map: Lrc<SourceMap>,
    bindings: BTreeMap<String, Option<SqlExpression>>,
    fragment_bindings: BTreeMap<String, Option<Vec<Vec<String>>>>,
    exports: BTreeSet<String>,
    imports: BTreeMap<String, ImportReference>,
    bootstraps: Vec<BootstrapCandidate>,
    migrations: Vec<MigrationCandidate>,
    queries: Vec<SqlExpression>,
}

impl ModuleCollector {
    pub(super) fn new(path: String, source_map: Lrc<SourceMap>) -> Self {
        Self {
            path,
            source_map,
            bindings: BTreeMap::new(),
            fragment_bindings: BTreeMap::new(),
            exports: BTreeSet::new(),
            imports: BTreeMap::new(),
            bootstraps: Vec::new(),
            migrations: Vec::new(),
            queries: Vec::new(),
        }
    }

    pub(super) fn finish(self) -> ModuleFacts {
        ModuleFacts {
            bindings: self
                .bindings
                .into_iter()
                .filter_map(|(name, value)| value.map(|value| (name, value)))
                .collect(),
            exports: self.exports,
            imports: self.imports,
            bootstraps: self.bootstraps,
            migrations: self.migrations,
            queries: self.queries,
        }
    }

    fn sql_expression(&self, expression: &Expr) -> Option<SqlExpression> {
        match expression {
            Expr::Lit(Lit::Str(value)) => Some(SqlExpression::Value(self.static_sql(
                value.value.to_string(),
                value.span,
                false,
            ))),
            Expr::Tpl(template) => Some(SqlExpression::Template(self.static_template(template))),
            Expr::TaggedTpl(template) => {
                Some(SqlExpression::Template(self.static_template(&template.tpl)))
            }
            Expr::Ident(identifier) => Some(SqlExpression::Binding(identifier.sym.to_string())),
            Expr::Paren(expression) => self.sql_expression(&expression.expr),
            Expr::TsAs(expression) => self.sql_expression(&expression.expr),
            Expr::TsSatisfies(expression) => self.sql_expression(&expression.expr),
            Expr::TsTypeAssertion(expression) => self.sql_expression(&expression.expr),
            Expr::TsConstAssertion(expression) => self.sql_expression(&expression.expr),
            Expr::TsNonNull(expression) => self.sql_expression(&expression.expr),
            _ => None,
        }
    }

    fn static_template(&self, template: &Tpl) -> StaticTemplate {
        let location = self.source_map.lookup_char_pos(template.span.lo);
        StaticTemplate {
            path: self.path.clone(),
            line: u32::try_from(location.line).unwrap_or(u32::MAX),
            column: u32::try_from(location.col.0 + 1).unwrap_or(u32::MAX),
            quasis: template
                .quasis
                .iter()
                .map(|quasi| {
                    quasi
                        .cooked
                        .as_ref()
                        .map_or_else(|| quasi.raw.as_ref(), |cooked| cooked.as_ref())
                        .to_string()
                })
                .collect(),
            expressions: template
                .exprs
                .iter()
                .enumerate()
                .map(|(index, expression)| {
                    let source = self
                        .source_map
                        .span_to_snippet(expression.span())
                        .unwrap_or_else(|_| format!("{expression:?}"));
                    StaticTemplateExpression {
                        value: self.sql_expression(expression),
                        unresolved_marker: format!(
                            "$codeatlas_{}_{:x}",
                            index + 1,
                            Sha256::digest(source.as_bytes())
                        ),
                    }
                })
                .collect(),
        }
    }

    fn tagged_query_sql(&self, template: &TaggedTpl) -> StaticSql {
        let tag = expression_chain(&template.tag);
        let static_text = template
            .tpl
            .quasis
            .iter()
            .map(|quasi| {
                quasi
                    .cooked
                    .as_ref()
                    .map_or_else(|| quasi.raw.as_ref(), |cooked| cooked.as_ref())
            })
            .collect::<String>();
        let static_parameters = parameters::analyze(&static_text);
        let mixed_parameter_syntax = static_parameters.count > 0 || static_parameters.dynamic;
        let mut text = String::new();
        let mut dynamic = mixed_parameter_syntax;
        for (index, quasi) in template.tpl.quasis.iter().enumerate() {
            if index > 0 {
                let expression = &template.tpl.exprs[index - 1];
                if mixed_parameter_syntax
                    || is_explicit_sql_fragment(expression)
                    || self.tagged_interpolation_is_dynamic(tag.as_deref(), expression)
                {
                    let source = self
                        .source_map
                        .span_to_snippet(expression.span())
                        .unwrap_or_else(|_| format!("{expression:?}"));
                    text.push_str(&format!(
                        "$codeatlas_{}_{:x}",
                        index,
                        Sha256::digest(source.as_bytes())
                    ));
                    dynamic = true;
                } else {
                    text.push('$');
                    let index = u32::try_from(index).unwrap_or(u32::MAX);
                    text.push_str(&index.to_string());
                }
            }
            text.push_str(
                quasi
                    .cooked
                    .as_ref()
                    .map_or_else(|| quasi.raw.as_ref(), |cooked| cooked.as_ref()),
            );
        }
        self.static_sql(text, template.span, dynamic)
    }

    fn fragment_expression_chains(&self, expression: &Expr) -> Vec<Vec<String>> {
        match unwrap_expression(expression) {
            Expr::Call(call) => match &call.callee {
                Callee::Expr(callee) => {
                    let mut chains = expression_chain(callee).into_iter().collect::<Vec<_>>();
                    chains.extend(
                        call.args
                            .iter()
                            .flat_map(|argument| self.fragment_expression_chains(&argument.expr)),
                    );
                    chains
                }
                _ => Vec::new(),
            },
            Expr::TaggedTpl(template) => expression_chain(&template.tag).into_iter().collect(),
            Expr::Ident(identifier) => self
                .fragment_bindings
                .get(identifier.sym.as_ref())
                .and_then(Clone::clone)
                .unwrap_or_default(),
            Expr::Cond(conditional) => self
                .fragment_expression_chains(&conditional.cons)
                .into_iter()
                .chain(self.fragment_expression_chains(&conditional.alt))
                .collect(),
            Expr::Bin(binary) => self
                .fragment_expression_chains(&binary.left)
                .into_iter()
                .chain(self.fragment_expression_chains(&binary.right))
                .collect(),
            Expr::Array(array) => array
                .elems
                .iter()
                .flatten()
                .flat_map(|element| self.fragment_expression_chains(&element.expr))
                .collect(),
            Expr::Seq(sequence) => sequence
                .exprs
                .iter()
                .flat_map(|expression| self.fragment_expression_chains(expression))
                .collect(),
            Expr::Await(awaited) => self.fragment_expression_chains(&awaited.arg),
            Expr::Unary(unary) => self.fragment_expression_chains(&unary.arg),
            _ => Vec::new(),
        }
    }

    fn tagged_interpolation_is_dynamic(&self, tag: Option<&[String]>, expression: &Expr) -> bool {
        tag.is_some_and(|tag| {
            self.fragment_expression_chains(expression)
                .iter()
                .any(|candidate| candidate.starts_with(tag))
        })
    }

    fn static_sql(&self, text: String, span: Span, dynamic: bool) -> StaticSql {
        let location = self.source_map.lookup_char_pos(span.lo);
        StaticSql {
            text,
            path: self.path.clone(),
            line: u32::try_from(location.line).unwrap_or(u32::MAX),
            column: u32::try_from(location.col.0 + 1).unwrap_or(u32::MAX),
            dynamic,
        }
    }

    fn bootstrap_candidates(&self, expression: &Expr) -> Vec<BootstrapCandidate> {
        let expression = unwrap_expression(expression);
        if let Expr::Array(array) = expression {
            return array
                .elems
                .iter()
                .enumerate()
                .filter_map(|(index, element)| {
                    let element = element.as_ref()?;
                    if element.spread.is_some() {
                        return None;
                    }
                    let sql = self.sql_expression(&element.expr)?;
                    let name = match &*element.expr {
                        Expr::Ident(identifier) => identifier.sym.to_string(),
                        _ => format!("bootstrapSql[{index}]"),
                    };
                    Some(BootstrapCandidate { name, sql })
                })
                .collect();
        }
        self.sql_expression(expression)
            .map(|sql| BootstrapCandidate {
                name: "bootstrapSql".to_string(),
                sql,
            })
            .into_iter()
            .collect()
    }
}

impl Visit for ModuleCollector {
    fn visit_export_decl(&mut self, export: &ExportDecl) {
        if let Decl::Var(declaration) = &export.decl {
            for declarator in &declaration.decls {
                if let Pat::Ident(identifier) = &declarator.name {
                    self.exports.insert(identifier.id.sym.to_string());
                }
            }
        }
        export.visit_children_with(self);
    }

    fn visit_named_export(&mut self, export: &NamedExport) {
        if export.type_only {
            return;
        }
        for specifier in &export.specifiers {
            if let ExportSpecifier::Named(specifier) = specifier {
                if specifier.is_type_only {
                    continue;
                }
                let original = module_export_name(&specifier.orig);
                let exported = specifier
                    .exported
                    .as_ref()
                    .map(module_export_name)
                    .unwrap_or_else(|| original.clone());
                self.exports.insert(exported.clone());
                if let Some(source) = &export.src {
                    self.imports.insert(
                        exported,
                        ImportReference {
                            source: source.value.to_string(),
                            imported: original,
                        },
                    );
                } else if exported != original {
                    self.bindings
                        .insert(exported, Some(SqlExpression::Binding(original)));
                }
            }
        }
    }

    fn visit_import_decl(&mut self, import: &ImportDecl) {
        if import.type_only {
            return;
        }
        for specifier in &import.specifiers {
            match specifier {
                ImportSpecifier::Named(specifier) if !specifier.is_type_only => {
                    self.imports.insert(
                        specifier.local.sym.to_string(),
                        ImportReference {
                            source: import.src.value.to_string(),
                            imported: specifier
                                .imported
                                .as_ref()
                                .map(module_export_name)
                                .unwrap_or_else(|| specifier.local.sym.to_string()),
                        },
                    );
                }
                ImportSpecifier::Default(specifier) => {
                    self.imports.insert(
                        specifier.local.sym.to_string(),
                        ImportReference {
                            source: import.src.value.to_string(),
                            imported: "default".to_string(),
                        },
                    );
                }
                ImportSpecifier::Namespace(_) | ImportSpecifier::Named(_) => {}
            }
        }
    }

    fn visit_var_declarator(&mut self, declaration: &VarDeclarator) {
        if let Pat::Ident(identifier) = &declaration.name {
            let fragment = declaration
                .init
                .as_deref()
                .map(|expression| self.fragment_expression_chains(expression))
                .filter(|chains| !chains.is_empty());
            self.fragment_bindings
                .entry(identifier.id.sym.to_string())
                .and_modify(|existing| *existing = None)
                .or_insert(fragment);
            let value = declaration
                .init
                .as_deref()
                .and_then(|expression| self.sql_expression(expression));
            self.bindings
                .entry(identifier.id.sym.to_string())
                .and_modify(|existing| *existing = None)
                .or_insert(value);
        }
        declaration.visit_children_with(self);
    }

    fn visit_object_lit(&mut self, object: &ObjectLit) {
        let mut name = None;
        let mut id = None;
        let mut sql = None;
        let mut file = None;
        let mut bootstraps = Vec::new();
        for property in &object.props {
            let PropOrSpread::Prop(property) = property else {
                continue;
            };
            match &**property {
                Prop::KeyValue(property) => match property_name(&property.key).as_deref() {
                    Some("name") => name = literal_string(&property.value),
                    Some("id") => id = literal_string(&property.value),
                    Some("sql") => sql = self.sql_expression(&property.value),
                    Some("file") => file = literal_string(&property.value),
                    Some("bootstrapSql") => {
                        bootstraps.extend(self.bootstrap_candidates(&property.value));
                    }
                    _ => {}
                },
                Prop::Shorthand(identifier) if identifier.sym == *"sql" => {
                    sql = Some(SqlExpression::Binding(identifier.sym.to_string()));
                }
                _ => {}
            }
        }
        if let (Some(name), Some(sql)) = (name, sql) {
            let line = self.source_map.lookup_char_pos(object.span.lo).line;
            self.migrations.push(MigrationCandidate {
                name,
                source: MigrationCandidateSource::Sql(sql),
                line: u32::try_from(line).unwrap_or(u32::MAX),
            });
        }
        if let (Some(id), Some(file)) = (id, file) {
            let line = self.source_map.lookup_char_pos(object.span.lo).line;
            self.migrations.push(MigrationCandidate {
                name: id,
                source: MigrationCandidateSource::ProjectFile(file),
                line: u32::try_from(line).unwrap_or(u32::MAX),
            });
        }
        self.bootstraps.extend(bootstraps);
        object.visit_children_with(self);
    }

    fn visit_call_expr(&mut self, call: &CallExpr) {
        if call_method_name(call).is_some_and(is_sql_call_method) {
            if let Some(argument) = call.args.first() {
                if argument.spread.is_none()
                    && !matches!(unwrap_expression(&argument.expr), Expr::TaggedTpl(_))
                {
                    if let Some(sql) = self.sql_expression(&argument.expr) {
                        self.queries.push(sql);
                    }
                }
            }
        }
        call.visit_children_with(self);
    }

    fn visit_tagged_tpl(&mut self, template: &TaggedTpl) {
        let sql = self.tagged_query_sql(template);
        if looks_like_sql(&sql.text) {
            self.queries.push(SqlExpression::Value(sql));
        }
    }
}

fn expression_chain(expression: &Expr) -> Option<Vec<String>> {
    match unwrap_expression(expression) {
        Expr::Ident(identifier) => Some(vec![identifier.sym.to_string()]),
        Expr::Member(member) => {
            let mut chain = expression_chain(&member.obj)?;
            let property = match &member.prop {
                MemberProp::Ident(identifier) => identifier.sym.to_string(),
                MemberProp::Computed(computed) => match unwrap_expression(&computed.expr) {
                    Expr::Lit(Lit::Str(value)) => value.value.to_string(),
                    _ => return None,
                },
                MemberProp::PrivateName(_) => return None,
            };
            chain.push(property);
            Some(chain)
        }
        _ => None,
    }
}

fn unwrap_expression(mut expression: &Expr) -> &Expr {
    loop {
        expression = match expression {
            Expr::Paren(value) => &value.expr,
            Expr::TsAs(value) => &value.expr,
            Expr::TsSatisfies(value) => &value.expr,
            Expr::TsTypeAssertion(value) => &value.expr,
            Expr::TsConstAssertion(value) => &value.expr,
            Expr::TsNonNull(value) => &value.expr,
            Expr::TsInstantiation(value) => &value.expr,
            _ => return expression,
        };
    }
}

fn module_export_name(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(identifier) => identifier.sym.to_string(),
        ModuleExportName::Str(value) => value.value.to_string(),
    }
}

fn property_name(name: &PropName) -> Option<String> {
    match name {
        PropName::Ident(identifier) => Some(identifier.sym.to_string()),
        PropName::Str(value) => Some(value.value.to_string()),
        _ => None,
    }
}

fn literal_string(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Lit(Lit::Str(value)) => Some(value.value.to_string()),
        Expr::Tpl(template) if template.exprs.is_empty() => Some(
            template
                .quasis
                .iter()
                .map(|quasi| {
                    quasi
                        .cooked
                        .as_ref()
                        .map_or_else(|| quasi.raw.as_ref(), |cooked| cooked.as_ref())
                })
                .collect(),
        ),
        Expr::Paren(expression) => literal_string(&expression.expr),
        Expr::TsAs(expression) => literal_string(&expression.expr),
        _ => None,
    }
}

fn call_method_name(call: &CallExpr) -> Option<&str> {
    let Callee::Expr(callee) = &call.callee else {
        return None;
    };
    expression_method_name(callee)
}

fn expression_method_name(expression: &Expr) -> Option<&str> {
    let Expr::Member(member) = unwrap_expression(expression) else {
        return None;
    };
    match &member.prop {
        MemberProp::Ident(identifier) => Some(identifier.sym.as_ref()),
        MemberProp::Computed(computed) => match &*computed.expr {
            Expr::Lit(Lit::Str(value)) => Some(value.value.as_ref()),
            _ => None,
        },
        MemberProp::PrivateName(_) => None,
    }
}

fn is_sql_call_method(name: &str) -> bool {
    matches!(
        name,
        "query"
            | "execute"
            | "none"
            | "any"
            | "many"
            | "one"
            | "oneOrNone"
            | "result"
            | "$queryRaw"
            | "$executeRaw"
            | "$queryRawUnsafe"
            | "$executeRawUnsafe"
    )
}

fn is_explicit_sql_fragment(expression: &Expr) -> bool {
    if let Expr::TaggedTpl(template) = unwrap_expression(expression) {
        return expression_method_name(&template.tag)
            .is_some_and(|name| matches!(name, "raw" | "join" | "sql"));
    }
    let Expr::Call(call) = unwrap_expression(expression) else {
        return false;
    };
    call_method_name(call).is_some_and(|name| matches!(name, "raw" | "join" | "sql"))
}
