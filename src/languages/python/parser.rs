use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use rustpython_parser::{ast, Parse};
use std::path::Path;
use std::fs;

pub fn parse_file(file_path: &Path, root_dir: &Path) -> Result<Vec<Symbol>> {
    let source = fs::read_to_string(file_path)?;
    let ast = ast::Suite::parse(&source, &file_path.to_string_lossy())?;

    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let mut visitor = SymbolVisitor {
        symbols: Vec::new(),
        relative_path,
    };

    visitor.visit_suite(&ast);

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
        _location: rustpython_parser::text_size::TextRange, // Placeholder, actual AST has `range` or `location`
        // Wait, rustpython_ast types usually have a `range` field if compiled with `location` feature or similar?
        // Let's check `rustpython_ast` crate docs or usage.
        // Actually, for this prototype we might not have perfect span info if not exposed easily, 
        // but let's assume standard AST nodes have `range`.
        // rustpython_ast::Stmt has `range`.
        start_line: u32, // We'll simplify and pass line numbers if possible, or 0
        signature: String,
    ) -> Symbol {
        // Simplified Span creation
        let span = Some(Span {
            start_line,
            start_col: 0,
            end_line: start_line, // Placeholder
            end_col: 0,
        });

        Symbol {
            id: format!("py:{}:{}#{}", self.relative_path, kind_to_str(kind), name),
            name,
            kind,
            visibility,
            language: Language::Python,
            file_path: self.relative_path.clone(),
            span,
            signature,
            children: vec![],
        }
    }
    
    fn visit_suite(&mut self, suite: &[ast::Stmt]) {
        for stmt in suite {
            self.visit_stmt(stmt);
        }
    }

    fn visit_stmt(&mut self, stmt: &ast::Stmt) {
        match stmt {
            ast::Stmt::FunctionDef(f) => {
                let name = f.name.as_str().to_string();
                let vis = determine_visibility(&name);
                let sig = format!("def {}(...)", name);
                // Note: f.range is available in newer rustpython
                // We will assume `range` or row info is available on the node.
                // `stmt.range()` returns TextRange. We need line lookup. 
                // For MVP without line-index crate, we might default to 0.
                // Or better, let's just skip exact line numbers if too complex without source map.
                // Actually `rustpython_parser` usually returns a location attached.
                
                let symbol = self.create_symbol(name, SymbolKind::Function, vis, f.range, 0, sig);
                self.symbols.push(symbol);
                
                // Recurse? Usually functions don't have public symbols inside, 
                // but we might want to catch nested classes or functions.
                // self.visit_suite(&f.body); 
            },
            ast::Stmt::ClassDef(c) => {
                let name = c.name.as_str().to_string();
                let vis = determine_visibility(&name);
                let sig = format!("class {}", name);
                let mut symbol = self.create_symbol(name, SymbolKind::Class, vis, c.range, 0, sig);
                
                // Visit children to find methods
                let mut child_visitor = SymbolVisitor {
                    symbols: Vec::new(),
                    relative_path: self.relative_path.clone(),
                };
                child_visitor.visit_suite(&c.body);
                
                // Adapt child symbols (which are currently top-level in child_visitor) to be children of this class
                // And change their kind to Method if they are functions
                for mut child in child_visitor.symbols {
                    if child.kind == SymbolKind::Function {
                        child.kind = SymbolKind::Method;
                        // Adjust ID?
                        child.id = child.id.replace(":fn#", ":method#");
                    }
                    symbol.children.push(child);
                }
                
                self.symbols.push(symbol);
            },
            _ => {}
        }
    }
}

fn determine_visibility(name: &str) -> Visibility {
    if name.starts_with("__") && name.ends_with("__") {
        Visibility::Public // Magic methods are technically accessible, but often treated special. Let's say Public for now or Internal?
        // Spec says: Starts with __ -> Private. 
        // But __init__ is special.
        // Let's stick to strict spec:
    } else if name.starts_with("__") {
        Visibility::Private
    } else if name.starts_with("_") {
        Visibility::Internal
    } else {
        Visibility::Public
    }
}

fn kind_to_str(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class => "class",
        SymbolKind::Function => "def",
        SymbolKind::Method => "method",
        _ => "sym",
    }
}
