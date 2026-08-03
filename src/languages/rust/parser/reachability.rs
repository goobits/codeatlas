use super::{
    span, RustEmbeddedSource, RustReachabilityFacts, RustUncertainty, RustUncertaintyKind,
};
use quote::ToTokens;
use std::collections::BTreeSet;
use syn::{parse::Parser, spanned::Spanned, visit::Visit, Attribute, ItemFn, ItemImpl};

pub(super) fn collect(file: &syn::File) -> RustReachabilityFacts {
    let mut facts = RustReachabilityFacts::default();
    for item in &file.items {
        collect_item_reachability(item, &mut facts);
    }
    let mut tests = TestSymbolVisitor::default();
    tests.visit_file(file);
    facts.test_symbols = tests.symbols;
    facts
}

fn collect_item_reachability(item: &syn::Item, facts: &mut RustReachabilityFacts) {
    match item {
        syn::Item::Impl(item_impl) => collect_impl_reachability(item_impl, facts),
        syn::Item::Mod(module) if module.content.is_some() => {
            collect_attribute_uncertainties(&module.attrs, None, &mut facts.uncertainties);
            for item in &module.content.as_ref().expect("checked inline module").1 {
                collect_item_reachability(item, facts);
            }
        }
        _ => {
            let (owner, attrs) = item_owner_and_attrs(item);
            collect_owned_references(facts, owner, attrs, |collector| {
                collector.visit_item(item);
            });
        }
    }
}

fn collect_impl_reachability(item: &ItemImpl, facts: &mut RustReachabilityFacts) {
    let type_owner = impl_owner(item);
    let mut header = ReferenceCollector::new(type_owner.clone());
    header.visit_generics(&item.generics);
    header.visit_type(&item.self_ty);
    if let Some((_, trait_path, _)) = &item.trait_ {
        header.visit_path(trait_path);
    }
    merge_reference_facts(facts, type_owner.clone(), header);
    collect_attribute_uncertainties(&item.attrs, type_owner.clone(), &mut facts.uncertainties);

    for impl_item in &item.items {
        let (owner, attributes): (Option<String>, &[Attribute]) = match impl_item {
            syn::ImplItem::Const(item) => (
                qualified_member_owner(type_owner.as_deref(), &item.ident.to_string()),
                item.attrs.as_slice(),
            ),
            syn::ImplItem::Fn(item) => (
                qualified_member_owner(type_owner.as_deref(), &item.sig.ident.to_string()),
                item.attrs.as_slice(),
            ),
            syn::ImplItem::Type(item) => (
                qualified_member_owner(type_owner.as_deref(), &item.ident.to_string()),
                item.attrs.as_slice(),
            ),
            syn::ImplItem::Macro(item) => (type_owner.clone(), item.attrs.as_slice()),
            syn::ImplItem::Verbatim(_) => (type_owner.clone(), &[]),
            _ => (type_owner.clone(), &[]),
        };
        collect_owned_references(facts, owner, attributes, |collector| {
            collector.visit_impl_item(impl_item);
        });
    }
}

fn qualified_member_owner(type_owner: Option<&str>, member: &str) -> Option<String> {
    Some(type_owner.map_or_else(|| member.to_string(), |owner| format!("{owner}.{member}")))
}

fn collect_owned_references(
    facts: &mut RustReachabilityFacts,
    owner: Option<String>,
    attributes: &[Attribute],
    visit: impl FnOnce(&mut ReferenceCollector),
) {
    let mut collector = ReferenceCollector::new(owner.clone());
    visit(&mut collector);
    merge_reference_facts(facts, owner.clone(), collector);
    collect_attribute_uncertainties(attributes, owner, &mut facts.uncertainties);
}

fn merge_reference_facts(
    facts: &mut RustReachabilityFacts,
    owner: Option<String>,
    collector: ReferenceCollector,
) {
    if let Some(owner) = owner {
        facts
            .symbol_paths
            .entry(owner.clone())
            .or_default()
            .extend(collector.paths);
        facts
            .symbol_method_calls
            .entry(owner)
            .or_default()
            .extend(collector.method_calls);
    } else {
        facts.top_level_paths.extend(collector.paths);
        facts.top_level_method_calls.extend(collector.method_calls);
    }
    facts.embedded_sources.extend(collector.embedded_sources);
    facts.uncertainties.extend(collector.uncertainties);
}

#[derive(Default)]
struct TestSymbolVisitor {
    symbols: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for TestSymbolVisitor {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        if item.attrs.iter().any(is_test_attribute) {
            self.symbols.insert(item.sig.ident.to_string());
        }
        syn::visit::visit_item_fn(self, item);
    }
}

fn is_test_attribute(attribute: &Attribute) -> bool {
    let path = attribute.path();
    path.is_ident("test")
        || path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "test")
        || ["rstest", "test_case", "test_matrix"]
            .iter()
            .any(|name| path.is_ident(name))
}

