use super::{collector::ModuleCollector, ModuleFacts, SqlExpression, StaticSql};
use anyhow::Result;
use codeatlas_languages::ecmascript::resolver::resolve_relative_module;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use swc_core::ecma::visit::VisitWith;

pub(super) struct StaticSqlResolver<'a> {
    root: &'a Path,
    modules: BTreeMap<String, ModuleFacts>,
}

impl<'a> StaticSqlResolver<'a> {
    pub(super) fn new(root: &'a Path) -> Self {
        Self {
            root,
            modules: BTreeMap::new(),
        }
    }

    pub(super) fn load(&mut self, display: &str) -> Result<&ModuleFacts> {
        if !self.modules.contains_key(display) {
            let path = self.root.join(display);
            let (module, source_map) =
                codeatlas_languages::typescript::parser::parse_syntax_tree(&path)?;
            let mut collector = ModuleCollector::new(display.to_string(), source_map);
            module.visit_with(&mut collector);
            self.modules.insert(display.to_string(), collector.finish());
        }
        Ok(&self.modules[display])
    }

    pub(super) fn resolve(
        &mut self,
        module_path: &str,
        expression: &SqlExpression,
        visited: &mut HashSet<(String, String)>,
    ) -> Result<Option<StaticSql>> {
        match expression {
            SqlExpression::Value(value) => Ok(Some(value.clone())),
            SqlExpression::Template(template) => {
                let mut text = String::new();
                let mut dynamic = false;
                for (index, quasi) in template.quasis.iter().enumerate() {
                    text.push_str(quasi);
                    let Some(expression) = template.expressions.get(index) else {
                        continue;
                    };
                    let mut branch = visited.clone();
                    match expression.value.as_ref() {
                        Some(value) => match self.resolve(module_path, value, &mut branch)? {
                            Some(value) => {
                                text.push_str(&value.text);
                                dynamic |= value.dynamic;
                            }
                            None => {
                                text.push_str(&expression.unresolved_marker);
                                dynamic = true;
                            }
                        },
                        None => {
                            text.push_str(&expression.unresolved_marker);
                            dynamic = true;
                        }
                    }
                }
                Ok(Some(StaticSql {
                    text,
                    path: template.path.clone(),
                    line: template.line,
                    column: template.column,
                    dynamic,
                }))
            }
            SqlExpression::Binding(name) => {
                if !visited.insert((module_path.to_string(), name.clone())) {
                    return Ok(None);
                }
                let facts = self.load(module_path)?.clone();
                if let Some(value) = facts.bindings.get(name) {
                    return self.resolve(module_path, value, visited);
                }
                let Some(import) = facts.imports.get(name) else {
                    return Ok(None);
                };
                let Some(target) = self.resolve_import(module_path, &import.source)? else {
                    return Ok(None);
                };
                self.resolve(
                    &target,
                    &SqlExpression::Binding(import.imported.clone()),
                    visited,
                )
            }
        }
    }

    fn resolve_import(&self, module_path: &str, specifier: &str) -> Result<Option<String>> {
        if let Some(relative) =
            resolve_relative_module(self.root, module_path, specifier, false, |candidate| {
                self.root.join(candidate).is_file()
            })
        {
            return Ok(is_ecmascript_source(Path::new(&relative)).then_some(relative));
        }
        let Some(dependency) = codeatlas_source::package::resolve_dependency(self.root, specifier)
        else {
            return Ok(None);
        };
        if !codeatlas_source::package::is_local_dependency(self.root, &dependency)? {
            return Ok(None);
        }
        let Some(package) = codeatlas_source::package::discover_javascript(&dependency.root)?
        else {
            return Ok(None);
        };
        let Some(export) = package
            .exports
            .iter()
            .find(|export| export.public_path == dependency.public_path)
        else {
            return Ok(None);
        };
        let target = dependency.root.join(&export.source_path);
        if !target.is_file() || !is_ecmascript_source(&target) {
            return Ok(None);
        }
        Ok(Some(codeatlas_source::paths::normalize_relative_path(
            &target, self.root,
        )))
    }
}

fn is_ecmascript_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx")
    )
}
