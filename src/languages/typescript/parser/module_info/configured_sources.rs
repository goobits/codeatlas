use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};

pub(super) struct ConfiguredSources {
    pub(super) test_entrypoints: BTreeSet<String>,
    pub(super) runtime_entrypoints: BTreeSet<String>,
    pub(super) aliases: BTreeMap<String, BTreeSet<String>>,
}

pub(super) fn collect(module: &Module) -> ConfiguredSources {
    let mut source_bindings = StaticConfiguredSourceBindingCollector::default();
    module.visit_with(&mut source_bindings);
    let source_bindings = source_bindings.unique();
    let mut tests = ConfiguredTestEntrypointCollector {
        bindings: source_bindings.clone(),
        ..Default::default()
    };
    module.visit_with(&mut tests);
    let mut runtime = ConfiguredRuntimeEntrypointCollector {
        bindings: source_bindings,
        ..Default::default()
    };
    module.visit_with(&mut runtime);

    let mut string_bindings = StaticStringBindingCollector::default();
    module.visit_with(&mut string_bindings);
    let mut aliases = ConfiguredAliasCollector {
        bindings: string_bindings.unique(),
        ..Default::default()
    };
    module.visit_with(&mut aliases);

    ConfiguredSources {
        test_entrypoints: tests.paths,
        runtime_entrypoints: runtime.paths,
        aliases: aliases.aliases,
    }
}

#[derive(Default)]
struct ConfiguredRuntimeEntrypointCollector {
    paths: BTreeSet<String>,
    bindings: BTreeMap<String, BTreeSet<String>>,
}

impl Visit for ConfiguredRuntimeEntrypointCollector {
    fn visit_key_value_prop(&mut self, property: &KeyValueProp) {
        if matches!(
            prop_name_string(&property.key).as_deref(),
            Some("entry" | "entryPoints" | "input")
        ) {
            collect_configured_source_paths(&property.value, &self.bindings, &mut self.paths);
        }
        property.visit_children_with(self);
    }
}

#[derive(Default)]
struct ConfiguredTestEntrypointCollector {
    paths: BTreeSet<String>,
    bindings: BTreeMap<String, BTreeSet<String>>,
}

impl Visit for ConfiguredTestEntrypointCollector {
    fn visit_key_value_prop(&mut self, property: &KeyValueProp) {
        let key = match &property.key {
            PropName::Ident(identifier) => identifier.sym.as_ref(),
            PropName::Str(string) => string.value.as_ref(),
            _ => "",
        };
        if matches!(key, "alias" | "aliases") {
            collect_configured_alias_source_paths(&property.value, &self.bindings, &mut self.paths);
        } else if matches!(
            key,
            "setupFiles" | "setupFilesAfterEnv" | "globalSetup" | "globalTeardown"
        ) {
            collect_configured_source_paths(&property.value, &self.bindings, &mut self.paths);
        }
        property.visit_children_with(self);
    }
}

fn collect_configured_alias_source_paths(
    expression: &Expr,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    paths: &mut BTreeSet<String>,
) {
    match expression {
        Expr::Ident(identifier) => {
            if let Some(bound) = bindings.get(identifier.sym.as_ref()) {
                paths.extend(bound.iter().cloned());
            }
        }
        Expr::Object(object) => {
            for property in &object.props {
                let PropOrSpread::Prop(property) = property else {
                    continue;
                };
                let Prop::KeyValue(property) = &**property else {
                    continue;
                };
                collect_configured_source_paths(&property.value, bindings, paths);
            }
        }
        Expr::Array(array) => {
            for element in array.elems.iter().flatten() {
                let Expr::Object(object) = &*element.expr else {
                    continue;
                };
                for property in &object.props {
                    let PropOrSpread::Prop(property) = property else {
                        continue;
                    };
                    let Prop::KeyValue(property) = &**property else {
                        continue;
                    };
                    if prop_name_string(&property.key).as_deref() == Some("replacement") {
                        collect_configured_source_paths(&property.value, bindings, paths);
                    }
                }
            }
        }
        _ => {}
    }
}

fn collect_configured_source_paths(
    expression: &Expr,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    paths: &mut BTreeSet<String>,
) {
    match expression {
        Expr::Ident(identifier) => {
            if let Some(bound) = bindings.get(identifier.sym.as_ref()) {
                paths.extend(bound.iter().cloned());
            }
        }
        Expr::Lit(Lit::Str(string)) => {
            let value = string.value.to_string();
            let source = value
                .split_once('?')
                .map_or(value.as_str(), |(source, _)| source);
            if matches!(
                Path::new(source)
                    .extension()
                    .and_then(|extension| extension.to_str()),
                Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte")
            ) {
                paths.insert(source.to_string());
            }
        }
        Expr::Array(array) => {
            for element in array.elems.iter().flatten() {
                collect_configured_source_paths(&element.expr, bindings, paths);
            }
        }
        Expr::Object(object) => {
            for property in &object.props {
                if let PropOrSpread::Prop(property) = property {
                    if let Prop::KeyValue(property) = &**property {
                        collect_configured_source_paths(&property.value, bindings, paths);
                    }
                }
            }
        }
        Expr::Call(call) if is_static_path_constructor_call(call) => {
            for argument in &call.args {
                collect_configured_source_paths(&argument.expr, bindings, paths);
            }
        }
        Expr::New(expression) => {
            for argument in expression.args.iter().flatten() {
                collect_configured_source_paths(&argument.expr, bindings, paths);
            }
        }
        Expr::Tpl(template) if template.exprs.is_empty() => {
            let value = template
                .quasis
                .iter()
                .map(|quasi| quasi.raw.to_string())
                .collect::<String>();
            if matches!(
                Path::new(&value)
                    .extension()
                    .and_then(|extension| extension.to_str()),
                Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte")
            ) {
                paths.insert(value);
            }
        }
        _ => {}
    }
}

