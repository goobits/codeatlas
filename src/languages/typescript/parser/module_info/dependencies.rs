use super::{DynamicDependency, DynamicDependencyKind, DynamicDependencyTarget};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use swc_core::common::{sync::Lrc, SourceMap};
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};

const MAX_STATIC_DEPENDENCY_TARGETS: usize = 128;

pub(super) fn collect(module: &Module, source_map: Lrc<SourceMap>) -> Vec<DynamicDependency> {
    let mut static_bindings = StaticDependencyBindingCollector::default();
    module.visit_with(&mut static_bindings);
    let mut dynamic = DynamicDependencyCollector {
        source_map,
        dependencies: Vec::new(),
        static_bindings: static_bindings.unique(),
        local_bindings: module.body.iter().flat_map(super::declared_names).collect(),
    };
    module.visit_with(&mut dynamic);
    dynamic.dependencies
}

struct DynamicDependencyCollector {
    source_map: Lrc<SourceMap>,
    dependencies: Vec<DynamicDependency>,
    static_bindings: BTreeMap<String, Vec<DynamicDependencyTarget>>,
    local_bindings: BTreeSet<String>,
}

#[derive(Default)]
struct StaticDependencyBindingCollector {
    bindings: BTreeMap<String, Option<Vec<DynamicDependencyTarget>>>,
}

impl StaticDependencyBindingCollector {
    fn unique(self) -> BTreeMap<String, Vec<DynamicDependencyTarget>> {
        self.bindings
            .into_iter()
            .filter_map(|(name, targets)| targets.map(|targets| (name, targets)))
            .collect()
    }

    fn unique_bindings(&self) -> BTreeMap<String, Vec<DynamicDependencyTarget>> {
        self.bindings
            .iter()
            .filter_map(|(name, targets)| {
                targets
                    .as_ref()
                    .map(|targets| (name.clone(), targets.clone()))
            })
            .collect()
    }
}

impl Visit for StaticDependencyBindingCollector {
    fn visit_var_declarator(&mut self, declaration: &VarDeclarator) {
        if let Pat::Ident(identifier) = &declaration.name {
            let bindings = self.unique_bindings();
            let targets = declaration
                .init
                .as_deref()
                .and_then(|expression| static_dependency_targets(expression, &bindings));
            self.bindings
                .entry(identifier.id.sym.to_string())
                .and_modify(|existing| *existing = None)
                .or_insert(targets);
        }
        declaration.visit_children_with(self);
    }
}

