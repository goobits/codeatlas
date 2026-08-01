use crate::domain::ScanReport;
use std::path::Path;

mod dependency_types;
pub(crate) mod docs;
mod imports;
mod package_exports;
pub(crate) mod reachability;
mod unused_public;

pub(crate) fn annotate_imports(
    report: &mut ScanReport,
    root_dir: &Path,
    no_default_ignore: bool,
) -> imports::UsageAnalysis {
    let (usage, file_edges) = imports::build_importers(report, root_dir, no_default_ignore);
    report.imports = imports::to_import_usage(&usage);
    report.file_edges = file_edges.into_iter().collect();
    usage
}

pub(crate) fn annotate_unused_public(
    report: &mut ScanReport,
    usage: &imports::UsageAnalysis,
    no_default_ignore: bool,
) {
    report.unused_public = unused_public::compute(report, usage, no_default_ignore);
}

pub(crate) fn annotate_package_consumers(
    report: &mut ScanReport,
    usage: &mut imports::UsageAnalysis,
    consumer_root: &Path,
    no_default_ignore: bool,
) {
    imports::collect_package_consumers(report, usage, consumer_root, no_default_ignore);
    report.imports = imports::to_import_usage(usage);
}

pub(crate) use dependency_types::annotate_dependency_types;
pub(crate) use docs::annotate_docs;
pub(crate) use package_exports::{
    annotate as annotate_package_exports, consolidate_declaration_symbols,
};
