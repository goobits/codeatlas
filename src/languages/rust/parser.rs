use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use quote::ToTokens;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use syn::{
    spanned::Spanned, visit::Visit, Attribute, ItemConst, ItemEnum, ItemFn, ItemImpl, ItemStruct,
    ItemTrait, ItemType, Visibility as SynVis,
};

#[derive(Debug, Clone)]
pub(crate) struct UseExport {
    pub module_path: Vec<String>,
    pub name: String,
    pub alias: String,
    pub is_glob: bool,
    pub visibility: RustVisibility,
}

#[derive(Debug, Clone)]
pub(crate) struct ModuleDeclaration {
    pub name: String,
    pub path_override: Option<String>,
    pub inline: bool,
    pub span: Span,
    pub visibility: RustVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RustVisibility {
    Public,
    Restricted(Vec<String>),
    Private,
}

impl RustVisibility {
    pub(crate) fn is_public(&self) -> bool {
        matches!(self, Self::Public)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RustReachabilityFacts {
    pub top_level_paths: BTreeSet<Vec<String>>,
    pub symbol_paths: BTreeMap<String, BTreeSet<Vec<String>>>,
    pub embedded_sources: Vec<RustEmbeddedSource>,
    pub uncertainties: Vec<RustUncertainty>,
    pub test_symbols: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RustEmbeddedSource {
    pub owner: Option<String>,
    pub path: String,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RustUncertaintyKind {
    ConditionalCompilation,
    MacroExpansion,
}

#[derive(Debug, Clone)]
pub(crate) struct RustUncertainty {
    pub owner: Option<String>,
    pub kind: RustUncertaintyKind,
    pub expression: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub(crate) struct RustModuleInfo {
    pub symbols: Vec<Symbol>,
    pub symbol_visibilities: BTreeMap<String, Vec<RustVisibility>>,
    pub uses: Vec<UseExport>,
    pub modules: Vec<ModuleDeclaration>,
    pub reachability: RustReachabilityFacts,
}

pub(crate) fn parse_file(file_path: &Path, root_dir: &Path, source: &str) -> Result<Vec<Symbol>> {
    Ok(parse_module_info(file_path, root_dir, source)?.symbols)
}

pub(crate) fn parse_module_info(
    file_path: &Path,
    root_dir: &Path,
    source: &str,
) -> Result<RustModuleInfo> {
    let syntax = syn::parse_file(source)?;

    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let mut visitor = SymbolVisitor {
        symbols: Vec::new(),
        relative_path,
        type_indices: std::collections::HashMap::new(),
        pending_methods: std::collections::HashMap::new(),
    };

    visitor.visit_file(&syntax);
    visitor.attach_pending_methods();

    let mut symbol_visibilities = BTreeMap::<String, Vec<RustVisibility>>::new();
    let mut uses = Vec::new();
    let mut modules = Vec::new();
    for item in &syntax.items {
        if let Some((name, visibility)) = item_symbol_visibility(item) {
            symbol_visibilities
                .entry(name)
                .or_default()
                .push(visibility);
        }
        if let syn::Item::Mod(item_mod) = item {
            modules.push(ModuleDeclaration {
                name: item_mod.ident.to_string(),
                path_override: path_override(&item_mod.attrs),
                inline: item_mod.content.is_some(),
                span: span(item_mod.ident.span()),
                visibility: rust_visibility(&item_mod.vis),
            });
        }
        if let syn::Item::Use(item_use) = item {
            collect_uses(
                &item_use.tree,
                Vec::new(),
                &rust_visibility(&item_use.vis),
                &mut uses,
            );
        }
    }
    let reachability = collect_reachability(&syntax);

    Ok(RustModuleInfo {
        symbols: visitor.symbols,
        symbol_visibilities,
        uses,
        modules,
        reachability,
    })
}

fn item_symbol_visibility(item: &syn::Item) -> Option<(String, RustVisibility)> {
    match item {
        syn::Item::Const(item) => Some((item.ident.to_string(), rust_visibility(&item.vis))),
        syn::Item::Enum(item) => Some((item.ident.to_string(), rust_visibility(&item.vis))),
        syn::Item::Fn(item) => Some((item.sig.ident.to_string(), rust_visibility(&item.vis))),
        syn::Item::Static(item) => Some((item.ident.to_string(), rust_visibility(&item.vis))),
        syn::Item::Struct(item) => Some((item.ident.to_string(), rust_visibility(&item.vis))),
        syn::Item::Trait(item) => Some((item.ident.to_string(), rust_visibility(&item.vis))),
        syn::Item::TraitAlias(item) => Some((item.ident.to_string(), rust_visibility(&item.vis))),
        syn::Item::Type(item) => Some((item.ident.to_string(), rust_visibility(&item.vis))),
        syn::Item::Union(item) => Some((item.ident.to_string(), rust_visibility(&item.vis))),
        _ => None,
    }
}

fn rust_visibility(visibility: &SynVis) -> RustVisibility {
    match visibility {
        SynVis::Public(_) => RustVisibility::Public,
        SynVis::Restricted(restricted) => RustVisibility::Restricted(
            restricted
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect(),
        ),
        SynVis::Inherited => RustVisibility::Private,
    }
}

fn collect_reachability(file: &syn::File) -> RustReachabilityFacts {
    let mut facts = RustReachabilityFacts::default();
    for item in &file.items {
        let (owner, attrs) = item_owner_and_attrs(item);
        if let Some(owner) = owner {
            let owner_for_attributes = owner.clone();
            let mut collector = ReferenceCollector::new(Some(owner.clone()));
            collector.visit_item(item);
            facts
                .symbol_paths
                .entry(owner)
                .or_default()
                .extend(collector.paths);
            facts.embedded_sources.extend(collector.embedded_sources);
            facts.uncertainties.extend(collector.uncertainties);
            collect_attribute_uncertainties(
                attrs,
                Some(owner_for_attributes),
                &mut facts.uncertainties,
            );
        } else {
            let mut collector = ReferenceCollector::new(None);
            collector.visit_item(item);
            facts.top_level_paths.extend(collector.paths);
            facts.embedded_sources.extend(collector.embedded_sources);
            facts.uncertainties.extend(collector.uncertainties);
            collect_attribute_uncertainties(attrs, None, &mut facts.uncertainties);
        }
    }
    let mut tests = TestSymbolVisitor::default();
    tests.visit_file(file);
    facts.test_symbols = tests.symbols;
    facts
}

#[derive(Default)]
struct TestSymbolVisitor {
    symbols: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for TestSymbolVisitor {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        if item
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("test"))
        {
            self.symbols.insert(item.sig.ident.to_string());
        }
        syn::visit::visit_item_fn(self, item);
    }
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
    embedded_sources: Vec<RustEmbeddedSource>,
    uncertainties: Vec<RustUncertainty>,
}

impl ReferenceCollector {
    fn new(owner: Option<String>) -> Self {
        Self {
            owner,
            paths: BTreeSet::new(),
            embedded_sources: Vec::new(),
            uncertainties: Vec::new(),
        }
    }
}

impl<'ast> Visit<'ast> for ReferenceCollector {
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
        if matches!(name.as_str(), "include_str" | "include_bytes") {
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
            | "cfg"
            | "column"
            | "concat"
            | "dbg"
            | "eprint"
            | "eprintln"
            | "env"
            | "file"
            | "format"
            | "format_args"
            | "include_bytes"
            | "include_str"
            | "line"
            | "matches"
            | "module_path"
            | "option_env"
            | "panic"
            | "print"
            | "println"
            | "stringify"
            | "todo"
            | "unreachable"
            | "vec"
            | "write"
            | "writeln"
    )
}

fn known_attribute(name: &str, attribute: &Attribute) -> bool {
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
                    | "Hash"
                    | "Ord"
                    | "PartialEq"
                    | "PartialOrd"
                    | "Serialize"
            );
            Ok(())
        })
        .is_err()
    {
        return false;
    }
    known
}

fn path_override(attributes: &[Attribute]) -> Option<String> {
    attributes.iter().find_map(|attribute| {
        if !attribute.path().is_ident("path") {
            return None;
        }
        let syn::Meta::NameValue(value) = &attribute.meta else {
            return None;
        };
        let syn::Expr::Lit(value) = &value.value else {
            return None;
        };
        let syn::Lit::Str(value) = &value.lit else {
            return None;
        };
        Some(value.value())
    })
}

fn span(value: proc_macro2::Span) -> Span {
    let start = value.start();
    let end = value.end();
    Span {
        start_line: start.line as u32,
        start_col: start.column as u32,
        end_line: end.line as u32,
        end_col: end.column as u32,
    }
}

struct SymbolVisitor {
    symbols: Vec<Symbol>,
    relative_path: String,
    // Map struct name to index in symbols vector
    type_indices: std::collections::HashMap<String, usize>,
    pending_methods: std::collections::HashMap<String, Vec<Symbol>>,
}

impl SymbolVisitor {
    fn create_symbol(
        &self,
        name: String,
        kind: SymbolKind,
        visibility: Visibility,
        span: proc_macro2::Span,
        signature: String,
    ) -> Symbol {
        let start = span.start();
        let end = span.end();

        let span_obj = Some(Span {
            start_line: start.line as u32,
            start_col: start.column as u32,
            end_line: end.line as u32,
            end_col: end.column as u32,
        });

        Symbol {
            id: format!("rs:{}:{}#{}", self.relative_path, kind_to_str(kind), name),
            name,
            kind,
            visibility,
            language: Language::Rust,
            file_path: self.relative_path.clone(),
            span: span_obj,
            signature,
            docs: None,
            export_paths: vec![],
            referenced: false,
            package: None,
            children: vec![],
        }
    }

