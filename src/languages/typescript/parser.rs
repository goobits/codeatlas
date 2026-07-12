use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use std::path::Path;
use swc_core::common::{
    errors::{ColorConfig, Handler},
    sync::Lrc,
    SourceMap,
};
use swc_core::ecma::ast::*;
use swc_core::ecma::parser::{lexer::Lexer, Parser, StringInput, Syntax, TsConfig};
use swc_core::ecma::visit::{Visit, VisitWith};

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

pub(crate) fn parse_file(file_path: &Path, root_dir: &Path) -> Result<Vec<Symbol>> {
    Ok(parse_module_info(file_path, root_dir)?.symbols)
}

pub(crate) fn parse_module_info(file_path: &Path, root_dir: &Path) -> Result<TypeScriptModuleInfo> {
    let (module, cm) = parse_module(file_path)?;

    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let mut visitor = SymbolVisitor {
        symbols: Vec::new(),
        relative_path,
        source_map: cm,
    };

    module.visit_with(&mut visitor);
    let exports = collect_exports(&module);
    let imports = collect_imports(&module);

    Ok(TypeScriptModuleInfo {
        symbols: visitor.symbols,
        exports,
        imports,
    })
}

fn parse_module(file_path: &Path) -> Result<(Module, Lrc<SourceMap>)> {
    let cm: Lrc<SourceMap> = Default::default();
    let handler = Handler::with_tty_emitter(ColorConfig::Auto, true, false, Some(cm.clone()));

    let fm = cm.load_file(file_path)?;

    let lexer = Lexer::new(
        Syntax::Typescript(TsConfig {
            tsx: file_path.extension().is_some_and(|e| e == "tsx"),
            decorators: true,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );

    let mut parser = Parser::new_from(lexer);

    for e in parser.take_errors() {
        e.into_diagnostic(&handler).emit();
    }

    let module = parser
        .parse_module()
        .map_err(|e| anyhow::anyhow!("Parse failed: {:?}", e))?;

    Ok((module, cm))
}

struct SymbolVisitor {
    symbols: Vec<Symbol>,
    relative_path: String,
    source_map: Lrc<SourceMap>,
}

impl SymbolVisitor {
    fn create_symbol(
        &self,
        name: String,
        kind: SymbolKind,
        visibility: Visibility,
        span: swc_core::common::Span,
        signature: String,
    ) -> Symbol {
        let (start, end) = (span.lo, span.hi);
        let start_loc = self.source_map.lookup_char_pos(start);
        let end_loc = self.source_map.lookup_char_pos(end);

        Symbol {
            id: format!("ts:{}:{}#{}", self.relative_path, kind_to_str(kind), name),
            name,
            kind,
            visibility,
            language: Language::TypeScript,
            file_path: self.relative_path.clone(),
            span: Some(Span {
                start_line: start_loc.line as u32,
                start_col: start_loc.col.0 as u32,
                end_line: end_loc.line as u32,
                end_col: end_loc.col.0 as u32,
            }),
            signature,
            children: vec![],
        }
    }
}

fn kind_to_str(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class => "class",
        SymbolKind::Function => "fn",
        SymbolKind::Interface => "interface",
        SymbolKind::Method => "method",
        _ => "sym",
    }
}

/// Format function/method parameters to readable string
fn format_params(params: &[Param]) -> String {
    params
        .iter()
        .map(format_param)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_param(param: &Param) -> String {
    let name = match &param.pat {
        Pat::Ident(ident) => {
            let mut s = ident.id.sym.to_string();
            if ident.optional {
                s.push('?');
            }
            s
        }
        Pat::Rest(rest) => {
            format!(
                "...{}",
                match &*rest.arg {
                    Pat::Ident(ident) => ident.id.sym.to_string(),
                    _ => "args".to_string(),
                }
            )
        }
        Pat::Object(_) => "{ ... }".to_string(),
        Pat::Array(_) => "[ ... ]".to_string(),
        Pat::Assign(assign) => match &*assign.left {
            Pat::Ident(ident) => format!("{} = ...", ident.id.sym),
            _ => "_ = ...".to_string(),
        },
        _ => "_".to_string(),
    };

    // Try to get type annotation
    if let Pat::Ident(ident) = &param.pat {
        if let Some(ts_type) = &ident.type_ann {
            let type_str = format_ts_type(&ts_type.type_ann);
            return format!("{}: {}", name, type_str);
        }
    }

    name
}

/// Format TypeScript type annotation
/// Format TsEntityName to string
fn format_entity_name(name: &TsEntityName) -> String {
    match name {
        TsEntityName::Ident(id) => id.sym.to_string(),
        TsEntityName::TsQualifiedName(qn) => {
            format!("{}.{}", format_entity_name(&qn.left), qn.right.sym)
        }
    }
}

fn format_ts_type(ts_type: &TsType) -> String {
    match ts_type {
        TsType::TsKeywordType(kw) => match kw.kind {
            TsKeywordTypeKind::TsStringKeyword => "string".to_string(),
            TsKeywordTypeKind::TsNumberKeyword => "number".to_string(),
            TsKeywordTypeKind::TsBooleanKeyword => "boolean".to_string(),
            TsKeywordTypeKind::TsVoidKeyword => "void".to_string(),
            TsKeywordTypeKind::TsAnyKeyword => "any".to_string(),
            TsKeywordTypeKind::TsNullKeyword => "null".to_string(),
            TsKeywordTypeKind::TsUndefinedKeyword => "undefined".to_string(),
            TsKeywordTypeKind::TsNeverKeyword => "never".to_string(),
            TsKeywordTypeKind::TsUnknownKeyword => "unknown".to_string(),
            TsKeywordTypeKind::TsObjectKeyword => "object".to_string(),
            TsKeywordTypeKind::TsBigIntKeyword => "bigint".to_string(),
            TsKeywordTypeKind::TsSymbolKeyword => "symbol".to_string(),
            TsKeywordTypeKind::TsIntrinsicKeyword => "intrinsic".to_string(),
        },
        TsType::TsTypeRef(type_ref) => {
            let name = format_entity_name(&type_ref.type_name);
            if let Some(params) = &type_ref.type_params {
                let inner: Vec<String> = params.params.iter().map(|p| format_ts_type(p)).collect();
                format!("{}<{}>", name, inner.join(", "))
            } else {
                name
            }
        }
        TsType::TsArrayType(arr) => format!("{}[]", format_ts_type(&arr.elem_type)),
        TsType::TsTupleType(tuple) => {
            let elems: Vec<String> = tuple
                .elem_types
                .iter()
                .map(|e| format_ts_type(&e.ty))
                .collect();
            format!("[{}]", elems.join(", "))
        }
        TsType::TsUnionOrIntersectionType(ui) => match ui {
            TsUnionOrIntersectionType::TsUnionType(union) => {
                let types: Vec<String> = union.types.iter().map(|t| format_ts_type(t)).collect();
                types.join(" | ")
            }
            TsUnionOrIntersectionType::TsIntersectionType(inter) => {
                let types: Vec<String> = inter.types.iter().map(|t| format_ts_type(t)).collect();
                types.join(" & ")
            }
        },
        TsType::TsFnOrConstructorType(fn_type) => match fn_type {
            TsFnOrConstructorType::TsFnType(fn_ty) => {
                let params_str = fn_ty
                    .params
                    .iter()
                    .map(format_ts_fn_param)
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret = format_ts_type(&fn_ty.type_ann.type_ann);
                format!("({}) => {}", params_str, ret)
            }
            TsFnOrConstructorType::TsConstructorType(_) => "new(...) => ...".to_string(),
        },
        TsType::TsTypeLit(_) => "{ ... }".to_string(),
        TsType::TsLitType(lit) => match &lit.lit {
            TsLit::Str(s) => format!("\"{}\"", s.value),
            TsLit::Number(n) => n.value.to_string(),
            TsLit::Bool(b) => b.value.to_string(),
            _ => "literal".to_string(),
        },
        _ => "...".to_string(),
    }
}

fn format_ts_fn_param(param: &TsFnParam) -> String {
    match param {
        TsFnParam::Ident(ident) => {
            let name = ident.id.sym.to_string();
            if let Some(ann) = &ident.type_ann {
                format!("{}: {}", name, format_ts_type(&ann.type_ann))
            } else {
                name
            }
        }
        TsFnParam::Rest(rest) => {
            format!(
                "...{}",
                match &*rest.arg {
                    Pat::Ident(id) => id.id.sym.to_string(),
                    _ => "args".to_string(),
                }
            )
        }
        _ => "_".to_string(),
    }
}

fn format_return_type(function: &Function) -> String {
    if let Some(ret_type) = &function.return_type {
        format!(" -> {}", format_ts_type(&ret_type.type_ann))
    } else {
        String::new()
    }
}

impl Visit for SymbolVisitor {
    fn visit_export_decl(&mut self, n: &ExportDecl) {
        let start_len = self.symbols.len();
        n.decl.visit_with(self);
        for i in start_len..self.symbols.len() {
            self.symbols[i].visibility = Visibility::Public;
        }
    }

    fn visit_class_decl(&mut self, n: &ClassDecl) {
        // n.ident is mandatory in newer SWC
        let ident = &n.ident;
        let name = ident.sym.to_string();

        // Check for extends
        let extends = n
            .class
            .super_class
            .as_ref()
            .map(|s| {
                if let Expr::Ident(id) = &**s {
                    format!(" extends {}", id.sym)
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();

        let sig = format!("class {}{}", name, extends);
        let mut symbol = self.create_symbol(
            name,
            SymbolKind::Class,
            Visibility::Internal,
            n.class.span,
            sig,
        );

        for member in &n.class.body {
            if let ClassMember::Method(m) = member {
                if let Some(key) = m.key.as_ident() {
                    let m_name = key.sym.to_string();
                    let params = format_params(&m.function.params);
                    let ret = format_return_type(&m.function);
                    let m_sig = format!("{}({}){}", m_name, params, ret);
                    let m_vis = if m.accessibility == Some(Accessibility::Private)
                        || m_name.starts_with('#')
                    {
                        Visibility::Private
                    } else if m.accessibility == Some(Accessibility::Protected) {
                        Visibility::Internal
                    } else {
                        Visibility::Public
                    };

                    let m_sym =
                        self.create_symbol(m_name, SymbolKind::Method, m_vis, m.span, m_sig);
                    symbol.children.push(m_sym);
                }
            }
        }

        self.symbols.push(symbol);
    }

    fn visit_fn_decl(&mut self, n: &FnDecl) {
        let name = n.ident.sym.to_string();
        let params = format_params(&n.function.params);
        let ret = format_return_type(&n.function);
        let sig = format!("function {}({}){}", name, params, ret);
        let symbol = self.create_symbol(
            name,
            SymbolKind::Function,
            Visibility::Internal,
            n.function.span,
            sig,
        );
        self.symbols.push(symbol);
    }

    fn visit_call_expr(&mut self, n: &CallExpr) {
        if let Callee::Expr(expr) = &n.callee {
            if let Expr::Member(member) = &**expr {
                if let Some(prop) = member.prop.as_ident() {
                    let method_name = prop.sym.to_string();
                    const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch"];
                    if HTTP_METHODS.contains(&method_name.as_str()) {
                        let obj_name = if let Expr::Ident(id) = &*member.obj {
                            id.sym.to_string()
                        } else {
                            "unknown".to_string()
                        };

                        let name = format!("{}.{}", obj_name, method_name);

                        let mut sig_args = String::new();
                        if let Some(arg) = n.args.first() {
                            if let Expr::Lit(Lit::Str(s)) = &*arg.expr {
                                sig_args = format!("'{}', ...", s.value);
                            }
                        }

                        if !sig_args.is_empty() {
                            let sig = format!("{}({})", name, sig_args);
                            let symbol = self.create_symbol(
                                name,
                                SymbolKind::Function,
                                Visibility::Internal,
                                n.span,
                                sig,
                            );
                            self.symbols.push(symbol);
                        }
                    }
                }
            }
        }

        n.callee.visit_with(self);
        for arg in &n.args {
            arg.visit_with(self);
        }
    }

    fn visit_ts_interface_decl(&mut self, n: &TsInterfaceDecl) {
        let name = n.id.sym.to_string();

        // Check for extends
        let extends = if !n.extends.is_empty() {
            let ext_names: Vec<String> = n
                .extends
                .iter()
                .filter_map(|e| {
                    // e.expr is Box<Expr>, extract name from identifier
                    if let Expr::Ident(id) = &*e.expr {
                        Some(id.sym.to_string())
                    } else {
                        None
                    }
                })
                .collect();
            if ext_names.is_empty() {
                String::new()
            } else {
                format!(" extends {}", ext_names.join(", "))
            }
        } else {
            String::new()
        };

        let sig = format!("interface {}{}", name, extends);
        let mut symbol = self.create_symbol(
            name,
            SymbolKind::Interface,
            Visibility::Internal,
            n.span,
            sig,
        );

        // Extract interface members
        for member in &n.body.body {
            match member {
                TsTypeElement::TsMethodSignature(method) => {
                    if let Expr::Ident(id) = &*method.key {
                        let m_name = id.sym.to_string();
                        let params: Vec<String> =
                            method.params.iter().map(format_ts_fn_param).collect();
                        let ret = method
                            .type_ann
                            .as_ref()
                            .map(|t| format!(" -> {}", format_ts_type(&t.type_ann)))
                            .unwrap_or_default();
                        let m_sig = format!("{}({}){}", m_name, params.join(", "), ret);
                        let m_sym = self.create_symbol(
                            m_name,
                            SymbolKind::Method,
                            Visibility::Public,
                            method.span,
                            m_sig,
                        );
                        symbol.children.push(m_sym);
                    }
                }
                TsTypeElement::TsPropertySignature(prop) => {
                    if let Expr::Ident(id) = &*prop.key {
                        let p_name = id.sym.to_string();
                        let type_str = prop
                            .type_ann
                            .as_ref()
                            .map(|t| format!(": {}", format_ts_type(&t.type_ann)))
                            .unwrap_or_default();
                        let optional = if prop.optional { "?" } else { "" };
                        let p_sig = format!("{}{}{}", p_name, optional, type_str);
                        let p_sym = self.create_symbol(
                            p_name,
                            SymbolKind::Const,
                            Visibility::Public,
                            prop.span,
                            p_sig,
                        );
                        symbol.children.push(p_sym);
                    }
                }
                _ => {}
            }
        }

        self.symbols.push(symbol);
    }
}

fn collect_exports(module: &Module) -> ExportInfo {
    let mut local_exports = Vec::new();
    let mut re_exports = Vec::new();
    let mut export_all = Vec::new();
    let mut default_export = None;

    for item in &module.body {
        let ModuleItem::ModuleDecl(decl) = item else {
            continue;
        };
        match decl {
            ModuleDecl::ExportDecl(export_decl) => match &export_decl.decl {
                Decl::Class(class_decl) => {
                    local_exports.push(class_decl.ident.sym.to_string());
                }
                Decl::Fn(fn_decl) => {
                    local_exports.push(fn_decl.ident.sym.to_string());
                }
                Decl::Var(var_decl) => {
                    for declarator in &var_decl.decls {
                        if let Pat::Ident(ident) = &declarator.name {
                            local_exports.push(ident.id.sym.to_string());
                        }
                    }
                }
                Decl::TsInterface(interface_decl) => {
                    local_exports.push(interface_decl.id.sym.to_string());
                }
                _ => {}
            },
            ModuleDecl::ExportNamed(named) => {
                let source = named.src.as_ref().map(|s| s.value.to_string());
                let mut names = Vec::new();
                for specifier in &named.specifiers {
                    if let ExportSpecifier::Named(named_spec) = specifier {
                        let exported = export_name_to_string(
                            named_spec.exported.as_ref().unwrap_or(&named_spec.orig),
                        );
                        let original = export_name_to_string(&named_spec.orig);
                        names.push(ExportName { exported, original });
                    }
                }
                if let Some(source) = source {
                    re_exports.push(ReExport { source, names });
                } else {
                    for name in names {
                        if name.exported == "default" {
                            default_export.get_or_insert_with(|| name.original.clone());
                        }
                        local_exports.push(name.exported);
                    }
                }
            }
            ModuleDecl::ExportAll(all) => {
                export_all.push(all.src.value.to_string());
            }
            ModuleDecl::ExportDefaultDecl(default_decl) => {
                local_exports.push("default".to_string());
                match &default_decl.decl {
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
            ModuleDecl::ExportDefaultExpr(default_expr) => {
                local_exports.push("default".to_string());
                if let Expr::Ident(ident) = &*default_expr.expr {
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

fn collect_imports(module: &Module) -> Vec<ImportInfo> {
    let mut imports = Vec::new();
    for item in &module.body {
        let ModuleItem::ModuleDecl(ModuleDecl::Import(import_decl)) = item else {
            continue;
        };

        let mut named = Vec::new();
        let mut default = None;
        let mut namespace = false;

        for specifier in &import_decl.specifiers {
            match specifier {
                ImportSpecifier::Named(named_spec) => {
                    let imported = named_spec
                        .imported
                        .as_ref()
                        .map(export_name_to_string)
                        .unwrap_or_else(|| named_spec.local.sym.to_string());
                    named.push(imported);
                }
                ImportSpecifier::Default(default_spec) => {
                    default = Some(default_spec.local.sym.to_string());
                }
                ImportSpecifier::Namespace(_) => {
                    namespace = true;
                }
            }
        }

        imports.push(ImportInfo {
            source: import_decl.src.value.to_string(),
            named,
            default,
            namespace,
        });
    }

    imports
}

fn export_name_to_string(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(string) => string.value.to_string(),
    }
}
