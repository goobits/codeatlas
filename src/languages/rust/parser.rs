use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use std::path::Path;
use syn::{
    visit::Visit, ItemConst, ItemEnum, ItemFn, ItemImpl, ItemStruct, ItemTrait, ItemType,
    Visibility as SynVis,
};

pub(crate) struct UseExport {
    pub module_path: Vec<String>,
    pub name: String,
    pub alias: String,
    pub is_glob: bool,
}

pub(crate) struct RustModuleInfo {
    pub symbols: Vec<Symbol>,
    pub public_mods: Vec<String>,
    pub public_uses: Vec<UseExport>,
    pub uses: Vec<UseExport>,
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
        struct_indices: std::collections::HashMap::new(),
        pending_methods: std::collections::HashMap::new(),
    };

    visitor.visit_file(&syntax);
    visitor.attach_pending_methods();

    let mut public_mods = Vec::new();
    let mut public_uses = Vec::new();
    let mut uses = Vec::new();
    for item in &syntax.items {
        if let syn::Item::Mod(item_mod) = item {
            if matches!(item_mod.vis, SynVis::Public(_)) {
                public_mods.push(item_mod.ident.to_string());
            }
        }
        if let syn::Item::Use(item_use) = item {
            collect_use_imports(&item_use.tree, Vec::new(), &mut uses);
            if matches!(item_use.vis, SynVis::Public(_)) {
                collect_use_exports(&item_use.tree, Vec::new(), &mut public_uses);
            }
        }
    }

    Ok(RustModuleInfo {
        symbols: visitor.symbols,
        public_mods,
        public_uses,
        uses,
    })
}

struct SymbolVisitor {
    symbols: Vec<Symbol>,
    relative_path: String,
    // Map struct name to index in symbols vector
    struct_indices: std::collections::HashMap<String, usize>,
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
            package: None,
            children: vec![],
        }
    }

    fn attach_pending_methods(&mut self) {
        for (type_name, methods) in self.pending_methods.drain() {
            if let Some(idx) = self.struct_indices.get(&type_name).copied() {
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
        self.struct_indices.insert(name.clone(), idx);

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
            let parent_idx = self.struct_indices.get(&type_name).copied();

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
            let parent_idx = self.struct_indices.get(&type_name).copied();

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

        self.symbols
            .push(self.create_symbol(name, SymbolKind::Enum, vis, node.ident.span(), sig));
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

fn collect_use_exports(tree: &syn::UseTree, prefix: Vec<String>, exports: &mut Vec<UseExport>) {
    match tree {
        syn::UseTree::Name(name) => {
            let name = name.ident.to_string();
            exports.push(UseExport {
                module_path: prefix,
                name: name.clone(),
                alias: name,
                is_glob: false,
            });
        }
        syn::UseTree::Rename(rename) => {
            exports.push(UseExport {
                module_path: prefix,
                name: rename.ident.to_string(),
                alias: rename.rename.to_string(),
                is_glob: false,
            });
        }
        syn::UseTree::Glob(_) => {
            exports.push(UseExport {
                module_path: prefix,
                name: "*".to_string(),
                alias: "*".to_string(),
                is_glob: true,
            });
        }
        syn::UseTree::Path(path) => {
            let mut next = prefix;
            next.push(path.ident.to_string());
            collect_use_exports(&path.tree, next, exports);
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_exports(tree, prefix.clone(), exports);
            }
        }
    }
}

fn collect_use_imports(tree: &syn::UseTree, prefix: Vec<String>, imports: &mut Vec<UseExport>) {
    match tree {
        syn::UseTree::Name(name) => {
            let name = name.ident.to_string();
            imports.push(UseExport {
                module_path: prefix,
                name,
                alias: String::new(),
                is_glob: false,
            });
        }
        syn::UseTree::Rename(rename) => {
            imports.push(UseExport {
                module_path: prefix,
                name: rename.ident.to_string(),
                alias: rename.rename.to_string(),
                is_glob: false,
            });
        }
        syn::UseTree::Glob(_) => {
            imports.push(UseExport {
                module_path: prefix,
                name: "*".to_string(),
                alias: "*".to_string(),
                is_glob: true,
            });
        }
        syn::UseTree::Path(path) => {
            let mut next = prefix;
            next.push(path.ident.to_string());
            collect_use_imports(&path.tree, next, imports);
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_imports(tree, prefix.clone(), imports);
            }
        }
    }
}
