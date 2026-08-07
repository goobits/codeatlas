use codeatlas_domain::Symbol;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use swc_core::common::{sync::Lrc, SourceMap};
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};

pub(super) use exports::collect_exports;
use exports::export_name_to_string;

mod configured_sources;
mod dependencies;
mod exports;
mod import_meta_globs;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ExportName {
    pub exported: String,
    pub original: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ReExport {
    pub source: String,
    pub names: Vec<ExportName>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ExportInfo {
    pub local_exports: Vec<String>,
    pub local_export_names: Vec<ExportName>,
    pub re_exports: Vec<ReExport>,
    pub export_all: Vec<String>,
    pub default_export: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ImportInfo {
    pub source: String,
    pub named: Vec<String>,
    pub default: Option<String>,
    pub namespace: bool,
    pub type_only: bool,
    pub bindings: Vec<ImportBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ImportBinding {
    pub imported: String,
    pub local: String,
    pub namespace: bool,
    pub type_only: bool,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct TypeScriptModuleInfo {
    pub symbols: Vec<Symbol>,
    pub exports: ExportInfo,
    pub imports: Vec<ImportInfo>,
    pub reachability: ReachabilityFacts,
    pub has_shebang: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ReachabilityFacts {
    pub top_level_references: BTreeSet<String>,
    pub symbol_references: BTreeMap<String, BTreeSet<String>>,
    pub dynamic_dependencies: Vec<DynamicDependency>,
    pub configured_test_entrypoints: BTreeSet<String>,
    pub configured_runtime_entrypoints: BTreeSet<String>,
    pub configured_aliases: BTreeMap<String, BTreeSet<String>>,
    pub configures_tests: bool,
    pub declaration_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DynamicDependency {
    pub target: DynamicDependencyTarget,
    pub kind: DynamicDependencyKind,
    pub span: codeatlas_domain::Span,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum DynamicDependencyTarget {
    Literal(String),
    Pattern {
        prefix: String,
        suffix: String,
    },
    GlobSet {
        includes: Vec<String>,
        excludes: Vec<String>,
    },
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum DynamicDependencyKind {
    Import,
    ImportMetaGlob,
    ImportScripts,
    Require,
    RuntimeFile,
    RuntimeProcess,
    RuntimeUrl,
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

    let configured = configured_sources::collect(module);
    facts.dynamic_dependencies = dependencies::collect(module, source_map);
    facts.configured_test_entrypoints = configured.test_entrypoints;
    facts.configured_runtime_entrypoints = configured.runtime_entrypoints;
    facts.configured_aliases = configured.aliases;
    facts.configures_tests = configured.configures_tests;
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
            ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(export))
                if export.type_only || (export.src.is_none() && export.specifiers.is_empty()) =>
            {
                continue;
            }
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
