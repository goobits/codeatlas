use crate::domain::ScanReport;
use std::path::Path;

mod imports;
mod unused_public;

pub fn build_importers(report: &ScanReport, root_dir: &Path) -> imports::Importers {
    imports::build_importers(report, root_dir)
}

pub fn annotate_imports(report: &mut ScanReport, root_dir: &Path) -> imports::Importers {
    let importers = build_importers(report, root_dir);
    report.imports = imports::to_import_usage(&importers);
    importers
}

pub fn annotate_unused_public(report: &mut ScanReport, importers: &imports::Importers) {
    report.unused_public = unused_public::compute(report, importers);
}
