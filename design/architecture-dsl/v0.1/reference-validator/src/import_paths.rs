use crate::ValidationError;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub fn resolve_local_import(
    allowed_root: &Path,
    importing_document: &Path,
    source: &str,
) -> Result<PathBuf, ValidationError> {
    if source.contains("://") {
        return Err(ValidationError::new(
            "import.network-source-prohibited",
            "network import sources are prohibited",
        ));
    }
    let source_path = Path::new(source);
    if source_path
        .components()
        .any(|component| matches!(component, Component::Prefix(_)))
    {
        return Err(ValidationError::new(
            "import.platform-prefix-prohibited",
            "platform-prefixed import sources are prohibited",
        ));
    }

    let canonical_root = fs::canonicalize(allowed_root).map_err(|error| {
        ValidationError::new(
            "import.root-unavailable",
            format!("{}: {error}", allowed_root.display()),
        )
    })?;
    let parent = importing_document.parent().ok_or_else(|| {
        ValidationError::new(
            "import.importer-parent-missing",
            "importing document has no parent directory",
        )
    })?;
    let candidate = if source_path.is_absolute() {
        source_path.to_path_buf()
    } else {
        parent.join(source_path)
    };
    let canonical_candidate = fs::canonicalize(&candidate).map_err(|error| {
        ValidationError::new(
            "import.source-unavailable",
            format!("{}: {error}", candidate.display()),
        )
    })?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(ValidationError::new(
            "import.path-escape",
            format!(
                "{} resolves outside {}",
                candidate.display(),
                canonical_root.display()
            ),
        ));
    }
    Ok(canonical_candidate)
}

#[cfg(test)]
mod tests {
    use super::resolve_local_import;
    use std::fs;
    use std::path::PathBuf;

    fn fixture_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "codeatlas-dsl-import-paths-{}-{name}",
            std::process::id()
        ))
    }

    fn remove_existing(root: &PathBuf) {
        if root.is_dir() {
            fs::remove_dir_all(root).expect("remove stale fixture directory");
        } else if root.exists() {
            fs::remove_file(root).expect("remove stale fixture file");
        }
    }

    #[test]
    fn local_imports_remain_inside_the_allowed_root() {
        let root = fixture_root("local");
        remove_existing(&root);
        let modules = root.join("modules");
        fs::create_dir_all(&modules).expect("create fixture");
        let importer = modules.join("root.atlas.yaml");
        let imported = root.join("base.atlas.yaml");
        fs::write(&importer, b"root").expect("write importer");
        fs::write(&imported, b"base").expect("write import");

        assert_eq!(
            resolve_local_import(&root, &importer, "../base.atlas.yaml").expect("resolve"),
            fs::canonicalize(&imported).expect("canonical import")
        );
        fs::remove_dir_all(&root).expect("clean fixture");
    }

    #[test]
    fn traversal_and_network_sources_are_rejected() {
        let root = fixture_root("traversal");
        let modules = root.join("modules");
        let outside = root.with_extension("outside.yaml");
        remove_existing(&root);
        remove_existing(&outside);
        fs::create_dir_all(&modules).expect("create fixture");
        let importer = modules.join("root.atlas.yaml");
        fs::write(&importer, b"root").expect("write importer");
        fs::write(&outside, b"outside").expect("write outside");

        let outside_name = outside
            .file_name()
            .expect("outside file name")
            .to_string_lossy();
        let traversal_source = format!("../../{outside_name}");
        let traversal =
            resolve_local_import(&root, &importer, &traversal_source).expect_err("path traversal");
        assert_eq!(traversal.diagnostic.code, "import.path-escape");
        assert_eq!(
            resolve_local_import(&root, &importer, "https://example.com/module")
                .expect_err("network")
                .diagnostic
                .code,
            "import.network-source-prohibited"
        );
        fs::remove_dir_all(&root).expect("clean fixture");
        fs::remove_file(&outside).expect("clean outside fixture");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = fixture_root("symlink");
        let modules = root.join("modules");
        let outside = root.with_extension("outside.yaml");
        remove_existing(&root);
        remove_existing(&outside);
        fs::create_dir_all(&modules).expect("create fixture");
        let importer = modules.join("root.atlas.yaml");
        let link = modules.join("escaped.atlas.yaml");
        fs::write(&importer, b"root").expect("write importer");
        fs::write(&outside, b"outside").expect("write outside");
        symlink(&outside, &link).expect("create symlink");

        let error = resolve_local_import(&root, &importer, "escaped.atlas.yaml")
            .expect_err("symlink escape");
        assert_eq!(error.diagnostic.code, "import.path-escape");
        fs::remove_dir_all(&root).expect("clean fixture");
        fs::remove_file(&outside).expect("clean outside fixture");
    }
}
