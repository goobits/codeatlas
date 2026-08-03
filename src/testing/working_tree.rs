use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn paths(repository_root: &Path) -> Result<Vec<PathBuf>> {
    let repository_root = repository_root.canonicalize().with_context(|| {
        format!(
            "Failed to resolve analysis root {}",
            repository_root.display()
        )
    })?;
    let git_root = git_root(&repository_root)?;
    let scope = repository_root.strip_prefix(&git_root).with_context(|| {
        format!(
            "Analysis root {} is outside Git repository {}",
            repository_root.display(),
            git_root.display()
        )
    })?;
    let mut repository_paths = BTreeSet::new();
    collect(
        &git_root,
        &["diff", "--name-only", "--no-renames", "-z", "--"],
        &mut repository_paths,
    )?;
    collect(
        &git_root,
        &[
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
            "-z",
            "--",
        ],
        &mut repository_paths,
    )?;
    collect(
        &git_root,
        &["ls-files", "--others", "--exclude-standard", "-z", "--"],
        &mut repository_paths,
    )?;
    let paths = repository_paths
        .into_iter()
        .filter_map(|path| path.strip_prefix(scope).ok().map(Path::to_path_buf))
        .collect::<Vec<_>>();
    if paths.is_empty() {
        anyhow::bail!(
            "Git working tree has no staged, unstaged, or untracked paths under {}",
            repository_root.display()
        );
    }
    Ok(paths)
}

fn git_root(repository_root: &Path) -> Result<PathBuf> {
    let output = run_git(repository_root, &["rev-parse", "--show-toplevel"])?;
    let path = std::str::from_utf8(&output.stdout)
        .context("Git returned a non-UTF-8 repository root")?
        .trim();
    PathBuf::from(path)
        .canonicalize()
        .with_context(|| format!("Failed to resolve Git repository root {path}"))
}

fn collect(repository_root: &Path, args: &[&str], paths: &mut BTreeSet<PathBuf>) -> Result<()> {
    let output = run_git(repository_root, args)?;
    for bytes in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(bytes).with_context(|| {
            format!(
                "Git returned a non-UTF-8 path under {}",
                repository_root.display()
            )
        })?;
        paths.insert(PathBuf::from(path));
    }
    Ok(())
}

fn run_git(repository_root: &Path, args: &[&str]) -> Result<std::process::Output> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository_root)
        .args(args)
        .output()
        .with_context(|| {
            format!(
                "Failed to run Git while reading {}",
                repository_root.display()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "Git could not read working-tree paths from {}: {}",
            repository_root.display(),
            if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            }
        );
    }
    Ok(output)
}
