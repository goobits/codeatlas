use super::cargo::{CargoLayout, CargoTarget};
use super::{Module, ModuleKey};
use crate::rust::parser;
use codeatlas_domain::source_graph::{NodeId, ProjectId};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(super) enum Resolution {
    Module(ModuleKey),
    UnresolvedInternal(String),
}

pub(super) enum UseResolution {
    Symbols(BTreeSet<NodeId>),
    Module(ModuleKey),
    External(String),
    UnresolvedInternal(String),
}

pub(super) struct RustResolver {
    module_paths: BTreeMap<PathBuf, ModuleKey>,
    module_files: BTreeMap<ModuleKey, PathBuf>,
    exports: BTreeMap<(ModuleKey, String), ResolvedRustExport>,
    targets: Vec<CargoTarget>,
    workspace_libraries: BTreeMap<String, CargoTarget>,
    methods: BTreeMap<(ProjectId, String), BTreeSet<NodeId>>,
    associated: BTreeMap<(NodeId, String), BTreeSet<NodeId>>,
}

impl RustResolver {
    pub(super) fn new(cargo: &CargoLayout, modules: &BTreeMap<ModuleKey, Module>) -> Self {
        let module_paths = modules
            .iter()
            .map(|(key, module)| (module.absolute_path.clone(), key.clone()))
            .collect();
        let module_files = modules
            .iter()
            .map(|(key, module)| (key.clone(), module.absolute_path.clone()))
            .collect();
        let workspace_libraries = cargo
            .targets()
            .iter()
            .filter(|target| target.library)
            .map(|target| (target.package.replace('-', "_"), target.clone()))
            .collect();
        let mut methods = BTreeMap::<(ProjectId, String), BTreeSet<NodeId>>::new();
        let mut associated = BTreeMap::<(NodeId, String), BTreeSet<NodeId>>::new();
        for module in modules.values() {
            for (name, nodes) in &module.methods {
                methods
                    .entry((module.project.clone(), name.clone()))
                    .or_default()
                    .extend(nodes.iter().cloned());
            }
            for (qualified, nodes) in &module.symbols {
                let Some((owner, member)) = qualified.split_once('.') else {
                    continue;
                };
                let Some(owners) = module.symbols.get(owner) else {
                    continue;
                };
                for owner in owners {
                    associated
                        .entry((owner.clone(), member.to_string()))
                        .or_default()
                        .extend(nodes.iter().cloned());
                }
            }
        }
        let mut resolver = Self {
            module_paths,
            module_files,
            exports: BTreeMap::new(),
            targets: cargo.targets().to_vec(),
            workspace_libraries,
            methods,
            associated,
        };
        for (key, module) in modules {
            for (name, visibilities) in &module.info.symbol_visibilities {
                let Some(nodes) = module.symbols.get(name) else {
                    continue;
                };
                for visibility in visibilities {
                    resolver.merge_export(
                        key.clone(),
                        name.clone(),
                        ResolvedRustPath {
                            module: key.clone(),
                            symbols: nodes.clone(),
                        },
                        visibility.clone(),
                    );
                }
            }
            for declaration in &module.info.modules {
                if declaration.inline {
                    continue;
                }
                if let Resolution::Module(target) =
                    resolver.resolve_module_declaration(module, declaration)
                {
                    resolver.merge_export(
                        key.clone(),
                        declaration.name.clone(),
                        ResolvedRustPath {
                            module: target,
                            symbols: BTreeSet::new(),
                        },
                        declaration.visibility.clone(),
                    );
                }
            }
        }
        resolver.index_reexports(modules);
        resolver
    }

    pub(super) fn methods_named(
        &self,
        project: &ProjectId,
        name: &str,
    ) -> Option<&BTreeSet<NodeId>> {
        self.methods.get(&(project.clone(), name.to_string()))
    }