impl Visit for DynamicDependencyCollector {
    fn visit_call_expr(&mut self, call: &CallExpr) {
        if matches!(&call.callee, Callee::Expr(expression) if is_static_existence_probe(expression))
        {
            return;
        }
        if let Some(targets) =
            runtime_process_targets(call, &self.static_bindings, &self.local_bindings)
        {
            let span = source_span(&self.source_map, call.span);
            self.dependencies.extend(
                targets
                    .into_iter()
                    .filter(runtime_path_targets_source_module)
                    .map(|target| DynamicDependency {
                        target,
                        kind: DynamicDependencyKind::RuntimeProcess,
                        span: span.clone(),
                    }),
            );
        }
        let kind = match &call.callee {
            Callee::Import(_) => Some(DynamicDependencyKind::Import),
            Callee::Expr(expression) if matches!(&**expression, Expr::Ident(identifier) if identifier.sym == *"require") => {
                Some(DynamicDependencyKind::Require)
            }
            Callee::Expr(expression) if is_import_meta_glob(expression) => {
                Some(DynamicDependencyKind::ImportMetaGlob)
            }
            Callee::Expr(expression) if matches!(&**expression, Expr::Ident(identifier) if identifier.sym == *"importScripts") => {
                Some(DynamicDependencyKind::ImportScripts)
            }
            Callee::Expr(expression) if is_static_file_reader(expression, &self.local_bindings) => {
                Some(DynamicDependencyKind::RuntimeFile)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            let span = source_span(&self.source_map, call.span);
            let targets = if kind == DynamicDependencyKind::ImportScripts {
                call.args
                    .iter()
                    .flat_map(|argument| {
                        dependency_targets(&argument.expr, kind, &self.static_bindings)
                    })
                    .collect::<Vec<_>>()
            } else {
                call.args
                    .first()
                    .map(|argument| dependency_targets(&argument.expr, kind, &self.static_bindings))
                    .unwrap_or_else(|| vec![DynamicDependencyTarget::Unknown])
            };
            self.dependencies.extend(
                targets
                    .into_iter()
                    .filter(|target| {
                        kind != DynamicDependencyKind::RuntimeFile
                            || runtime_path_targets_source_module(target)
                    })
                    .map(|target| DynamicDependency {
                        target,
                        kind,
                        span: span.clone(),
                    }),
            );
        }
        call.visit_children_with(self);
    }

    fn visit_new_expr(&mut self, expression: &NewExpr) {
        let is_url =
            matches!(&*expression.callee, Expr::Ident(identifier) if identifier.sym == *"URL");
        let args = expression.args.as_deref().unwrap_or_default();
        if is_url
            && args
                .get(1)
                .is_some_and(|argument| is_import_meta_url(&argument.expr))
        {
            let targets = args
                .first()
                .map(|argument| {
                    dependency_targets(
                        &argument.expr,
                        DynamicDependencyKind::RuntimeUrl,
                        &self.static_bindings,
                    )
                })
                .unwrap_or_else(|| vec![DynamicDependencyTarget::Unknown]);
            let span = source_span(&self.source_map, expression.span);
            self.dependencies.extend(
                targets
                    .into_iter()
                    .filter(runtime_path_targets_source_module)
                    .map(|target| DynamicDependency {
                        target,
                        kind: DynamicDependencyKind::RuntimeUrl,
                        span: span.clone(),
                    }),
            );
        }
        expression.visit_children_with(self);
    }
}

fn runtime_process_targets(
    call: &CallExpr,
    static_bindings: &BTreeMap<String, Vec<DynamicDependencyTarget>>,
    local_bindings: &BTreeSet<String>,
) -> Option<Vec<DynamicDependencyTarget>> {
    let Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let launcher = process_launcher_name(callee, local_bindings)?;
    let command = call.args.first()?;
    if launcher == "fork" {
        return Some(dependency_targets(
            &command.expr,
            DynamicDependencyKind::RuntimeProcess,
            static_bindings,
        ));
    }

    let mut targets = dependency_targets(
        &command.expr,
        DynamicDependencyKind::RuntimeProcess,
        static_bindings,
    );
    if is_javascript_runtime(&command.expr) {
        if let Some(arguments) = call.args.get(1) {
            targets.extend(dependency_targets(
                &arguments.expr,
                DynamicDependencyKind::RuntimeProcess,
                static_bindings,
            ));
        }
    }
    Some(targets)
}

fn process_launcher_name<'a>(
    expression: &'a Expr,
    local_bindings: &BTreeSet<String>,
) -> Option<&'a str> {
    let name = match expression {
        Expr::Ident(identifier) if !local_bindings.contains(identifier.sym.as_ref()) => {
            identifier.sym.as_ref()
        }
        Expr::Member(member) => match &member.prop {
            MemberProp::Ident(identifier) => identifier.sym.as_ref(),
            MemberProp::Computed(computed) => match &*computed.expr {
                Expr::Lit(Lit::Str(value)) => value.value.as_ref(),
                _ => return None,
            },
            MemberProp::PrivateName(_) => return None,
        },
        _ => return None,
    };
    matches!(
        name,
        "execFile" | "execFileSync" | "fork" | "spawn" | "spawnSync"
    )
    .then_some(name)
}

