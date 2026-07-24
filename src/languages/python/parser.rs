use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use rustpython_ast::{Ranged, Visitor};
use rustpython_parser::source_code::LineIndex;
use rustpython_parser::text_size::TextRange;
use rustpython_parser::{ast, Parse};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct PythonImport {
    pub module: String,
    pub names: Vec<String>,
    pub is_star: bool,
    pub level: usize,
    pub aliases: Vec<Option<String>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PythonReachabilityFacts {
    pub top_level_references: BTreeSet<String>,
    pub symbol_references: BTreeMap<String, BTreeSet<String>>,
    pub dynamic_dependencies: Vec<PythonDynamicDependency>,
    pub uncertainties: Vec<PythonUncertainty>,
}

#[derive(Debug, Clone)]
pub(crate) struct PythonDynamicDependency {
    pub owner: Option<String>,
    pub module: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PythonUncertaintyKind {
    DynamicImport,
    Reflection,
}

#[derive(Debug, Clone)]
pub(crate) struct PythonUncertainty {
    pub owner: Option<String>,
    pub kind: PythonUncertaintyKind,
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone)]
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
    let reachability = collect_reachability(&ast, &source, &line_index);

    let mut visitor = SymbolVisitor {
        symbols: Vec::new(),
        relative_path,
        source,
        line_index,
        recurse_into_functions: true,
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

    fn visit_stmt(&mut self, stmt: &ast::Stmt) {
        match stmt {
            ast::Stmt::FunctionDef(f) => {
                let name = f.name.as_str().to_string();
                let vis = determine_visibility(&name);
                let args_str = format_py_args(&f.args);
                let ret_str = format_py_returns(&f.returns);
                let dec_str = format_decorators(&f.decorator_list);
                let sig = format!("{}def {}({}){}", dec_str, name, args_str, ret_str);
                let symbol = self.create_symbol(name, SymbolKind::Function, vis, f.range, sig);
                self.symbols.push(symbol);

                if self.recurse_into_functions {
                    self.visit_suite(&f.body);
                }
            }
            ast::Stmt::AsyncFunctionDef(f) => {
                let name = f.name.as_str().to_string();
                let vis = determine_visibility(&name);
                let args_str = format_py_args(&f.args);
                let ret_str = format_py_returns(&f.returns);
                let dec_str = format_decorators(&f.decorator_list);
                let sig = format!("{}async def {}({}){}", dec_str, name, args_str, ret_str);
                let symbol = self.create_symbol(name, SymbolKind::Function, vis, f.range, sig);
                self.symbols.push(symbol);

                if self.recurse_into_functions {
                    self.visit_suite(&f.body);
                }
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
                    recurse_into_functions: false,
                };
                child_visitor.visit_suite(&c.body);

                // Adapt child symbols to be children of this class
                for mut child in child_visitor.symbols {
                    if child.kind == SymbolKind::Function {
                        child.kind = SymbolKind::Method;
                        child.id =
                            format!("py:{}:method#{}.{}", self.relative_path, name, child.name);
                    }
                    symbol.children.push(child);
                }

                self.symbols.push(symbol);
            }
            _ => {}
        }
    }
}

