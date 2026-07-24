use crate::domain::Symbol;
use std::collections::{BTreeMap, BTreeSet};
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DynamicDependency {
    pub specifier: Option<String>,
    pub kind: DynamicDependencyKind,
    pub span: crate::domain::Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DynamicDependencyKind {
    Import,
    Require,
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

    let mut dynamic = DynamicDependencyCollector {
        source_map,
        dependencies: Vec::new(),
    };
    module.visit_with(&mut dynamic);
    facts.dynamic_dependencies = dynamic.dependencies;
    facts
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

struct DynamicDependencyCollector {
    source_map: Lrc<SourceMap>,
    dependencies: Vec<DynamicDependency>,
}

impl Visit for DynamicDependencyCollector {
    fn visit_call_expr(&mut self, call: &CallExpr) {
        let kind = match &call.callee {
            Callee::Import(_) => Some(DynamicDependencyKind::Import),
            Callee::Expr(expression) if matches!(&**expression, Expr::Ident(identifier) if identifier.sym == *"require") => {
                Some(DynamicDependencyKind::Require)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            let specifier = call
                .args
                .first()
                .and_then(|argument| match &*argument.expr {
                    Expr::Lit(Lit::Str(value)) => Some(value.value.to_string()),
                    _ => None,
                });
            let start = self.source_map.lookup_char_pos(call.span.lo);
            let end = self.source_map.lookup_char_pos(call.span.hi);
            self.dependencies.push(DynamicDependency {
                specifier,
                kind,
                span: crate::domain::Span {
                    start_line: start.line as u32,
                    start_col: start.col.0 as u32,
                    end_line: end.line as u32,
                    end_col: end.col.0 as u32,
                },
            });
        }
        call.visit_children_with(self);
    }
}

fn export_name_to_string(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(string) => string.value.to_string(),
    }
}
