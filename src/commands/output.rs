use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;

pub(super) fn render_json(value: &impl Serialize) -> anyhow::Result<String> {
    let mut rendered = serde_json::to_string_pretty(value)?;
    rendered.push('\n');
    Ok(rendered)
}

pub(super) fn read_json<T: DeserializeOwned>(path: &Path, label: &str) -> anyhow::Result<T> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Could not read {label} {}", path.display()))?;
    serde_json::from_str(&source)
        .with_context(|| format!("Invalid {label} JSON at {}", path.display()))
}

pub(super) fn write_file(path: &Path, content: &str) -> anyhow::Result<()> {
    crate::filesystem::replace_file(path, content)
}

pub(super) fn write_text_or_print(
    content: &str,
    out: Option<&Path>,
    label: &str,
) -> anyhow::Result<()> {
    if let Some(path) = out {
        write_file(path, content)?;
        eprintln!("{label} written to {}", path.display());
    } else {
        print!("{content}");
        if !content.ends_with('\n') {
            println!();
        }
    }
    Ok(())
}

pub(super) fn write_or_print(
    value: &impl Serialize,
    out: Option<&Path>,
    label: &str,
) -> anyhow::Result<()> {
    write_text_or_print(&render_json(value)?, out, label)
}
