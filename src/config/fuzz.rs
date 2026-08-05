use super::execution::validate_positive_safe_integer;
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FuzzConfig {
    pub limits: FuzzLimitsConfig,
}

impl FuzzConfig {
    pub(crate) fn validate_values(&self) -> Result<()> {
        self.limits.validate_values()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FuzzLimitsConfig {
    pub max_cases: u64,
    pub max_shrinks: u64,
    pub max_failures: u64,
    pub case_timeout_ms: u64,
}

impl Default for FuzzLimitsConfig {
    fn default() -> Self {
        Self {
            max_cases: 50,
            max_shrinks: 100,
            max_failures: 5,
            case_timeout_ms: 3_000,
        }
    }
}

impl FuzzLimitsConfig {
    pub(crate) fn validate_values(&self) -> Result<()> {
        for (name, value) in [
            ("fuzz.limits.max_cases", self.max_cases),
            ("fuzz.limits.max_shrinks", self.max_shrinks),
            ("fuzz.limits.max_failures", self.max_failures),
            ("fuzz.limits.case_timeout_ms", self.case_timeout_ms),
        ] {
            validate_positive_safe_integer(name, value)?;
        }
        if self.max_failures > self.max_cases {
            anyhow::bail!("fuzz.limits.max_failures may not exceed fuzz.limits.max_cases");
        }
        Ok(())
    }
}