    fn index_reexports(&mut self, modules: &BTreeMap<ModuleKey, Module>) {
        loop {
            let mut additions = Vec::new();
            for (key, module) in modules {
                for export in &module.info.uses {
                    if export.is_glob {
                        let Some(target) = self.resolve_symbol_path(module, &export.module_path)
                        else {
                            continue;
                        };
                        additions.extend(
                            self.exported_paths(module, &target.module).into_iter().map(
                                |(name, resolved)| {
                                    (key.clone(), name, resolved, export.visibility.clone())
                                },
                            ),
                        );
                        continue;
                    }

                    let mut path = export.module_path.clone();
                    if export.name != "self" {
                        path.push(export.name.clone());
                    }
                    if let Some(resolved) = self.resolve_symbol_path(module, &path) {
                        additions.push((
                            key.clone(),
                            export.alias.clone(),
                            resolved,
                            export.visibility.clone(),
                        ));
                    }
                }
            }

            let mut changed = false;
            for (module, name, resolved, visibility) in additions {
                changed |= self.merge_export(module, name, resolved, visibility);
            }
            if !changed {
                break;
            }
        }
    }

    fn merge_export(
        &mut self,
        module: ModuleKey,
        name: String,
        resolved: ResolvedRustPath,
        visibility: parser::RustVisibility,
    ) -> bool {
        let key = (module, name);
        let Some(existing) = self.exports.get_mut(&key) else {
            self.exports.insert(
                key,
                ResolvedRustExport {
                    resolved,
                    visibilities: vec![visibility],
                },
            );
            return true;
        };
        let previous_symbols = existing.resolved.symbols.len();
        existing.resolved.symbols.extend(resolved.symbols);
        if previous_symbols == 0 && !existing.resolved.symbols.is_empty() {
            existing.resolved.module = resolved.module;
        }
        let added_visibility = if existing.visibilities.contains(&visibility) {
            false
        } else {
            existing.visibilities.push(visibility);
            true
        };
        existing.resolved.symbols.len() != previous_symbols || added_visibility
    }

    fn exported_paths(
        &self,
        requester: &Module,
        module: &ModuleKey,
    ) -> Vec<(String, ResolvedRustPath)> {
        self.exports
            .iter()
            .filter(|((owner, _), export)| {
                owner == module && self.export_is_visible(requester, owner, export)
            })
            .map(|((_, name), export)| (name.clone(), export.resolved.clone()))
            .collect()
    }

    pub(super) fn exported_symbols(
        &self,
        requester: &Module,
        module: &ModuleKey,
    ) -> BTreeSet<(String, NodeId)> {
        self.exported_paths(requester, module)
            .into_iter()
            .flat_map(|(name, resolved)| {
                resolved
                    .symbols
                    .into_iter()
                    .map(move |symbol| (name.clone(), symbol))
            })
            .collect()
    }

    pub(super) fn symbols_named(
        &self,
        requester: &Module,
        module: &ModuleKey,
        name: &str,
    ) -> BTreeSet<NodeId> {
        self.exports
            .get(&(module.clone(), name.to_string()))
            .filter(|export| self.export_is_visible(requester, module, export))
            .map(|export| export.resolved.symbols.clone())
            .unwrap_or_default()
    }

    pub(super) fn resolve_module_declaration(
        &self,
        module: &Module,
        declaration: &parser::ModuleDeclaration,
    ) -> Resolution {
        let raw = module_declaration_base(
            &module.absolute_path,
            declaration,
            self.targets
                .iter()
                .any(|target| target.root == module.absolute_path),
        );
        self.resolve_file_candidates(&raw).map_or_else(
            || Resolution::UnresolvedInternal(declaration.name.clone()),
            Resolution::Module,
        )
    }