fn collect_reachability(
    suite: &[ast::Stmt],
    source: &str,
    line_index: &LineIndex,
) -> PythonReachabilityFacts {
    let mut facts = PythonReachabilityFacts::default();
    for statement in suite {
        match statement {
            ast::Stmt::FunctionDef(function) => {
                collect_callable_reachability(
                    &function.name,
                    &function.body,
                    &function.decorator_list,
                    source,
                    line_index,
                    &mut facts,
                );
            }
            ast::Stmt::AsyncFunctionDef(function) => {
                collect_callable_reachability(
                    &function.name,
                    &function.body,
                    &function.decorator_list,
                    source,
                    line_index,
                    &mut facts,
                );
            }
            ast::Stmt::ClassDef(class) => {
                let owner = class.name.as_str().to_string();
                let mut body = ReferenceCollector::new(Some(owner.clone()), source, line_index);
                for statement in &class.body {
                    body.visit_stmt(statement.clone());
                }
                merge_symbol_collector(&mut facts, owner, body);

                let mut definition = ReferenceCollector::new(None, source, line_index);
                for base in &class.bases {
                    definition.visit_expr(base.clone());
                }
                for keyword in &class.keywords {
                    definition.visit_keyword(keyword.clone());
                }
                for decorator in &class.decorator_list {
                    definition.visit_expr(decorator.clone());
                }
                record_unknown_decorators(
                    &class.decorator_list,
                    source,
                    line_index,
                    &mut definition,
                );
                merge_top_level_collector(&mut facts, definition);
            }
            _ => {
                let mut collector = ReferenceCollector::new(None, source, line_index);
                collector.visit_stmt(statement.clone());
                merge_top_level_collector(&mut facts, collector);
            }
        }
    }
    facts
}

fn collect_callable_reachability(
    name: &ast::Identifier,
    body: &[ast::Stmt],
    decorators: &[ast::Expr],
    source: &str,
    line_index: &LineIndex,
    facts: &mut PythonReachabilityFacts,
) {
    let owner = name.as_str().to_string();
    let mut callable = ReferenceCollector::new(Some(owner.clone()), source, line_index);
    for statement in body {
        callable.visit_stmt(statement.clone());
    }
    merge_symbol_collector(facts, owner, callable);

    let mut definition = ReferenceCollector::new(None, source, line_index);
    for decorator in decorators {
        definition.visit_expr(decorator.clone());
    }
    record_unknown_decorators(decorators, source, line_index, &mut definition);
    merge_top_level_collector(facts, definition);
}

fn merge_symbol_collector(
    facts: &mut PythonReachabilityFacts,
    owner: String,
    collector: ReferenceCollector<'_>,
) {
    facts
        .symbol_references
        .entry(owner)
        .or_default()
        .extend(collector.names);
    facts
        .dynamic_dependencies
        .extend(collector.dynamic_dependencies);
    facts.uncertainties.extend(collector.uncertainties);
}

fn merge_top_level_collector(
    facts: &mut PythonReachabilityFacts,
    collector: ReferenceCollector<'_>,
) {
    facts.top_level_references.extend(collector.names);
    facts
        .dynamic_dependencies
        .extend(collector.dynamic_dependencies);
    facts.uncertainties.extend(collector.uncertainties);
}

struct ReferenceCollector<'a> {
    owner: Option<String>,
    names: BTreeSet<String>,
    dynamic_dependencies: Vec<PythonDynamicDependency>,
    uncertainties: Vec<PythonUncertainty>,
    source: &'a str,
    line_index: &'a LineIndex,
}

impl<'a> ReferenceCollector<'a> {
    fn new(owner: Option<String>, source: &'a str, line_index: &'a LineIndex) -> Self {
        Self {
            owner,
            names: BTreeSet::new(),
            dynamic_dependencies: Vec::new(),
            uncertainties: Vec::new(),
            source,
            line_index,
        }
    }

    fn span(&self, range: TextRange) -> Span {
        span_from_range(range, self.source, self.line_index)
    }

    fn add_reflection(&mut self, range: TextRange, message: impl Into<String>) {
        self.uncertainties.push(PythonUncertainty {
            owner: self.owner.clone(),
            kind: PythonUncertaintyKind::Reflection,
            span: self.span(range),
            message: message.into(),
        });
    }
}

