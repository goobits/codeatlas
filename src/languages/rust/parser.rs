use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use std::path::Path;
use std::fs;
use syn::{visit::Visit, ItemFn, ItemStruct, ItemImpl, Visibility as SynVis};

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
    };

    visitor.visit_file(&syntax);

    Ok(visitor.symbols)
}

struct SymbolVisitor {
    symbols: Vec<Symbol>,
    relative_path: String,
}

impl SymbolVisitor {
    fn create_symbol(
        &self,
        name: String,
        kind: SymbolKind,
        visibility: Visibility,
        _span: proc_macro2::Span, // Extracting line numbers from Span requires SourceMap/SpanUtils which syn doesn't expose easily directly without feature "extra-traits" and some work.
        signature: String,
    ) -> Symbol {
        
        // Simplified Span (default to 0 for MVP)
        let span = Some(Span {
            start_line: 0, 
            start_col: 0,
            end_line: 0,
            end_col: 0,
        });

        Symbol {
            id: format!("rs:{}:{}#{}", self.relative_path, kind_to_str(kind), name),
            name,
            kind,
            visibility,
            language: Language::Rust,
            file_path: self.relative_path.clone(),
            span,
            signature,
            children: vec![],
        }
    }
}

fn map_vis(v: &SynVis) -> Visibility {
    match v {
        SynVis::Public(_) => Visibility::Public,
        SynVis::Restricted(_) => Visibility::Internal, // pub(crate) etc
        SynVis::Inherited => Visibility::Internal, // private to module, but visible to children. Often treated as "Internal" in Rust.
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
        
        // Check if it's a test? skip?
        
        self.symbols.push(self.create_symbol(name, SymbolKind::Function, vis, node.sig.ident.span(), sig));
        
        // visit children?
    }

    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        let name = node.ident.to_string();
        let vis = map_vis(&node.vis);
        let sig = format!("struct {}", name);
        
        self.symbols.push(self.create_symbol(name, SymbolKind::Struct, vis, node.ident.span(), sig));
    }
    
    // Impl blocks are tricky because they don't have a name themselves, they attach to a Type.
    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
         // Try to find the type name
         if let Some((_, _path, _)) = &node.trait_ {
             // Implementation of a trait
             // self.visit_impl_items...
         } else if let syn::Type::Path(type_path) = &*node.self_ty {
             // Inherent impl
             let type_name = type_path.path.segments.last().map(|s| s.ident.to_string()).unwrap_or("?".to_string());
             
             // We want to attach these methods to the struct symbol if it exists?
             // OR just emit them as methods with IDs linking to the struct?
             // "rs:src/lib.rs:method#StructName.methodName"
             
             for item in &node.items {
                 if let syn::ImplItem::Fn(method) = item {
                     let m_name = method.sig.ident.to_string();
                     let m_vis = map_vis(&method.vis);
                     let m_sig = format!("fn {}(...)", m_name);
                     
                     // We create a standalone symbol for now, maybe with a special ID convention
                     let sym = self.create_symbol(format!("{}.{}", type_name, m_name), SymbolKind::Method, m_vis, method.sig.ident.span(), m_sig);
                     self.symbols.push(sym);
                 }
             }
         }
    }
}