    fn attach_pending_methods(&mut self) {
        for (type_name, methods) in self.pending_methods.drain() {
            if let Some(idx) = self.type_indices.get(&type_name).copied() {
                self.symbols[idx].children.extend(methods);
            } else {
                for mut method in methods {
                    method.name = format!("{}.{}", type_name, method.name);
                    self.symbols.push(method);
                }
            }
        }
    }

    fn push_pending(&mut self, type_name: String, method: Symbol, trait_name: Option<String>) {
        let mut method = method;
        self.qualify_child(&type_name, &mut method);
        let method = if let Some(trait_name) = trait_name {
            let mut updated = method;
            updated.signature = format!("{}::{}", trait_name, updated.signature);
            updated
        } else {
            method
        };

        self.pending_methods
            .entry(type_name)
            .or_default()
            .push(method);
    }

    fn qualify_child(&self, parent: &str, child: &mut Symbol) {
        child.id = format!(
            "rs:{}:{}#{}.{}",
            self.relative_path,
            kind_to_str(child.kind),
            parent,
            child.name
        );
    }
}

fn map_vis(v: &SynVis) -> Visibility {
    match v {
        SynVis::Public(_) => Visibility::Public,
        SynVis::Restricted(_) => Visibility::Internal, // pub(crate) etc
        SynVis::Inherited => Visibility::Internal,     // private to module
    }
}

fn kind_to_str(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class => "struct",
        SymbolKind::Function => "fn",
        SymbolKind::Method => "fn",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Const => "const",
        SymbolKind::TypeAlias => "type",
        _ => "sym",
    }
}

/// Extract a readable function signature from syn::Signature
fn format_fn_signature(sig: &syn::Signature) -> String {
    let name = sig.ident.to_string();

    // Extract parameters
    let params: Vec<String> = sig
        .inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Receiver(r) => {
                let mut s = String::new();
                if r.reference.is_some() {
                    s.push('&');
                    if r.mutability.is_some() {
                        s.push_str("mut ");
                    }
                }
                s.push_str("self");
                s
            }
            syn::FnArg::Typed(pat_type) => {
                let pat_name = match &*pat_type.pat {
                    syn::Pat::Ident(ident) => ident.ident.to_string(),
                    _ => "_".to_string(),
                };
                let type_str = format_type(&pat_type.ty);
                format!("{}: {}", pat_name, type_str)
            }
        })
        .collect();

    // Extract return type
    let ret = match &sig.output {
        syn::ReturnType::Default => String::new(),
        syn::ReturnType::Type(_, ty) => format!(" -> {}", format_type(ty)),
    };

    format!("fn {}({}){}", name, params.join(", "), ret)
}

