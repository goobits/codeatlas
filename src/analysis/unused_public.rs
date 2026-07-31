use crate::analysis::imports::UsageAnalysis;
use crate::domain::{ScanReport, UnusedPublic, Visibility};
use crate::source_discovery;

pub(crate) fn compute(
    report: &ScanReport,
    usage: &UsageAnalysis,
    no_default_ignore: bool,
) -> Vec<UnusedPublic> {
    let mut unused = Vec::new();

    for symbol in &report.symbols {
        if symbol.visibility != Visibility::Public {
            continue;
        }
        if source_discovery::is_ignored_path(&symbol.file_path, no_default_ignore) {
            continue;
        }
        if !usage.is_referenced(&symbol.id) {
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
