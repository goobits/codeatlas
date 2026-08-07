use crate::languages;
use crate::languages::ecmascript::resolver;
use crate::package;
use anyhow::{Context, Result};
use codeatlas_domain::{Language, ScanConfig, ScanReport, Symbol, Visibility};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::path::{Path, PathBuf};

struct PackageBundle {
    package_name: String,
    root: PathBuf,
    no_default_ignore: bool,
    report: ScanReport,
    selected: HashSet<String>,
}

struct DependencyRequest {
    importer_root: PathBuf,
    specifier: String,
    imported: String,
}

pub(crate) fn annotate_dependency_types(
    report: &mut ScanReport,
    root_dir: &Path,
    no_default_ignore: bool,
) -> Result<()> {
    let root = root_dir
        .canonicalize()
        .with_context(|| format!("Could not resolve {}", root_dir.display()))?;
    let root_package = report
        .package
        .as_ref()
        .context("Dependency type documentation requires package metadata")?;
    let root_selected = report
        .symbols
        .iter()
        .filter(|symbol| is_public_export(symbol))
        .map(|symbol| symbol.id.clone())
        .collect::<HashSet<_>>();

    let mut bundles = BTreeMap::new();
    bundles.insert(
        root.clone(),
        PackageBundle {
            package_name: root_package.name.clone(),
            root: root.clone(),
            no_default_ignore,
            report: report.clone(),
            selected: root_selected.clone(),
        },
    );

    let mut symbol_queue = root_selected
        .into_iter()
        .map(|id| (root.clone(), id))
        .collect::<VecDeque<_>>();
    let mut dependency_queue = VecDeque::new();
    let mut processed_symbols = HashSet::new();
    let mut processed_requests = HashSet::new();

    while !symbol_queue.is_empty() || !dependency_queue.is_empty() {
        if let Some((bundle_root, symbol_id)) = symbol_queue.pop_front() {
            if !processed_symbols.insert((bundle_root.clone(), symbol_id.clone())) {
                continue;
            }
            let (local_ids, requests) = inspect_symbol(
                bundles
                    .get(&bundle_root)
                    .context("Dependency package disappeared during traversal")?,
                &bundle_root,
                &symbol_id,
            )?;
            for id in local_ids {
                select_symbol(&mut bundles, &mut symbol_queue, &bundle_root, &id);
            }
            dependency_queue.extend(requests);
            continue;
        }

        let Some(request) = dependency_queue.pop_front() else {
            continue;
        };
        let request_key = (
            request.importer_root.clone(),
            request.specifier.clone(),
            request.imported.clone(),
        );
        if !processed_requests.insert(request_key) {
            continue;
        }

        let resolved = package::resolve_dependency(&request.importer_root, &request.specifier)
            .with_context(|| {
                format!(
                    "Could not resolve public type {} from {}",
                    request.imported, request.specifier
                )
            })?;
        if !package::is_local_dependency(&request.importer_root, &resolved)? {
            continue;
        }
        if !bundles.contains_key(&resolved.root) {
            let bundle = scan_dependency(&resolved.root, no_default_ignore)?;
            bundles.insert(resolved.root.clone(), bundle);
        }
        let bundle = bundles
            .get(&resolved.root)
            .context("Resolved dependency package was not scanned")?;
        let ids = resolve_exported_symbols(
            bundle,
            &request.specifier,
            &resolved.public_path,
            &request.imported,
        )?;
        for id in ids {
            select_symbol(&mut bundles, &mut symbol_queue, &resolved.root, &id);
        }
    }

    let mut additions = Vec::new();
    for (bundle_root, bundle) in bundles {
        if bundle_root == root {
            continue;
        }
        for mut symbol in bundle
            .report
            .symbols
            .into_iter()
            .filter(|symbol| bundle.selected.contains(&symbol.id))
        {
            qualify_dependency_symbol(&mut symbol, &bundle.package_name);
            additions.push(symbol);
        }
    }
    additions.sort_by(|left, right| {
        left.package
            .cmp(&right.package)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
    report.stats.symbols_found += additions.len();
    report.symbols.extend(additions);
    Ok(())
}

fn inspect_symbol(
    bundle: &PackageBundle,
    bundle_root: &Path,
    symbol_id: &str,
) -> Result<(Vec<String>, Vec<DependencyRequest>)> {
    let symbol = bundle
        .report
        .symbols
        .iter()
        .find(|symbol| symbol.id == symbol_id)
        .with_context(|| format!("Could not find selected symbol {symbol_id}"))?;
    let references = languages::typescript::referenced_identifiers(symbol);
    let mut local_ids = bundle
        .report
        .symbols
        .iter()
        .filter(|candidate| {
            candidate.file_path == symbol.file_path
                && candidate.visibility == Visibility::Public
                && references.contains(&candidate.name)
        })
        .map(|candidate| candidate.id.clone())
        .collect::<Vec<_>>();
    if symbol.language != Language::TypeScript {
        return Ok((local_ids, Vec::new()));
    }

    let source_path = bundle_root.join(&symbol.file_path);
    let module = languages::typescript::parser::parse_module_info(&source_path, bundle_root)
        .with_context(|| format!("Could not inspect imports in {}", source_path.display()))?;
    let mut requests = Vec::new();
    for import in module.imports {
        for binding in import.bindings {
            let imported_names = if binding.namespace {
                languages::typescript::referenced_namespace_members(symbol, &binding.local)
            } else if references.contains(&binding.local) {
                BTreeSet::from([binding.imported])
            } else {
                BTreeSet::new()
            };

            for imported in imported_names {
                if import.source.starts_with('.') {
                    local_ids.extend(resolve_relative_symbols(
                        bundle,
                        &symbol.file_path,
                        &import.source,
                        &imported,
                    ));
                } else {
                    requests.push(DependencyRequest {
                        importer_root: bundle_root.to_path_buf(),
                        specifier: import.source.clone(),
                        imported,
                    });
                }
            }
        }
    }
    local_ids.sort();
    local_ids.dedup();
    Ok((local_ids, requests))
}

fn scan_dependency(root: &Path, no_default_ignore: bool) -> Result<PackageBundle> {
    let package = package::discover(root)?
        .with_context(|| format!("No package metadata found at {}", root.display()))?;
    if package.exports.is_empty() {
        anyhow::bail!(
            "Package {} has no discoverable public exports",
            package.name
        );
    }
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(
            package
                .exports
                .iter()
                .map(|export| export.source_path.clone())
                .collect(),
        ),
        no_default_ignore,
    };
    let mut report = languages::scan_all(root, &config, languages::get_scanners_auto(root));
    let package_name = package.name.clone();
    super::package_exports::annotate(&mut report, root, package, no_default_ignore);
    super::annotate_docs(&mut report, root);
    Ok(PackageBundle {
        package_name,
        root: root.to_path_buf(),
        no_default_ignore,
        report,
        selected: HashSet::new(),
    })
}

fn resolve_exported_symbols(
    bundle: &PackageBundle,
    specifier: &str,
    public_path: &str,
    imported: &str,
) -> Result<Vec<String>> {
    let package = bundle
        .report
        .package
        .as_ref()
        .context("Dependency scan lost its package metadata")?;
    let export = package
        .exports
        .iter()
        .find(|export| export.public_path == public_path)
        .with_context(|| format!("Package {} does not export {specifier}", package.name))?;
    let reachable = languages::typescript::reachable_symbol_ids_for_exports(
        &bundle.root,
        &export.source_path,
        HashSet::from([imported.to_string()]),
        bundle.no_default_ignore,
    );
    let ids = bundle
        .report
        .symbols
        .iter()
        .filter(|symbol| reachable.contains(&symbol.id))
        .map(|symbol| symbol.id.clone())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        anyhow::bail!("Public type {imported} is not exported by {specifier}");
    }
    Ok(ids)
}

