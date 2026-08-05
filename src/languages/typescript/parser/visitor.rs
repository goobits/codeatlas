use super::callable;
use super::format::{
    format_binding_ident, format_constructor_param, format_params, format_pat, format_prop_name,
    format_return_type, format_ts_fn_param, format_ts_type, format_type_args, format_type_params,
    kind_to_str, member_visibility,
};
use crate::domain::{
    CallableContract, CallableKind, Language, Span, Symbol, SymbolKind, Visibility,
};
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
            callable: None,
            docs: None,
            export_paths: vec![],
            referenced: false,
            package: None,
            children: vec![],
        }
    }

    fn create_callable_symbol(
        &self,
        name: String,
        kind: SymbolKind,
        visibility: Visibility,
        span: swc_core::common::Span,
        signature: String,
        callable: CallableContract,
    ) -> Symbol {
        let mut symbol = self.create_symbol(name, kind, visibility, span, signature);
        symbol.callable = Some(callable);
        symbol
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
                        symbols.push(self.create_callable_symbol(
                            name.clone(),
                            SymbolKind::Method,
                            Visibility::Public,
                            method.span,
                            format!(
                                "{}{}({}){}",
                                name,
                                format_type_params(method.type_params.as_deref()),
                                params,
                                return_type
                            ),
                            callable::method_signature_contract(method),
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
                        let readonly = if property.readonly { "readonly " } else { "" };
                        symbols.push(self.create_symbol(
                            name.clone(),
                            SymbolKind::Property,
                            Visibility::Public,
                            property.span,
                            format!("{}{}{}{}", readonly, name, optional, type_annotation),
                        ));
                    }
                }
                TsTypeElement::TsGetterSignature(getter) => {
                    if let Expr::Ident(id) = &*getter.key {
                        let name = id.sym.to_string();
                        let readonly = if getter.readonly { "readonly " } else { "" };
                        let optional = if getter.optional { "?" } else { "" };
                        let return_type = getter
                            .type_ann
                            .as_ref()
                            .map(|annotation| {
                                format!(" -> {}", format_ts_type(&annotation.type_ann))
                            })
                            .unwrap_or_default();
                        symbols.push(self.create_callable_symbol(
                            name.clone(),
                            SymbolKind::Property,
                            Visibility::Public,
                            getter.span,
                            format!("{}get {}{}(){}", readonly, name, optional, return_type),
                            callable::getter_signature_contract(getter),
                        ));
                    }
                }
                TsTypeElement::TsSetterSignature(setter) => {
                    if let Expr::Ident(id) = &*setter.key {
                        let name = id.sym.to_string();
                        let readonly = if setter.readonly { "readonly " } else { "" };
                        let optional = if setter.optional { "?" } else { "" };
                        symbols.push(self.create_callable_symbol(
                            name.clone(),
                            SymbolKind::Property,
                            Visibility::Public,
                            setter.span,
                            format!(
                                "{}set {}{}({})",
                                readonly,
                                name,
                                optional,
                                format_ts_fn_param(&setter.param)
                            ),
                            callable::setter_signature_contract(setter),
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

    fn visit_export_default_decl(&mut self, declaration: &ExportDefaultDecl) {
        let start_len = self.symbols.len();
        match &declaration.decl {
            DefaultDecl::Class(expression) => {
                self.visit_class_decl(&ClassDecl {
                    ident: expression
                        .ident
                        .clone()
                        .unwrap_or_else(|| Ident::new("default".into(), expression.class.span)),
                    declare: false,
                    class: expression.class.clone(),
                });
            }
            DefaultDecl::Fn(expression) => {
                self.visit_fn_decl(&FnDecl {
                    ident: expression
                        .ident
                        .clone()
                        .unwrap_or_else(|| Ident::new("default".into(), expression.function.span)),
                    declare: false,
                    function: expression.function.clone(),
                });
            }
            DefaultDecl::TsInterfaceDecl(interface) => {
                self.visit_ts_interface_decl(interface);
            }
        }
        for symbol in &mut self.symbols[start_len..] {
            symbol.visibility = Visibility::Public;
        }
    }

    fn visit_export_default_expr(&mut self, declaration: &ExportDefaultExpr) {
        if matches!(&*declaration.expr, Expr::Ident(_)) {
            return;
        }
        let kind = match &*declaration.expr {
            Expr::Arrow(_) | Expr::Fn(_) => SymbolKind::Function,
            Expr::Class(_) => SymbolKind::Class,
            _ => SymbolKind::Const,
        };
        let mut symbol = self.create_symbol(
            "default".to_string(),
            kind,
            Visibility::Public,
            declaration.span,
            "default export".to_string(),
        );
        symbol.callable = match &*declaration.expr {
            Expr::Arrow(arrow) => Some(callable::arrow_contract(arrow)),
            Expr::Fn(function) => Some(callable::function_contract(
                &function.function,
                CallableKind::Function,
                callable::receiver(true),
            )),
            _ => None,
        };
        self.symbols.push(symbol);
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
            format!(
                "class {}{}{}",
                name,
                format_type_params(declaration.class.type_params.as_deref()),
                extends
            ),
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
                    symbol.children.push(self.create_callable_symbol(
                        "constructor".to_string(),
                        SymbolKind::Method,
                        member_visibility(constructor.accessibility, false),
                        constructor.span,
                        format!("constructor({})", params),
                        callable::constructor_contract(constructor),
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
                            "{}{}{}{}{}({}){}",
                            static_prefix,
                            method_kind,
                            name,
                            optional,
                            format_type_params(method.function.type_params.as_deref()),
                            format_params(&method.function.params),
                            format_return_type(&method.function)
                        );
                        symbol.children.push(self.create_callable_symbol(
                            name,
                            if method.kind == MethodKind::Method {
                                SymbolKind::Method
                            } else {
                                SymbolKind::Property
                            },
                            member_visibility(method.accessibility, false),
                            method.span,
                            signature,
                            callable::function_contract(
                                &method.function,
                                match method.kind {
                                    MethodKind::Method => CallableKind::Method,
                                    MethodKind::Getter => CallableKind::Getter,
                                    MethodKind::Setter => CallableKind::Setter,
                                },
                                callable::receiver(method.is_static),
                            ),
                        ));
                    }
                }
                ClassMember::PrivateMethod(method) => {
                    let name = format!("#{}", method.key.id.sym);
                    symbol.children.push(self.create_callable_symbol(
                        name.clone(),
                        if method.kind == MethodKind::Method {
                            SymbolKind::Method
                        } else {
                            SymbolKind::Property
                        },
                        Visibility::Private,
                        method.span,
                        format!(
                            "{}({}){}",
                            name,
                            format_params(&method.function.params),
                            format_return_type(&method.function)
                        ),
                        callable::function_contract(
                            &method.function,
                            match method.kind {
                                MethodKind::Method => CallableKind::Method,
                                MethodKind::Getter => CallableKind::Getter,
                                MethodKind::Setter => CallableKind::Setter,
                            },
                            callable::receiver(method.is_static),
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
        self.symbols.push(self.create_callable_symbol(
            name.clone(),
            SymbolKind::Function,
            Visibility::Internal,
            declaration.function.span,
            format!(
                "function {}{}({}){}",
                name,
                format_type_params(declaration.function.type_params.as_deref()),
                format_params(&declaration.function.params),
                format_return_type(&declaration.function)
            ),
            callable::function_contract(
                &declaration.function,
                CallableKind::Function,
                callable::receiver(true),
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
            let (kind, signature, callable) = match variable.init.as_deref() {
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
                        Some(callable::arrow_contract(arrow)),
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
                    Some(callable::function_contract(
                        &function.function,
                        CallableKind::Function,
                        callable::receiver(true),
                    )),
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
                        None,
                    )
                }
            };
            let mut symbol =
                self.create_symbol(name, kind, Visibility::Internal, variable.span, signature);
            symbol.callable = callable;
            self.symbols.push(symbol);
        }
    }

    fn visit_ts_type_alias_decl(&mut self, declaration: &TsTypeAliasDecl) {
        let name = declaration.id.sym.to_string();
        let type_params = format_type_params(declaration.type_params.as_deref());
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

    fn visit_ts_interface_decl(&mut self, declaration: &TsInterfaceDecl) {
        let name = declaration.id.sym.to_string();
        let extended = declaration
            .extends
            .iter()
            .filter_map(|extension| {
                if let Expr::Ident(ident) = &*extension.expr {
                    Some(format!(
                        "{}{}",
                        ident.sym,
                        format_type_args(extension.type_args.as_deref())
                    ))
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
            format!(
                "interface {}{}{}",
                name,
                format_type_params(declaration.type_params.as_deref()),
                extends
            ),
        );
        symbol.children = self.create_type_members(&declaration.body.body);
        self.qualify_children(&name, &mut symbol.children);
        self.symbols.push(symbol);
    }
}
