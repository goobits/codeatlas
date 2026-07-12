use super::format::{
    format_binding_ident, format_constructor_param, format_params, format_pat, format_prop_name,
    format_return_type, format_ts_fn_param, format_ts_type, kind_to_str, member_visibility,
};
use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use swc_core::common::{sync::Lrc, SourceMap};
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};

pub(super) struct SymbolVisitor {
    pub(super) symbols: Vec<Symbol>,
    pub(super) relative_path: String,
    pub(super) source_map: Lrc<SourceMap>,
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
        let start = self.source_map.lookup_char_pos(span.lo);
        let end = self.source_map.lookup_char_pos(span.hi);
        Symbol {
            id: format!("ts:{}:{}#{}", self.relative_path, kind_to_str(kind), name),
            name,
            kind,
            visibility,
            language: Language::TypeScript,
            file_path: self.relative_path.clone(),
            span: Some(Span {
                start_line: start.line as u32,
                start_col: start.col.0 as u32,
                end_line: end.line as u32,
                end_col: end.col.0 as u32,
            }),
            signature,
            docs: None,
            export_paths: vec![],
            package: None,
            children: vec![],
        }
    }

    fn create_type_members(&self, members: &[TsTypeElement]) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        for member in members {
            match member {
                TsTypeElement::TsMethodSignature(method) => {
                    if let Expr::Ident(id) = &*method.key {
                        let name = id.sym.to_string();
                        let params = method
                            .params
                            .iter()
                            .map(format_ts_fn_param)
                            .collect::<Vec<_>>()
                            .join(", ");
                        let return_type = method
                            .type_ann
                            .as_ref()
                            .map(|annotation| {
                                format!(" -> {}", format_ts_type(&annotation.type_ann))
                            })
                            .unwrap_or_default();
                        symbols.push(self.create_symbol(
                            name.clone(),
                            SymbolKind::Method,
                            Visibility::Public,
                            method.span,
                            format!("{}({}){}", name, params, return_type),
                        ));
                    }
                }
                TsTypeElement::TsPropertySignature(property) => {
                    if let Expr::Ident(id) = &*property.key {
                        let name = id.sym.to_string();
                        let type_annotation = property
                            .type_ann
                            .as_ref()
                            .map(|annotation| format!(": {}", format_ts_type(&annotation.type_ann)))
                            .unwrap_or_default();
                        let optional = if property.optional { "?" } else { "" };
                        symbols.push(self.create_symbol(
                            name.clone(),
                            SymbolKind::Property,
                            Visibility::Public,
                            property.span,
                            format!("{}{}{}", name, optional, type_annotation),
                        ));
                    }
                }
                _ => {}
            }
        }
        symbols
    }

    fn qualify_children(&self, parent: &str, children: &mut [Symbol]) {
        for child in children {
            let qualified_name = format!("{}.{}", parent, child.name);
            child.id = format!(
                "ts:{}:{}#{}",
                self.relative_path,
                kind_to_str(child.kind),
                qualified_name
            );
            self.qualify_children(&qualified_name, &mut child.children);
        }
    }
}

impl Visit for SymbolVisitor {
    fn visit_export_decl(&mut self, declaration: &ExportDecl) {
        let start_len = self.symbols.len();
        declaration.decl.visit_with(self);
        for symbol in &mut self.symbols[start_len..] {
            symbol.visibility = Visibility::Public;
        }
    }