#[derive(Default)]
struct StaticConfiguredSourceBindingCollector {
    bindings: BTreeMap<String, Option<BTreeSet<String>>>,
    external_package_paths: BTreeSet<String>,
}

impl StaticConfiguredSourceBindingCollector {
    fn unique(self) -> BTreeMap<String, BTreeSet<String>> {
        self.bindings
            .into_iter()
            .filter_map(|(name, paths)| paths.map(|paths| (name, paths)))
            .collect()
    }
}

impl Visit for StaticConfiguredSourceBindingCollector {
    fn visit_var_declarator(&mut self, declaration: &VarDeclarator) {
        if let Pat::Ident(identifier) = &declaration.name {
            let name = identifier.id.sym.to_string();
            if self.bindings.contains_key(&name) {
                self.bindings.insert(name.clone(), None);
                self.external_package_paths.remove(&name);
                declaration.visit_children_with(self);
                return;
            }
            let external_package_path = declaration.init.as_deref().is_some_and(|expression| {
                derives_from_external_package_path(expression, &self.external_package_paths)
            });
            let mut paths = BTreeSet::new();
            if !external_package_path {
                if let Some(expression) = declaration.init.as_deref() {
                    collect_configured_source_paths(expression, &BTreeMap::new(), &mut paths);
                }
            }
            let paths = (!paths.is_empty()).then_some(paths);
            if external_package_path {
                self.external_package_paths.insert(name.clone());
            }
            self.bindings.insert(name, paths);
        }
        declaration.visit_children_with(self);
    }
}

#[derive(Default)]
struct ConfiguredAliasCollector {
    aliases: BTreeMap<String, BTreeSet<String>>,
    bindings: BTreeMap<String, BTreeSet<String>>,
}

impl Visit for ConfiguredAliasCollector {
    fn visit_object_lit(&mut self, object: &ObjectLit) {
        collect_alias_descriptor(object, &self.bindings, &mut self.aliases);
        object.visit_children_with(self);
    }

    fn visit_key_value_prop(&mut self, property: &KeyValueProp) {
        let key = prop_name_string(&property.key);
        if matches!(key.as_deref(), Some("alias" | "aliases")) {
            collect_aliases(&property.value, &self.bindings, &mut self.aliases);
        }
        property.visit_children_with(self);
    }
}

fn collect_aliases(
    expression: &Expr,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    aliases: &mut BTreeMap<String, BTreeSet<String>>,
) {
    match expression {
        Expr::Object(object) => {
            for property in &object.props {
                let PropOrSpread::Prop(property) = property else {
                    continue;
                };
                let Prop::KeyValue(property) = &**property else {
                    continue;
                };
                let Some(pattern) = prop_name_string(&property.key) else {
                    continue;
                };
                let targets = static_strings(&property.value, bindings);
                if !targets.is_empty() {
                    aliases.entry(pattern).or_default().extend(targets);
                }
            }
        }
        Expr::Array(array) => {
            for element in array.elems.iter().flatten() {
                let Expr::Object(object) = &*element.expr else {
                    continue;
                };
                collect_alias_descriptor(object, bindings, aliases);
            }
        }
        _ => {}
    }
}

fn collect_alias_descriptor(
    object: &ObjectLit,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    aliases: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let mut patterns = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for property in &object.props {
        let PropOrSpread::Prop(property) = property else {
            continue;
        };
        let Prop::KeyValue(property) = &**property else {
            continue;
        };
        match prop_name_string(&property.key).as_deref() {
            Some("find") => patterns.extend(static_strings(&property.value, bindings)),
            Some("replacement") => targets.extend(static_strings(&property.value, bindings)),
            _ => {}
        }
    }
    for pattern in patterns {
        aliases
            .entry(pattern)
            .or_default()
            .extend(targets.iter().cloned());
    }
}

fn prop_name_string(name: &PropName) -> Option<String> {
    match name {
        PropName::Ident(identifier) => Some(identifier.sym.to_string()),
        PropName::Str(string) => Some(string.value.to_string()),
        _ => None,
    }
}

fn static_strings(
    expression: &Expr,
    bindings: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    collect_static_strings(expression, bindings, &mut values);
    values
}

