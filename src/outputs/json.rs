use anyhow::Result;
use codeatlas_domain::{ScanReport, Symbol};

pub(crate) fn render(report: &ScanReport) -> Result<String> {
    let mut report = report.clone();
    canonicalize(&mut report);
    Ok(serde_json::to_string_pretty(&report)?)
}

fn canonicalize(report: &mut ScanReport) {
    sort_symbols(&mut report.symbols);
    report
        .skipped_files
        .sort_by(|left, right| left.path.cmp(&right.path));
    for usage in &mut report.imports {
        usage.importers.sort();
        usage.importers.dedup();
    }
    report.imports.sort_by(|left, right| left.id.cmp(&right.id));
    report
        .unused_public
        .sort_by(|left, right| left.id.cmp(&right.id));
    report.file_edges.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
    });
    if let Some(package) = &mut report.package {
        package.exports.sort_by(|left, right| {
            left.public_path
                .cmp(&right.public_path)
                .then_with(|| left.source_path.cmp(&right.source_path))
        });
    }
}

fn sort_symbols(symbols: &mut [Symbol]) {
    for symbol in symbols.iter_mut() {
        symbol.export_paths.sort();
        symbol.export_paths.dedup();
        sort_symbols(&mut symbol.children);
    }
    symbols.sort_by(|left, right| left.id.cmp(&right.id));
}