fn is_javascript_runtime(expression: &Expr) -> bool {
    if let Expr::Member(member) = expression {
        return matches!(&*member.obj, Expr::Ident(identifier) if identifier.sym == *"process")
            && matches!(&member.prop, MemberProp::Ident(identifier) if identifier.sym == *"execPath");
    }
    let Expr::Lit(Lit::Str(value)) = expression else {
        return false;
    };
    Path::new(value.value.as_ref())
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "bun" | "deno" | "node" | "nodejs" | "ts-node" | "tsx"))
}

fn runtime_path_targets_source_module(target: &DynamicDependencyTarget) -> bool {
    let path = match target {
        DynamicDependencyTarget::Literal(path) => path.as_str(),
        DynamicDependencyTarget::Pattern { suffix, .. } => suffix.as_str(),
        DynamicDependencyTarget::Glob(_) | DynamicDependencyTarget::Unknown => return false,
    };
    matches!(
        Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte")
    )
}

fn is_static_file_reader(expression: &Expr, local_bindings: &BTreeSet<String>) -> bool {
    fn is_reader(name: &str) -> bool {
        matches!(name, "load" | "read" | "readFile" | "readFileSync")
    }

    match expression {
        Expr::Ident(identifier) => {
            is_reader(identifier.sym.as_ref()) && !local_bindings.contains(identifier.sym.as_ref())
        }
        Expr::Member(member) => match &member.prop {
            MemberProp::Ident(identifier) => is_reader(identifier.sym.as_ref()),
            MemberProp::Computed(computed) => {
                matches!(&*computed.expr, Expr::Lit(Lit::Str(value)) if is_reader(value.value.as_ref()))
            }
            MemberProp::PrivateName(_) => false,
        },
        _ => false,
    }
}

fn is_static_existence_probe(expression: &Expr) -> bool {
    match expression {
        Expr::Ident(identifier) => identifier.sym == *"existsSync",
        Expr::Member(member) => match &member.prop {
            MemberProp::Ident(identifier) => identifier.sym == *"existsSync",
            MemberProp::Computed(computed) => {
                matches!(&*computed.expr, Expr::Lit(Lit::Str(value)) if value.value == *"existsSync")
            }
            MemberProp::PrivateName(_) => false,
        },
        _ => false,
    }
}

fn source_span(source_map: &SourceMap, span: swc_core::common::Span) -> crate::domain::Span {
    let start = source_map.lookup_char_pos(span.lo);
    let end = source_map.lookup_char_pos(span.hi);
    crate::domain::Span {
        start_line: start.line as u32,
        start_col: start.col.0 as u32,
        end_line: end.line as u32,
        end_col: end.col.0 as u32,
    }
}

fn is_import_meta_glob(expression: &Expr) -> bool {
    let Expr::Member(member) = expression else {
        return false;
    };
    let Expr::MetaProp(meta) = &*member.obj else {
        return false;
    };
    meta.kind == MetaPropKind::ImportMeta
        && matches!(&member.prop, MemberProp::Ident(identifier) if identifier.sym == *"glob")
}

fn is_import_meta_url(expression: &Expr) -> bool {
    let Expr::Member(member) = expression else {
        return false;
    };
    let Expr::MetaProp(meta) = &*member.obj else {
        return false;
    };
    meta.kind == MetaPropKind::ImportMeta
        && matches!(&member.prop, MemberProp::Ident(identifier) if identifier.sym == *"url")
}

