use crate::languages::ecmascript::resolver::resolve_relative_module;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use swc_core::common::{sync::Lrc, SourceMap, SourceMapper, Span, Spanned};
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};

#[derive(Clone, Debug)]
pub(super) struct EmbeddedBootstrap {
    pub name: String,
    pub sql: StaticSql,
}

#[derive(Clone, Debug)]
pub(super) struct EmbeddedMigration {
    pub name: String,
    pub sql: StaticSql,
}

#[derive(Clone, Debug)]
pub(super) struct EmbeddedQuery {
    pub sql: StaticSql,
}

#[derive(Clone, Debug)]
pub(super) struct StaticSql {
    pub text: String,
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub dynamic: bool,
}

#[derive(Clone, Debug)]
pub(super) struct UnresolvedMigration {
    pub name: String,
    pub path: String,
    pub line: u32,
}

#[derive(Default)]
pub(super) struct ExtractedSource {
    pub bootstraps: Vec<EmbeddedBootstrap>,
    pub migrations: Vec<EmbeddedMigration>,
    pub queries: Vec<EmbeddedQuery>,
    pub unresolved_migrations: Vec<UnresolvedMigration>,
}

pub(super) fn extract(root: &Path, paths: &[PathBuf]) -> Result<ExtractedSource> {
    let mut resolver = StaticSqlResolver::new(root);
    let mut extracted = ExtractedSource::default();
    for path in paths {
        let display = crate::paths::normalize_relative_path(path, root);
        let facts = resolver.load(&display)?.clone();
        for name in &facts.exports {
            let Some(expression) = facts.bindings.get(name) else {
                continue;
            };
            if let Some(sql) = resolver.resolve(&display, expression, &mut HashSet::new())? {
                if !sql.dynamic && looks_like_bootstrap_sql(&sql.text) {
                    extracted.bootstraps.push(EmbeddedBootstrap {
                        name: name.clone(),
                        sql,
                    });
                }
            }
        }
        for bootstrap in facts.bootstraps {
            if let Some(sql) = resolver.resolve(&display, &bootstrap.sql, &mut HashSet::new())? {
                if !sql.dynamic && looks_like_bootstrap_sql(&sql.text) {
                    extracted.bootstraps.push(EmbeddedBootstrap {
                        name: bootstrap.name,
                        sql,
                    });
                }
            }
        }
        for migration in facts.migrations {
            let resolved = match &migration.source {
                MigrationCandidateSource::Sql(sql) => {
                    resolver.resolve(&display, sql, &mut HashSet::new())?
                }
                MigrationCandidateSource::ProjectFile(path) => {
                    resolve_project_sql_file(root, path)?
                }
            };
            match resolved {
                Some(sql) if !sql.dynamic && looks_like_sql(&sql.text) => {
                    extracted.migrations.push(EmbeddedMigration {
                        name: migration.name,
                        sql,
                    });
                }
                Some(_) => extracted.unresolved_migrations.push(UnresolvedMigration {
                    name: migration.name,
                    path: display.clone(),
                    line: migration.line,
                }),
                None => extracted.unresolved_migrations.push(UnresolvedMigration {
                    name: migration.name,
                    path: display.clone(),
                    line: migration.line,
                }),
            }
        }
        for query in facts.queries {
            if let Some(sql) = resolver.resolve(&display, &query, &mut HashSet::new())? {
                if looks_like_sql(&sql.text) {
                    extracted.queries.push(EmbeddedQuery { sql });
                }
            }
        }
    }
    extracted.migrations.sort_by(|left, right| {
        (&left.name, &left.sql.path, left.sql.line, left.sql.column).cmp(&(
            &right.name,
            &right.sql.path,
            right.sql.line,
            right.sql.column,
        ))
    });
    extracted.queries.sort_by(|left, right| {
        (&left.sql.path, left.sql.line, left.sql.column).cmp(&(
            &right.sql.path,
            right.sql.line,
            right.sql.column,
        ))
    });
    extracted.queries.dedup_by(|left, right| {
        left.sql.path == right.sql.path
            && left.sql.line == right.sql.line
            && left.sql.column == right.sql.column
    });
    extracted.unresolved_migrations.sort_by(|left, right| {
        (&left.name, &left.path, left.line).cmp(&(&right.name, &right.path, right.line))
    });
    Ok(extracted)
}

