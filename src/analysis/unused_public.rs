use crate::analysis::imports::Importers;
use crate::analysis::ignore;
use crate::domain::{ScanReport, UnusedPublic, Visibility};

pub(crate) fn compute(
    report: &ScanReport,
    importers: &Importers,
    no_default_ignore: bool,
) -> Vec<UnusedPublic> {
    let mut unused = Vec::new();

    for symbol in &report.symbols {
        if symbol.visibility != Visibility::Public {
            continue;
        }
        if ignore::is_ignored_path(&symbol.file_path, no_default_ignore) {
            continue;
        }
        if importers.get(&symbol.id).map_or(true, |files| files.is_empty()) {
            unused.push(UnusedPublic {
                id: symbol.id.clone(),
                suggestion: suggestion_for(symbol.language),
            });
        }
    }

    unused
}

fn suggestion_for(language: crate::domain::Language) -> String {
    match language {
        crate::domain::Language::Rust => "pub(crate)".to_string(),
        crate::domain::Language::TypeScript => "remove export".to_string(),
        crate::domain::Language::Python => "prefix with _".to_string(),
        _ => "internal".to_string(),
    }
}
