use crate::domain::{FuzzPolicyEvidence, Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use syn::{
    spanned::Spanned, visit::Visit, Attribute, ItemConst, ItemEnum, ItemFn, ItemImpl, ItemStruct,
    ItemTrait, ItemType, Visibility as SynVis,
};

mod callable;
mod callable_effects;
mod reachability;
mod signatures;

use signatures::{format_fn_signature, format_type};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UseExport {
    pub module_path: Vec<String>,
    pub name: String,
    pub alias: String,
    pub is_glob: bool,
    pub visibility: RustVisibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ModuleDeclaration {
    pub name: String,
    pub path_override: Option<String>,
    pub inline: bool,
    pub test_only: bool,
    pub span: Span,
    pub visibility: RustVisibility,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct RustReachabilityFacts {
    pub top_level_paths: BTreeSet<Vec<String>>,
    pub symbol_paths: BTreeMap<String, BTreeSet<Vec<String>>>,
    pub top_level_method_calls: BTreeSet<String>,
    pub symbol_method_calls: BTreeMap<String, BTreeSet<String>>,
    pub embedded_sources: Vec<RustEmbeddedSource>,
    pub uncertainties: Vec<RustUncertainty>,
    pub test_symbols: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RustEmbeddedSource {
    pub owner: Option<String>,
    pub path: String,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum RustUncertaintyKind {
    ConditionalCompilation,
    MacroExpansion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RustUncertainty {
    pub owner: Option<String>,
    pub kind: RustUncertaintyKind,
    pub expression: String,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
                test_only: has_exact_cfg_test(&item_mod.attrs),
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
    let reachability = reachability::collect(&syntax);

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

fn has_exact_cfg_test(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        let syn::Meta::List(meta) = &attribute.meta else {
            return false;
        };
        attribute.path().is_ident("cfg")
            && syn::parse2::<syn::Path>(meta.tokens.clone()).is_ok_and(|path| path.is_ident("test"))
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
            callable: None,
            fuzz_policy: None,
            docs: None,
            export_paths: vec![],
            referenced: false,
            package: None,
            children: vec![],
        }
    }

    fn create_callable_symbol(
        &self,
        signature: &syn::Signature,
        kind: SymbolKind,
        visibility: Visibility,
        contract: crate::domain::CallableContract,
        attributes: &[Attribute],
    ) -> Symbol {
        let mut symbol = self.create_symbol(
            signature.ident.to_string(),
            kind,
            visibility,
            signature.ident.span(),
            format_fn_signature(signature),
        );
        symbol.callable = Some(contract);
        symbol.fuzz_policy = fuzz_policy(attributes);
        symbol
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

impl<'ast> Visit<'ast> for SymbolVisitor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let vis = map_vis(&node.vis);

        self.symbols.push(self.create_callable_symbol(
            &node.sig,
            SymbolKind::Function,
            vis,
            callable::contract(
                &node.sig,
                crate::domain::CallableKind::Function,
                crate::domain::CallableBody::Present,
                Some(&node.block),
            ),
            &node.attrs,
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
                    let m_vis = map_vis(&method.vis);

                    let sym = self.create_callable_symbol(
                        &method.sig,
                        SymbolKind::Method,
                        m_vis,
                        callable::contract(
                            &method.sig,
                            crate::domain::CallableKind::Method,
                            crate::domain::CallableBody::Present,
                            Some(&method.block),
                        ),
                        &method.attrs,
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
                    let m_vis = map_vis(&method.vis);

                    let mut sym = self.create_callable_symbol(
                        &method.sig,
                        SymbolKind::Method,
                        m_vis,
                        callable::contract(
                            &method.sig,
                            crate::domain::CallableKind::Method,
                            crate::domain::CallableBody::Present,
                            Some(&method.block),
                        ),
                        &method.attrs,
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
                // Trait methods are public by default (part of the trait's contract)
                let m_vis = Visibility::Public;

                let mut sym = self.create_callable_symbol(
                    &method.sig,
                    SymbolKind::Method,
                    m_vis,
                    callable::contract(
                        &method.sig,
                        crate::domain::CallableKind::Method,
                        if method.default.is_some() {
                            crate::domain::CallableBody::Present
                        } else {
                            crate::domain::CallableBody::DeclarationOnly
                        },
                        method.default.as_ref(),
                    ),
                    &method.attrs,
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

fn fuzz_policy(attributes: &[Attribute]) -> Option<FuzzPolicyEvidence> {
    crate::fuzz::directive::parse_directive_lines(attributes.iter().filter_map(|attribute| {
        if !attribute.path().is_ident("doc") {
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
        Some((attribute.span().start().line as u32, value.value()))
    }))
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
            let alias = if name == "self" {
                prefix.last().cloned().unwrap_or_else(|| name.clone())
            } else {
                name.clone()
            };
            uses.push(UseExport {
                module_path: prefix,
                name,
                alias,
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
mod tests;
