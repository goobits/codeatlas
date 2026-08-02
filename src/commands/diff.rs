use super::{annotate_report, build_scan_config, exit_code, load_project, scan_project};
use crate::domain::{PackageInfo, ScanReport, Symbol, SymbolKind};
use anyhow::{Context, Result};
use base64::Engine;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const BASELINE_FORMAT: &str = "codeatlas.public-api-baseline";
const BASELINE_SCHEMA_VERSION: u32 = 1;
const ROOT_EXPORT_PATH: &str = "<root>";
const SUPPORTING_EXPORT_PATH: &str = "<supporting>";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PublicApiBaseline {
    format: String,
    schema_version: u32,
    tool_version: String,
    pub(crate) workspace: bool,
    pub(crate) packages: Vec<PublicApiPackage>,
}

impl PublicApiBaseline {
    pub(crate) fn symbol_count(&self) -> usize {
        self.packages
            .iter()
            .map(|package| package.symbols.len())
            .sum()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PublicApiPackage {
    name: String,
    root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exports: Vec<String>,
    symbols: Vec<PublicApiSymbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PublicApiSymbol {
    export_path: String,
    kind: SymbolKind,
    qualified_name: String,
    contracts: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct BaselineScan {
    pub(crate) baseline: PublicApiBaseline,
    pub(crate) unused_public: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SymbolIdentity {
    package: String,
    export_path: String,
    kind: SymbolKind,
    qualified_name: String,
}

pub(crate) fn run(
    baseline_path: &Path,
    path: &Path,
    workspace: bool,
    exact: bool,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(compare(baseline_path, path, workspace, exact, config_path))
}

pub(crate) fn create_baseline(
    path: &Path,
    workspace: bool,
    audit_unused: bool,
    consumer_root: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<BaselineScan> {
    if workspace {
        create_workspace_baseline(path, audit_unused, consumer_root, config_path)
    } else {
        create_single_baseline(path, audit_unused, consumer_root, config_path)
    }
}

pub(crate) fn render_baseline(baseline: &PublicApiBaseline) -> Result<String> {
    let mut rendered = serde_json::to_string(baseline)?;
    rendered.push('\n');
    Ok(rendered)
}

fn create_single_baseline(
    path: &Path,
    audit_unused: bool,
    consumer_root: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<BaselineScan> {
    let project = load_project(path, config_path)?;
    let (report, unused_public) = scan_report(&project, audit_unused, consumer_root)?;
    let root = crate::paths::normalize_path(&project.root);
    Ok(BaselineScan {
        baseline: baseline_from_reports(vec![(root, report)], false)?,
        unused_public,
    })
}

fn create_workspace_baseline(
    path: &Path,
    audit_unused: bool,
    consumer_root: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<BaselineScan> {
    let project = load_project(path, config_path)?;
    let workspace = crate::package::discover_workspace(&project.root)?;
    let mut reports = Vec::new();
    let mut unused_public = Vec::new();

    for member in workspace.members {
        let member_config_path = member.root.join("codeatlas.json");
        let member_project = load_project(
            &member.root,
            member_config_path
                .is_file()
                .then_some(member_config_path.as_path()),
        )?;
        let Some(package) = crate::package::discover_for_docs(
            &member_project.root,
            member_project.config.docs.declaration_contract,
        )?
        else {
            continue;
        };
        if package.exports.is_empty() {
            continue;
        }

        if crate::languages::get_scanners_auto(&member_project.root).is_empty()
            && member_project.config.languages.is_empty()
        {
            reports.push((
                member.report_root,
                ScanReport {
                    package: Some(package),
                    ..ScanReport::default()
                },
            ));
            continue;
        }

        let (report, member_unused) = scan_report(&member_project, audit_unused, consumer_root)
            .with_context(|| format!("Could not scan workspace package {}", member.name))?;
        unused_public.extend(
            member_unused
                .into_iter()
                .map(|unused| format!("{}::{unused}", member.name)),
        );
        reports.push((member.report_root, report));
    }

    if reports.is_empty() {
        anyhow::bail!(
            "No public package exports were discovered in workspace scope {}",
            project.root.display()
        );
    }
    unused_public.sort();
    unused_public.dedup();
    Ok(BaselineScan {
        baseline: baseline_from_reports(reports, true)?,
        unused_public,
    })
}

fn scan_report(
    project: &crate::config::ProjectConfig,
    audit_unused: bool,
    consumer_root: Option<&Path>,
) -> Result<(ScanReport, Vec<String>)> {
    let config = build_scan_config(project, false, None)?;
    let mut report = scan_project(project, &config)?;
    annotate_report(&mut report, project)?;
    if audit_unused {
        let mut importers = crate::analysis::annotate_imports(
            &mut report,
            &project.root,
            project.config.no_default_ignore,
        );
        if let Some(consumer_root) = consumer_root {
            crate::analysis::annotate_package_consumers(
                &mut report,
                &mut importers,
                &project.root,
                consumer_root,
            );
        }
        crate::analysis::annotate_unused_public(
            &mut report,
            &importers,
            project.config.no_default_ignore,
        );
    }
    let unused_public = report
        .unused_public
        .iter()
        .map(|unused| unused.id.clone())
        .collect();
    Ok((report, unused_public))
}

fn compare(
    baseline_path: &Path,
    path: &Path,
    workspace: bool,
    exact: bool,
    config_path: Option<&Path>,
) -> Result<i32> {
    let baseline_content = std::fs::read_to_string(baseline_path)
        .with_context(|| format!("Could not read {}", baseline_path.display()))?;
    let baseline = parse_baseline(&baseline_content, baseline_path)?;
    let current = create_baseline(
        path,
        workspace || baseline.workspace,
        false,
        None,
        config_path,
    )?
    .baseline;

    let baseline_symbols = symbols_by_stable_key(&baseline)?;
    let current_symbols = symbols_by_stable_key(&current)?;
    let added = current_symbols
        .iter()
        .filter(|(key, _)| !baseline_symbols.contains_key(*key))
        .collect::<Vec<_>>();
    let removed = baseline_symbols
        .iter()
        .filter(|(key, _)| !current_symbols.contains_key(*key))
        .collect::<Vec<_>>();
    let changed = current_symbols
        .iter()
        .filter_map(|(key, symbol)| {
            let previous = baseline_symbols.get(key)?;
            (previous.contracts != symbol.contracts).then_some((key, *previous, *symbol))
        })
        .collect::<Vec<_>>();

    let baseline_exports = export_keys(&baseline);
    let current_exports = export_keys(&current);
    let added_exports = current_exports
        .difference(&baseline_exports)
        .collect::<Vec<_>>();
    let removed_exports = baseline_exports
        .difference(&current_exports)
        .collect::<Vec<_>>();

    println!("\n{}", " CodeAtlas Diff ".on_blue().white().bold());
    println!("{}\n", "================".blue());

    if added.is_empty()
        && removed.is_empty()
        && changed.is_empty()
        && added_exports.is_empty()
        && removed_exports.is_empty()
    {
        println!("{} No public API changes detected.\n", "OK".green().bold());
        println!("  Baseline: {} symbols", baseline_symbols.len());
        println!("  Current:  {} symbols", current_symbols.len());
        return Ok(0);
    }

    print_export_changes(&added_exports, &removed_exports);
    if !added.is_empty() {
        println!(
            "{} {} ADDITIVE public symbol(s):\n",
            "+".green().bold(),
            added.len()
        );
        for (identity, _) in &added {
            println!("  {} {}", "+".green(), display_key(identity).yellow());
        }
        println!();
    }
    if !removed.is_empty() {
        println!(
            "{} {} BREAKING removed public symbol(s):\n",
            "-".red().bold(),
            removed.len()
        );
        for (identity, _) in &removed {
            println!("  {} {}", "-".red(), display_key(identity).yellow());
        }
        println!();
    }
    if !changed.is_empty() {
        println!(
            "{} {} BREAKING changed public symbol(s):\n",
            "~".yellow().bold(),
            changed.len()
        );
        for (identity, previous, current) in &changed {
            println!("  {} {}", "~".yellow(), display_key(identity).yellow());
            println!("    - {}", previous.contracts.join(" | ").red());
            println!("    + {}", current.contracts.join(" | ").green());
        }
        println!();
    }

    let breaking_changes = removed.len() + removed_exports.len() + changed.len();
    let additive_changes = added.len() + added_exports.len();
    println!("{}", "-".repeat(50).dimmed());
    println!("\n{}", "Summary:".white().bold());
    println!("  Baseline: {} symbols", baseline_symbols.len());
    println!("  Current:  {} symbols", current_symbols.len());
    println!("  Additive: {}", format!("+{}", additive_changes).green());
    println!("  Breaking: {}", format!("!{}", breaking_changes).red());
    println!("  Policy:   {}", if exact { "exact" } else { "breaking" });
    Ok(i32::from(
        breaking_changes > 0 || exact && additive_changes > 0,
    ))
}

fn parse_baseline(content: &str, path: &Path) -> Result<PublicApiBaseline> {
    let value: serde_json::Value = serde_json::from_str(content)
        .with_context(|| format!("Invalid baseline JSON at {}", path.display()))?;
    if value.get("format").and_then(serde_json::Value::as_str) == Some(BASELINE_FORMAT) {
        let baseline: PublicApiBaseline = serde_json::from_value(value)
            .with_context(|| format!("Invalid public API baseline at {}", path.display()))?;
        if baseline.schema_version != BASELINE_SCHEMA_VERSION {
            anyhow::bail!(
                "Unsupported public API baseline schema {} at {}",
                baseline.schema_version,
                path.display()
            );
        }
        return Ok(baseline);
    }

    let report: ScanReport = serde_json::from_value(value).with_context(|| {
        format!(
            "Baseline at {} is neither a CodeAtlas public API baseline nor a released scan report",
            path.display()
        )
    })?;
    baseline_from_reports(vec![(".".to_string(), report)], false)
}

fn baseline_from_reports(
    reports: Vec<(String, ScanReport)>,
    workspace: bool,
) -> Result<PublicApiBaseline> {
    let mut packages = BTreeMap::<String, PublicApiPackage>::new();
    let mut symbol_maps = BTreeMap::<String, BTreeMap<SymbolIdentity, PublicApiSymbol>>::new();

    for (root, report) in reports {
        let default_package = report
            .package
            .as_ref()
            .map(|package| package.name.clone())
            .unwrap_or_else(|| "public-api".to_string());
        let default_version = report
            .package
            .as_ref()
            .and_then(|package| package.version.clone());
        let default_exports = report
            .package
            .as_ref()
            .map(public_export_paths)
            .unwrap_or_default();
        packages
            .entry(default_package.clone())
            .or_insert_with(|| PublicApiPackage {
                name: default_package.clone(),
                root: root.clone(),
                version: default_version,
                exports: default_exports,
                symbols: Vec::new(),
            });

        for symbol in &report.symbols {
            collect_symbol(
                symbol,
                &default_package,
                report.package.as_ref(),
                &root,
                &[],
                &mut packages,
                &mut symbol_maps,
            )?;
        }
    }

    for (package_name, symbols) in symbol_maps {
        let package = packages
            .get_mut(&package_name)
            .expect("symbol package should be initialized");
        package.symbols = symbols.into_values().collect();
    }
    let mut packages = packages.into_values().collect::<Vec<_>>();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    for package in &mut packages {
        package.exports.sort();
        package.exports.dedup();
    }
    Ok(PublicApiBaseline {
        format: BASELINE_FORMAT.to_string(),
        schema_version: BASELINE_SCHEMA_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        workspace,
        packages,
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_symbol(
    symbol: &Symbol,
    default_package: &str,
    package_info: Option<&PackageInfo>,
    root: &str,
    inherited_exports: &[String],
    packages: &mut BTreeMap<String, PublicApiPackage>,
    symbol_maps: &mut BTreeMap<String, BTreeMap<SymbolIdentity, PublicApiSymbol>>,
) -> Result<()> {
    let package_name = symbol.package.as_deref().unwrap_or(default_package);
    packages
        .entry(package_name.to_string())
        .or_insert_with(|| PublicApiPackage {
            name: package_name.to_string(),
            root: root.to_string(),
            version: None,
            exports: Vec::new(),
            symbols: Vec::new(),
        });

    let exports = if symbol.export_paths.is_empty() {
        if !inherited_exports.is_empty() {
            inherited_exports.to_vec()
        } else if symbol.referenced {
            vec![SUPPORTING_EXPORT_PATH.to_string()]
        } else if package_info.is_none() {
            vec![ROOT_EXPORT_PATH.to_string()]
        } else {
            Vec::new()
        }
    } else {
        let mut exports = symbol.export_paths.clone();
        exports.sort();
        exports.dedup();
        exports
    };
    let qualified_name = symbol
        .id
        .split_once('#')
        .map_or_else(|| symbol.name.clone(), |(_, name)| name.to_string());

    for export_path in &exports {
        let identity = SymbolIdentity {
            package: package_name.to_string(),
            export_path: export_path.clone(),
            kind: symbol.kind,
            qualified_name: qualified_name.clone(),
        };
        let entry = PublicApiSymbol {
            export_path: export_path.clone(),
            kind: symbol.kind,
            qualified_name: qualified_name.clone(),
            contracts: vec![contract_fingerprint(symbol)],
        };
        let symbols = symbol_maps.entry(package_name.to_string()).or_default();
        if let Some(previous) = symbols.get_mut(&identity) {
            let fingerprint = contract_fingerprint(symbol);
            if !previous.contracts.contains(&fingerprint) {
                previous.contracts.push(fingerprint);
                previous.contracts.sort();
            }
        } else {
            symbols.insert(identity, entry);
        }
    }
    for child in &symbol.children {
        collect_symbol(
            child,
            default_package,
            package_info,
            root,
            &exports,
            packages,
            symbol_maps,
        )?;
    }
    Ok(())
}

fn public_export_paths(package: &PackageInfo) -> Vec<String> {
    package
        .exports
        .iter()
        .map(|export| {
            if export.public_path == "." {
                package.name.clone()
            } else {
                format!(
                    "{}{}",
                    package.name,
                    export.public_path.trim_start_matches('.')
                )
            }
        })
        .collect()
}

fn symbols_by_stable_key(
    baseline: &PublicApiBaseline,
) -> Result<BTreeMap<SymbolIdentity, &PublicApiSymbol>> {
    let mut symbols = BTreeMap::new();
    for package in &baseline.packages {
        for symbol in &package.symbols {
            let identity = SymbolIdentity {
                package: package.name.clone(),
                export_path: symbol.export_path.clone(),
                kind: symbol.kind,
                qualified_name: symbol.qualified_name.clone(),
            };
            if symbols.insert(identity.clone(), symbol).is_some() {
                anyhow::bail!(
                    "Baseline contains duplicate public symbol identity {}",
                    display_key(&identity)
                );
            }
        }
    }
    Ok(symbols)
}

fn export_keys(baseline: &PublicApiBaseline) -> BTreeSet<(String, String)> {
    baseline
        .packages
        .iter()
        .flat_map(|package| {
            package
                .exports
                .iter()
                .map(|export| (package.name.clone(), export.clone()))
        })
        .collect()
}

fn print_export_changes(added: &[&(String, String)], removed: &[&(String, String)]) {
    if !added.is_empty() {
        println!(
            "{} {} ADDITIVE package export(s):\n",
            "+".green().bold(),
            added.len()
        );
        for (package, export) in added {
            println!("  {} {package} ({export})", "+".green());
        }
        println!();
    }
    if !removed.is_empty() {
        println!(
            "{} {} BREAKING removed package export(s):\n",
            "-".red().bold(),
            removed.len()
        );
        for (package, export) in removed {
            println!("  {} {package} ({export})", "-".red());
        }
        println!();
    }
}

fn display_key(identity: &SymbolIdentity) -> String {
    format!(
        "{}::{}::{:?}#{}",
        identity.package, identity.export_path, identity.kind, identity.qualified_name
    )
}

fn contract_fingerprint(symbol: &Symbol) -> String {
    let mut digest = Sha256::new();
    digest.update(format!("{:?}\0", symbol.visibility));
    digest.update(symbol.signature.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::{
        baseline_from_reports, symbols_by_stable_key, ROOT_EXPORT_PATH, SUPPORTING_EXPORT_PATH,
    };
    use crate::domain::{
        Language, PackageExport, PackageInfo, ScanReport, Symbol, SymbolKind, Visibility,
    };

    fn symbol(file: &str, signature: &str, exports: &[&str]) -> Symbol {
        Symbol {
            id: format!("ts:{file}:interface#PublicAPI"),
            name: "PublicAPI".to_string(),
            kind: SymbolKind::Interface,
            visibility: Visibility::Public,
            language: Language::TypeScript,
            file_path: file.to_string(),
            span: None,
            signature: signature.to_string(),
            docs: None,
            export_paths: exports.iter().map(|value| (*value).to_string()).collect(),
            referenced: false,
            package: Some("@example/sdk".to_string()),
            children: Vec::new(),
        }
    }

    #[test]
    fn identity_uses_package_export_kind_and_qualified_name() {
        let report = ScanReport {
            package: Some(PackageInfo {
                name: "@example/sdk".to_string(),
                version: Some("1.0.0".to_string()),
                exports: vec![
                    PackageExport {
                        public_path: ".".to_string(),
                        source_path: "src/index.ts".to_string(),
                    },
                    PackageExport {
                        public_path: "./admin".to_string(),
                        source_path: "src/admin.ts".to_string(),
                    },
                ],
            }),
            symbols: vec![symbol(
                "src/index.ts",
                "interface PublicAPI",
                &["@example/sdk", "@example/sdk/admin"],
            )],
            ..ScanReport::default()
        };
        let baseline = baseline_from_reports(vec![("packages/sdk".to_string(), report)], true)
            .expect("baseline");
        let symbols = symbols_by_stable_key(&baseline).expect("unique symbols");
        assert_eq!(symbols.len(), 2);
        assert!(symbols.keys().any(|key| key.export_path == "@example/sdk"));
        assert!(symbols
            .keys()
            .any(|key| key.export_path == "@example/sdk/admin"));
    }

    #[test]
    fn identity_ignores_source_file_moves() {
        let left = ScanReport {
            symbols: vec![symbol("src/old.ts", "interface PublicAPI", &[])],
            ..ScanReport::default()
        };
        let right = ScanReport {
            symbols: vec![symbol("dist/index.d.ts", "interface PublicAPI", &[])],
            ..ScanReport::default()
        };
        let left = baseline_from_reports(vec![(".".to_string(), left)], false).expect("left");
        let right = baseline_from_reports(vec![(".".to_string(), right)], false).expect("right");
        assert_eq!(
            symbols_by_stable_key(&left)
                .expect("left symbols")
                .into_keys()
                .collect::<Vec<_>>(),
            symbols_by_stable_key(&right)
                .expect("right symbols")
                .into_keys()
                .collect::<Vec<_>>()
        );
        assert_eq!(left.packages[0].symbols[0].export_path, ROOT_EXPORT_PATH);
    }

    #[test]
    fn referenced_contracts_are_retained_without_becoming_importable_exports() {
        let mut referenced = symbol("src/index.ts", "interface Detail", &[]);
        referenced.referenced = true;
        let report = ScanReport {
            package: Some(PackageInfo {
                name: "@example/sdk".to_string(),
                version: None,
                exports: vec![PackageExport {
                    public_path: ".".to_string(),
                    source_path: "src/index.ts".to_string(),
                }],
            }),
            symbols: vec![referenced],
            ..ScanReport::default()
        };
        let baseline = baseline_from_reports(vec![("packages/sdk".to_string(), report)], false)
            .expect("baseline");
        assert_eq!(
            baseline.packages[0].symbols[0].export_path,
            SUPPORTING_EXPORT_PATH
        );
    }
}
