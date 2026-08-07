use super::{exit_code, load_project, output};
use crate::config::{ConfigEdit, ConfigSubject, ProjectConfig};
use anyhow::Result;
use serde_json::{Map, Value};
use std::path::Path;

pub(crate) fn run_code(path: &Path, write: bool, config_path: Option<&Path>) -> i32 {
    exit_code(init_code(path, write, config_path))
}

pub(crate) fn run_http(path: &Path, write: bool, config_path: Option<&Path>) -> i32 {
    exit_code(init_http(path, write, config_path))
}

pub(crate) fn run_postgres(path: &Path, write: bool, config_path: Option<&Path>) -> i32 {
    exit_code(init_postgres(path, write, config_path))
}

fn init_code(path: &Path, write: bool, config_path: Option<&Path>) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let languages = crate::languages::detect_language_ids(&project.root);
    if languages.is_empty() {
        anyhow::bail!(
            "No supported code languages were discovered in {}",
            project.root.display()
        );
    }
    let mut entrypoints = codeatlas_source::package::discover_runtime_entrypoints(&project.root)?;
    entrypoints.extend(codeatlas_source::package::discover_bundled_entrypoints(
        &project.root,
    )?);
    entrypoints.sort();
    entrypoints.dedup();

    let mut fragment = Map::new();
    fragment.insert("languages".to_string(), serde_json::to_value(languages)?);
    if !entrypoints.is_empty() {
        fragment.insert(
            "entrypoints".to_string(),
            serde_json::to_value(entrypoints)?,
        );
    }
    finish(
        &project,
        ConfigSubject::Code,
        Value::Object(fragment),
        write,
        "Code",
    )
}

fn init_http(path: &Path, write: bool, config_path: Option<&Path>) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let http = crate::http::proposed_config(&project)?;
    finish(
        &project,
        ConfigSubject::Http,
        serde_json::json!({ "http": http }),
        write,
        "HTTP",
    )
}

fn init_postgres(path: &Path, write: bool, config_path: Option<&Path>) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let postgres = crate::postgres::proposed_config(&project)?;
    finish(
        &project,
        ConfigSubject::Postgres,
        serde_json::json!({ "postgres": postgres }),
        write,
        "PostgreSQL",
    )
}

fn finish(
    project: &ProjectConfig,
    subject: ConfigSubject,
    fragment: Value,
    write: bool,
    label: &str,
) -> Result<i32> {
    let edit = ConfigEdit::plan(project, subject, &fragment)?;
    if !write {
        output::write_or_print(edit.fragment(), None, &format!("{label} config"))?;
        return Ok(0);
    }

    let destination = edit.write()?;
    eprintln!("{label} config added to {}", destination.display());
    Ok(0)
}
