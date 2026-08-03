use super::{
    python_import, python_import_from, span_from_range, PythonDynamicDependency,
    PythonReachabilityFacts, PythonScopedImport, PythonUncertainty, PythonUncertaintyKind,
};
use crate::domain::Span;
use rustpython_ast::{Ranged, Visitor};
use rustpython_parser::ast;
use rustpython_parser::source_code::LineIndex;
use rustpython_parser::text_size::TextRange;
use std::collections::BTreeSet;

pub(super) fn collect(
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
                if has_unknown_decorator(&class.decorator_list) {
                    facts.dynamic_entrypoints.insert(owner.clone());
                }
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
    if has_unknown_decorator(decorators) {
        facts.dynamic_entrypoints.insert(owner.clone());
    }
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
        .entry(owner.clone())
        .or_default()
        .extend(collector.names);
    facts
        .symbol_qualified_references
        .entry(owner)
        .or_default()
        .extend(collector.qualified_names);
    facts.scoped_imports.extend(collector.scoped_imports);
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
        .top_level_qualified_references
        .extend(collector.qualified_names);
    facts.scoped_imports.extend(collector.scoped_imports);
    facts
        .dynamic_dependencies
        .extend(collector.dynamic_dependencies);
    facts.uncertainties.extend(collector.uncertainties);
}

struct ReferenceCollector<'a> {
    owner: Option<String>,
    names: BTreeSet<String>,
    qualified_names: BTreeSet<String>,
    scoped_imports: Vec<PythonScopedImport>,
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
            qualified_names: BTreeSet::new(),
            scoped_imports: Vec::new(),
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

    fn visit_expr_attribute(&mut self, node: ast::ExprAttribute) {
        if matches!(node.ctx, ast::ExprContext::Load) {
            if let Some(parent) = qualified_expr_name(&node.value) {
                let name = format!("{parent}.{}", node.attr.as_str());
                self.qualified_names.insert(name);
            }
        }
        self.generic_visit_expr_attribute(node);
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

    fn visit_stmt_import(&mut self, node: ast::StmtImport) {
        if let (Some(owner), Some(import)) = (self.owner.clone(), python_import(&node)) {
            self.scoped_imports
                .push(PythonScopedImport { owner, import });
        }
    }

    fn visit_stmt_import_from(&mut self, node: ast::StmtImportFrom) {
        if let Some(owner) = self.owner.clone() {
            self.scoped_imports.push(PythonScopedImport {
                owner,
                import: python_import_from(&node),
            });
        }
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

fn has_unknown_decorator(decorators: &[ast::Expr]) -> bool {
    decorators
        .iter()
        .any(|decorator| !known_decorator(decorator))
}

fn known_decorator(expression: &ast::Expr) -> bool {
    let name = qualified_expr_name(expression);
    matches!(
        name.as_deref(),
        Some(
            "staticmethod"
                | "classmethod"
                | "property"
                | "typing.overload"
                | "overload"
                | "dataclasses.dataclass"
                | "dataclass"
                | "functools.cached_property"
                | "cached_property"
                | "contextlib.contextmanager"
                | "contextmanager"
                | "typing.runtime_checkable"
                | "runtime_checkable"
        )
    ) || name.is_some_and(|name| name.starts_with("pytest.mark."))
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
