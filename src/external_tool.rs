use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn resolve(
    explicit: Option<&Path>,
    environment: &str,
    fallback: &str,
    label: &str,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return existing(path, &format!("--{}", label.to_ascii_lowercase()));
    }
    if let Some(path) = std::env::var_os(environment) {
        return existing(Path::new(&path), environment);
    }
    Ok(PathBuf::from(fallback))
}

fn existing(path: &Path, source: &str) -> Result<PathBuf> {
    if path.components().count() == 1
        && path
            .parent()
            .is_some_and(|parent| parent.as_os_str().is_empty())
    {
        return Ok(path.to_path_buf());
    }
    let path = path.canonicalize().with_context(|| {
        format!(
            "External tool from {source} does not exist: {}",
            path.display()
        )
    })?;
    if !path.is_file() {
        anyhow::bail!(
            "External tool from {source} is not a file: {}",
            path.display()
        );
    }
    Ok(path)
}

pub(crate) fn command(executable: &Path) -> Command {
    if executable.extension() == Some(OsStr::new("js")) {
        let mut command = Command::new("node");
        command.arg(executable);
        command
    } else {
        Command::new(executable)
    }
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use std::path::Path;

    #[test]
    fn bare_tool_names_remain_path_resolved() {
        assert_eq!(
            resolve(
                Some(Path::new("tool-name")),
                "UNUSED_TOOL_ENV",
                "fallback",
                "Tool"
            )
            .expect("bare executable"),
            Path::new("tool-name")
        );
    }
}
