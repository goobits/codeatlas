pub(super) fn format_fn_signature(signature: &syn::Signature) -> String {
    let name = signature.ident.to_string();
    let parameters = signature
        .inputs
        .iter()
        .map(|argument| match argument {
            syn::FnArg::Receiver(receiver) => {
                let mut rendered = String::new();
                if receiver.reference.is_some() {
                    rendered.push('&');
                    if receiver.mutability.is_some() {
                        rendered.push_str("mut ");
                    }
                }
                rendered.push_str("self");
                rendered
            }
            syn::FnArg::Typed(typed) => {
                let name = match &*typed.pat {
                    syn::Pat::Ident(identifier) => identifier.ident.to_string(),
                    _ => "_".to_string(),
                };
                format!("{}: {}", name, format_type(&typed.ty))
            }
        })
        .collect::<Vec<_>>();
    let return_type = match &signature.output {
        syn::ReturnType::Default => String::new(),
        syn::ReturnType::Type(_, value_type) => format!(" -> {}", format_type(value_type)),
    };
    format!("fn {}({}){}", name, parameters.join(", "), return_type)
}

pub(super) fn format_type(value_type: &syn::Type) -> String {
    match value_type {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .iter()
            .map(|segment| {
                let name = segment.ident.to_string();
                match &segment.arguments {
                    syn::PathArguments::None => name,
                    syn::PathArguments::AngleBracketed(arguments) => {
                        let inner = arguments
                            .args
                            .iter()
                            .filter_map(|argument| match argument {
                                syn::GenericArgument::Type(value_type) => {
                                    Some(format_type(value_type))
                                }
                                syn::GenericArgument::Lifetime(lifetime) => {
                                    Some(format!("'{}", lifetime.ident))
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        if inner.is_empty() {
                            name
                        } else {
                            format!("{}<{}>", name, inner.join(", "))
                        }
                    }
                    syn::PathArguments::Parenthesized(arguments) => {
                        let inputs = arguments.inputs.iter().map(format_type).collect::<Vec<_>>();
                        let return_type = match &arguments.output {
                            syn::ReturnType::Default => String::new(),
                            syn::ReturnType::Type(_, value_type) => {
                                format!(" -> {}", format_type(value_type))
                            }
                        };
                        format!("{}({}){}", name, inputs.join(", "), return_type)
                    }
                }
            })
            .collect::<Vec<_>>()
            .join("::"),
        syn::Type::Reference(reference) => {
            let mut rendered = String::from("&");
            if let Some(lifetime) = &reference.lifetime {
                rendered.push_str(&format!("'{} ", lifetime.ident));
            }
            if reference.mutability.is_some() {
                rendered.push_str("mut ");
            }
            rendered.push_str(&format_type(&reference.elem));
            rendered
        }
        syn::Type::Slice(slice) => format!("[{}]", format_type(&slice.elem)),
        syn::Type::Array(array) => {
            let length = match &array.len {
                syn::Expr::Lit(literal) => match &literal.lit {
                    syn::Lit::Int(integer) => integer.base10_digits().to_string(),
                    _ => "N".to_string(),
                },
                _ => "N".to_string(),
            };
            format!("[{}; {}]", format_type(&array.elem), length)
        }
        syn::Type::Tuple(tuple) => {
            let elements = tuple.elems.iter().map(format_type).collect::<Vec<_>>();
            format!("({})", elements.join(", "))
        }
        syn::Type::Ptr(pointer) => {
            let mut rendered = String::from("*");
            if pointer.mutability.is_some() {
                rendered.push_str("mut ");
            } else {
                rendered.push_str("const ");
            }
            rendered.push_str(&format_type(&pointer.elem));
            rendered
        }
        syn::Type::ImplTrait(implementation) => {
            format!("impl {}", format_trait_bounds(&implementation.bounds))
        }
        syn::Type::TraitObject(object) => {
            format!("dyn {}", format_trait_bounds(&object.bounds))
        }
        _ => "...".to_string(),
    }
}

fn format_trait_bounds(
    bounds: &syn::punctuated::Punctuated<syn::TypeParamBound, syn::token::Plus>,
) -> String {
    bounds
        .iter()
        .filter_map(|bound| match bound {
            syn::TypeParamBound::Trait(trait_bound) => Some(
                trait_bound
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::"),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" + ")
}
