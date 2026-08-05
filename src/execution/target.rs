use serde::{Deserialize, Serialize};

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TargetClass {
    LocalDisposable,
    RemoteDisposable,
    Staging,
    Production,
    Unknown,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TargetEnvironmentClass {
    Disposable,
    Staging,
    Production,
    Unknown,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EffectCorroboration {
    None,
    Contained,
    Effectful,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TargetEvidence {
    pub is_local: bool,
    pub is_disposable: bool,
    pub environment: TargetEnvironmentClass,
    pub effects: EffectCorroboration,
    pub is_preauthorized: bool,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TargetDisposition {
    ReviewedPlanRequired,
    PreauthorizedIsolated,
    Blocked,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetDecision {
    pub class: TargetClass,
    pub disposition: TargetDisposition,
    pub reasons: Vec<String>,
}

pub(crate) fn classify_target(evidence: &TargetEvidence) -> TargetDecision {
    let class = match (
        evidence.is_local,
        evidence.is_disposable,
        evidence.environment,
    ) {
        (_, _, TargetEnvironmentClass::Production) => TargetClass::Production,
        (_, _, TargetEnvironmentClass::Staging) => TargetClass::Staging,
        (true, true, TargetEnvironmentClass::Disposable) => TargetClass::LocalDisposable,
        (false, true, TargetEnvironmentClass::Disposable) => TargetClass::RemoteDisposable,
        _ => TargetClass::Unknown,
    };
    if class == TargetClass::Production {
        return TargetDecision {
            class,
            disposition: TargetDisposition::Blocked,
            reasons: vec!["production targets are blocked".to_string()],
        };
    }

    let mut reasons = Vec::new();
    if !evidence.is_preauthorized {
        reasons.push("target is not checked-in as preauthorized".to_string());
    }
    if !evidence.is_local {
        reasons.push("remote targets require reviewed authorization".to_string());
    }
    if !evidence.is_disposable {
        reasons.push("target disposability is not corroborated".to_string());
    }
    if evidence.environment != TargetEnvironmentClass::Disposable {
        reasons.push("target environment is not classified as disposable".to_string());
    }
    if evidence.effects != EffectCorroboration::None {
        reasons.push("target effects are not corroborated as absent".to_string());
    }
    let disposition = if reasons.is_empty() {
        TargetDisposition::PreauthorizedIsolated
    } else {
        TargetDisposition::ReviewedPlanRequired
    };
    TargetDecision {
        class,
        disposition,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_target, EffectCorroboration, TargetDisposition, TargetEnvironmentClass,
        TargetEvidence,
    };

    fn eligible() -> TargetEvidence {
        TargetEvidence {
            is_local: true,
            is_disposable: true,
            environment: TargetEnvironmentClass::Disposable,
            effects: EffectCorroboration::None,
            is_preauthorized: true,
        }
    }

    #[test]
    fn remote_and_effectful_targets_never_receive_single_shot_authorization() {
        let mut remote = eligible();
        remote.is_local = false;
        assert_eq!(
            classify_target(&remote).disposition,
            TargetDisposition::ReviewedPlanRequired
        );

        let mut effectful = eligible();
        effectful.effects = EffectCorroboration::Effectful;
        assert_eq!(
            classify_target(&effectful).disposition,
            TargetDisposition::ReviewedPlanRequired
        );

        let mut production = eligible();
        production.environment = TargetEnvironmentClass::Production;
        assert_eq!(
            classify_target(&production).disposition,
            TargetDisposition::Blocked
        );
    }
}