/// Format a type to a readable string (simplified)
fn format_type(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(type_path) => {
            let segments: Vec<String> = type_path
                .path
                .segments
                .iter()
                .map(|seg| {
                    let name = seg.ident.to_string();
                    match &seg.arguments {
                        syn::PathArguments::None => name,
                        syn::PathArguments::AngleBracketed(args) => {
                            let inner: Vec<String> = args
                                .args
                                .iter()
                                .filter_map(|arg| match arg {
                                    syn::GenericArgument::Type(t) => Some(format_type(t)),
                                    syn::GenericArgument::Lifetime(lt) => {
                                        Some(format!("'{}", lt.ident))
                                    }
                                    _ => None,
                                })
                                .collect();
                            if inner.is_empty() {
                                name
                            } else {
                                format!("{}<{}>", name, inner.join(", "))
                            }
                        }
                        syn::PathArguments::Parenthesized(args) => {
                            let inputs: Vec<String> = args.inputs.iter().map(format_type).collect();
                            let ret = match &args.output {
                                syn::ReturnType::Default => String::new(),
                                syn::ReturnType::Type(_, t) => format!(" -> {}", format_type(t)),
                            };
                            format!("{}({}){}", name, inputs.join(", "), ret)
                        }
                    }
                })
                .collect();
            segments.join("::")
        }
        syn::Type::Reference(r) => {
            let mut s = String::from("&");
            if let Some(lt) = &r.lifetime {
                s.push_str(&format!("'{} ", lt.ident));
            }
            if r.mutability.is_some() {
                s.push_str("mut ");
            }
            s.push_str(&format_type(&r.elem));
            s
        }
        syn::Type::Slice(s) => format!("[{}]", format_type(&s.elem)),
        syn::Type::Array(a) => {
            let len = match &a.len {
                syn::Expr::Lit(lit) => match &lit.lit {
                    syn::Lit::Int(i) => i.base10_digits().to_string(),
                    _ => "N".to_string(),
                },
                _ => "N".to_string(),
            };
            format!("[{}; {}]", format_type(&a.elem), len)
        }
        syn::Type::Tuple(t) => {
            let elems: Vec<String> = t.elems.iter().map(format_type).collect();
            format!("({})", elems.join(", "))
        }
        syn::Type::Ptr(p) => {
            let mut s = String::from("*");
            if p.mutability.is_some() {
                s.push_str("mut ");
            } else {
                s.push_str("const ");
            }
            s.push_str(&format_type(&p.elem));
            s
        }
        syn::Type::ImplTrait(it) => {
            let bounds: Vec<String> = it
                .bounds
                .iter()
                .filter_map(|b| match b {
                    syn::TypeParamBound::Trait(t) => Some(
                        t.path
                            .segments
                            .iter()
                            .map(|s| s.ident.to_string())
                            .collect::<Vec<_>>()
                            .join("::"),
                    ),
                    _ => None,
                })
                .collect();
            format!("impl {}", bounds.join(" + "))
        }
        syn::Type::TraitObject(to) => {
            let bounds: Vec<String> = to
                .bounds
                .iter()
                .filter_map(|b| match b {
                    syn::TypeParamBound::Trait(t) => Some(
                        t.path
                            .segments
                            .iter()
                            .map(|s| s.ident.to_string())
                            .collect::<Vec<_>>()
                            .join("::"),
                    ),
                    _ => None,
                })
                .collect();
            format!("dyn {}", bounds.join(" + "))
        }
        _ => "...".to_string(),
    }
}