fn item_owner_and_attrs(item: &syn::Item) -> (Option<String>, &[Attribute]) {
    match item {
        syn::Item::Const(item) => (Some(item.ident.to_string()), &item.attrs),
        syn::Item::Enum(item) => (Some(item.ident.to_string()), &item.attrs),
        syn::Item::Fn(item) => (Some(item.sig.ident.to_string()), &item.attrs),
        syn::Item::Impl(item) => (impl_owner(item), &item.attrs),
        syn::Item::Static(item) => (Some(item.ident.to_string()), &item.attrs),
        syn::Item::Struct(item) => (Some(item.ident.to_string()), &item.attrs),
        syn::Item::Trait(item) => (Some(item.ident.to_string()), &item.attrs),
        syn::Item::Type(item) => (Some(item.ident.to_string()), &item.attrs),
        syn::Item::Union(item) => (Some(item.ident.to_string()), &item.attrs),
        syn::Item::ExternCrate(item) => (None, &item.attrs),
        syn::Item::ForeignMod(item) => (None, &item.attrs),
        syn::Item::Macro(item) => (None, &item.attrs),
        syn::Item::Mod(item) => (None, &item.attrs),
        syn::Item::TraitAlias(item) => (Some(item.ident.to_string()), &item.attrs),
        syn::Item::Use(item) => (None, &item.attrs),
        syn::Item::Verbatim(_) => (None, &[]),
        _ => (None, &[]),
    }
}

fn impl_owner(item: &ItemImpl) -> Option<String> {
    match &*item.self_ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

struct ReferenceCollector {
    owner: Option<String>,
    paths: BTreeSet<Vec<String>>,
    method_calls: BTreeSet<String>,
    embedded_sources: Vec<RustEmbeddedSource>,
    uncertainties: Vec<RustUncertainty>,
}

impl ReferenceCollector {
    fn new(owner: Option<String>) -> Self {
        Self {
            owner,
            paths: BTreeSet::new(),
            method_calls: BTreeSet::new(),
            embedded_sources: Vec::new(),
            uncertainties: Vec::new(),
        }
    }
}

impl<'ast> Visit<'ast> for ReferenceCollector {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.method_calls.insert(call.method.to_string());
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        if attribute.path().is_ident("serde") {
            let _ = attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("default") && meta.input.peek(syn::Token![=]) {
                    let literal: syn::LitStr = meta.value()?.parse()?;
                    let path = literal
                        .value()
                        .split("::")
                        .filter(|segment| !segment.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>();
                    if !path.is_empty() {
                        self.paths.insert(path);
                    }
                } else if meta.input.peek(syn::Token![=]) {
                    let _: syn::Expr = meta.value()?.parse()?;
                }
                Ok(())
            });
        }
        syn::visit::visit_attribute(self, attribute);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        if !segments.is_empty() {
            self.paths.insert(segments);
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        let name = item
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        let expressions =
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated
                .parse2(item.tokens.clone());
        if let Ok(expressions) = expressions {
            if let Some(index) = format_string_expression_index(&name) {
                if let Some(syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(literal),
                    ..
                })) = expressions.iter().nth(index)
                {
                    self.paths.extend(
                        format_capture_identifiers(&literal.value())
                            .into_iter()
                            .map(|identifier| vec![identifier]),
                    );
                }
            }
            for expression in &expressions {
                self.visit_expr(expression);
            }
        }
        if is_tauri_macro(item, "generate_handler") {
            let parser = syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated;
            match parser.parse2(item.tokens.clone()) {
                Ok(paths) => self.paths.extend(paths.into_iter().map(|path| {
                    path.segments
                        .into_iter()
                        .map(|segment| segment.ident.to_string())
                        .collect()
                })),
                Err(_) => self.uncertainties.push(RustUncertainty {
                    owner: self.owner.clone(),
                    kind: RustUncertaintyKind::MacroExpansion,
                    expression: item.to_token_stream().to_string(),
                    span: span(item.span()),
                }),
            }
        } else if matches!(name.as_str(), "include_str" | "include_bytes") {
            match syn::parse2::<syn::LitStr>(item.tokens.clone()) {
                Ok(path) => self.embedded_sources.push(RustEmbeddedSource {
                    owner: self.owner.clone(),
                    path: path.value(),
                    span: span(item.span()),
                }),
                Err(_) => self.uncertainties.push(RustUncertainty {
                    owner: self.owner.clone(),
                    kind: RustUncertaintyKind::MacroExpansion,
                    expression: item.to_token_stream().to_string(),
                    span: span(item.span()),
                }),
            }
        } else if is_tauri_macro(item, "generate_context") {
            // The macro embeds Tauri configuration but does not register Rust callables.
        } else if !known_macro(&name) {
            self.uncertainties.push(RustUncertainty {
                owner: self.owner.clone(),
                kind: RustUncertaintyKind::MacroExpansion,
                expression: item.path.to_token_stream().to_string(),
                span: span(item.span()),
            });
        }
        syn::visit::visit_macro(self, item);
    }
}

