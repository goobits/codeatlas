use crate::domain::ScanReport;
use std::path::Path;

mod dependency_types;
pub(crate) mod docs;
pub(crate) mod ignore;
mod imports;
mod unused_public;

pub(crate) fn build_importers(
    report: &ScanReport,
    root_dir: &Path,
    no_default_ignore: bool,
) -> imports::Importers {
    let (importers, _file_edges) = imports::build_importers(report, root_dir, no_default_ignore);
    importers
}

pub(crate) fn annotate_imports(
    report: &mut ScanReport,
    root_dir: &Path,
    no_default_ignore: bool,
) -> imports::Importers {
    let (importers, file_edges) = imports::build_importers(report, root_dir, no_default_ignore);
    report.imports = imports::to_import_usage(&importers);
    report.file_edges = file_edges.into_iter().collect();
    importers
}

pub(crate) fn annotate_unused_public(
    report: &mut ScanReport,
    importers: &imports::Importers,
    no_default_ignore: bool,
) {
    report.unused_public = unused_public::compute(report, importers, no_default_ignore);
}

pub(crate) use dependency_types::annotate_dependency_types;
pub(crate) use docs::annotate_docs;