impl<'ast> Visit<'ast> for SymbolVisitor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let name = node.sig.ident.to_string();
        let vis = map_vis(&node.vis);
        let sig = format_fn_signature(&node.sig);

        self.symbols.push(self.create_symbol(
            name,
            SymbolKind::Function,
            vis,
            node.sig.ident.span(),
            sig,
        ));
    }

    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        let name = node.ident.to_string();
        let vis = map_vis(&node.vis);

        // Build signature with field names
        let sig = match &node.fields {
            syn::Fields::Named(fields) => {
                let field_strs: Vec<String> = fields
                    .named
                    .iter()
                    .map(|f| {
                        let fname = f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
                        let ftype = format_type(&f.ty);
                        format!("{}: {}", fname, ftype)
                    })
                    .collect();
                if field_strs.len() <= 3 {
                    format!("struct {} {{ {} }}", name, field_strs.join(", "))
                } else {
                    format!("struct {} {{ {}, ... }}", name, field_strs[..3].join(", "))
                }
            }
            syn::Fields::Unnamed(fields) => {
                let field_strs: Vec<String> =
                    fields.unnamed.iter().map(|f| format_type(&f.ty)).collect();
                format!("struct {}({})", name, field_strs.join(", "))
            }
            syn::Fields::Unit => format!("struct {}", name),
        };

        let idx = self.symbols.len();
        self.symbols.push(self.create_symbol(
            name.clone(),
            SymbolKind::Struct,
            vis,
            node.ident.span(),
            sig,
        ));
        self.type_indices.insert(name.clone(), idx);

        if let Some(methods) = self.pending_methods.remove(&name) {
            self.symbols[idx].children.extend(methods);
        }
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        // Try to find the type name
        if let Some((_, trait_path, _)) = &node.trait_ {
            let trait_name = trait_path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or("?".to_string());
            let type_name = match &*node.self_ty {
                syn::Type::Path(type_path) => type_path
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or("?".to_string()),
                _ => "?".to_string(),
            };
            let parent_idx = self.type_indices.get(&type_name).copied();

            for item in &node.items {
                if let syn::ImplItem::Fn(method) = item {
                    let m_name = method.sig.ident.to_string();
                    let m_vis = map_vis(&method.vis);
                    let m_sig = format_fn_signature(&method.sig);

                    let sym = self.create_symbol(
                        m_name.clone(),
                        SymbolKind::Method,
                        m_vis,
                        method.sig.ident.span(),
                        m_sig,
                    );

                    if let Some(idx) = parent_idx {
                        let mut sym = sym;
                        self.qualify_child(&type_name, &mut sym);
                        sym.signature = format!("{}::{}", trait_name, sym.signature);
                        self.symbols[idx].children.push(sym);
                    } else if type_name != "?" {
                        self.push_pending(type_name.clone(), sym, Some(trait_name.clone()));
                    } else {
                        let mut orphan = sym;
                        orphan.name = format!("{}::{}", trait_name, orphan.name);
                        self.qualify_child(&trait_name, &mut orphan);
                        self.symbols.push(orphan);
                    }
                }
            }
        } else if let syn::Type::Path(type_path) = &*node.self_ty {
            // Inherent impl
            let type_name = type_path
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or("?".to_string());

            // Check if we have seen this struct
            let parent_idx = self.type_indices.get(&type_name).copied();

            for item in &node.items {
                if let syn::ImplItem::Fn(method) = item {
                    let m_name = method.sig.ident.to_string();
                    let m_vis = map_vis(&method.vis);
                    let m_sig = format_fn_signature(&method.sig);

                    let mut sym = self.create_symbol(
                        m_name,
                        SymbolKind::Method,
                        m_vis,
                        method.sig.ident.span(),
                        m_sig,
                    );

                    if let Some(idx) = parent_idx {
                        self.qualify_child(&type_name, &mut sym);
                        self.symbols[idx].children.push(sym);
                    } else {
                        self.push_pending(type_name.clone(), sym, None);
                    }
                }
            }
        }
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        let name = node.ident.to_string();
        let vis = map_vis(&node.vis);

        // Build signature with variants
        let variants: Vec<String> = node.variants.iter().map(|v| v.ident.to_string()).collect();
        let sig = if variants.len() <= 4 {
            format!("enum {} {{ {} }}", name, variants.join(", "))
        } else {
            format!("enum {} {{ {}, ... }}", name, variants[..4].join(", "))
        };

        let idx = self.symbols.len();
        self.symbols.push(self.create_symbol(
            name.clone(),
            SymbolKind::Enum,
            vis,
            node.ident.span(),
            sig,
        ));
        self.type_indices.insert(name.clone(), idx);
        if let Some(methods) = self.pending_methods.remove(&name) {
            self.symbols[idx].children.extend(methods);
        }
    }

    fn visit_item_trait(&mut self, node: &'ast ItemTrait) {
        let name = node.ident.to_string();
        let vis = map_vis(&node.vis);
        let sig = format!("trait {}", name);

        let idx = self.symbols.len();
        self.symbols.push(self.create_symbol(
            name.clone(),
            SymbolKind::Trait,
            vis,
            node.ident.span(),
            sig,
        ));
        self.type_indices.insert(name.clone(), idx);
        if let Some(methods) = self.pending_methods.remove(&name) {
            self.symbols[idx].children.extend(methods);
        }

        // Collect trait methods as children
        for item in &node.items {
            if let syn::TraitItem::Fn(method) = item {
                let m_name = method.sig.ident.to_string();
                let m_sig = format_fn_signature(&method.sig);
                // Trait methods are public by default (part of the trait's contract)
                let m_vis = Visibility::Public;

                let mut sym = self.create_symbol(
                    m_name,
                    SymbolKind::Method,
                    m_vis,
                    method.sig.ident.span(),
                    m_sig,
                );
                self.qualify_child(&name, &mut sym);
                self.symbols[idx].children.push(sym);
            }
        }
    }

    fn visit_item_const(&mut self, node: &'ast ItemConst) {
        let name = node.ident.to_string();
        let vis = map_vis(&node.vis);
        let type_str = format_type(&node.ty);
        let sig = format!("const {}: {}", name, type_str);

        self.symbols
            .push(self.create_symbol(name, SymbolKind::Const, vis, node.ident.span(), sig));
    }

    fn visit_item_type(&mut self, node: &'ast ItemType) {
        let name = node.ident.to_string();
        let vis = map_vis(&node.vis);
        let type_str = format_type(&node.ty);
        let sig = format!("type {} = {}", name, type_str);

        self.symbols.push(self.create_symbol(
            name,
            SymbolKind::TypeAlias,
            vis,
            node.ident.span(),
            sig,
        ));
    }
}

