use anyhow::Result;

pub(super) const SCHEMATHESIS_VERSION: &str = "4.24.3";
pub(super) const DEFAULT_SCHEMATHESIS_EXECUTABLE: &str = "/usr/local/bin/schemathesis";
const LOCKED_REQUIREMENTS: &str = include_str!("requirements.txt");

pub(super) fn fingerprint_schemathesis(
    executable: &str,
    workload_image: Option<&str>,
) -> Result<crate::external_tool::ExternalToolFingerprint> {
    validate_container_executable(executable)?;
    crate::external_tool::fingerprint_bytes(
        "schemathesis",
        SCHEMATHESIS_VERSION,
        format!(
            "image={}\nexecutable={executable}\n{LOCKED_REQUIREMENTS}",
            workload_image.unwrap_or("unconfigured")
        )
        .as_bytes(),
    )
}

pub(super) fn container_executable(override_path: Option<&str>) -> Result<String> {
    let executable = override_path
        .map(str::to_string)
        .unwrap_or_else(|| DEFAULT_SCHEMATHESIS_EXECUTABLE.to_string());
    validate_container_executable(&executable)?;
    Ok(executable)
}

pub(super) fn validate_container_executable(executable: &str) -> Result<()> {
    if !executable.starts_with('/')
        || executable.ends_with('/')
        || executable.contains(['\\', '\0', '\n', '\r'])
        || executable
            .strip_prefix('/')
            .expect("absolute path has a leading slash")
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        anyhow::bail!(
            "--schemathesis must be an absolute normalized executable path inside the workload image"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{container_executable, LOCKED_REQUIREMENTS, SCHEMATHESIS_VERSION};

    #[test]
    fn workload_executable_is_an_absolute_normalized_container_path() {
        assert_eq!(
            container_executable(None).expect("default executable"),
            "/usr/local/bin/schemathesis"
        );
        assert_eq!(
            container_executable(Some("/opt/codeatlas/schemathesis"))
                .expect("configured executable"),
            "/opt/codeatlas/schemathesis"
        );
        for invalid in [
            "schemathesis",
            "/usr/local/../bin/schemathesis",
            "/usr//bin/schemathesis",
            "/usr/bin/schemathesis/",
            "C:\\schemathesis.exe",
        ] {
            assert!(container_executable(Some(invalid)).is_err(), "{invalid}");
        }
    }

    #[test]
    fn managed_requirements_are_exact_hash_locked_and_versioned_in_the_cache_key() {
        let requirements = LOCKED_REQUIREMENTS
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with([' ', '#']))
            .collect::<Vec<_>>();

        assert!(requirements
            .iter()
            .all(|line| line.contains("==") && line.ends_with('\\')));
        assert!(LOCKED_REQUIREMENTS.contains(&format!("schemathesis=={SCHEMATHESIS_VERSION} \\")));
        assert!(LOCKED_REQUIREMENTS.matches("--hash=sha256:").count() > requirements.len());
    }
}