#[derive(Clone)]
struct ModuleFacts {
    bindings: BTreeMap<String, SqlExpression>,
    exports: BTreeSet<String>,
    imports: BTreeMap<String, ImportReference>,
    bootstraps: Vec<BootstrapCandidate>,
    migrations: Vec<MigrationCandidate>,
    queries: Vec<SqlExpression>,
}

#[derive(Clone)]
struct BootstrapCandidate {
    name: String,
    sql: SqlExpression,
}

#[derive(Clone)]
struct MigrationCandidate {
    name: String,
    source: MigrationCandidateSource,
    line: u32,
}

#[derive(Clone)]
enum MigrationCandidateSource {
    Sql(SqlExpression),
    ProjectFile(String),
}

#[derive(Clone)]
struct ImportReference {
    source: String,
    imported: String,
}

#[derive(Clone, Debug)]
enum SqlExpression {
    Value(StaticSql),
    Binding(String),
}

struct StaticSqlResolver<'a> {
    root: &'a Path,
    modules: BTreeMap<String, ModuleFacts>,
}

impl<'a> StaticSqlResolver<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            modules: BTreeMap::new(),
        }
    }

    fn load(&mut self, display: &str) -> Result<&ModuleFacts> {
        if !self.modules.contains_key(display) {
            let path = self.root.join(display);
            let (module, source_map) =
                crate::languages::typescript::parser::parse_syntax_tree(&path)?;
            let mut collector = ModuleCollector::new(display.to_string(), source_map);
            module.visit_with(&mut collector);
            self.modules.insert(display.to_string(), collector.finish());
        }
        Ok(&self.modules[display])
    }

    fn resolve(
        &mut self,
        module_path: &str,
        expression: &SqlExpression,
        visited: &mut HashSet<(String, String)>,
    ) -> Result<Option<StaticSql>> {
        match expression {
            SqlExpression::Value(value) => Ok(Some(value.clone())),
            SqlExpression::Binding(name) => {
                if !visited.insert((module_path.to_string(), name.clone())) {
                    return Ok(None);
                }
                let facts = self.load(module_path)?.clone();
                if let Some(value) = facts.bindings.get(name) {
                    return self.resolve(module_path, value, visited);
                }
                let Some(import) = facts.imports.get(name) else {
                    return Ok(None);
                };
                let Some(target) = self.resolve_import(module_path, &import.source)? else {
                    return Ok(None);
                };
                self.resolve(
                    &target,
                    &SqlExpression::Binding(import.imported.clone()),
                    visited,
                )
            }
        }
    }

    fn resolve_import(&self, module_path: &str, specifier: &str) -> Result<Option<String>> {
        if let Some(relative) =
            resolve_relative_module(self.root, module_path, specifier, false, |candidate| {
                self.root.join(candidate).is_file()
            })
        {
            return Ok(is_ecmascript_source(Path::new(&relative)).then_some(relative));
        }
        let Some(dependency) = crate::package::resolve_dependency(self.root, specifier) else {
            return Ok(None);
        };
        if !crate::package::is_local_dependency(self.root, &dependency)? {
            return Ok(None);
        }
        let Some(package) = crate::package::discover_javascript(&dependency.root)? else {
            return Ok(None);
        };
        let Some(export) = package
            .exports
            .iter()
            .find(|export| export.public_path == dependency.public_path)
        else {
            return Ok(None);
        };
        let target = dependency.root.join(&export.source_path);
        if !target.is_file() || !is_ecmascript_source(&target) {
            return Ok(None);
        }
        Ok(Some(crate::paths::normalize_relative_path(
            &target, self.root,
        )))
    }
}

