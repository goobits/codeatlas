use crate::domain::{FuzzPolicyEvidence, Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use rustpython_parser::source_code::LineIndex;
use rustpython_parser::text_size::TextRange;
use rustpython_parser::{ast, Parse};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

mod callable;
mod callable_effects;
mod reachability;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PythonImport {
    pub module: String,
    pub names: Vec<String>,
    pub is_star: bool,
    pub level: usize,
    pub aliases: Vec<Option<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct PythonReachabilityFacts {
    pub top_level_references: BTreeSet<String>,
    pub top_level_qualified_references: BTreeSet<String>,
    pub annotation_references: BTreeMap<String, BTreeSet<String>>,
    pub symbol_references: BTreeMap<String, BTreeSet<String>>,
    pub symbol_qualified_references: BTreeMap<String, BTreeSet<String>>,
    pub scoped_imports: Vec<PythonScopedImport>,
    pub dynamic_dependencies: Vec<PythonDynamicDependency>,
    pub dynamic_entrypoints: BTreeSet<String>,
    pub uncertainties: Vec<PythonUncertainty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PythonScopedImport {
    pub owner: String,
    pub import: PythonImport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PythonDynamicDependency {
    pub owner: Option<String>,
    pub module: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum PythonUncertaintyKind {
    DynamicImport,
    Reflection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PythonUncertainty {
    pub owner: Option<String>,
    pub kind: PythonUncertaintyKind,
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PythonModuleInfo {
    pub symbols: Vec<Symbol>,
    pub exports: Option<Vec<String>>,
    pub imports: Vec<PythonImport>,
    pub reachability: PythonReachabilityFacts,
}

pub(crate) fn parse_file(file_path: &Path, root_dir: &Path, source: &str) -> Result<Vec<Symbol>> {
    Ok(parse_module_info(file_path, root_dir, source)?.symbols)
}

pub(crate) fn parse_module_info(
    file_path: &Path,
    root_dir: &Path,
    source: &str,
) -> Result<PythonModuleInfo> {
    let ast = ast::Suite::parse(source, &file_path.to_string_lossy())?;

    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let source = Arc::<str>::from(source);
    let line_index = LineIndex::from_source_text(&source);
    let reachability = reachability::collect(&ast, &source, &line_index);

    let mut visitor = SymbolVisitor {
        symbols: Vec::new(),
        relative_path,
        source,
        line_index,
        assignment_kind: Some(SymbolKind::Const),
        class_member: false,
    };

    visitor.visit_suite(&ast);

    let (exports, imports) = collect_exports_and_imports(&ast);

    Ok(PythonModuleInfo {
        symbols: visitor.symbols,
        exports,
        imports,
        reachability,
    })
}

struct SymbolVisitor {
    symbols: Vec<Symbol>,
    relative_path: String,
    source: Arc<str>,
    line_index: LineIndex,
    assignment_kind: Option<SymbolKind>,
    class_member: bool,
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
            callable: None,
            fuzz_policy: None,
            docs: None,
            export_paths: vec![],
            referenced: false,
            package: None,
            children: vec![],
        }
    }

    fn visit_suite(&mut self, suite: &[ast::Stmt]) {
        for stmt in suite {
            self.visit_stmt(stmt);
        }
    }

    fn add_assignment(&mut self, name: &str, range: TextRange, annotation: Option<&ast::Expr>) {
        let Some(kind) = self.assignment_kind else {
            return;
        };
        if name == "__all__" {
            return;
        }
        let signature = annotation.map_or_else(
            || name.to_string(),
            |annotation| format!("{name}: {}", format_py_expr(annotation)),
        );
        let symbol = self.create_symbol(
            name.to_string(),
            kind,
            determine_visibility(name),
            range,
            signature,
        );
        if let Some(existing) = self
            .symbols
            .iter_mut()
            .find(|existing| existing.name == name && existing.kind == kind)
        {
            if !existing.signature.contains(": ") && symbol.signature.contains(": ") {
                *existing = symbol;
            }
        } else {
            self.symbols.push(symbol);
        }
    }

    fn visit_stmt(&mut self, stmt: &ast::Stmt) {
        match stmt {
            ast::Stmt::FunctionDef(f) => {
                let name = f.name.as_str().to_string();
                let vis = determine_visibility(&name);
                let args_str = format_py_args(&f.args);
                let ret_str = format_py_returns(&f.returns);
                let dec_str = format_decorators(&f.decorator_list);
                let sig = format!("{}def {}({}){}", dec_str, name, args_str, ret_str);
                let mut symbol = self.create_symbol(name, SymbolKind::Function, vis, f.range, sig);
                symbol.callable = Some(callable::contract(callable::PythonCallable {
                    args: &f.args,
                    returns: &f.returns,
                    type_parameters: &f.type_params,
                    decorators: &f.decorator_list,
                    body: &f.body,
                    is_async: false,
                    is_class_member: self.class_member,
                    is_declaration_file: self.relative_path.ends_with(".pyi"),
                }));
                symbol.fuzz_policy = python_fuzz_policy(&f.body, &self.source, &self.line_index);
                self.symbols.push(symbol);
            }
            ast::Stmt::AsyncFunctionDef(f) => {
                let name = f.name.as_str().to_string();
                let vis = determine_visibility(&name);
                let args_str = format_py_args(&f.args);
                let ret_str = format_py_returns(&f.returns);
                let dec_str = format_decorators(&f.decorator_list);
                let sig = format!("{}async def {}({}){}", dec_str, name, args_str, ret_str);
                let mut symbol = self.create_symbol(name, SymbolKind::Function, vis, f.range, sig);
                symbol.callable = Some(callable::contract(callable::PythonCallable {
                    args: &f.args,
                    returns: &f.returns,
                    type_parameters: &f.type_params,
                    decorators: &f.decorator_list,
                    body: &f.body,
                    is_async: true,
                    is_class_member: self.class_member,
                    is_declaration_file: self.relative_path.ends_with(".pyi"),
                }));
                symbol.fuzz_policy = python_fuzz_policy(&f.body, &self.source, &self.line_index);
                self.symbols.push(symbol);
            }
            ast::Stmt::ClassDef(c) => {
                let name = c.name.as_str().to_string();
                let vis = determine_visibility(&name);

                // Build class signature with bases
                let bases: Vec<String> = c.bases.iter().map(format_py_expr).collect();
                let bases_str = if bases.is_empty() {
                    String::new()
                } else {
                    format!("({})", bases.join(", "))
                };
                let dec_str = format_decorators(&c.decorator_list);
                let sig = format!("{}class {}{}", dec_str, name, bases_str);

                let mut symbol =
                    self.create_symbol(name.clone(), SymbolKind::Class, vis, c.range, sig);

                // Visit children to find methods
                let mut child_visitor = SymbolVisitor {
                    symbols: Vec::new(),
                    relative_path: self.relative_path.clone(),
                    source: self.source.clone(),
                    line_index: self.line_index.clone(),
                    assignment_kind: Some(SymbolKind::Property),
                    class_member: true,
                };
                child_visitor.visit_suite(&c.body);

                // Adapt child symbols to be children of this class
                for mut child in child_visitor.symbols {
                    if child.kind == SymbolKind::Function {
                        child.kind = SymbolKind::Method;
                    }
                    self.qualify_child(&name, &mut child);
                    symbol.children.push(child);
                }

                self.symbols.push(symbol);
            }
            ast::Stmt::Assign(assign) => {
                for target in &assign.targets {
                    if let ast::Expr::Name(name) = target {
                        self.add_assignment(name.id.as_str(), assign.range, None);
                    }
                }
            }
            ast::Stmt::AnnAssign(assign) => {
                if let ast::Expr::Name(name) = assign.target.as_ref() {
                    self.add_assignment(
                        name.id.as_str(),
                        assign.range,
                        Some(assign.annotation.as_ref()),
                    );
                }
            }
            _ => {}
        }
    }

    fn qualify_child(&self, parent: &str, child: &mut Symbol) {
        let qualified_name = format!("{parent}.{}", child.name);
        child.id = format!(
            "py:{}:{}#{}",
            self.relative_path,
            kind_to_str(child.kind),
            qualified_name
        );
        for descendant in &mut child.children {
            self.qualify_child(&qualified_name, descendant);
        }
    }
}

fn python_fuzz_policy(
    body: &[ast::Stmt],
    source: &str,
    line_index: &LineIndex,
) -> Option<FuzzPolicyEvidence> {
    let Some(ast::Stmt::Expr(expression)) = body.first() else {
        return None;
    };
    let ast::Expr::Constant(constant) = &*expression.value else {
        return None;
    };
    let ast::Constant::Str(documentation) = &constant.value else {
        return None;
    };
    let start = line_index
        .source_location(expression.range.start(), source)
        .row
        .get();
    crate::fuzz::directive::parse_directive_lines(
        documentation
            .lines()
            .enumerate()
            .map(|(offset, line)| (start.saturating_add(offset as u32), line.to_string())),
    )
}

fn span_from_range(range: TextRange, source: &str, line_index: &LineIndex) -> Span {
    let start = line_index.source_location(range.start(), source);
    let end = line_index.source_location(range.end(), source);
    Span {
        start_line: start.row.get(),
        start_col: start.column.get(),
        end_line: end.row.get(),
        end_col: end.column.get(),
    }
}

fn determine_visibility(name: &str) -> Visibility {
    if name.starts_with("__") {
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
        SymbolKind::Const => "const",
        SymbolKind::Property => "property",
        _ => "sym",
    }
}

/// Format Python function arguments to a signature string
fn format_py_args(args: &ast::Arguments) -> String {
    let mut params = Vec::new();

    // Regular positional args (ArgWithDefault has .def field containing Arg)
    for arg in &args.args {
        let name = arg.def.arg.as_str();
        let type_str = arg
            .def
            .annotation
            .as_ref()
            .map(|ann| format!(": {}", format_py_expr(ann)))
            .unwrap_or_default();
        params.push(format!("{}{}", name, type_str));
    }

    // *args
    if let Some(vararg) = &args.vararg {
        let name = vararg.arg.as_str();
        params.push(format!("*{}", name));
    }

    // **kwargs
    if let Some(kwarg) = &args.kwarg {
        let name = kwarg.arg.as_str();
        params.push(format!("**{}", name));
    }

    params.join(", ")
}

/// Format Python return type annotation
fn format_py_returns(returns: &Option<Box<ast::Expr>>) -> String {
    returns
        .as_ref()
        .map(|r| format!(" -> {}", format_py_expr(r)))
        .unwrap_or_default()
}

/// Format Python expression (simplified, for type annotations)
fn format_py_expr(expr: &ast::Expr) -> String {
    match expr {
        ast::Expr::Name(name) => name.id.as_str().to_string(),
        ast::Expr::Attribute(attr) => {
            format!("{}.{}", format_py_expr(&attr.value), attr.attr.as_str())
        }
        ast::Expr::Subscript(sub) => {
            format!(
                "{}[{}]",
                format_py_expr(&sub.value),
                format_py_expr(&sub.slice)
            )
        }
        ast::Expr::Tuple(tuple) => {
            let elems: Vec<String> = tuple.elts.iter().map(format_py_expr).collect();
            elems.join(", ")
        }
        ast::Expr::List(list) => {
            let elems: Vec<String> = list.elts.iter().map(format_py_expr).collect();
            format!("[{}]", elems.join(", "))
        }
        ast::Expr::Constant(c) => match &c.value {
            ast::Constant::Str(s) => format!("\"{}\"", s),
            ast::Constant::Int(i) => i.to_string(),
            ast::Constant::Float(f) => f.to_string(),
            ast::Constant::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            ast::Constant::None => "None".to_string(),
            _ => "...".to_string(),
        },
        ast::Expr::BinOp(bin) => {
            // For Union types like `int | str`
            format!(
                "{} | {}",
                format_py_expr(&bin.left),
                format_py_expr(&bin.right)
            )
        }
        _ => "...".to_string(),
    }
}

/// Format class/function decorators
fn format_decorators(decorators: &[ast::Expr]) -> String {
    if decorators.is_empty() {
        return String::new();
    }

    let dec_names: Vec<String> = decorators
        .iter()
        .take(2)
        .map(|d| match d {
            ast::Expr::Name(n) => format!("@{}", n.id.as_str()),
            ast::Expr::Call(c) => {
                if let ast::Expr::Name(n) = &*c.func {
                    format!("@{}(...)", n.id.as_str())
                } else {
                    "@...".to_string()
                }
            }
            _ => "@...".to_string(),
        })
        .collect();

    if !dec_names.is_empty() {
        format!("{} ", dec_names.join(" "))
    } else {
        String::new()
    }
}

fn collect_exports_and_imports(suite: &[ast::Stmt]) -> (Option<Vec<String>>, Vec<PythonImport>) {
    let mut exports = None;
    let mut imports = Vec::new();

    for stmt in suite {
        match stmt {
            ast::Stmt::Assign(assign) => {
                if exports.is_some() {
                    continue;
                }
                let is_all = assign
                    .targets
                    .iter()
                    .any(|target| matches!(target, ast::Expr::Name(name) if name.id.as_str() == "__all__"));
                if !is_all {
                    continue;
                }
                if let Some(values) = extract_string_list(&assign.value) {
                    exports = Some(values);
                }
            }
            ast::Stmt::Import(import) => {
                if let Some(import) = python_import(import) {
                    imports.push(import);
                }
            }
            ast::Stmt::ImportFrom(import) => {
                imports.push(python_import_from(import));
            }
            _ => {}
        }
    }

    (exports, imports)
}

fn python_import(import: &ast::StmtImport) -> Option<PythonImport> {
    let mut modules = Vec::new();
    let mut aliases = Vec::new();
    for name in &import.names {
        modules.push(name.name.as_str().to_string());
        aliases.push(name.asname.as_ref().map(|alias| alias.as_str().to_string()));
    }
    (!modules.is_empty()).then_some(PythonImport {
        module: String::new(),
        names: modules,
        is_star: false,
        level: 0,
        aliases,
    })
}

fn python_import_from(import: &ast::StmtImportFrom) -> PythonImport {
    let module = import
        .module
        .as_ref()
        .map(|module| module.as_str().to_string())
        .unwrap_or_default();
    let mut names = Vec::new();
    let mut is_star = false;
    let mut aliases = Vec::new();
    for name in &import.names {
        let imported = name.name.as_str().to_string();
        if imported == "*" {
            is_star = true;
        } else {
            names.push(imported);
            aliases.push(name.asname.as_ref().map(|alias| alias.as_str().to_string()));
        }
    }
    PythonImport {
        module,
        names,
        is_star,
        level: import.level.map(|level| level.to_usize()).unwrap_or(0),
        aliases,
    }
}

fn extract_string_list(expr: &ast::Expr) -> Option<Vec<String>> {
    match expr {
        ast::Expr::List(list) => extract_strings(&list.elts),
        ast::Expr::Tuple(tuple) => extract_strings(&tuple.elts),
        _ => None,
    }
}

fn extract_strings(exprs: &[ast::Expr]) -> Option<Vec<String>> {
    let mut values = Vec::new();
    for expr in exprs {
        match expr {
            ast::Expr::Constant(constant) => {
                if let ast::Constant::Str(value) = &constant.value {
                    values.push(value.clone());
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(values)
}
