use crate::domain::Symbol;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use swc_core::common::{sync::Lrc, SourceMap};
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};

#[derive(Clone)]
pub(crate) struct ExportName {
    pub exported: String,
    pub original: String,
}

pub(crate) struct ReExport {
    pub source: String,
    pub names: Vec<ExportName>,
}

pub(crate) struct ExportInfo {
    pub local_exports: Vec<String>,
    pub local_export_names: Vec<ExportName>,
    pub re_exports: Vec<ReExport>,
    pub export_all: Vec<String>,
    pub default_export: Option<String>,
}

pub(crate) struct ImportInfo {
    pub source: String,
    pub named: Vec<String>,
    pub default: Option<String>,
    pub namespace: bool,
    pub type_only: bool,
    pub bindings: Vec<ImportBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportBinding {
    pub imported: String,
    pub local: String,
    pub namespace: bool,
    pub type_only: bool,
}

pub(crate) struct TypeScriptModuleInfo {
    pub symbols: Vec<Symbol>,
    pub exports: ExportInfo,
    pub imports: Vec<ImportInfo>,
    pub reachability: ReachabilityFacts,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ReachabilityFacts {
    pub top_level_references: BTreeSet<String>,
    pub symbol_references: BTreeMap<String, BTreeSet<String>>,
    pub dynamic_dependencies: Vec<DynamicDependency>,
    pub configured_test_entrypoints: BTreeSet<String>,
    pub configured_aliases: BTreeMap<String, BTreeSet<String>>,
    pub declaration_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DynamicDependency {
    pub target: DynamicDependencyTarget,
    pub kind: DynamicDependencyKind,
    pub span: crate::domain::Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DynamicDependencyTarget {
    Literal(String),
    Pattern { prefix: String, suffix: String },
    Glob(String),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DynamicDependencyKind {
    Import,
    ImportMetaGlob,
    ImportScripts,
    Require,
    RuntimeFile,
    RuntimeUrl,
}

pub(super) fn collect_exports(module: &Module) -> ExportInfo {
    let mut local_exports = Vec::new();
    let mut local_export_names = Vec::new();
    let mut re_exports = Vec::new();
    let mut export_all = Vec::new();
    let mut default_export = None;

    for item in &module.body {
        let ModuleItem::ModuleDecl(declaration) = item else {
            continue;
        };
        match declaration {
            ModuleDecl::ExportDecl(export) => match &export.decl {
                Decl::Class(class) => push_local_export(
                    &mut local_exports,
                    &mut local_export_names,
                    class.ident.sym.to_string(),
                ),
                Decl::Fn(function) => push_local_export(
                    &mut local_exports,
                    &mut local_export_names,
                    function.ident.sym.to_string(),
                ),
                Decl::Var(variables) => {
                    for variable in &variables.decls {
                        if let Pat::Ident(ident) = &variable.name {
                            push_local_export(
                                &mut local_exports,
                                &mut local_export_names,
                                ident.id.sym.to_string(),
                            );
                        }
                    }
                }
                Decl::TsInterface(interface) => push_local_export(
                    &mut local_exports,
                    &mut local_export_names,
                    interface.id.sym.to_string(),
                ),
                Decl::TsTypeAlias(alias) => push_local_export(
                    &mut local_exports,
                    &mut local_export_names,
                    alias.id.sym.to_string(),
                ),
                Decl::TsEnum(enumeration) => push_local_export(
                    &mut local_exports,
                    &mut local_export_names,
                    enumeration.id.sym.to_string(),
                ),
                _ => {}
            },
            ModuleDecl::ExportNamed(named) => {
                let source = named.src.as_ref().map(|source| source.value.to_string());
                let names = named
                    .specifiers
                    .iter()
                    .filter_map(|specifier| {
                        let ExportSpecifier::Named(specifier) = specifier else {
                            return None;
                        };
                        Some(ExportName {
                            exported: export_name_to_string(
                                specifier.exported.as_ref().unwrap_or(&specifier.orig),
                            ),
                            original: export_name_to_string(&specifier.orig),
                        })
                    })
                    .collect::<Vec<_>>();
                if let Some(source) = source {
                    re_exports.push(ReExport { source, names });
                } else {
                    for name in names {
                        if name.exported == "default" {
                            default_export.get_or_insert_with(|| name.original.clone());
                        }
                        local_exports.push(name.original.clone());
                        local_export_names.push(name);
                    }
                }
            }
            ModuleDecl::ExportAll(all) => export_all.push(all.src.value.to_string()),
            ModuleDecl::ExportDefaultDecl(default) => {
                local_exports.push("default".to_string());
                let original = match &default.decl {
                    DefaultDecl::Class(class) => {
                        if let Some(ident) = &class.ident {
                            ident.sym.to_string()
                        } else {
                            "default".to_string()
                        }
                    }
                    DefaultDecl::Fn(function) => {
                        if let Some(ident) = &function.ident {
                            ident.sym.to_string()
                        } else {
                            "default".to_string()
                        }
                    }
                    _ => "default".to_string(),
                };
                default_export.get_or_insert_with(|| original.clone());
                local_export_names.push(ExportName {
                    exported: "default".to_string(),
                    original,
                });
            }
            ModuleDecl::ExportDefaultExpr(default) => {
                local_exports.push("default".to_string());
                let original = if let Expr::Ident(ident) = &*default.expr {
                    ident.sym.to_string()
                } else {
                    "default".to_string()
                };
                default_export.get_or_insert_with(|| original.clone());
                local_export_names.push(ExportName {
                    exported: "default".to_string(),
                    original,
                });
            }
            _ => {}
        }
    }

    ExportInfo {
        local_exports,
        local_export_names,
        re_exports,
        export_all,
        default_export,
    }
}

fn push_local_export(
    local_exports: &mut Vec<String>,
    local_export_names: &mut Vec<ExportName>,
    name: String,
) {
    local_exports.push(name.clone());
    local_export_names.push(ExportName {
        exported: name.clone(),
        original: name,
    });
}

pub(super) fn collect_imports(module: &Module) -> Vec<ImportInfo> {
    module
        .body
        .iter()
        .filter_map(|item| {
            let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
                return None;
            };
            let mut named = Vec::new();
            let mut default = None;
            let mut namespace = false;
            let mut bindings = Vec::new();
            for specifier in &import.specifiers {
                match specifier {
                    ImportSpecifier::Named(specifier) => {
                        let imported = specifier
                            .imported
                            .as_ref()
                            .map(export_name_to_string)
                            .unwrap_or_else(|| specifier.local.sym.to_string());
                        let local = specifier.local.sym.to_string();
                        named.push(imported.clone());
                        bindings.push(ImportBinding {
                            imported,
                            local,
                            namespace: false,
                            type_only: import.type_only || specifier.is_type_only,
                        });
                    }
                    ImportSpecifier::Default(specifier) => {
                        let local = specifier.local.sym.to_string();
                        default = Some(local.clone());
                        bindings.push(ImportBinding {
                            imported: "default".to_string(),
                            local,
                            namespace: false,
                            type_only: import.type_only,
                        });
                    }
                    ImportSpecifier::Namespace(specifier) => {
                        namespace = true;
                        bindings.push(ImportBinding {
                            imported: "*".to_string(),
                            local: specifier.local.sym.to_string(),
                            namespace: true,
                            type_only: import.type_only,
                        });
                    }
                }
            }
            Some(ImportInfo {
                source: import.src.value.to_string(),
                named,
                default,
                namespace,
                type_only: import.type_only,
                bindings,
            })
        })
        .collect()
}

pub(super) fn collect_reachability_facts(
    module: &Module,
    source_map: Lrc<SourceMap>,
) -> ReachabilityFacts {
    let mut facts = ReachabilityFacts::default();

    for item in &module.body {
        let owners = declared_names(item);
        let mut collector = IdentifierCollector::default();
        item.visit_with(&mut collector);
        if owners.is_empty() {
            facts.top_level_references.extend(collector.identifiers);
        } else {
            for owner in owners {
                facts
                    .symbol_references
                    .entry(owner)
                    .or_default()
                    .extend(collector.identifiers.iter().cloned());
            }
        }
    }

    let mut static_bindings = StaticDependencyBindingCollector::default();
    module.visit_with(&mut static_bindings);
    let mut configured_source_bindings = StaticConfiguredSourceBindingCollector::default();
    module.visit_with(&mut configured_source_bindings);
    let mut configured_tests = ConfiguredTestEntrypointCollector {
        bindings: configured_source_bindings.unique(),
        ..Default::default()
    };
    module.visit_with(&mut configured_tests);
    let mut static_string_bindings = StaticStringBindingCollector::default();
    module.visit_with(&mut static_string_bindings);
    let mut configured_aliases = ConfiguredAliasCollector {
        bindings: static_string_bindings.unique(),
        ..Default::default()
    };
    module.visit_with(&mut configured_aliases);
    let mut dynamic = DynamicDependencyCollector {
        source_map,
        dependencies: Vec::new(),
        static_bindings: static_bindings.unique(),
        local_bindings: module.body.iter().flat_map(declared_names).collect(),
    };
    module.visit_with(&mut dynamic);
    facts.dynamic_dependencies = dynamic.dependencies;
    facts.configured_test_entrypoints = configured_tests.paths;
    facts.configured_aliases = configured_aliases.aliases;
    facts.declaration_only = is_declaration_only(module);
    facts
}

fn is_declaration_only(module: &Module) -> bool {
    let mut declarations = 0;
    for item in &module.body {
        let declaration = match item {
            ModuleItem::Stmt(Stmt::Decl(declaration))
            | ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                decl: declaration,
                ..
            })) => declaration,
            ModuleItem::ModuleDecl(ModuleDecl::Import(import)) if import.type_only => continue,
            ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(export)) if export.type_only => continue,
            ModuleItem::Stmt(Stmt::Empty(_)) => continue,
            _ => return false,
        };
        if !matches!(
            declaration,
            Decl::TsInterface(_) | Decl::TsTypeAlias(_) | Decl::TsModule(_)
        ) {
            return false;
        }
        declarations += 1;
    }
    declarations > 0
}

