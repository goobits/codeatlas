use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use std::path::Path;
use swc_core::common::{
    errors::{ColorConfig, Handler},
    sync::Lrc,
    SourceMap,
};
use swc_core::ecma::parser::{lexer::Lexer, Parser, StringInput, Syntax, TsConfig};
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};

pub fn parse_file(file_path: &Path, root_dir: &Path) -> Result<Vec<Symbol>> {
    let cm: Lrc<SourceMap> = Default::default();
    let handler = Handler::with_tty_emitter(ColorConfig::Auto, true, false, Some(cm.clone()));

    let fm = cm.load_file(file_path)?;

    let lexer = Lexer::new(
        Syntax::Typescript(TsConfig {
            tsx: file_path.extension().map_or(false, |e| e == "tsx"),
            decorators: true,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );

    let mut parser = Parser::new_from(lexer);

    for e in parser.take_errors() {
        e.into_diagnostic(&handler).emit();
    }

    let module = parser
        .parse_module()
        .map_err(|e| {
            e.into_diagnostic(&handler).emit();
            anyhow::anyhow!("Parse failed")
        })?;

    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let mut visitor = SymbolVisitor {
        symbols: Vec::new(),
        relative_path,
        source_map: cm,
    };

    module.visit_with(&mut visitor);

    Ok(visitor.symbols)
}

struct SymbolVisitor {
    symbols: Vec<Symbol>,
    relative_path: String,
    source_map: Lrc<SourceMap>,
}

impl SymbolVisitor {
    fn create_symbol(
        &self,
        name: String,
        kind: SymbolKind,
        visibility: Visibility,
        span: swc_core::common::Span,
        signature: String,
    ) -> Symbol {
        let (start, end) = (span.lo, span.hi);
        let start_loc = self.source_map.lookup_char_pos(start);
        let end_loc = self.source_map.lookup_char_pos(end);

        Symbol {
            id: format!("ts:{}:{}#{}", self.relative_path, kind_to_str(kind), name),
            name,
            kind,
            visibility,
            language: Language::TypeScript,
            file_path: self.relative_path.clone(),
            span: Some(Span {
                start_line: start_loc.line as u32,
                start_col: start_loc.col.0 as u32,
                end_line: end_loc.line as u32,
                end_col: end_loc.col.0 as u32,
            }),
            signature,
            children: vec![],
        }
    }
}

fn kind_to_str(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class => "class",
        SymbolKind::Function => "fn",
        SymbolKind::Interface => "interface",
        SymbolKind::Method => "method",
        _ => "sym",
    }
}

impl Visit for SymbolVisitor {
    fn visit_export_decl(&mut self, n: &ExportDecl) {
        let start_len = self.symbols.len();
        n.decl.visit_with(self);
        for i in start_len..self.symbols.len() {
            self.symbols[i].visibility = Visibility::Public;
        }
    }

    fn visit_class_decl(&mut self, n: &ClassDecl) {
        // n.ident is mandatory in newer SWC
        let ident = &n.ident;
        let name = ident.sym.to_string();
        let sig = format!("class {}", name);
        let mut symbol = self.create_symbol(name, SymbolKind::Class, Visibility::Internal, n.class.span, sig);
        
        for member in &n.class.body {
            if let ClassMember::Method(m) = member {
                if let Some(key) = m.key.as_ident() {
                        let m_name = key.sym.to_string();
                        let m_sig = format!("method {}(...)", m_name);
                        let m_vis = if m.accessibility == Some(Accessibility::Private) || m_name.starts_with("#") {
                            Visibility::Private
                        } else {
                            Visibility::Public
                        };
                        
                        let m_sym = self.create_symbol(m_name, SymbolKind::Method, m_vis, m.span, m_sig);
                        symbol.children.push(m_sym);
                }
            }
        }
        
        self.symbols.push(symbol);
    }

    fn visit_fn_decl(&mut self, n: &FnDecl) {
        let name = n.ident.sym.to_string();
        let sig = format!("fn {}(...)", name);
        let symbol = self.create_symbol(name, SymbolKind::Function, Visibility::Internal, n.function.span, sig);
        self.symbols.push(symbol);
    }
    
    fn visit_call_expr(&mut self, n: &CallExpr) {
        if let Callee::Expr(expr) = &n.callee {
            if let Expr::Member(member) = &**expr {
                if let Some(prop) = member.prop.as_ident() {
                    let method_name = prop.sym.to_string();
                    const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch"];
                    if HTTP_METHODS.contains(&method_name.as_str()) {
                         let obj_name = if let Expr::Ident(id) = &*member.obj {
                             id.sym.to_string()
                         } else {
                             "unknown".to_string()
                         };
                         
                         let name = format!("{}.{}", obj_name, method_name);
                         
                         let mut sig_args = String::new();
                         if let Some(arg) = n.args.first() {
                             if let Expr::Lit(Lit::Str(s)) = &*arg.expr {
                                 sig_args = format!("'{}', ...", s.value);
                             }
                         }
                         
                         if !sig_args.is_empty() {
                             let sig = format!("{}({})", name, sig_args);
                             let symbol = self.create_symbol(name, SymbolKind::Function, Visibility::Internal, n.span, sig);
                             self.symbols.push(symbol);
                         }
                    }
                }
            }
        }
        
        n.callee.visit_with(self);
        for arg in &n.args {
            arg.visit_with(self);
        }
    }

    fn visit_ts_interface_decl(&mut self, n: &TsInterfaceDecl) {
        let name = n.id.sym.to_string();
        let sig = format!("interface {}", name);
        let symbol = self.create_symbol(name, SymbolKind::Interface, Visibility::Internal, n.span, sig);
        self.symbols.push(symbol);
    }
}
