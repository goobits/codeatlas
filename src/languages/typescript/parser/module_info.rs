use crate::domain::Symbol;
use swc_core::ecma::ast::*;

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
    pub re_exports: Vec<ReExport>,
    pub export_all: Vec<String>,
    pub default_export: Option<String>,
}

pub(crate) struct ImportInfo {
    pub source: String,
    pub named: Vec<String>,
    pub default: Option<String>,
    pub namespace: bool,
}

pub(crate) struct TypeScriptModuleInfo {
    pub symbols: Vec<Symbol>,
    pub exports: ExportInfo,
    pub imports: Vec<ImportInfo>,
}

pub(super) fn collect_exports(module: &Module) -> ExportInfo {
    let mut local_exports = Vec::new();
    let mut re_exports = Vec::new();
    let mut export_all = Vec::new();
    let mut default_export = None;

    for item in &module.body {
        let ModuleItem::ModuleDecl(declaration) = item else {
            continue;
        };
        match declaration {
            ModuleDecl::ExportDecl(export) => match &export.decl {
                Decl::Class(class) => local_exports.push(class.ident.sym.to_string()),
                Decl::Fn(function) => local_exports.push(function.ident.sym.to_string()),
                Decl::Var(variables) => {
                    for variable in &variables.decls {
                        if let Pat::Ident(ident) = &variable.name {
                            local_exports.push(ident.id.sym.to_string());
                        }
                    }
                }
                Decl::TsInterface(interface) => local_exports.push(interface.id.sym.to_string()),
                Decl::TsTypeAlias(alias) => local_exports.push(alias.id.sym.to_string()),
                Decl::TsEnum(enumeration) => local_exports.push(enumeration.id.sym.to_string()),
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
                        local_exports.push(name.original);
                    }
                }
            }
            ModuleDecl::ExportAll(all) => export_all.push(all.src.value.to_string()),
            ModuleDecl::ExportDefaultDecl(default) => {
                local_exports.push("default".to_string());
                match &default.decl {
                    DefaultDecl::Class(class) => {
                        if let Some(ident) = &class.ident {
                            default_export.get_or_insert_with(|| ident.sym.to_string());
                        }
                    }
                    DefaultDecl::Fn(function) => {
                        if let Some(ident) = &function.ident {
                            default_export.get_or_insert_with(|| ident.sym.to_string());
                        }
                    }
                    _ => {}
                }
            }
            ModuleDecl::ExportDefaultExpr(default) => {
                local_exports.push("default".to_string());
                if let Expr::Ident(ident) = &*default.expr {
                    default_export.get_or_insert_with(|| ident.sym.to_string());
                }
            }
            _ => {}
        }
    }

    ExportInfo {
        local_exports,
        re_exports,
        export_all,
        default_export,
    }
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
            for specifier in &import.specifiers {
                match specifier {
                    ImportSpecifier::Named(specifier) => named.push(
                        specifier
                            .imported
                            .as_ref()
                            .map(export_name_to_string)
                            .unwrap_or_else(|| specifier.local.sym.to_string()),
                    ),
                    ImportSpecifier::Default(specifier) => {
                        default = Some(specifier.local.sym.to_string());
                    }
                    ImportSpecifier::Namespace(_) => namespace = true,
                }
            }
            Some(ImportInfo {
                source: import.src.value.to_string(),
                named,
                default,
                namespace,
            })
        })
        .collect()
}

fn export_name_to_string(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(string) => string.value.to_string(),
    }
}
