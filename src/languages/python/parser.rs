use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use rustpython_parser::{ast, Parse};
use rustpython_parser::source_code::LineIndex;
use rustpython_parser::text_size::TextRange;
use std::path::Path;
use std::sync::Arc;

pub fn parse_file(file_path: &Path, root_dir: &Path, source: &str) -> Result<Vec<Symbol>> {
    let ast = ast::Suite::parse(source, &file_path.to_string_lossy())?;

    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let source = Arc::<str>::from(source);
    let line_index = LineIndex::from_source_text(&source);

    let mut visitor = SymbolVisitor {
        symbols: Vec::new(),
        relative_path,
        source,
        line_index,
        recurse_into_functions: true,
    };

    visitor.visit_suite(&ast);

    Ok(visitor.symbols)
}

struct SymbolVisitor {
    symbols: Vec<Symbol>,
    relative_path: String,
    source: Arc<str>,
    line_index: LineIndex,
    recurse_into_functions: bool,
}

impl SymbolVisitor {
    fn create_symbol(
        &self,
        name: String,
        kind: SymbolKind,
        visibility: Visibility,
        range: TextRange,
        signature: String,
    ) -> Symbol {
        let start_loc = self.line_index.source_location(range.start(), &self.source);
        let end_loc = self.line_index.source_location(range.end(), &self.source);
        let span = Some(Span {
            start_line: start_loc.row.get(),
            start_col: start_loc.column.get(),
            end_line: end_loc.row.get(),
            end_col: end_loc.column.get(),
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
                let symbol = self.create_symbol(name, SymbolKind::Function, vis, f.range, sig);
                self.symbols.push(symbol);
                
                if self.recurse_into_functions {
                    self.visit_suite(&f.body);
                }
            },
            ast::Stmt::ClassDef(c) => {
                let name = c.name.as_str().to_string();
                let vis = determine_visibility(&name);
                let sig = format!("class {}", name);
                let mut symbol = self.create_symbol(name, SymbolKind::Class, vis, c.range, sig);
                
                // Visit children to find methods
                let mut child_visitor = SymbolVisitor {
                    symbols: Vec::new(),
                    relative_path: self.relative_path.clone(),
                    source: self.source.clone(),
                    line_index: self.line_index.clone(),
                    recurse_into_functions: false,
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