    pub(super) fn resolve_use(&self, module: &Module, import: &parser::UseExport) -> UseResolution {
        let mut full_path = import.module_path.clone();
        if import.name != "self" && import.name != "*" {
            full_path.push(import.name.clone());
        }
        if let Some(resolved) = self.resolve_symbol_path(module, &full_path) {
            if !resolved.symbols.is_empty() {
                return UseResolution::Symbols(resolved.symbols);
            }
            return UseResolution::Module(resolved.module);
        }
        let first = full_path.first().cloned().unwrap_or_default();
        if matches!(first.as_str(), "crate" | "self" | "super")
            || module.package.as_deref() == Some(first.as_str())
            || self.workspace_libraries.contains_key(&first)
        {
            UseResolution::UnresolvedInternal(format_use(import))
        } else {
            UseResolution::External(format_use(import))
        }
    }

    pub(super) fn resolve_use_module(&self, module: &Module, path: &[String]) -> Option<ModuleKey> {
        self.resolve_symbol_path(module, path)
            .map(|resolved| resolved.module)
    }

    pub(super) fn resolve_imported_reference(
        &self,
        module: &Module,
        path: &[String],
    ) -> Option<ResolvedRustPath> {
        let local = path.first()?;
        module.info.uses.iter().find_map(|import| {
            if import.is_glob {
                let imported_module = self.resolve_use_module(module, &import.module_path)?;
                return self.resolve_from_module(module, &imported_module, path);
            }
            if &import.alias != local {
                return None;
            }
            let mut expanded = import.module_path.clone();
            if import.name != "self" {
                expanded.push(import.name.clone());
            }
            expanded.extend_from_slice(&path[1..]);
            self.resolve_symbol_path(module, &expanded)
        })
    }

    pub(super) fn resolve_symbol_path(
        &self,
        module: &Module,
        path: &[String],
    ) -> Option<ResolvedRustPath> {
        if path.is_empty() {
            return None;
        }
        if !matches!(path[0].as_str(), "crate" | "self" | "super")
            && module.package.as_deref().map(|name| name.replace('-', "_")) != Some(path[0].clone())
        {
            if let Some(declaration) = module
                .info
                .modules
                .iter()
                .find(|declaration| declaration.name == path[0] && !declaration.inline)
            {
                if let Resolution::Module(target) =
                    self.resolve_module_declaration(module, declaration)
                {
                    if let Some(resolved) = self.resolve_from_module(module, &target, &path[1..]) {
                        return Some(resolved);
                    }
                }
            }
        }
        for (target, segments) in self.target_and_segment_options(module, path) {
            let mut exact_module = None;
            for split in (0..=segments.len()).rev() {
                let module_segments = &segments[..split];
                let raw = module_segments
                    .iter()
                    .fold(target.module_base.clone(), |path, segment| {
                        path.join(segment)
                    });
                let key = if module_segments.is_empty() {
                    self.module_paths.get(&target.root).cloned()
                } else {
                    self.resolve_file_candidates(&raw)
                };
                let Some(key) = key else {
                    continue;
                };
                let Some(symbol) = segments.get(split) else {
                    exact_module = Some(key);
                    continue;
                };
                if let Some(export) = self.exports.get(&(key.clone(), symbol.clone())) {
                    if self.export_is_visible(module, &key, export) {
                        let remaining = &segments[split + 1..];
                        if export.resolved.symbols.is_empty() && !remaining.is_empty() {
                            if let Some(resolved) =
                                self.resolve_from_module(module, &export.resolved.module, remaining)
                            {
                                return Some(resolved);
                            }
                            continue;
                        }
                        return Some(self.with_associated(export.resolved.clone(), remaining));
                    }
                }
            }
            if let Some(module) = exact_module {
                return Some(ResolvedRustPath {
                    module,
                    symbols: BTreeSet::new(),
                });
            }
        }
        None
    }

