use super::{ExportInfo, ExportName, ReExport};
use swc_core::ecma::ast::*;

pub(in crate::languages::typescript::parser) fn collect_exports(module: &Module) -> ExportInfo {
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

pub(super) fn export_name_to_string(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(string) => string.value.to_string(),
    }
}
