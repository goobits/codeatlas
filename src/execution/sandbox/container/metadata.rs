use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub(super) const RUNTIME_VERSION_FORMAT: &str = concat!(
    r#"{"version":{{json .Server.Version}},"#,
    r#""api_version":{{json .Server.APIVersion}},"#,
    r#""os":{{json .Server.Os}},"#,
    r#""arch":{{json .Server.Arch}}}"#,
);
pub(super) const RUNTIME_INFO_FORMAT: &str = concat!(
    r#"{"security_options":{{json .SecurityOptions}},"#,
    r#""cgroup_version":{{json .CgroupVersion}},"#,
    r#""driver":{{json .Driver}}}"#,
);

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeVersion {
    pub version: String,
    pub api_version: String,
    pub os: String,
    pub arch: String,
}

impl RuntimeVersion {
    pub(super) fn from_output(output: &[u8]) -> Result<Self> {
        let version = serde_json::from_slice::<Self>(output)
            .context("Container runtime server identity is not strict JSON")?;
        for (name, value) in [
            ("version", version.version.as_str()),
            ("API version", version.api_version.as_str()),
            ("operating system", version.os.as_str()),
            ("architecture", version.arch.as_str()),
        ] {
            require_value(name, value)?;
        }
        Ok(version)
    }

    pub(super) fn canonical_json(&self) -> Result<String> {
        serde_json::to_string(self).context("Could not canonicalize container server identity")
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeInfo {
    pub security_options: Vec<String>,
    pub cgroup_version: String,
    pub driver: String,
}

impl RuntimeInfo {
    pub(super) fn from_output(output: &[u8]) -> Result<Self> {
        let mut info = serde_json::from_slice::<Self>(output)
            .context("Container runtime isolation metadata is not strict JSON")?;
        require_value("cgroup version", &info.cgroup_version)?;
        require_value("storage driver", &info.driver)?;
        if info
            .security_options
            .iter()
            .any(|value| value.trim().is_empty())
        {
            anyhow::bail!("Container runtime returned an empty security option");
        }
        info.security_options.sort();
        Ok(info)
    }

    pub(super) fn is_rootless(&self) -> bool {
        self.security_options
            .iter()
            .any(|option| option.split(',').any(|field| field == "name=rootless"))
    }

    pub(super) fn canonical_json(&self) -> Result<String> {
        serde_json::to_string(self).context("Could not canonicalize container isolation metadata")
    }
}

fn require_value(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("Container runtime returned an empty {name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RuntimeInfo, RuntimeVersion};

    #[test]
    fn parses_strict_metadata_without_delimiter_assumptions() {
        let version = RuntimeVersion::from_output(
            br#"{"version":"29.7.1","api_version":"1.55","os":"linux","arch":"arm64"}"#,
        )
        .expect("runtime version");
        assert_eq!(version.version, "29.7.1");

        let info = RuntimeInfo::from_output(
            br#"{"security_options":["name=seccomp","name=rootless"],"cgroup_version":"2","driver":"overlayfs"}"#,
        )
        .expect("runtime info");
        assert!(info.is_rootless());
    }

    #[test]
    fn rejects_legacy_delimiters_and_incomplete_metadata() {
        assert!(RuntimeVersion::from_output(b"29.7.1  1.55  linux  arm64\n").is_err());
        assert!(RuntimeInfo::from_output(
            br#"{"security_options":[],"cgroup_version":"","driver":"overlayfs"}"#,
        )
        .is_err());
    }
}