    fn resolve_from_module(
        &self,
        requester: &Module,
        module: &ModuleKey,
        path: &[String],
    ) -> Option<ResolvedRustPath> {
        let Some(first) = path.first() else {
            return Some(ResolvedRustPath {
                module: module.clone(),
                symbols: BTreeSet::new(),
            });
        };
        if let Some(export) = self.exports.get(&(module.clone(), first.clone())) {
            if !self.export_is_visible(requester, module, export) {
                return None;
            }
            if path.len() == 1 {
                return Some(export.resolved.clone());
            }
            if !export.resolved.symbols.is_empty() {
                return Some(self.with_associated(export.resolved.clone(), &path[1..]));
            }
            return self.resolve_from_module(requester, &export.resolved.module, &path[1..]);
        }
        let absolute = self
            .module_paths
            .iter()
            .find_map(|(path, key)| (key == module).then_some(path))?;
        let child = self.resolve_file_candidates(&module_child_base(absolute).join(first))?;
        self.resolve_from_module(requester, &child, &path[1..])
    }

    pub(super) fn with_associated(
        &self,
        mut resolved: ResolvedRustPath,
        members: &[String],
    ) -> ResolvedRustPath {
        let mut owners = resolved.symbols.clone();
        for member in members {
            let targets = owners
                .iter()
                .flat_map(|owner| {
                    self.associated
                        .get(&(owner.clone(), member.clone()))
                        .into_iter()
                        .flatten()
                        .cloned()
                })
                .collect::<BTreeSet<_>>();
            if targets.is_empty() {
                break;
            }
            resolved.symbols.extend(targets.iter().cloned());
            owners = targets;
        }
        resolved
    }

    fn export_is_visible(
        &self,
        requester: &Module,
        owner: &ModuleKey,
        export: &ResolvedRustExport,
    ) -> bool {
        export
            .visibilities
            .iter()
            .any(|visibility| self.visibility_allows(requester, owner, visibility))
    }

    fn visibility_allows(
        &self,
        requester: &Module,
        owner: &ModuleKey,
        visibility: &parser::RustVisibility,
    ) -> bool {
        if visibility.is_public() {
            return true;
        }
        let Some(owner_path) = self.module_files.get(owner) else {
            return false;
        };
        let Some((owner_target, requester_target)) =
            self.shared_target(owner_path, &requester.absolute_path)
        else {
            return false;
        };

        let owner_segments = module_segments(owner_target, owner_path);
        let requester_segments = module_segments(requester_target, &requester.absolute_path);
        let scope = match visibility {
            parser::RustVisibility::Public => return true,
            parser::RustVisibility::Private => owner_segments,
            parser::RustVisibility::Restricted(path) => restricted_scope(&owner_segments, path),
        };
        requester_segments.starts_with(&scope)
    }

    fn target_and_segment_options(
        &self,
        module: &Module,
        path: &[String],
    ) -> Vec<(CargoTarget, Vec<String>)> {
        let Some(first) = path.first().map(String::as_str) else {
            return Vec::new();
        };
        if let Some(target) = self.workspace_libraries.get(first) {
            return vec![(target.clone(), path[1..].to_vec())];
        }
        let Some(target) = self.target_for_module(module) else {
            return Vec::new();
        };
        let mut segments = module_segments(target, &module.absolute_path);
        let mut index = 0;
        match first {
            "crate" => {
                segments.clear();
                index = 1;
            }
            "self" => index = 1,
            "super" => {
                while path.get(index).is_some_and(|part| part == "super") {
                    segments.pop();
                    index += 1;
                }
            }
            _ => {
                if module.package.as_deref().map(|name| name.replace('-', "_"))
                    == Some(first.to_string())
                {
                    segments.clear();
                    index = 1;
                } else {
                    let mut local = segments.clone();
                    local.extend_from_slice(path);
                    let mut options = vec![(target.clone(), local)];
                    if !segments.is_empty() {
                        options.push((target.clone(), path.to_vec()));
                    }
                    return options;
                }
            }
        }
        segments.extend_from_slice(&path[index..]);
        vec![(target.clone(), segments)]
    }

    fn target_for_module(&self, module: &Module) -> Option<&CargoTarget> {
        self.target_for_path(&module.absolute_path)
    }

    fn target_for_path(&self, path: &Path) -> Option<&CargoTarget> {
        self.targets_for_path(path)
            .into_iter()
            .max_by_key(|target| target.module_base.components().count())
    }