fn is_ecmascript_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx")
    )
}

struct ModuleCollector {
    path: String,
    source_map: Lrc<SourceMap>,
    bindings: BTreeMap<String, Option<SqlExpression>>,
    exports: BTreeSet<String>,
    imports: BTreeMap<String, ImportReference>,
    bootstraps: Vec<BootstrapCandidate>,
    migrations: Vec<MigrationCandidate>,
    queries: Vec<SqlExpression>,
}

impl ModuleCollector {
    fn new(path: String, source_map: Lrc<SourceMap>) -> Self {
        Self {
            path,
            source_map,
            bindings: BTreeMap::new(),
            exports: BTreeSet::new(),
            imports: BTreeMap::new(),
            bootstraps: Vec::new(),
            migrations: Vec::new(),
            queries: Vec::new(),
        }
    }

    fn finish(self) -> ModuleFacts {
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
            Expr::Tpl(template) => Some(SqlExpression::Value(self.template_sql(template))),
            Expr::TaggedTpl(template) => {
                Some(SqlExpression::Value(self.template_sql(&template.tpl)))
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

    fn template_sql(&self, template: &Tpl) -> StaticSql {
        let mut text = String::new();
        for (index, quasi) in template.quasis.iter().enumerate() {
            if index > 0 {
                let expression = &template.exprs[index - 1];
                let source = self
                    .source_map
                    .span_to_snippet(expression.span())
                    .unwrap_or_else(|_| format!("{expression:?}"));
                text.push_str(&format!(
                    "$codeatlas_{index}_{:x}",
                    Sha256::digest(source.as_bytes())
                ));
            }
            text.push_str(
                quasi
                    .cooked
                    .as_ref()
                    .map_or_else(|| quasi.raw.as_ref(), |cooked| cooked.as_ref()),
            );
        }
        self.static_sql(text, template.span, !template.exprs.is_empty())
    }

    fn parameterized_template_sql(&self, template: &Tpl) -> StaticSql {
        if template
            .exprs
            .iter()
            .any(|expression| is_explicit_sql_fragment(expression))
        {
            return self.template_sql(template);
        }
        let mut text = String::new();
        for (index, quasi) in template.quasis.iter().enumerate() {
            if index > 0 {
                text.push_str(&format!("${index}"));
            }
            text.push_str(
                quasi
                    .cooked
                    .as_ref()
                    .map_or_else(|| quasi.raw.as_ref(), |cooked| cooked.as_ref()),
            );
        }
        self.static_sql(text, template.span, false)
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
                if argument.spread.is_none() {
                    if let Some(sql) = self.sql_expression(&argument.expr) {
                        self.queries.push(sql);
                    }
                }
            }
        }
        call.visit_children_with(self);
    }