impl Visitor for ReferenceCollector<'_> {
    fn visit_expr_name(&mut self, node: ast::ExprName) {
        if matches!(node.ctx, ast::ExprContext::Load) {
            self.names.insert(node.id.as_str().to_string());
        }
    }

    fn visit_expr_call(&mut self, node: ast::ExprCall) {
        let callable = qualified_expr_name(&node.func);
        if matches!(
            callable.as_deref(),
            Some("importlib.import_module" | "__import__")
        ) {
            let module = node.args.first().and_then(string_constant);
            let span = self.span(node.range);
            self.dynamic_dependencies.push(PythonDynamicDependency {
                owner: self.owner.clone(),
                module,
                span: span.clone(),
            });
            if self
                .dynamic_dependencies
                .last()
                .is_some_and(|dependency| dependency.module.is_none())
            {
                self.uncertainties.push(PythonUncertainty {
                    owner: self.owner.clone(),
                    kind: PythonUncertaintyKind::DynamicImport,
                    span,
                    message: "Non-literal Python import prevents complete resolution.".to_string(),
                });
            }
        } else if matches!(
            callable.as_deref(),
            Some("eval" | "exec" | "getattr" | "setattr" | "globals" | "locals")
        ) {
            self.add_reflection(
                node.range,
                format!("Reflective Python call {callable:?} may hide references."),
            );
        }
        self.generic_visit_expr_call(node);
    }

    fn visit_stmt_assign(&mut self, node: ast::StmtAssign) {
        if self.owner.is_none()
            && node
                .targets
                .iter()
                .any(|target| matches!(target, ast::Expr::Attribute(_)))
        {
            self.add_reflection(
                node.range,
                "Top-level attribute assignment may monkey patch another object.",
            );
        }
        self.generic_visit_stmt_assign(node);
    }
}

fn record_unknown_decorators(
    decorators: &[ast::Expr],
    source: &str,
    line_index: &LineIndex,
    collector: &mut ReferenceCollector<'_>,
) {
    for decorator in decorators {
        if known_decorator(decorator) {
            continue;
        }
        collector.uncertainties.push(PythonUncertainty {
            owner: None,
            kind: PythonUncertaintyKind::Reflection,
            span: span_from_range(decorator.range(), source, line_index),
            message: format!(
                "Decorator {:?} may register or replace the declared symbol.",
                qualified_expr_name(decorator).unwrap_or_else(|| "<dynamic>".to_string())
            ),
        });
    }
}

fn known_decorator(expression: &ast::Expr) -> bool {
    matches!(
        qualified_expr_name(expression).as_deref(),
        Some(
            "staticmethod"
                | "classmethod"
                | "property"
                | "typing.overload"
                | "dataclasses.dataclass"
                | "functools.cached_property"
        )
    )
}

fn qualified_expr_name(expression: &ast::Expr) -> Option<String> {
    match expression {
        ast::Expr::Name(name) => Some(name.id.as_str().to_string()),
        ast::Expr::Attribute(attribute) => Some(format!(
            "{}.{}",
            qualified_expr_name(&attribute.value)?,
            attribute.attr.as_str()
        )),
        ast::Expr::Call(call) => qualified_expr_name(&call.func),
        _ => None,
    }
}

fn string_constant(expression: &ast::Expr) -> Option<String> {
    match expression {
        ast::Expr::Constant(constant) => match &constant.value {
            ast::Constant::Str(value) => Some(value.clone()),
            _ => None,
        },
        _ => None,
    }
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
                let mut modules = Vec::new();
                let mut aliases = Vec::new();
                for name in &import.names {
                    let module = name.name.as_str().to_string();
                    modules.push(module);
                    aliases.push(name.asname.as_ref().map(|alias| alias.as_str().to_string()));
                }
                if !modules.is_empty() {
                    imports.push(PythonImport {
                        module: String::new(),
                        names: modules,
                        is_star: false,
                        level: 0,
                        aliases,
                    });
                }
            }
            ast::Stmt::ImportFrom(import) => {
                let module = import
                    .module
                    .as_ref()
                    .map(|m| m.as_str().to_string())
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
                    }
                    aliases.push(name.asname.as_ref().map(|alias| alias.as_str().to_string()));
                }
                imports.push(PythonImport {
                    module,
                    names,
                    is_star,
                    level: import.level.map(|level| level.to_usize()).unwrap_or(0),
                    aliases,
                });
            }
            _ => {}
        }
    }

    (exports, imports)
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