    fn visit_class_decl(&mut self, declaration: &ClassDecl) {
        let name = declaration.ident.sym.to_string();
        let extends = declaration
            .class
            .super_class
            .as_ref()
            .and_then(|super_class| {
                if let Expr::Ident(ident) = &**super_class {
                    Some(format!(" extends {}", ident.sym))
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let mut symbol = self.create_symbol(
            name.clone(),
            SymbolKind::Class,
            Visibility::Internal,
            declaration.class.span,
            format!("class {}{}", name, extends),
        );

        for member in &declaration.class.body {
            match member {
                ClassMember::Constructor(constructor) => {
                    let params = constructor
                        .params
                        .iter()
                        .map(format_constructor_param)
                        .collect::<Vec<_>>()
                        .join(", ");
                    symbol.children.push(self.create_symbol(
                        "constructor".to_string(),
                        SymbolKind::Method,
                        member_visibility(constructor.accessibility, false),
                        constructor.span,
                        format!("constructor({})", params),
                    ));

                    for param in &constructor.params {
                        let ParamOrTsParamProp::TsParamProp(property) = param else {
                            continue;
                        };
                        let (name, signature) = match &property.param {
                            TsParamPropParam::Ident(ident) => {
                                (ident.id.sym.to_string(), format_binding_ident(ident))
                            }
                            TsParamPropParam::Assign(assign) => {
                                let Pat::Ident(ident) = &*assign.left else {
                                    continue;
                                };
                                (
                                    ident.id.sym.to_string(),
                                    format!("{} = ...", format_binding_ident(ident)),
                                )
                            }
                        };
                        let readonly = if property.readonly { "readonly " } else { "" };
                        let mut parameter_property = self.create_symbol(
                            name,
                            SymbolKind::Property,
                            member_visibility(property.accessibility, false),
                            property.span,
                            format!("{}{}", readonly, signature),
                        );
                        // Parameter properties share their constructor's source range and docs.
                        parameter_property.span = None;
                        symbol.children.push(parameter_property);
                    }
                }
                ClassMember::Method(method) => {
                    if let Some(name) = format_prop_name(&method.key) {
                        let method_kind = match method.kind {
                            MethodKind::Getter => "get ",
                            MethodKind::Setter => "set ",
                            MethodKind::Method => "",
                        };
                        let static_prefix = if method.is_static { "static " } else { "" };
                        let optional = if method.is_optional { "?" } else { "" };
                        let signature = format!(
                            "{}{}{}{}({}){}",
                            static_prefix,
                            method_kind,
                            name,
                            optional,
                            format_params(&method.function.params),
                            format_return_type(&method.function)
                        );
                        symbol.children.push(self.create_symbol(
                            name,
                            SymbolKind::Method,
                            member_visibility(method.accessibility, false),
                            method.span,
                            signature,
                        ));
                    }
                }
                ClassMember::PrivateMethod(method) => {
                    let name = format!("#{}", method.key.id.sym);
                    symbol.children.push(self.create_symbol(
                        name.clone(),
                        SymbolKind::Method,
                        Visibility::Private,
                        method.span,
                        format!(
                            "{}({}){}",
                            name,
                            format_params(&method.function.params),
                            format_return_type(&method.function)
                        ),
                    ));
                }
                ClassMember::ClassProp(property) => {
                    if let Some(name) = format_prop_name(&property.key) {
                        let static_prefix = if property.is_static { "static " } else { "" };
                        let readonly = if property.readonly { "readonly " } else { "" };
                        let optional = if property.is_optional { "?" } else { "" };
                        let type_annotation = property
                            .type_ann
                            .as_ref()
                            .map(|annotation| format!(": {}", format_ts_type(&annotation.type_ann)))
                            .unwrap_or_default();
                        symbol.children.push(self.create_symbol(
                            name.clone(),
                            SymbolKind::Property,
                            member_visibility(property.accessibility, false),
                            property.span,
                            format!(
                                "{}{}{}{}{}",
                                static_prefix, readonly, name, optional, type_annotation
                            ),
                        ));
                    }
                }
                ClassMember::PrivateProp(property) => {
                    let name = format!("#{}", property.key.id.sym);
                    let static_prefix = if property.is_static { "static " } else { "" };
                    let readonly = if property.readonly { "readonly " } else { "" };
                    let type_annotation = property
                        .type_ann
                        .as_ref()
                        .map(|annotation| format!(": {}", format_ts_type(&annotation.type_ann)))
                        .unwrap_or_default();
                    symbol.children.push(self.create_symbol(
                        name.clone(),
                        SymbolKind::Property,
                        Visibility::Private,
                        property.span,
                        format!("{}{}{}{}", static_prefix, readonly, name, type_annotation),
                    ));
                }
                _ => {}
            }
        }

        self.qualify_children(&name, &mut symbol.children);
        self.symbols.push(symbol);
    }

    fn visit_fn_decl(&mut self, declaration: &FnDecl) {
        let name = declaration.ident.sym.to_string();
        self.symbols.push(self.create_symbol(
            name.clone(),
            SymbolKind::Function,
            Visibility::Internal,
            declaration.function.span,
            format!(
                "function {}({}){}",
                name,
                format_params(&declaration.function.params),
                format_return_type(&declaration.function)
            ),
        ));
    }

    fn visit_var_decl(&mut self, declaration: &VarDecl) {
        let declaration_kind = match declaration.kind {
            VarDeclKind::Const => "const",
            VarDeclKind::Let => "let",
            VarDeclKind::Var => "var",
        };
        for variable in &declaration.decls {
            let Pat::Ident(binding) = &variable.name else {
                continue;
            };
            let name = binding.id.sym.to_string();
            let (kind, signature) = match variable.init.as_deref() {
                Some(Expr::Arrow(arrow)) => {
                    let params = arrow
                        .params
                        .iter()
                        .map(format_pat)
                        .collect::<Vec<_>>()
                        .join(", ");
                    let return_type = arrow
                        .return_type
                        .as_ref()
                        .map(|return_type| format_ts_type(&return_type.type_ann))
                        .unwrap_or_else(|| "...".to_string());
                    (
                        SymbolKind::Function,
                        format!("const {} = ({}) => {}", name, params, return_type),
                    )
                }
                Some(Expr::Fn(function)) => (
                    SymbolKind::Function,
                    format!(
                        "const {} = function({}){}",
                        name,
                        format_params(&function.function.params),
                        format_return_type(&function.function)
                    ),
                ),
                _ => {
                    let type_annotation = binding
                        .type_ann
                        .as_ref()
                        .map(|annotation| format!(": {}", format_ts_type(&annotation.type_ann)))
                        .unwrap_or_default();
                    (
                        SymbolKind::Const,
                        format!("{} {}{}", declaration_kind, name, type_annotation),
                    )
                }
            };
            self.symbols.push(self.create_symbol(
                name,
                kind,
                Visibility::Internal,
                variable.span,
                signature,
            ));
        }
    }

    fn visit_ts_type_alias_decl(&mut self, declaration: &TsTypeAliasDecl) {
        let name = declaration.id.sym.to_string();
        let type_params = declaration
            .type_params
            .as_ref()
            .map(|params| {
                let names = params
                    .params
                    .iter()
                    .map(|param| param.name.sym.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("<{}>", names)
            })
            .unwrap_or_default();
        let mut symbol = self.create_symbol(
            name.clone(),
            SymbolKind::TypeAlias,
            Visibility::Internal,
            declaration.span,
            format!(
                "type {}{} = {}",
                name,
                type_params,
                format_ts_type(&declaration.type_ann)
            ),
        );
        if let TsType::TsTypeLit(type_literal) = &*declaration.type_ann {
            symbol.children = self.create_type_members(&type_literal.members);
        }
        self.qualify_children(&name, &mut symbol.children);
        self.symbols.push(symbol);
    }

    fn visit_ts_enum_decl(&mut self, declaration: &TsEnumDecl) {
        let name = declaration.id.sym.to_string();
        self.symbols.push(self.create_symbol(
            name.clone(),
            SymbolKind::Enum,
            Visibility::Internal,
            declaration.span,
            format!("enum {}", name),
        ));
    }

    fn visit_call_expr(&mut self, call: &CallExpr) {
        if let Callee::Expr(expression) = &call.callee {
            if let Expr::Member(member) = &**expression {
                if let Some(property) = member.prop.as_ident() {
                    let method_name = property.sym.to_string();
                    const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch"];
                    if HTTP_METHODS.contains(&method_name.as_str()) {
                        let object_name = if let Expr::Ident(ident) = &*member.obj {
                            ident.sym.to_string()
                        } else {
                            "unknown".to_string()
                        };
                        let name = format!("{}.{}", object_name, method_name);
                        if let Some(argument) = call.args.first() {
                            if let Expr::Lit(Lit::Str(path)) = &*argument.expr {
                                self.symbols.push(self.create_symbol(
                                    name.clone(),
                                    SymbolKind::Function,
                                    Visibility::Internal,
                                    call.span,
                                    format!("{}('{}', ...)", name, path.value),
                                ));
                            }
                        }
                    }
                }
            }
        }

        call.callee.visit_with(self);
        for argument in &call.args {
            argument.visit_with(self);
        }
    }

    fn visit_ts_interface_decl(&mut self, declaration: &TsInterfaceDecl) {
        let name = declaration.id.sym.to_string();
        let extended = declaration
            .extends
            .iter()
            .filter_map(|extension| {
                if let Expr::Ident(ident) = &*extension.expr {
                    Some(ident.sym.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let extends = if extended.is_empty() {
            String::new()
        } else {
            format!(" extends {}", extended.join(", "))
        };
        let mut symbol = self.create_symbol(
            name.clone(),
            SymbolKind::Interface,
            Visibility::Internal,
            declaration.span,
            format!("interface {}{}", name, extends),
        );
        symbol.children = self.create_type_members(&declaration.body.body);
        self.qualify_children(&name, &mut symbol.children);
        self.symbols.push(symbol);
    }
}
