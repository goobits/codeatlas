use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use std::path::Path;
use std::fs;
use syn::{visit::Visit, ItemFn, ItemStruct, ItemImpl, Visibility as SynVis, spanned::Spanned};

pub fn parse_file(file_path: &Path, root_dir: &Path) -> Result<Vec<Symbol>> {
    let source = fs::read_to_string(file_path)?;
    let syntax = syn::parse_file(&source)?;

    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let mut visitor = SymbolVisitor {
        symbols: Vec::new(),
        relative_path,
        struct_indices: std::collections::HashMap::new(),
    };

    visitor.visit_file(&syntax);

    Ok(visitor.symbols)
}

struct SymbolVisitor {
    symbols: Vec<Symbol>,
    relative_path: String,
    // Map struct name to index in symbols vector
    struct_indices: std::collections::HashMap<String, usize>,
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
            children: vec![],
        }
    }
}

fn map_vis(v: &SynVis) -> Visibility {
    match v {
        SynVis::Public(_) => Visibility::Public,
        SynVis::Restricted(_) => Visibility::Internal, // pub(crate) etc
        SynVis::Inherited => Visibility::Internal, // private to module
    }
}

fn kind_to_str(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class => "struct",
        SymbolKind::Function => "fn",
        SymbolKind::Method => "fn",
        SymbolKind::Struct => "struct",
        _ => "sym",
    }
}

impl<'ast> Visit<'ast> for SymbolVisitor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let name = node.sig.ident.to_string();
        let vis = map_vis(&node.vis);
        let sig = format!("fn {}(...)", name);
        
        self.symbols.push(self.create_symbol(name, SymbolKind::Function, vis, node.sig.ident.span(), sig));
    }

    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        let name = node.ident.to_string();
        let vis = map_vis(&node.vis);
        let sig = format!("struct {}", name);
        
        let idx = self.symbols.len();
        self.symbols.push(self.create_symbol(name.clone(), SymbolKind::Struct, vis, node.ident.span(), sig));
        self.struct_indices.insert(name, idx);
    }
    
    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
         // Try to find the type name
         if let Some((_, _path, _)) = &node.trait_ {
             // Trait impl - skip for now or treat as standalone
         } else if let syn::Type::Path(type_path) = &*node.self_ty {
             // Inherent impl
             let type_name = type_path.path.segments.last().map(|s| s.ident.to_string()).unwrap_or("?".to_string());
             
             // Check if we have seen this struct
             let parent_idx = self.struct_indices.get(&type_name).copied();
             
             for item in &node.items {
                 if let syn::ImplItem::Fn(method) = item {
                     let m_name = method.sig.ident.to_string();
                     let m_vis = map_vis(&method.vis);
                     let m_sig = format!("fn {}(...)", m_name);
                     
                     let sym = self.create_symbol(m_name, SymbolKind::Method, m_vis, method.sig.ident.span(), m_sig);
                     
                     if let Some(idx) = parent_idx {
                         self.symbols[idx].children.push(sym);
                     } else {
                         // Orphan method (impl block before struct or struct in other file)
                         // Treat as top-level function but with class-like name prefix for context
                         let mut orphan = sym;
                         orphan.name = format!("{}.{}", type_name, orphan.name);
                         self.symbols.push(orphan);
                     }
                 }
             }
         }
    }
}
