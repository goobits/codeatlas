use codeatlas_domain::{SymbolKind, Visibility};
use swc_core::ecma::ast::*;

pub(super) fn kind_to_str(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class => "class",
        SymbolKind::Function => "fn",
        SymbolKind::Interface => "interface",
        SymbolKind::Method => "method",
        _ => "sym",
    }
}

pub(super) fn format_params(params: &[Param]) -> String {
    params
        .iter()
        .map(|param| format_pat(&param.pat))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn format_type_params(params: Option<&TsTypeParamDecl>) -> String {
    params
        .map(|params| {
            format!(
                "<{}>",
                params
                    .params
                    .iter()
                    .map(format_type_param)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .unwrap_or_default()
}

pub(super) fn format_binding_ident(ident: &BindingIdent) -> String {
    let mut name = ident.id.sym.to_string();
    if ident.optional {
        name.push('?');
    }
    if let Some(type_ann) = &ident.type_ann {
        name.push_str(": ");
        name.push_str(&format_ts_type(&type_ann.type_ann));
    }
    name
}

pub(super) fn format_pat(pat: &Pat) -> String {
    match pat {
        Pat::Ident(ident) => format_binding_ident(ident),
        Pat::Rest(rest) => {
            let mut value = format!("...{}", format_pat(&rest.arg));
            append_type_annotation(&mut value, rest.type_ann.as_deref());
            value
        }
        Pat::Object(object) => {
            let mut value = "{ ... }".to_string();
            append_type_annotation(&mut value, object.type_ann.as_deref());
            value
        }
        Pat::Array(array) => {
            let mut value = "[ ... ]".to_string();
            append_type_annotation(&mut value, array.type_ann.as_deref());
            value
        }
        Pat::Assign(assign) => format!(
            "{} = {}",
            format_pat(&assign.left),
            format_default_expr(&assign.right)
        ),
        _ => "_".to_string(),
    }
}

fn append_type_annotation(value: &mut String, annotation: Option<&TsTypeAnn>) {
    if let Some(annotation) = annotation {
        value.push_str(": ");
        value.push_str(&format_ts_type(&annotation.type_ann));
    }
}

fn format_default_expr(expression: &Expr) -> String {
    match expression {
        Expr::Ident(ident) => ident.sym.to_string(),
        Expr::Lit(Lit::Str(value)) => format!("\"{}\"", value.value),
        Expr::Lit(Lit::Num(value)) => value.value.to_string(),
        Expr::Lit(Lit::Bool(value)) => value.value.to_string(),
        Expr::Lit(Lit::Null(_)) => "null".to_string(),
        Expr::Lit(Lit::BigInt(value)) => format!("{}n", value.value),
        Expr::Object(object) if object.props.is_empty() => "{}".to_string(),
        Expr::Array(array) if array.elems.is_empty() => "[]".to_string(),
        Expr::Call(call) => match &call.callee {
            Callee::Expr(callee) => format!(
                "{}(...)",
                expression_name(callee).unwrap_or_else(|| "...".to_string())
            ),
            _ => "...".to_string(),
        },
        _ => "...".to_string(),
    }
}

pub(super) fn expression_name(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Ident(ident) => Some(ident.sym.to_string()),
        Expr::Member(member) => {
            let object = expression_name(&member.obj)?;
            let property = member.prop.as_ident()?.sym.to_string();
            Some(format!("{}.{}", object, property))
        }
        Expr::Paren(parenthesized) => expression_name(&parenthesized.expr),
        Expr::TsAs(assertion) => expression_name(&assertion.expr),
        Expr::TsNonNull(non_null) => expression_name(&non_null.expr),
        Expr::TsTypeAssertion(assertion) => expression_name(&assertion.expr),
        _ => None,
    }
}

pub(super) fn format_prop_name(name: &PropName) -> Option<String> {
    match name {
        PropName::Ident(ident) => Some(ident.sym.to_string()),
        PropName::Str(value) => Some(format!("\"{}\"", value.value)),
        PropName::Num(value) => Some(value.value.to_string()),
        PropName::BigInt(value) => Some(value.value.to_string()),
        PropName::Computed(_) => None,
    }
}

pub(super) fn member_visibility(
    accessibility: Option<Accessibility>,
    private_name: bool,
) -> Visibility {
    if private_name || accessibility == Some(Accessibility::Private) {
        Visibility::Private
    } else if accessibility == Some(Accessibility::Protected) {
        Visibility::Internal
    } else {
        Visibility::Public
    }
}

pub(super) fn format_constructor_param(param: &ParamOrTsParamProp) -> String {
    match param {
        ParamOrTsParamProp::Param(param) => format_pat(&param.pat),
        ParamOrTsParamProp::TsParamProp(property) => {
            let value = match &property.param {
                TsParamPropParam::Ident(ident) => format_binding_ident(ident),
                TsParamPropParam::Assign(assign) => format!(
                    "{} = {}",
                    format_pat(&assign.left),
                    format_default_expr(&assign.right)
                ),
            };
            let accessibility = match property.accessibility {
                Some(Accessibility::Public) => "public ",
                Some(Accessibility::Protected) => "protected ",
                Some(Accessibility::Private) => "private ",
                None => "",
            };
            let readonly = if property.readonly { "readonly " } else { "" };
            format!("{}{}{}", accessibility, readonly, value)
        }
    }
}

pub(super) fn format_entity_name(name: &TsEntityName) -> String {
    match name {
        TsEntityName::Ident(id) => id.sym.to_string(),
        TsEntityName::TsQualifiedName(name) => {
            format!("{}.{}", format_entity_name(&name.left), name.right.sym)
        }
    }
}

pub(super) fn format_ts_type(ts_type: &TsType) -> String {
    match ts_type {
        TsType::TsKeywordType(keyword) => match keyword.kind {
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
                let inner = params
                    .params
                    .iter()
                    .map(|param| format_ts_type(param))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{}>", name, inner)
            } else {
                name
            }
        }
        TsType::TsArrayType(array) => format!("{}[]", format_ts_type(&array.elem_type)),
        TsType::TsTupleType(tuple) => {
            let elements = tuple
                .elem_types
                .iter()
                .map(|element| {
                    let value = format_ts_type(&element.ty);
                    element
                        .label
                        .as_ref()
                        .map(|label| format!("{}: {}", format_pat(label), value))
                        .unwrap_or(value)
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", elements)
        }
        TsType::TsOptionalType(optional) => format!("{}?", format_ts_type(&optional.type_ann)),
        TsType::TsRestType(rest) => format!("...{}", format_ts_type(&rest.type_ann)),
        TsType::TsUnionOrIntersectionType(union_or_intersection) => match union_or_intersection {
            TsUnionOrIntersectionType::TsUnionType(union) => union
                .types
                .iter()
                .map(|value| format_ts_type(value))
                .collect::<Vec<_>>()
                .join(" | "),
            TsUnionOrIntersectionType::TsIntersectionType(intersection) => intersection
                .types
                .iter()
                .map(|value| format_ts_type(value))
                .collect::<Vec<_>>()
                .join(" & "),
        },
        TsType::TsFnOrConstructorType(function) => match function {
            TsFnOrConstructorType::TsFnType(function) => {
                let params = function
                    .params
                    .iter()
                    .map(format_ts_fn_param)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{}({}) => {}",
                    format_type_params(function.type_params.as_deref()),
                    params,
                    format_ts_type(&function.type_ann.type_ann)
                )
            }
            TsFnOrConstructorType::TsConstructorType(function) => {
                let params = function
                    .params
                    .iter()
                    .map(format_ts_fn_param)
                    .collect::<Vec<_>>()
                    .join(", ");
                let abstract_prefix = if function.is_abstract {
                    "abstract "
                } else {
                    ""
                };
                format!(
                    "{}new {}({}) => {}",
                    abstract_prefix,
                    format_type_params(function.type_params.as_deref()),
                    params,
                    format_ts_type(&function.type_ann.type_ann)
                )
            }
        },
        TsType::TsTypeLit(literal) => format_type_literal(&literal.members),
        TsType::TsLitType(literal) => match &literal.lit {
            TsLit::Str(value) => format!("\"{}\"", value.value),
            TsLit::Number(value) => value.value.to_string(),
            TsLit::Bool(value) => value.value.to_string(),
            TsLit::BigInt(value) => format!("{}n", value.value),
            TsLit::Tpl(template) => format_template_type(template),
        },
        TsType::TsThisType(_) => "this".to_string(),
        TsType::TsTypePredicate(predicate) => {
            let param = match &predicate.param_name {
                TsThisTypeOrIdent::TsThisType(_) => "this".to_string(),
                TsThisTypeOrIdent::Ident(ident) => ident.sym.to_string(),
            };
            let assertion = if predicate.asserts { "asserts " } else { "" };
            let type_annotation = predicate
                .type_ann
                .as_ref()
                .map(|annotation| format!(" is {}", format_ts_type(&annotation.type_ann)))
                .unwrap_or_default();
            format!("{}{}{}", assertion, param, type_annotation)
        }
        TsType::TsTypeQuery(query) => {
            let expression = match &query.expr_name {
                TsTypeQueryExpr::TsEntityName(name) => format_entity_name(name),
                TsTypeQueryExpr::Import(import) => format_import_type(import),
            };
            format!(
                "typeof {}{}",
                expression,
                format_type_args(query.type_args.as_deref())
            )
        }
        TsType::TsImportType(import) => format_import_type(import),
        TsType::TsConditionalType(conditional) => format!(
            "{} extends {} ? {} : {}",
            format_ts_type(&conditional.check_type),
            format_ts_type(&conditional.extends_type),
            format_ts_type(&conditional.true_type),
            format_ts_type(&conditional.false_type)
        ),
        TsType::TsInferType(inferred) => {
            format!("infer {}", format_type_param(&inferred.type_param))
        }
        TsType::TsParenthesizedType(parenthesized) => {
            format!("({})", format_ts_type(&parenthesized.type_ann))
        }
        TsType::TsTypeOperator(operator) => {
            let operator_name = match operator.op {
                TsTypeOperatorOp::KeyOf => "keyof",
                TsTypeOperatorOp::Unique => "unique",
                TsTypeOperatorOp::ReadOnly => "readonly",
            };
            format!("{} {}", operator_name, format_ts_type(&operator.type_ann))
        }
        TsType::TsIndexedAccessType(access) => {
            let readonly = if access.readonly { "readonly " } else { "" };
            format!(
                "{}{}[{}]",
                readonly,
                format_ts_type(&access.obj_type),
                format_ts_type(&access.index_type)
            )
        }
        TsType::TsMappedType(mapped) => {
            let readonly = format_mapped_modifier(mapped.readonly, "readonly ");
            let optional = format_mapped_modifier(mapped.optional, "?");
            let remap = mapped
                .name_type
                .as_ref()
                .map(|name| format!(" as {}", format_ts_type(name)))
                .unwrap_or_default();
            let value = mapped
                .type_ann
                .as_ref()
                .map(|value| format_ts_type(value))
                .unwrap_or_else(|| "unknown".to_string());
            format!(
                "{{ {}[{} in {}{}]{}: {} }}",
                readonly,
                mapped.type_param.name.sym,
                mapped
                    .type_param
                    .constraint
                    .as_ref()
                    .map(|constraint| format_ts_type(constraint))
                    .unwrap_or_else(|| "unknown".to_string()),
                remap,
                optional,
                value
            )
        }
    }
}

pub(super) fn format_type_args(args: Option<&TsTypeParamInstantiation>) -> String {
    args.map(|args| {
        format!(
            "<{}>",
            args.params
                .iter()
                .map(|param| format_ts_type(param))
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
    .unwrap_or_default()
}

fn format_import_type(import: &TsImportType) -> String {
    let qualifier = import
        .qualifier
        .as_ref()
        .map(|qualifier| format!(".{}", format_entity_name(qualifier)))
        .unwrap_or_default();
    format!(
        "import(\"{}\"){}{}",
        import.arg.value,
        qualifier,
        format_type_args(import.type_args.as_deref())
    )
}

fn format_type_param(param: &TsTypeParam) -> String {
    let constraint = param
        .constraint
        .as_ref()
        .map(|constraint| format!(" extends {}", format_ts_type(constraint)))
        .unwrap_or_default();
    let default = param
        .default
        .as_ref()
        .map(|default| format!(" = {}", format_ts_type(default)))
        .unwrap_or_default();
    format!("{}{}{}", param.name.sym, constraint, default)
}

fn format_type_literal(members: &[TsTypeElement]) -> String {
    let members = members
        .iter()
        .filter_map(format_type_element)
        .collect::<Vec<_>>();
    if members.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", members.join("; "))
    }
}

fn format_type_element(member: &TsTypeElement) -> Option<String> {
    match member {
        TsTypeElement::TsMethodSignature(method) => {
            let name = method.key.as_ident()?.sym.to_string();
            let optional = if method.optional { "?" } else { "" };
            let params = method
                .params
                .iter()
                .map(format_ts_fn_param)
                .collect::<Vec<_>>()
                .join(", ");
            let return_type = method
                .type_ann
                .as_ref()
                .map(|annotation| format_ts_type(&annotation.type_ann))
                .unwrap_or_else(|| "unknown".to_string());
            Some(format!(
                "{}{}{}({}): {}",
                name,
                optional,
                format_type_params(method.type_params.as_deref()),
                params,
                return_type
            ))
        }
        TsTypeElement::TsPropertySignature(property) => {
            let name = property.key.as_ident()?.sym.to_string();
            let readonly = if property.readonly { "readonly " } else { "" };
            let optional = if property.optional { "?" } else { "" };
            let value = property
                .type_ann
                .as_ref()
                .map(|annotation| format_ts_type(&annotation.type_ann))
                .unwrap_or_else(|| "unknown".to_string());
            Some(format!("{}{}{}: {}", readonly, name, optional, value))
        }
        TsTypeElement::TsCallSignatureDecl(call) => {
            let params = call
                .params
                .iter()
                .map(format_ts_fn_param)
                .collect::<Vec<_>>()
                .join(", ");
            let return_type = call
                .type_ann
                .as_ref()
                .map(|annotation| format_ts_type(&annotation.type_ann))
                .unwrap_or_else(|| "unknown".to_string());
            Some(format!(
                "{}({}): {}",
                format_type_params(call.type_params.as_deref()),
                params,
                return_type
            ))
        }
        TsTypeElement::TsIndexSignature(index) => {
            let params = index
                .params
                .iter()
                .map(format_ts_fn_param)
                .collect::<Vec<_>>()
                .join(", ");
            let value = index
                .type_ann
                .as_ref()
                .map(|annotation| format_ts_type(&annotation.type_ann))
                .unwrap_or_else(|| "unknown".to_string());
            let readonly = if index.readonly { "readonly " } else { "" };
            Some(format!("{}[{}]: {}", readonly, params, value))
        }
        _ => None,
    }
}

fn format_mapped_modifier(modifier: Option<TruePlusMinus>, name: &str) -> String {
    match modifier {
        Some(TruePlusMinus::True) => name.to_string(),
        Some(TruePlusMinus::Plus) => format!("+{}", name),
        Some(TruePlusMinus::Minus) => format!("-{}", name),
        None => String::new(),
    }
}

fn format_template_type(template: &TsTplLitType) -> String {
    let mut output = String::from("`");
    for (index, quasi) in template.quasis.iter().enumerate() {
        output.push_str(&quasi.raw);
        if let Some(value) = template.types.get(index) {
            output.push_str("${");
            output.push_str(&format_ts_type(value));
            output.push('}');
        }
    }
    output.push('`');
    output
}

pub(super) fn format_ts_fn_param(param: &TsFnParam) -> String {
    match param {
        TsFnParam::Ident(ident) => format_binding_ident(ident),
        TsFnParam::Rest(rest) => format!("...{}", format_pat(&rest.arg)),
        _ => "_".to_string(),
    }
}

pub(super) fn format_return_type(function: &Function) -> String {
    function
        .return_type
        .as_ref()
        .map(|return_type| format!(" -> {}", format_ts_type(&return_type.type_ann)))
        .unwrap_or_default()
}