fn format_string_expression_index(macro_name: &str) -> Option<usize> {
    match macro_name {
        "anyhow" | "bail" | "eprint" | "eprintln" | "format" | "format_args" | "panic"
        | "print" | "println" | "todo" | "unreachable" => Some(0),
        "assert" | "ensure" | "write" | "writeln" => Some(1),
        "assert_eq" | "assert_ne" => Some(2),
        _ => None,
    }
}

pub(super) fn format_capture_identifiers(format_string: &str) -> BTreeSet<String> {
    let bytes = format_string.as_bytes();
    let mut identifiers = BTreeSet::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'{' {
            cursor += 1;
            continue;
        }
        if bytes.get(cursor + 1) == Some(&b'{') {
            cursor += 2;
            continue;
        }
        let start = cursor + 1;
        let Some(relative_end) = bytes[start..].iter().position(|byte| *byte == b'}') else {
            break;
        };
        let end = start + relative_end;
        collect_format_field_identifiers(&format_string[start..end], &mut identifiers);
        cursor = end + 1;
    }
    identifiers
}

fn collect_format_field_identifiers(field: &str, identifiers: &mut BTreeSet<String>) {
    let argument_end = field.find([':', '!']).unwrap_or(field.len());
    insert_rust_identifier(field[..argument_end].trim(), identifiers);

    let Some((_, specification)) = field.split_once(':') else {
        return;
    };
    for (dollar, _) in specification.match_indices('$') {
        let prefix = &specification[..dollar];
        let start = prefix
            .char_indices()
            .rev()
            .find_map(|(index, character)| {
                (!character.is_alphanumeric() && character != '_' && character != '#')
                    .then_some(index + character.len_utf8())
            })
            .unwrap_or(0);
        insert_rust_identifier(&prefix[start..], identifiers);
    }
}

fn insert_rust_identifier(candidate: &str, identifiers: &mut BTreeSet<String>) {
    if let Ok(identifier) = syn::parse_str::<syn::Ident>(candidate) {
        identifiers.insert(identifier.to_string());
    }
}

fn is_tauri_macro(item: &syn::Macro, name: &str) -> bool {
    item.path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .eq(["tauri", name].into_iter().map(str::to_string))
}

fn collect_attribute_uncertainties(
    attributes: &[Attribute],
    owner: Option<String>,
    uncertainties: &mut Vec<RustUncertainty>,
) {
    for attribute in attributes {
        let name = attribute
            .path()
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        if name == "cfg" || name == "cfg_attr" {
            uncertainties.push(RustUncertainty {
                owner: owner.clone(),
                kind: RustUncertaintyKind::ConditionalCompilation,
                expression: attribute.meta.to_token_stream().to_string(),
                span: span(attribute.span()),
            });
        } else if !known_attribute(&name, attribute) {
            uncertainties.push(RustUncertainty {
                owner: owner.clone(),
                kind: RustUncertaintyKind::MacroExpansion,
                expression: attribute.meta.to_token_stream().to_string(),
                span: span(attribute.span()),
            });
        }
    }
}

fn known_macro(name: &str) -> bool {
    matches!(
        name,
        "assert"
            | "assert_eq"
            | "assert_ne"
            | "anyhow"
            | "bail"
            | "cfg"
            | "column"
            | "concat"
            | "dbg"
            | "eprint"
            | "eprintln"
            | "env"
            | "ensure"
            | "file"
            | "format"
            | "format_args"
            | "include_bytes"
            | "include_str"
            | "json"
            | "line"
            | "matches"
            | "module_path"
            | "option_env"
            | "panic"
            | "print"
            | "println"
            | "stringify"
            | "todo"
            | "Token"
            | "unreachable"
            | "vec"
            | "write"
            | "writeln"
    )
}

fn known_attribute(name: &str, attribute: &Attribute) -> bool {
    if matches!(
        name,
        "arg"
            | "command"
            | "error"
            | "from"
            | "group"
            | "serde"
            | "source"
            | "transparent"
            | "value"
    ) || is_tauri_attribute(attribute, "command")
    {
        return true;
    }
    if matches!(
        name,
        "allow"
            | "cold"
            | "deny"
            | "deprecated"
            | "doc"
            | "ignore"
            | "inline"
            | "must_use"
            | "non_exhaustive"
            | "path"
            | "repr"
            | "should_panic"
            | "test"
            | "warn"
    ) {
        return true;
    }
    if name != "derive" {
        return false;
    }
    let mut known = true;
    if attribute
        .parse_nested_meta(|meta| {
            let derive = meta
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            known &= matches!(
                derive.as_str(),
                "Clone"
                    | "Copy"
                    | "Debug"
                    | "Default"
                    | "Deserialize"
                    | "Eq"
                    | "Error"
                    | "Hash"
                    | "Ord"
                    | "PartialEq"
                    | "PartialOrd"
                    | "Serialize"
                    | "Args"
                    | "Parser"
                    | "Subcommand"
                    | "ValueEnum"
            );
            Ok(())
        })
        .is_err()
    {
        return false;
    }
    known
}

fn is_tauri_attribute(attribute: &Attribute, name: &str) -> bool {
    attribute
        .path()
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .eq(["tauri", name].into_iter().map(str::to_string))
}
