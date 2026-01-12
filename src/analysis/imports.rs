use crate::domain::{ImportUsage, Language, ScanReport, Symbol, Visibility};
use std::collections::{HashMap, HashSet};
use std::path::Path;

mod python;
mod rust;
mod typescript;

pub type Importers = HashMap<String, HashSet<String>>;

pub fn build_importers(report: &ScanReport, root_dir: &Path) -> Importers {
    let mut importers = HashMap::new();

    let public_symbols: Vec<&Symbol> = report
        .symbols
        .iter()
        .filter(|symbol| symbol.visibility == Visibility::Public)
        .collect();

    let symbol_index = build_symbol_index(&public_symbols);

    python::collect_importers(root_dir, &symbol_index, &mut importers);
    rust::collect_importers(root_dir, &symbol_index, &mut importers);
    typescript::collect_importers(root_dir, &symbol_index, &mut importers);

    importers
}

fn build_symbol_index(
    symbols: &[&Symbol],
) -> HashMap<Language, HashMap<String, HashMap<String, String>>> {
    let mut index: HashMap<Language, HashMap<String, HashMap<String, String>>> = HashMap::new();
    for symbol in symbols {
        let by_lang = index.entry(symbol.language).or_default();
        let by_file = by_lang.entry(symbol.file_path.clone()).or_default();
        by_file.entry(symbol.name.clone()).or_insert_with(|| symbol.id.clone());
    }
    index
}

pub fn add_importer(importers: &mut Importers, symbol_id: String, importer: String) {
    importers.entry(symbol_id).or_default().insert(importer);
}

pub fn to_import_usage(importers: &Importers) -> Vec<ImportUsage> {
    let mut usage = Vec::new();
    for (id, files) in importers {
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