fn resolve_relative_symbols(
    bundle: &PackageBundle,
    from_file: &str,
    specifier: &str,
    imported: &str,
) -> Vec<String> {
    let target = resolve_relative_module(&bundle.report, from_file, specifier);
    let mut exact = bundle
        .report
        .symbols
        .iter()
        .filter(|symbol| {
            is_public_export(symbol)
                && symbol.name == imported
                && target
                    .as_ref()
                    .is_some_and(|target| symbol.file_path == *target)
        })
        .map(|symbol| symbol.id.clone())
        .collect::<Vec<_>>();
    if exact.is_empty() {
        exact.extend(
            bundle
                .report
                .symbols
                .iter()
                .filter(|symbol| is_public_export(symbol) && symbol.name == imported)
                .map(|symbol| symbol.id.clone()),
        );
    }
    exact.sort();
    exact.dedup();
    exact
}

fn resolve_relative_module(
    report: &ScanReport,
    from_file: &str,
    specifier: &str,
) -> Option<String> {
    resolver::resolve_relative_module(Path::new(""), from_file, specifier, false, |candidate| {
        report
            .symbols
            .iter()
            .any(|symbol| symbol.file_path == candidate)
    })
}

fn select_symbol(
    bundles: &mut BTreeMap<PathBuf, PackageBundle>,
    queue: &mut VecDeque<(PathBuf, String)>,
    root: &Path,
    id: &str,
) {
    let Some(bundle) = bundles.get_mut(root) else {
        return;
    };
    if bundle.selected.insert(id.to_string()) {
        queue.push_back((root.to_path_buf(), id.to_string()));
    }
}

fn is_public_export(symbol: &Symbol) -> bool {
    symbol.visibility == Visibility::Public && !symbol.export_paths.is_empty()
}

fn qualify_dependency_symbol(symbol: &mut Symbol, package_name: &str) {
    symbol.referenced = true;
    symbol.id = format!("{package_name}:{}", symbol.id);
    symbol.file_path = format!("{package_name}/{}", symbol.file_path);
    for child in &mut symbol.children {
        qualify_dependency_symbol(child, package_name);
    }
}