    fn shared_target(&self, left: &Path, right: &Path) -> Option<(&CargoTarget, &CargoTarget)> {
        let left_targets = self.targets_for_path(left);
        let right_targets = self.targets_for_path(right);
        left_targets.into_iter().find_map(|left_target| {
            right_targets
                .iter()
                .find(|right_target| right_target.root == left_target.root)
                .map(|right_target| (left_target, *right_target))
        })
    }

    fn targets_for_path(&self, path: &Path) -> Vec<&CargoTarget> {
        let exact = self
            .targets
            .iter()
            .filter(|target| path == target.root)
            .collect::<Vec<_>>();
        if !exact.is_empty() {
            return exact;
        }
        self.targets
            .iter()
            .filter(|target| path.starts_with(&target.module_base))
            .collect()
    }

    fn resolve_file_candidates(&self, raw: &Path) -> Option<ModuleKey> {
        module_file_candidates(raw)
            .into_iter()
            .find_map(|candidate| {
                self.module_paths.get(&candidate).cloned().or_else(|| {
                    candidate
                        .canonicalize()
                        .ok()
                        .and_then(|candidate| self.module_paths.get(&candidate).cloned())
                })
            })
    }
}

#[derive(Clone)]
struct ResolvedRustExport {
    resolved: ResolvedRustPath,
    visibilities: Vec<parser::RustVisibility>,
}

#[derive(Clone)]
pub(super) struct ResolvedRustPath {
    pub(super) module: ModuleKey,
    pub(super) symbols: BTreeSet<NodeId>,
}

fn module_child_base(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    if matches!(stem, "lib" | "main" | "mod") {
        path.parent().unwrap_or_else(|| Path::new("")).to_path_buf()
    } else {
        path.parent().unwrap_or_else(|| Path::new("")).join(stem)
    }
}

pub(super) fn module_declaration_base(
    path: &Path,
    declaration: &parser::ModuleDeclaration,
    crate_root: bool,
) -> PathBuf {
    declaration
        .path_override
        .as_ref()
        .map(|override_path| {
            path.parent()
                .unwrap_or_else(|| Path::new(""))
                .join(override_path)
        })
        .unwrap_or_else(|| {
            let base = if crate_root {
                path.parent().unwrap_or_else(|| Path::new("")).to_path_buf()
            } else {
                module_child_base(path)
            };
            base.join(&declaration.name)
        })
}

pub(super) fn module_file_candidates(raw: &Path) -> [PathBuf; 2] {
    [raw.with_extension("rs"), raw.join("mod.rs")]
}

fn module_segments(target: &CargoTarget, path: &Path) -> Vec<String> {
    if path == target.root {
        return Vec::new();
    }
    let relative = path
        .strip_prefix(&target.module_base)
        .unwrap_or(path)
        .to_path_buf();
    let normalized = codeatlas_source::paths::normalize_path(&relative);
    let normalized = normalized.strip_suffix(".rs").unwrap_or(&normalized);
    let normalized = normalized.strip_suffix("/mod").unwrap_or(normalized);
    normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

fn restricted_scope(owner: &[String], restriction: &[String]) -> Vec<String> {
    let Some(first) = restriction.first().map(String::as_str) else {
        return owner.to_vec();
    };
    let mut scope;
    let mut index;
    match first {
        "crate" => {
            scope = Vec::new();
            index = 1;
        }
        "self" => {
            scope = owner.to_vec();
            index = 1;
        }
        "super" => {
            scope = owner.to_vec();
            index = 0;
            while restriction.get(index).is_some_and(|part| part == "super") {
                scope.pop();
                index += 1;
            }
        }
        _ => {
            scope = owner.to_vec();
            index = 0;
        }
    }
    scope.extend_from_slice(&restriction[index..]);
    scope
}

fn format_use(import: &parser::UseExport) -> String {
    let mut path = import.module_path.clone();
    if import.name != "*" {
        path.push(import.name.clone());
    }
    path.join("::")
}