    fn visit_tagged_tpl(&mut self, template: &TaggedTpl) {
        if expression_method_name(&template.tag).is_some_and(is_parameterized_sql_tag) {
            self.queries.push(SqlExpression::Value(
                self.parameterized_template_sql(&template.tpl),
            ));
        }
        template.visit_children_with(self);
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

fn is_parameterized_sql_tag(name: &str) -> bool {
    matches!(name, "$queryRaw" | "$executeRaw")
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

fn resolve_project_sql_file(root: &Path, configured: &str) -> Result<Option<StaticSql>> {
    let relative = Path::new(configured);
    if relative.is_absolute() {
        anyhow::bail!("Embedded migration file must be project-relative: {configured}");
    }
    let canonical_root = root.canonicalize()?;
    let canonical = root.join(relative).canonicalize()?;
    if canonical.strip_prefix(&canonical_root).is_err() {
        anyhow::bail!("Embedded migration file escapes the project root: {configured}");
    }
    if canonical.extension().and_then(|value| value.to_str()) != Some("sql") {
        return Ok(None);
    }
    Ok(Some(StaticSql {
        text: std::fs::read_to_string(&canonical)?,
        path: crate::paths::normalize_relative_path(&canonical, &canonical_root),
        line: 1,
        column: 1,
        dynamic: false,
    }))
}

fn looks_like_sql(source: &str) -> bool {
    sql_keyword(source).is_some()
}

fn looks_like_bootstrap_sql(source: &str) -> bool {
    matches!(
        sql_keyword(source),
        Some("create" | "alter" | "drop" | "do")
    )
}

pub(super) fn sql_keyword(source: &str) -> Option<&str> {
    let mut source = source.trim_start();
    loop {
        if let Some(comment) = source.strip_prefix("--") {
            source = comment
                .split_once('\n')
                .map_or("", |(_, remaining)| remaining)
                .trim_start();
            continue;
        }
        if let Some(comment) = source.strip_prefix("/*") {
            let (_, remaining) = comment.split_once("*/")?;
            source = remaining.trim_start();
            continue;
        }
        break;
    }
    let keyword = source
        .split(|character: char| !character.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    [
        "select", "insert", "update", "delete", "with", "create", "alter", "drop", "truncate",
        "grant", "revoke", "do", "set", "lock", "begin", "commit", "rollback",
    ]
    .into_iter()
    .find(|candidate| keyword.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    use super::{extract, looks_like_sql};
    use std::path::Path;

    #[test]
    fn recognizes_sql_after_comments_without_accepting_ordinary_strings() {
        assert!(looks_like_sql(
            "-- migration\nCREATE TABLE users(id bigint);"
        ));
        assert!(looks_like_sql("/* query */ SELECT 1"));
        assert!(looks_like_sql("LOCK TABLE users IN ACCESS EXCLUSIVE MODE"));
        assert!(!looks_like_sql("query failed"));
    }

    #[test]
    fn extracts_embedded_migrations_imported_sql_and_queries() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/postgres/embedded");
        let source = extract(
            &root,
            &[
                root.join("migrations.ts"),
                root.join("manifest.ts"),
                root.join("queries.ts"),
                root.join("runner.ts"),
            ],
        )
        .expect("embedded PostgreSQL source");

        assert_eq!(source.migrations.len(), 3);
        assert_eq!(source.migrations[0].name, "001_inline.sql");
        assert_eq!(source.migrations[1].name, "002_imported.sql");
        assert_eq!(source.migrations[1].sql.path, "schema.ts");
        assert_eq!(source.migrations[2].name, "003_file.sql");
        assert_eq!(source.migrations[2].sql.path, "003_file.sql");
        assert_eq!(source.unresolved_migrations.len(), 1);
        assert_eq!(source.unresolved_migrations[0].name, "004_dynamic.ts");
        assert_eq!(source.bootstraps.len(), 1);
        assert_eq!(source.bootstraps[0].name, "IMPORTED_SCHEMA_SQL");
        assert_eq!(source.queries.len(), 6);
        let parameterized = source
            .queries
            .iter()
            .filter(|query| {
                query.sql.text.contains("prisma_users") && query.sql.text.contains("$1")
            })
            .collect::<Vec<_>>();
        assert_eq!(parameterized.len(), 2);
        assert!(parameterized.iter().all(|query| !query.sql.dynamic));
        assert!(source.queries.iter().any(|query| {
            query.sql.text.contains("prisma_users WHERE")
                && query.sql.text.contains("$codeatlas_1_")
                && query.sql.dynamic
        }));
        let dynamic = source
            .queries
            .iter()
            .find(|query| query.sql.dynamic)
            .expect("dynamic query boundary");
        assert!(dynamic.sql.text.contains("$codeatlas_1_"));
        assert!(!dynamic.sql.text.contains("ownerId"));
    }
}