fn collect_static_strings(
    expression: &Expr,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    values: &mut BTreeSet<String>,
) {
    match expression {
        Expr::Ident(identifier) => {
            if let Some(bound) = bindings.get(identifier.sym.as_ref()) {
                values.extend(bound.iter().cloned());
            }
        }
        Expr::Lit(Lit::Str(string)) => {
            values.insert(string.value.to_string());
        }
        Expr::Tpl(template) if template.exprs.is_empty() => {
            values.insert(
                template
                    .quasis
                    .iter()
                    .map(|quasi| quasi.raw.to_string())
                    .collect(),
            );
        }
        Expr::Call(call) if is_static_path_constructor_call(call) => {
            for argument in &call.args {
                collect_static_strings(&argument.expr, bindings, values);
            }
        }
        Expr::New(expression) => {
            for argument in expression.args.iter().flatten() {
                collect_static_strings(&argument.expr, bindings, values);
            }
        }
        Expr::Array(array) => {
            for element in array.elems.iter().flatten() {
                collect_static_strings(&element.expr, bindings, values);
            }
        }
        _ => {}
    }
}

fn is_static_path_constructor_call(call: &CallExpr) -> bool {
    let Callee::Expr(callee) = &call.callee else {
        return false;
    };
    match &**callee {
        Expr::Ident(identifier) => matches!(
            identifier.sym.as_ref(),
            "fileURLToPath" | "join" | "resolve"
        ),
        Expr::Member(member) => match &member.prop {
            MemberProp::Ident(identifier) => matches!(
                identifier.sym.as_ref(),
                "fileURLToPath" | "join" | "resolve"
            ),
            MemberProp::Computed(computed) => matches!(
                &*computed.expr,
                Expr::Lit(Lit::Str(string))
                    if matches!(string.value.as_ref(), "fileURLToPath" | "join" | "resolve")
            ),
            MemberProp::PrivateName(_) => false,
        },
        _ => false,
    }
}

#[derive(Default)]
struct StaticStringBindingCollector {
    bindings: BTreeMap<String, Option<BTreeSet<String>>>,
    external_package_paths: BTreeSet<String>,
}

impl StaticStringBindingCollector {
    fn unique(self) -> BTreeMap<String, BTreeSet<String>> {
        self.bindings
            .into_iter()
            .filter_map(|(name, values)| values.map(|values| (name, values)))
            .collect()
    }
}

impl Visit for StaticStringBindingCollector {
    fn visit_var_declarator(&mut self, declaration: &VarDeclarator) {
        if let Pat::Ident(identifier) = &declaration.name {
            let name = identifier.id.sym.to_string();
            if self.bindings.contains_key(&name) {
                self.bindings.insert(name.clone(), None);
                self.external_package_paths.remove(&name);
                declaration.visit_children_with(self);
                return;
            }
            let external_package_path = declaration.init.as_deref().is_some_and(|expression| {
                derives_from_external_package_path(expression, &self.external_package_paths)
            });
            let values = (!external_package_path)
                .then(|| {
                    declaration
                        .init
                        .as_deref()
                        .map(|expression| static_strings(expression, &self.unique_bindings()))
                        .filter(|values| !values.is_empty())
                })
                .flatten();
            if external_package_path {
                self.external_package_paths.insert(name.clone());
            }
            self.bindings.insert(name, values);
        }
        declaration.visit_children_with(self);
    }
}

fn derives_from_external_package_path(
    expression: &Expr,
    external_package_paths: &BTreeSet<String>,
) -> bool {
    match expression {
        Expr::Ident(identifier) => external_package_paths.contains(identifier.sym.as_ref()),
        Expr::Call(call) if is_package_resolve_call(call) => true,
        Expr::Call(call) if is_path_derivation_call(call) => call.args.iter().any(|argument| {
            derives_from_external_package_path(&argument.expr, external_package_paths)
        }),
        _ => false,
    }
}

fn is_package_resolve_call(call: &CallExpr) -> bool {
    let Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let Expr::Member(member) = &**callee else {
        return false;
    };
    matches!(&*member.obj, Expr::Ident(identifier) if identifier.sym == "require")
        && matches!(
            &member.prop,
            MemberProp::Ident(identifier) if identifier.sym == "resolve"
        )
}

fn is_path_derivation_call(call: &CallExpr) -> bool {
    let Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let name = match &**callee {
        Expr::Ident(identifier) => Some(identifier.sym.as_ref()),
        Expr::Member(member) => match &member.prop {
            MemberProp::Ident(identifier) => Some(identifier.sym.as_ref()),
            MemberProp::Computed(computed) => match &*computed.expr {
                Expr::Lit(Lit::Str(string)) => Some(string.value.as_ref()),
                _ => None,
            },
            MemberProp::PrivateName(_) => None,
        },
        _ => None,
    };
    matches!(name, Some("dirname" | "fileURLToPath" | "join" | "resolve"))
}

impl StaticStringBindingCollector {
    fn unique_bindings(&self) -> BTreeMap<String, BTreeSet<String>> {
        self.bindings
            .iter()
            .filter_map(|(name, values)| {
                values.as_ref().map(|values| (name.clone(), values.clone()))
            })
            .collect()
    }
}
