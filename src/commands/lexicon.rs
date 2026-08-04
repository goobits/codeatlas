use super::{annotate_report, build_scan_config, exit_code, load_project, scan_project};
use crate::config::ProjectConfig;
use crate::domain::{ScanReport, Symbol};
use crate::{lexicon, outputs};
use anyhow::{Context, Result};
use clap::ValueEnum;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum LexiconFormat {
    #[default]
    Text,
    Json,
}

pub(crate) fn run(
    path: &Path,
    workspace: bool,
    format: LexiconFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(run_inner(path, workspace, format, out, config_path))
}

fn run_inner(
    path: &Path,
    workspace: bool,
    format: LexiconFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let policy = lexicon::load_concept_policy(&project.config.lexicon, project.config_base())?;
    let scan = if workspace {
        scan_workspace(&project)?
    } else {
        scan_source(&project)?
    };
    let report = lexicon::analyze(&scan, &policy);
    let rendered = match format {
        LexiconFormat::Text => outputs::lexicon::render_text(&report),
        LexiconFormat::Json => outputs::lexicon::render_json(&report)?,
    };
    super::output::write_text_or_print(&rendered, out, "Lexicon report")?;
    Ok(0)
}

fn scan_source(project: &ProjectConfig) -> Result<ScanReport> {
    let mut config = build_scan_config(project, true, None)?;
    config.entrypoints = None;
    let mut scan = scan_project(project, &config)?;
    annotate_report(&mut scan, project)?;
    Ok(scan)
}

fn scan_workspace(project: &ProjectConfig) -> Result<ScanReport> {
    let workspace = crate::package::discover_workspace(&project.root)?;
    let mut source_project = project.clone();
    source_project.config.languages.clear();
    let mut config = build_scan_config(&source_project, true, None)?;
    config.entrypoints = None;
    let mut scan = scan_project(&source_project, &config)?;

    let mut members = workspace
        .members
        .into_iter()
        .filter_map(|member| {
            let prefix = crate::paths::normalize_relative_path(&member.root, &project.root);
            (!prefix.is_empty()).then_some((prefix, member.name, member.root))
        })
        .collect::<Vec<_>>();
    members.sort_by(|left, right| {
        right
            .0
            .split('/')
            .count()
            .cmp(&left.0.split('/').count())
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut member_symbols = BTreeMap::<String, Vec<Symbol>>::new();
    let mut root_symbols = Vec::new();
    for mut symbol in std::mem::take(&mut scan.symbols) {
        let owner = members
            .iter()
            .find(|(prefix, _, _)| Path::new(&symbol.file_path).strip_prefix(prefix).is_ok());
        if let Some((prefix, _, _)) = owner {
            strip_symbol_prefix(&mut symbol, prefix)?;
            member_symbols
                .entry(prefix.clone())
                .or_default()
                .push(symbol);
        } else {
            root_symbols.push(symbol);
        }
    }

    scan.symbols = root_symbols;
    annotate_report(&mut scan, project)?;

    for (prefix, member_name, member_root) in members {
        let Some(symbols) = member_symbols.remove(&prefix) else {
            continue;
        };
        let local_config = member_root.join("codeatlas.json");
        let member_project = load_project(
            &member_root,
            local_config.is_file().then_some(local_config.as_path()),
        )?;
        let mut member_scan = ScanReport {
            symbols,
            ..ScanReport::default()
        };
        annotate_report(&mut member_scan, &member_project)
            .with_context(|| format!("Could not annotate workspace package {member_name}"))?;
        for mut symbol in member_scan.symbols {
            add_symbol_prefix(&mut symbol, &prefix);
            scan.symbols.push(symbol);
        }
    }
    scan.symbols.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(scan)
}

fn strip_symbol_prefix(symbol: &mut Symbol, prefix: &str) -> Result<()> {
    let relative = Path::new(&symbol.file_path)
        .strip_prefix(prefix)
        .with_context(|| {
            format!(
                "Symbol path {} does not belong to workspace package {}",
                symbol.file_path, prefix
            )
        })?;
    let relative = crate::paths::normalize_path(relative);
    rewrite_symbol_path(symbol, &relative);
    for child in &mut symbol.children {
        strip_symbol_prefix(child, prefix)?;
    }
    Ok(())
}

fn add_symbol_prefix(symbol: &mut Symbol, prefix: &str) {
    let rebased = crate::paths::normalize_path(&Path::new(prefix).join(&symbol.file_path));
    rewrite_symbol_path(symbol, &rebased);
    for child in &mut symbol.children {
        add_symbol_prefix(child, prefix);
    }
}

fn rewrite_symbol_path(symbol: &mut Symbol, path: &str) {
    let previous = std::mem::replace(&mut symbol.file_path, path.to_string());
    let marker = format!(":{previous}:");
    let replacement = format!(":{path}:");
    symbol.id = symbol.id.replacen(&marker, &replacement, 1);
}
