use crate::analysis::imports::UsageAnalysis;
use codeatlas_domain::{ScanReport, UnusedPublic, Visibility};
use codeatlas_source::source_policy;

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
        if source_policy::is_ignored_path(&symbol.file_path, no_default_ignore) {
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

fn suggestion_for(language: codeatlas_domain::Language) -> String {
    match language {
        codeatlas_domain::Language::Rust => "pub(crate)".to_string(),
        codeatlas_domain::Language::TypeScript => "remove export".to_string(),
        codeatlas_domain::Language::Python => "prefix with _".to_string(),
        _ => "internal".to_string(),
    }
}
