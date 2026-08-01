use crate::domain::{FileEdge, ImportUsage, Language, ScanReport, Symbol, Visibility};
use std::collections::{HashMap, HashSet};
use std::path::Path;

mod python;
mod rust;
mod typescript;

pub(crate) type Importers = HashMap<String, HashSet<String>>;
pub(crate) type FileEdges = HashSet<FileEdge>;

pub(crate) struct UsageAnalysis {
    importers: Importers,
    dynamic_references: HashSet<String>,
}

impl UsageAnalysis {
    pub(crate) fn is_referenced(&self, symbol_id: &str) -> bool {
        self.dynamic_references.contains(symbol_id)
            || self
                .importers
                .get(symbol_id)
                .is_some_and(|files| !files.is_empty())
    }
}

pub(crate) fn build_importers(
    report: &ScanReport,
    root_dir: &Path,
    no_default_ignore: bool,
) -> (UsageAnalysis, FileEdges) {
    let mut importers = HashMap::new();
    let mut dynamic_references = HashSet::new();
    let mut file_edges = HashSet::new();

    let public_symbols: Vec<&Symbol> = report
        .symbols
        .iter()
        .filter(|symbol| symbol.visibility == Visibility::Public)
        .collect();

    let symbol_index = build_symbol_index(&public_symbols);

    python::collect_importers(
        root_dir,
        &symbol_index,
        &mut importers,
        &mut dynamic_references,
        &mut file_edges,
        no_default_ignore,
    );
    rust::collect_importers(
        root_dir,
        &symbol_index,
        &mut importers,
        &mut file_edges,
        no_default_ignore,
    );
    typescript::collect_importers(
        root_dir,
        &symbol_index,
        &mut importers,
        &mut file_edges,
        no_default_ignore,
    );

    (
        UsageAnalysis {
            importers,
            dynamic_references,
        },
        file_edges,
    )
}

pub(crate) fn collect_package_consumers(
    report: &ScanReport,
    usage: &mut UsageAnalysis,
    consumer_root: &Path,
    no_default_ignore: bool,
) {
    typescript::collect_package_consumers(
        report,
        consumer_root,
        &mut usage.importers,
        no_default_ignore,
    );
}

/// Add a file-to-file dependency edge
pub(crate) fn add_file_edge(file_edges: &mut FileEdges, from: &str, to: &str) {
    if from != to {
        file_edges.insert(FileEdge {
            from: from.to_string(),
            to: to.to_string(),
        });
    }
}

fn build_symbol_index(
    symbols: &[&Symbol],
) -> HashMap<Language, HashMap<String, HashMap<String, String>>> {
    let mut index: HashMap<Language, HashMap<String, HashMap<String, String>>> = HashMap::new();
    for symbol in symbols {
        let by_lang = index.entry(symbol.language).or_default();
        let by_file = by_lang.entry(symbol.file_path.clone()).or_default();
        by_file
            .entry(symbol.name.clone())
            .or_insert_with(|| symbol.id.clone());
    }
    index
}

pub(crate) fn add_importer(importers: &mut Importers, symbol_id: &str, importer: &str) {
    importers
        .entry(symbol_id.to_string())
        .or_default()
        .insert(importer.to_string());
}

pub(crate) fn to_import_usage(analysis: &UsageAnalysis) -> Vec<ImportUsage> {
    let mut usage = Vec::new();
    for (id, files) in &analysis.importers {
        let mut importers: Vec<String> = files.iter().cloned().collect();
        importers.sort();
        usage.push(ImportUsage {
            id: id.clone(),
            importers,
        });
    }
    usage.sort_by(|a, b| a.id.cmp(&b.id));
    usage
}
