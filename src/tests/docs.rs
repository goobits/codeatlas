use crate::config::ProjectConfig;
use crate::{analysis, commands, languages, outputs, package};
use codeatlas_domain::ScanConfig;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs")
}

fn fixture_report(include_private: bool) -> codeatlas_domain::ScanReport {
    let root = fixture_root();
    let config = ScanConfig {
        include_types: true,
        include_private,
        entrypoints: None,
        no_default_ignore: false,
    };
    let mut report = languages::scan_all(&root, &config, languages::get_scanners_auto(&root));
    let package = package::discover(&root)
        .expect("package manifest")
        .expect("package metadata");
    analysis::annotate_package_exports(&mut report, &root, package, false);
    analysis::annotate_docs(&mut report, &root);
    report
}

mod declarations;
mod rendering;
mod signatures;