fn collect_uses(
    tree: &syn::UseTree,
    prefix: Vec<String>,
    visibility: &RustVisibility,
    uses: &mut Vec<UseExport>,
) {
    match tree {
        syn::UseTree::Name(name) => {
            let name = name.ident.to_string();
            uses.push(UseExport {
                module_path: prefix,
                name: name.clone(),
                alias: name,
                is_glob: false,
                visibility: visibility.clone(),
            });
        }
        syn::UseTree::Rename(rename) => {
            uses.push(UseExport {
                module_path: prefix,
                name: rename.ident.to_string(),
                alias: rename.rename.to_string(),
                is_glob: false,
                visibility: visibility.clone(),
            });
        }
        syn::UseTree::Glob(_) => {
            uses.push(UseExport {
                module_path: prefix,
                name: "*".to_string(),
                alias: "*".to_string(),
                is_glob: true,
                visibility: visibility.clone(),
            });
        }
        syn::UseTree::Path(path) => {
            let mut next = prefix;
            next.push(path.ident.to_string());
            collect_uses(&path.tree, next, visibility, uses);
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                collect_uses(tree, prefix.clone(), visibility, uses);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_module_info;
    use std::path::Path;

    #[test]
    fn reachability_tracks_attributes_methods_and_embedded_sources() {
        let source = r#"
            struct Config {
                #[serde(default = "fallback")]
                value: u32,
            }

            fn fallback() -> u32 {
                1
            }

            enum Mode {
                One,
            }

            impl From<u32> for Mode {
                fn from(_: u32) -> Self {
                    Self::One
                }
            }

            const HOOK: &str = include_str!("hooks.py");
        "#;
        let info =
            parse_module_info(Path::new("src/lib.rs"), Path::new("."), source).expect("Rust facts");
        assert!(info
            .reachability
            .symbol_paths
            .get("Config")
            .is_some_and(|paths| paths.contains(&vec!["fallback".to_string()])));
        let mode = info
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Mode")
            .expect("enum symbol");
        assert!(mode.children.iter().any(|symbol| symbol.name == "from"));
        assert!(!info.symbols.iter().any(|symbol| symbol.name == "Mode.from"));
        assert!(info.reachability.embedded_sources.iter().any(|source| {
            source.owner.as_deref() == Some("HOOK") && source.path == "hooks.py"
        }));
    }
}
