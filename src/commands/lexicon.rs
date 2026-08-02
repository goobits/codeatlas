use super::{annotate_report, build_scan_config, exit_code, load_project, scan_project};
use crate::{lexicon, outputs};
use anyhow::Result;
use clap::ValueEnum;
use std::path::Path;

#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum LexiconFormat {
    #[default]
    Text,
    Json,
}

pub(crate) fn run(
    path: &Path,
    format: LexiconFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(run_inner(path, format, out, config_path))
}

fn run_inner(
    path: &Path,
    format: LexiconFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let mut config = build_scan_config(&project, true, None)?;
    config.entrypoints = None;
    let mut scan = scan_project(&project, &config)?;
    annotate_report(&mut scan, &project)?;
    let report = lexicon::analyze(&scan);
    let rendered = match format {
        LexiconFormat::Text => outputs::lexicon::render_text(&report),
        LexiconFormat::Json => outputs::lexicon::render_json(&report)?,
    };
    super::output::write_text_or_print(&rendered, out, "Lexicon report")?;
    Ok(0)
}