fn dependency_targets(
    expression: &Expr,
    kind: DynamicDependencyKind,
    static_bindings: &BTreeMap<String, Vec<DynamicDependencyTarget>>,
) -> Vec<DynamicDependencyTarget> {
    match expression {
        Expr::Ident(identifier) => static_bindings
            .get(identifier.sym.as_ref())
            .cloned()
            .unwrap_or_else(|| vec![DynamicDependencyTarget::Unknown]),
        Expr::Lit(Lit::Str(value)) => vec![if kind == DynamicDependencyKind::ImportMetaGlob {
            DynamicDependencyTarget::Glob(value.value.to_string())
        } else {
            DynamicDependencyTarget::Literal(value.value.to_string())
        }],
        Expr::Tpl(template) if template.exprs.is_empty() => vec![DynamicDependencyTarget::Literal(
            template
                .quasis
                .iter()
                .map(|quasi| quasi.raw.to_string())
                .collect(),
        )],
        Expr::Tpl(template) => static_template_targets(template, kind, static_bindings)
            .unwrap_or_else(|| {
                vec![DynamicDependencyTarget::Pattern {
                    prefix: template
                        .quasis
                        .first()
                        .map(|quasi| quasi.raw.to_string())
                        .unwrap_or_default(),
                    suffix: template
                        .quasis
                        .last()
                        .map(|quasi| quasi.raw.to_string())
                        .unwrap_or_default(),
                }]
            }),
        Expr::Member(member) if is_href_member(member) => {
            dependency_targets(&member.obj, kind, static_bindings)
        }
        Expr::New(expression) if is_import_meta_relative_url(expression) => expression
            .args
            .as_deref()
            .and_then(|arguments| arguments.first())
            .map(|argument| dependency_targets(&argument.expr, kind, static_bindings))
            .unwrap_or_else(|| vec![DynamicDependencyTarget::Unknown]),
        Expr::Array(array) => {
            let targets = array
                .elems
                .iter()
                .flat_map(|element| match element {
                    Some(element) => dependency_targets(&element.expr, kind, static_bindings),
                    None => vec![DynamicDependencyTarget::Unknown],
                })
                .collect::<Vec<_>>();
            if targets.is_empty() {
                vec![DynamicDependencyTarget::Unknown]
            } else {
                targets
            }
        }
        _ => vec![DynamicDependencyTarget::Unknown],
    }
}

fn static_template_targets(
    template: &Tpl,
    kind: DynamicDependencyKind,
    static_bindings: &BTreeMap<String, Vec<DynamicDependencyTarget>>,
) -> Option<Vec<DynamicDependencyTarget>> {
    let first = template.quasis.first()?.raw.to_string();
    let mut values = vec![first];
    for (index, expression) in template.exprs.iter().enumerate() {
        let replacements = dependency_targets(expression, kind, static_bindings)
            .into_iter()
            .map(|target| match target {
                DynamicDependencyTarget::Literal(value) => Some(value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        if replacements.is_empty()
            || values.len().saturating_mul(replacements.len()) > MAX_STATIC_DEPENDENCY_TARGETS
        {
            return None;
        }
        let suffix = template.quasis.get(index + 1)?.raw.to_string();
        let mut expanded = Vec::with_capacity(values.len() * replacements.len());
        for prefix in &values {
            for replacement in &replacements {
                expanded.push(format!("{prefix}{replacement}{suffix}"));
            }
        }
        values = expanded;
    }
    Some(
        values
            .into_iter()
            .map(DynamicDependencyTarget::Literal)
            .collect(),
    )
}

fn is_href_member(member: &MemberExpr) -> bool {
    match &member.prop {
        MemberProp::Ident(identifier) => identifier.sym == *"href",
        MemberProp::Computed(computed) => {
            matches!(&*computed.expr, Expr::Lit(Lit::Str(value)) if value.value == *"href")
        }
        MemberProp::PrivateName(_) => false,
    }
}

fn is_import_meta_relative_url(expression: &NewExpr) -> bool {
    matches!(&*expression.callee, Expr::Ident(identifier) if identifier.sym == *"URL")
        && expression
            .args
            .as_deref()
            .and_then(|arguments| arguments.get(1))
            .is_some_and(|argument| is_import_meta_url(&argument.expr))
}

fn static_dependency_targets(
    expression: &Expr,
    static_bindings: &BTreeMap<String, Vec<DynamicDependencyTarget>>,
) -> Option<Vec<DynamicDependencyTarget>> {
    let targets = dependency_targets(
        expression,
        DynamicDependencyKind::RuntimeUrl,
        static_bindings,
    );
    if targets.is_empty() {
        return None;
    }
    targets
        .iter()
        .all(|target| !matches!(target, DynamicDependencyTarget::Unknown))
        .then_some(targets)
}