fn declared_names(item: &ModuleItem) -> Vec<String> {
    match item {
        ModuleItem::Stmt(Stmt::Decl(declaration))
        | ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
            decl: declaration, ..
        })) => declaration_names(declaration),
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(default)) => match &default.decl {
            DefaultDecl::Class(class) => class
                .ident
                .as_ref()
                .map(|ident| vec![ident.sym.to_string()])
                .unwrap_or_default(),
            DefaultDecl::Fn(function) => function
                .ident
                .as_ref()
                .map(|ident| vec![ident.sym.to_string()])
                .unwrap_or_default(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn declaration_names(declaration: &Decl) -> Vec<String> {
    match declaration {
        Decl::Class(class) => vec![class.ident.sym.to_string()],
        Decl::Fn(function) => vec![function.ident.sym.to_string()],
        Decl::Var(variables) => variables
            .decls
            .iter()
            .filter_map(|variable| match &variable.name {
                Pat::Ident(ident) => Some(ident.id.sym.to_string()),
                _ => None,
            })
            .collect(),
        Decl::TsInterface(interface) => vec![interface.id.sym.to_string()],
        Decl::TsTypeAlias(alias) => vec![alias.id.sym.to_string()],
        Decl::TsEnum(enumeration) => vec![enumeration.id.sym.to_string()],
        _ => Vec::new(),
    }
}

#[derive(Default)]
struct IdentifierCollector {
    identifiers: BTreeSet<String>,
}

impl Visit for IdentifierCollector {
    fn visit_ident(&mut self, identifier: &Ident) {
        self.identifiers.insert(identifier.sym.to_string());
    }
}

#[derive(Default)]
struct ConfiguredTestEntrypointCollector {
    paths: BTreeSet<String>,
    bindings: BTreeMap<String, BTreeSet<String>>,
}

impl Visit for ConfiguredTestEntrypointCollector {
    fn visit_key_value_prop(&mut self, property: &KeyValueProp) {
        let key = match &property.key {
            PropName::Ident(identifier) => identifier.sym.as_ref(),
            PropName::Str(string) => string.value.as_ref(),
            _ => "",
        };
        if matches!(key, "alias" | "aliases") {
            collect_configured_alias_source_paths(&property.value, &self.bindings, &mut self.paths);
        } else if matches!(
            key,
            "setupFiles" | "setupFilesAfterEnv" | "globalSetup" | "globalTeardown"
        ) {
            collect_configured_source_paths(&property.value, &self.bindings, &mut self.paths);
        }
        property.visit_children_with(self);
    }
}

fn collect_configured_alias_source_paths(
    expression: &Expr,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    paths: &mut BTreeSet<String>,
) {
    match expression {
        Expr::Ident(identifier) => {
            if let Some(bound) = bindings.get(identifier.sym.as_ref()) {
                paths.extend(bound.iter().cloned());
            }
        }
        Expr::Object(object) => {
            for property in &object.props {
                let PropOrSpread::Prop(property) = property else {
                    continue;
                };
                let Prop::KeyValue(property) = &**property else {
                    continue;
                };
                collect_configured_source_paths(&property.value, bindings, paths);
            }
        }
        Expr::Array(array) => {
            for element in array.elems.iter().flatten() {
                let Expr::Object(object) = &*element.expr else {
                    continue;
                };
                for property in &object.props {
                    let PropOrSpread::Prop(property) = property else {
                        continue;
                    };
                    let Prop::KeyValue(property) = &**property else {
                        continue;
                    };
                    if prop_name_string(&property.key).as_deref() == Some("replacement") {
                        collect_configured_source_paths(&property.value, bindings, paths);
                    }
                }
            }
        }
        _ => {}
    }
}

fn collect_configured_source_paths(
    expression: &Expr,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    paths: &mut BTreeSet<String>,
) {
    match expression {
        Expr::Ident(identifier) => {
            if let Some(bound) = bindings.get(identifier.sym.as_ref()) {
                paths.extend(bound.iter().cloned());
            }
        }
        Expr::Lit(Lit::Str(string)) => {
            let value = string.value.to_string();
            let source = value
                .split_once('?')
                .map_or(value.as_str(), |(source, _)| source);
            if matches!(
                Path::new(source)
                    .extension()
                    .and_then(|extension| extension.to_str()),
                Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte")
            ) {
                paths.insert(source.to_string());
            }
        }
        Expr::Array(array) => {
            for element in array.elems.iter().flatten() {
                collect_configured_source_paths(&element.expr, bindings, paths);
            }
        }
        Expr::Object(object) => {
            for property in &object.props {
                if let PropOrSpread::Prop(property) = property {
                    if let Prop::KeyValue(property) = &**property {
                        collect_configured_source_paths(&property.value, bindings, paths);
                    }
                }
            }
        }
        Expr::Call(call) if is_static_path_constructor_call(call) => {
            for argument in &call.args {
                collect_configured_source_paths(&argument.expr, bindings, paths);
            }
        }
        Expr::New(expression) => {
            for argument in expression.args.iter().flatten() {
                collect_configured_source_paths(&argument.expr, bindings, paths);
            }
        }
        Expr::Tpl(template) if template.exprs.is_empty() => {
            let value = template
                .quasis
                .iter()
                .map(|quasi| quasi.raw.to_string())
                .collect::<String>();
            if matches!(
                Path::new(&value)
                    .extension()
                    .and_then(|extension| extension.to_str()),
                Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte")
            ) {
                paths.insert(value);
            }
        }
        _ => {}
    }
}

#[derive(Default)]
struct StaticConfiguredSourceBindingCollector {
    bindings: BTreeMap<String, Option<BTreeSet<String>>>,
}

impl StaticConfiguredSourceBindingCollector {
    fn unique(self) -> BTreeMap<String, BTreeSet<String>> {
        self.bindings
            .into_iter()
            .filter_map(|(name, paths)| paths.map(|paths| (name, paths)))
            .collect()
    }
}

impl Visit for StaticConfiguredSourceBindingCollector {
    fn visit_var_declarator(&mut self, declaration: &VarDeclarator) {
        if let Pat::Ident(identifier) = &declaration.name {
            let mut paths = BTreeSet::new();
            if let Some(expression) = declaration.init.as_deref() {
                collect_configured_source_paths(expression, &BTreeMap::new(), &mut paths);
            }
            let paths = (!paths.is_empty()).then_some(paths);
            self.bindings
                .entry(identifier.id.sym.to_string())
                .and_modify(|existing| *existing = None)
                .or_insert(paths);
        }
        declaration.visit_children_with(self);
    }
}

#[derive(Default)]
struct ConfiguredAliasCollector {
    aliases: BTreeMap<String, BTreeSet<String>>,
    bindings: BTreeMap<String, BTreeSet<String>>,
}

impl Visit for ConfiguredAliasCollector {
    fn visit_object_lit(&mut self, object: &ObjectLit) {
        collect_alias_descriptor(object, &self.bindings, &mut self.aliases);
        object.visit_children_with(self);
    }

    fn visit_key_value_prop(&mut self, property: &KeyValueProp) {
        let key = prop_name_string(&property.key);
        if matches!(key.as_deref(), Some("alias" | "aliases")) {
            collect_aliases(&property.value, &self.bindings, &mut self.aliases);
        }
        property.visit_children_with(self);
    }
}

fn collect_aliases(
    expression: &Expr,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    aliases: &mut BTreeMap<String, BTreeSet<String>>,
) {
    match expression {
        Expr::Object(object) => {
            for property in &object.props {
                let PropOrSpread::Prop(property) = property else {
                    continue;
                };
                let Prop::KeyValue(property) = &**property else {
                    continue;
                };
                let Some(pattern) = prop_name_string(&property.key) else {
                    continue;
                };
                let targets = static_strings(&property.value, bindings);
                if !targets.is_empty() {
                    aliases.entry(pattern).or_default().extend(targets);
                }
            }
        }
        Expr::Array(array) => {
            for element in array.elems.iter().flatten() {
                let Expr::Object(object) = &*element.expr else {
                    continue;
                };
                collect_alias_descriptor(object, bindings, aliases);
            }
        }
        _ => {}
    }
}

fn collect_alias_descriptor(
    object: &ObjectLit,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    aliases: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let mut patterns = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for property in &object.props {
        let PropOrSpread::Prop(property) = property else {
            continue;
        };
        let Prop::KeyValue(property) = &**property else {
            continue;
        };
        match prop_name_string(&property.key).as_deref() {
            Some("find") => patterns.extend(static_strings(&property.value, bindings)),
            Some("replacement") => targets.extend(static_strings(&property.value, bindings)),
            _ => {}
        }
    }
    for pattern in patterns {
        aliases
            .entry(pattern)
            .or_default()
            .extend(targets.iter().cloned());
    }
}

fn prop_name_string(name: &PropName) -> Option<String> {
    match name {
        PropName::Ident(identifier) => Some(identifier.sym.to_string()),
        PropName::Str(string) => Some(string.value.to_string()),
        _ => None,
    }
}

fn static_strings(
    expression: &Expr,
    bindings: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    collect_static_strings(expression, bindings, &mut values);
    values
}

fn collect_static_strings(
    expression: &Expr,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    values: &mut BTreeSet<String>,
) {
    match expression {
        Expr::Ident(identifier) => {
            if let Some(bound) = bindings.get(identifier.sym.as_ref()) {
                values.extend(bound.iter().cloned());
            }
        }
        Expr::Lit(Lit::Str(string)) => {
            values.insert(string.value.to_string());
        }
        Expr::Tpl(template) if template.exprs.is_empty() => {
            values.insert(
                template
                    .quasis
                    .iter()
                    .map(|quasi| quasi.raw.to_string())
                    .collect(),
            );
        }
        Expr::Call(call) if is_static_path_constructor_call(call) => {
            for argument in &call.args {
                collect_static_strings(&argument.expr, bindings, values);
            }
        }
        Expr::New(expression) => {
            for argument in expression.args.iter().flatten() {
                collect_static_strings(&argument.expr, bindings, values);
            }
        }
        Expr::Array(array) => {
            for element in array.elems.iter().flatten() {
                collect_static_strings(&element.expr, bindings, values);
            }
        }
        _ => {}
    }
}

fn is_static_path_constructor_call(call: &CallExpr) -> bool {
    let Callee::Expr(callee) = &call.callee else {
        return false;
    };
    match &**callee {
        Expr::Ident(identifier) => matches!(
            identifier.sym.as_ref(),
            "fileURLToPath" | "join" | "resolve"
        ),
        Expr::Member(member) => match &member.prop {
            MemberProp::Ident(identifier) => matches!(
                identifier.sym.as_ref(),
                "fileURLToPath" | "join" | "resolve"
            ),
            MemberProp::Computed(computed) => matches!(
                &*computed.expr,
                Expr::Lit(Lit::Str(string))
                    if matches!(string.value.as_ref(), "fileURLToPath" | "join" | "resolve")
            ),
            MemberProp::PrivateName(_) => false,
        },
        _ => false,
    }
}

#[derive(Default)]
struct StaticStringBindingCollector {
    bindings: BTreeMap<String, Option<BTreeSet<String>>>,
}

impl StaticStringBindingCollector {
    fn unique(self) -> BTreeMap<String, BTreeSet<String>> {
        self.bindings
            .into_iter()
            .filter_map(|(name, values)| values.map(|values| (name, values)))
            .collect()
    }
}

impl Visit for StaticStringBindingCollector {
    fn visit_var_declarator(&mut self, declaration: &VarDeclarator) {
        if let Pat::Ident(identifier) = &declaration.name {
            let values = declaration
                .init
                .as_deref()
                .map(|expression| static_strings(expression, &self.unique_bindings()))
                .filter(|values| !values.is_empty());
            self.bindings
                .entry(identifier.id.sym.to_string())
                .and_modify(|existing| *existing = None)
                .or_insert(values);
        }
        declaration.visit_children_with(self);
    }
}

impl StaticStringBindingCollector {
    fn unique_bindings(&self) -> BTreeMap<String, BTreeSet<String>> {
        self.bindings
            .iter()
            .filter_map(|(name, values)| {
                values.as_ref().map(|values| (name.clone(), values.clone()))
            })
            .collect()
    }
}

struct DynamicDependencyCollector {
    source_map: Lrc<SourceMap>,
    dependencies: Vec<DynamicDependency>,
    static_bindings: BTreeMap<String, Vec<DynamicDependencyTarget>>,
    local_bindings: BTreeSet<String>,
}

#[derive(Default)]
struct StaticDependencyBindingCollector {
    bindings: BTreeMap<String, Option<Vec<DynamicDependencyTarget>>>,
}

impl StaticDependencyBindingCollector {
    fn unique(self) -> BTreeMap<String, Vec<DynamicDependencyTarget>> {
        self.bindings
            .into_iter()
            .filter_map(|(name, targets)| targets.map(|targets| (name, targets)))
            .collect()
    }
}

impl Visit for StaticDependencyBindingCollector {
    fn visit_var_declarator(&mut self, declaration: &VarDeclarator) {
        if let Pat::Ident(identifier) = &declaration.name {
            let targets = declaration
                .init
                .as_deref()
                .and_then(static_dependency_targets);
            self.bindings
                .entry(identifier.id.sym.to_string())
                .and_modify(|existing| *existing = None)
                .or_insert(targets);
        }
        declaration.visit_children_with(self);
    }
}

impl Visit for DynamicDependencyCollector {
    fn visit_call_expr(&mut self, call: &CallExpr) {
        let kind = match &call.callee {
            Callee::Import(_) => Some(DynamicDependencyKind::Import),
            Callee::Expr(expression) if matches!(&**expression, Expr::Ident(identifier) if identifier.sym == *"require") => {
                Some(DynamicDependencyKind::Require)
            }
            Callee::Expr(expression) if is_import_meta_glob(expression) => {
                Some(DynamicDependencyKind::ImportMetaGlob)
            }
            Callee::Expr(expression) if matches!(&**expression, Expr::Ident(identifier) if identifier.sym == *"importScripts") => {
                Some(DynamicDependencyKind::ImportScripts)
            }
            Callee::Expr(expression) if is_static_file_reader(expression, &self.local_bindings) => {
                Some(DynamicDependencyKind::RuntimeFile)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            let span = source_span(&self.source_map, call.span);
            let targets = if kind == DynamicDependencyKind::ImportScripts {
                call.args
                    .iter()
                    .flat_map(|argument| {
                        dependency_targets(&argument.expr, kind, &self.static_bindings)
                    })
                    .collect::<Vec<_>>()
            } else {
                call.args
                    .first()
                    .map(|argument| dependency_targets(&argument.expr, kind, &self.static_bindings))
                    .unwrap_or_else(|| vec![DynamicDependencyTarget::Unknown])
            };
            self.dependencies.extend(
                targets
                    .into_iter()
                    .filter(|target| {
                        kind != DynamicDependencyKind::RuntimeFile
                            || runtime_path_targets_source_module(target)
                    })
                    .map(|target| DynamicDependency {
                        target,
                        kind,
                        span: span.clone(),
                    }),
            );
        }
        call.visit_children_with(self);
    }

    fn visit_new_expr(&mut self, expression: &NewExpr) {
        let is_url =
            matches!(&*expression.callee, Expr::Ident(identifier) if identifier.sym == *"URL");
        let args = expression.args.as_deref().unwrap_or_default();
        if is_url
            && args
                .get(1)
                .is_some_and(|argument| is_import_meta_url(&argument.expr))
        {
            let targets = args
                .first()
                .map(|argument| {
                    dependency_targets(
                        &argument.expr,
                        DynamicDependencyKind::RuntimeUrl,
                        &self.static_bindings,
                    )
                })
                .unwrap_or_else(|| vec![DynamicDependencyTarget::Unknown]);
            let span = source_span(&self.source_map, expression.span);
            self.dependencies.extend(
                targets
                    .into_iter()
                    .filter(runtime_path_targets_source_module)
                    .map(|target| DynamicDependency {
                        target,
                        kind: DynamicDependencyKind::RuntimeUrl,
                        span: span.clone(),
                    }),
            );
        }
        expression.visit_children_with(self);
    }
}

fn runtime_path_targets_source_module(target: &DynamicDependencyTarget) -> bool {
    let path = match target {
        DynamicDependencyTarget::Literal(path) => path.as_str(),
        DynamicDependencyTarget::Pattern { suffix, .. } => suffix.as_str(),
        DynamicDependencyTarget::Glob(_) | DynamicDependencyTarget::Unknown => return false,
    };
    matches!(
        Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte")
    )
}

fn is_static_file_reader(expression: &Expr, local_bindings: &BTreeSet<String>) -> bool {
    fn is_reader(name: &str) -> bool {
        matches!(name, "load" | "read" | "readFile" | "readFileSync")
    }

    match expression {
        Expr::Ident(identifier) => {
            is_reader(identifier.sym.as_ref()) && !local_bindings.contains(identifier.sym.as_ref())
        }
        Expr::Member(member) => match &member.prop {
            MemberProp::Ident(identifier) => is_reader(identifier.sym.as_ref()),
            MemberProp::Computed(computed) => {
                matches!(&*computed.expr, Expr::Lit(Lit::Str(value)) if is_reader(value.value.as_ref()))
            }
            MemberProp::PrivateName(_) => false,
        },
        _ => false,
    }
}

fn source_span(source_map: &SourceMap, span: swc_core::common::Span) -> crate::domain::Span {
    let start = source_map.lookup_char_pos(span.lo);
    let end = source_map.lookup_char_pos(span.hi);
    crate::domain::Span {
        start_line: start.line as u32,
        start_col: start.col.0 as u32,
        end_line: end.line as u32,
        end_col: end.col.0 as u32,
    }
}

fn is_import_meta_glob(expression: &Expr) -> bool {
    let Expr::Member(member) = expression else {
        return false;
    };
    let Expr::MetaProp(meta) = &*member.obj else {
        return false;
    };
    meta.kind == MetaPropKind::ImportMeta
        && matches!(&member.prop, MemberProp::Ident(identifier) if identifier.sym == *"glob")
}

fn is_import_meta_url(expression: &Expr) -> bool {
    let Expr::Member(member) = expression else {
        return false;
    };
    let Expr::MetaProp(meta) = &*member.obj else {
        return false;
    };
    meta.kind == MetaPropKind::ImportMeta
        && matches!(&member.prop, MemberProp::Ident(identifier) if identifier.sym == *"url")
}

fn dependency_targets(
    expression: &Expr,
    kind: DynamicDependencyKind,
    static_bindings: &BTreeMap<String, Vec<DynamicDependencyTarget>>,
) -> Vec<DynamicDependencyTarget> {
    match expression {
        Expr::Ident(identifier) => static_bindings
            .get(identifier.sym.as_ref())
            .cloned()
            .unwrap_or_else(|| vec![DynamicDependencyTarget::Unknown]),
        Expr::Lit(Lit::Str(value)) => vec![if kind == DynamicDependencyKind::ImportMetaGlob {
            DynamicDependencyTarget::Glob(value.value.to_string())
        } else {
            DynamicDependencyTarget::Literal(value.value.to_string())
        }],
        Expr::Tpl(template) if template.exprs.is_empty() => vec![DynamicDependencyTarget::Literal(
            template
                .quasis
                .iter()
                .map(|quasi| quasi.raw.to_string())
                .collect(),
        )],
        Expr::Tpl(template) => vec![DynamicDependencyTarget::Pattern {
            prefix: template
                .quasis
                .first()
                .map(|quasi| quasi.raw.to_string())
                .unwrap_or_default(),
            suffix: template
                .quasis
                .last()
                .map(|quasi| quasi.raw.to_string())
                .unwrap_or_default(),
        }],
        Expr::Array(array) if kind == DynamicDependencyKind::ImportMetaGlob => {
            let targets = array
                .elems
                .iter()
                .map(|element| match element {
                    Some(element) => match &*element.expr {
                        Expr::Lit(Lit::Str(value)) => {
                            DynamicDependencyTarget::Glob(value.value.to_string())
                        }
                        _ => DynamicDependencyTarget::Unknown,
                    },
                    None => DynamicDependencyTarget::Unknown,
                })
                .collect::<Vec<_>>();
            if targets.is_empty() {
                vec![DynamicDependencyTarget::Unknown]
            } else {
                targets
            }
        }
        _ => vec![DynamicDependencyTarget::Unknown],
    }
}

fn static_dependency_targets(expression: &Expr) -> Option<Vec<DynamicDependencyTarget>> {
    let targets = dependency_targets(
        expression,
        DynamicDependencyKind::RuntimeUrl,
        &BTreeMap::new(),
    );
    targets
        .iter()
        .all(|target| !matches!(target, DynamicDependencyTarget::Unknown))
        .then_some(targets)
}

fn export_name_to_string(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(string) => string.value.to_string(),
    }
}
