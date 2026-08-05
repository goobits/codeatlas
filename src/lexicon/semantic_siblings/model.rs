use anyhow::{bail, Result};
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(JsonSchema, Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub(crate) struct SemanticSiblingAnalysis {
    comparison_sets: Vec<SemanticSiblingComparisonSetAnalysis>,
}

impl SemanticSiblingAnalysis {
    pub(crate) fn new(
        mut comparison_sets: Vec<SemanticSiblingComparisonSetAnalysis>,
    ) -> Result<Self> {
        comparison_sets.sort_by(|left, right| left.id.cmp(&right.id));
        reject_adjacent_duplicates(
            comparison_sets.iter().map(|set| set.id.as_str()),
            "semantic sibling comparison-set analysis ID",
        )?;
        Ok(Self { comparison_sets })
    }

    pub(crate) fn evaluation_count(&self) -> usize {
        self.comparison_sets
            .iter()
            .map(|set| set.evaluations.len())
            .sum()
    }

    pub(crate) fn comparison_set_count(&self) -> usize {
        self.comparison_sets.len()
    }

    pub(crate) fn review_candidate_count(&self) -> usize {
        self.comparison_sets
            .iter()
            .flat_map(|set| &set.evaluations)
            .filter(|evaluation| {
                evaluation.disposition == SemanticSiblingDisposition::ReviewCandidate
            })
            .count()
    }

    pub(crate) fn omitted_nomination_count(&self) -> usize {
        self.comparison_sets
            .iter()
            .map(|set| set.omitted_nominations)
            .sum()
    }
}

#[derive(JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SemanticSiblingComparisonSetAnalysis {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    purpose: Option<String>,
    members: Vec<SemanticSiblingMember>,
    maximum_nominations: usize,
    nominations_considered: usize,
    evaluations: Vec<SemanticSiblingEvaluation>,
    omitted_nominations: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    omissions: Vec<SemanticSiblingOmission>,
}

impl SemanticSiblingComparisonSetAnalysis {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: String,
        purpose: Option<String>,
        mut members: Vec<SemanticSiblingMember>,
        maximum_nominations: usize,
        nominations_considered: usize,
        mut evaluations: Vec<SemanticSiblingEvaluation>,
        mut omissions: Vec<SemanticSiblingOmission>,
    ) -> Result<Self> {
        if id.is_empty() {
            bail!("Semantic sibling comparison-set analysis ID must not be empty");
        }
        if members.len() < 2 {
            bail!("Semantic sibling comparison-set analysis {id:?} needs at least two members");
        }
        if maximum_nominations == 0 {
            bail!(
                "Semantic sibling comparison-set analysis {id:?} needs a finite nomination limit"
            );
        }

        members.sort_by(|left, right| left.id.cmp(&right.id));
        reject_adjacent_duplicates(
            members.iter().map(|member| member.id.as_str()),
            "semantic sibling report member ID",
        )?;
        let member_ids = members
            .iter()
            .map(|member| member.id.as_str())
            .collect::<BTreeSet<_>>();
        for evaluation in &evaluations {
            for target in &evaluation.targets {
                if !member_ids.contains(target.member_id.as_str()) {
                    bail!(
                        "Semantic sibling evaluation target {:?} references member {:?} outside comparison set {:?}",
                        target.id,
                        target.member_id,
                        id
                    );
                }
            }
        }

        evaluations.sort_by(|left, right| {
            left.targets
                .cmp(&right.targets)
                .then_with(|| left.nomination.kind.cmp(&right.nomination.kind))
                .then_with(|| left.nomination.key.cmp(&right.nomination.key))
        });
        omissions.sort();
        let omitted_nominations = omissions.iter().map(|omission| omission.count).sum();
        let bounded_total = evaluations
            .len()
            .checked_add(omitted_nominations)
            .ok_or_else(|| anyhow::anyhow!("Semantic sibling nomination count overflow"))?;
        if nominations_considered != evaluations.len() {
            bail!(
                "Semantic sibling comparison set {id:?} considered {nominations_considered} nominations but contains {} evaluations",
                evaluations.len()
            );
        }
        if bounded_total > maximum_nominations {
            bail!(
                "Semantic sibling comparison set {id:?} accounts for {bounded_total} nominations above its limit {maximum_nominations}"
            );
        }

        Ok(Self {
            id,
            purpose,
            members,
            maximum_nominations,
            nominations_considered,
            evaluations,
            omitted_nominations,
            omissions,
        })
    }
}

#[derive(JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct SemanticSiblingMember {
    id: String,
    paths: Vec<String>,
}

impl SemanticSiblingMember {
    pub(crate) fn new(id: String, mut paths: Vec<String>) -> Result<Self> {
        if id.is_empty() || paths.is_empty() || paths.iter().any(String::is_empty) {
            bail!("Semantic sibling report members need an ID and at least one nonempty path");
        }
        paths.sort();
        paths.dedup();
        Ok(Self { id, paths })
    }
}

#[derive(JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SemanticSiblingEvaluation {
    targets: [SemanticSiblingTarget; 2],
    nomination: SemanticSiblingNomination,
    corroborations: Vec<SemanticSiblingCorroboration>,
    counterevidence_checks: Vec<SemanticSiblingCounterevidenceCheck>,
    corroboration_count: usize,
    disposition: SemanticSiblingDisposition,
}

impl SemanticSiblingEvaluation {
    pub(crate) fn new(
        mut targets: [SemanticSiblingTarget; 2],
        nomination: SemanticSiblingNomination,
        mut corroborations: Vec<SemanticSiblingCorroboration>,
        counterevidence_checks: Vec<SemanticSiblingCounterevidenceCheck>,
    ) -> Result<Self> {
        targets.sort();
        if targets[0].id == targets[1].id || targets[0].member_id == targets[1].member_id {
            bail!("Semantic sibling evaluations require two distinct cross-member targets");
        }

        corroborations.sort_by_key(|corroboration| corroboration.kind);
        reject_adjacent_duplicates(
            corroborations
                .iter()
                .map(|corroboration| corroboration.kind),
            "semantic sibling corroboration kind",
        )?;
        let counterevidence_checks = complete_counterevidence(counterevidence_checks)?;
        let corroboration_count = corroborations.len();
        let disposition = derive_disposition(corroboration_count, &counterevidence_checks);

        Ok(Self {
            targets,
            nomination,
            corroborations,
            counterevidence_checks,
            corroboration_count,
            disposition,
        })
    }
}

#[derive(JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct SemanticSiblingTarget {
    id: String,
    member_id: String,
    file_path: String,
}

impl SemanticSiblingTarget {
    pub(crate) fn new(id: String, member_id: String, file_path: String) -> Result<Self> {
        if id.is_empty() || member_id.is_empty() || file_path.is_empty() {
            bail!("Semantic sibling targets require nonempty target, member, and file identities");
        }
        Ok(Self {
            id,
            member_id,
            file_path,
        })
    }
}

#[derive(JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SemanticSiblingNomination {
    kind: SemanticSiblingNominationKind,
    key: String,
    evidence: Vec<SemanticSiblingEvidence>,
}

impl SemanticSiblingNomination {
    pub(crate) fn new(
        kind: SemanticSiblingNominationKind,
        key: String,
        mut evidence: Vec<SemanticSiblingEvidence>,
    ) -> Result<Self> {
        if key.is_empty() || evidence.is_empty() {
            bail!("Semantic sibling nominations require a key and exact evidence");
        }
        evidence.sort();
        evidence.dedup();
        Ok(Self {
            kind,
            key,
            evidence,
        })
    }
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticSiblingNominationKind {
    SameDeclaredContract,
    CanonicalActionObject,
    NamedModelRole,
    ConfiguredConcept,
}

#[derive(JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SemanticSiblingCorroboration {
    kind: SemanticSiblingCorroborationKind,
    evidence: Vec<SemanticSiblingEvidence>,
}

impl SemanticSiblingCorroboration {
    pub(crate) fn new(
        kind: SemanticSiblingCorroborationKind,
        mut evidence: Vec<SemanticSiblingEvidence>,
    ) -> Result<Self> {
        if evidence.is_empty() {
            bail!("Semantic sibling corroboration requires exact member-local evidence");
        }
        if evidence
            .iter()
            .any(|item| item.origin == SemanticSiblingEvidenceOrigin::SharedContract)
        {
            bail!(
                "Shared contract evidence may nominate semantic siblings but cannot corroborate them"
            );
        }
        evidence.sort();
        evidence.dedup();
        Ok(Self { kind, evidence })
    }
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(
    clippy::enum_variant_names,
    reason = "The public evidence vocabulary deliberately names each independent corroboration as a match"
)]
pub(crate) enum SemanticSiblingCorroborationKind {
    ImplementationRoleMatch,
    EffectSetMatch,
    ModelRoleMatch,
    ProducerPositionMatch,
    ConsumerPositionMatch,
    DependencyRoleMatch,
    LifecycleRoleMatch,
}

#[derive(JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct SemanticSiblingEvidence {
    origin: SemanticSiblingEvidenceOrigin,
    source: SemanticSiblingTarget,
    fact: String,
}

impl SemanticSiblingEvidence {
    pub(crate) fn new(
        origin: SemanticSiblingEvidenceOrigin,
        source: SemanticSiblingTarget,
        fact: String,
    ) -> Result<Self> {
        if fact.trim().is_empty() || fact.trim() != fact {
            bail!("Semantic sibling evidence facts must be canonical nonblank strings");
        }
        Ok(Self {
            origin,
            source,
            fact,
        })
    }
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticSiblingEvidenceOrigin {
    MemberLocal,
    SharedContract,
}

#[derive(JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SemanticSiblingCounterevidenceCheck {
    kind: SemanticSiblingCounterevidenceKind,
    state: SemanticSiblingCounterevidenceState,
    reason: String,
    evidence: Vec<SemanticSiblingEvidence>,
}

impl SemanticSiblingCounterevidenceCheck {
    pub(crate) fn new(
        kind: SemanticSiblingCounterevidenceKind,
        state: SemanticSiblingCounterevidenceState,
        reason: String,
        mut evidence: Vec<SemanticSiblingEvidence>,
    ) -> Result<Self> {
        if reason.trim().is_empty() || reason.trim() != reason {
            bail!("Semantic sibling counterevidence checks require a canonical reason");
        }
        evidence.sort();
        evidence.dedup();
        Ok(Self {
            kind,
            state,
            reason,
            evidence,
        })
    }

    fn unknown(kind: SemanticSiblingCounterevidenceKind) -> Self {
        Self {
            kind,
            state: SemanticSiblingCounterevidenceState::Unknown,
            reason: "required evidence was not supplied".to_string(),
            evidence: Vec::new(),
        }
    }
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticSiblingCounterevidenceKind {
    ConflictingOrUnknownEffects,
    DifferentAuthorityOrSecurityBoundaries,
    DifferentLifecycleOrCleanupOwnership,
    IncompatibleResultOrErrorSemantics,
    DisjointProducerOrConsumerRoles,
    DifferentExternallyOwnedProtocolObligations,
    DistinctConfiguredConcepts,
    IncompleteGraphOrTypeEvidence,
}

impl SemanticSiblingCounterevidenceKind {
    const ALL: [Self; 8] = [
        Self::ConflictingOrUnknownEffects,
        Self::DifferentAuthorityOrSecurityBoundaries,
        Self::DifferentLifecycleOrCleanupOwnership,
        Self::IncompatibleResultOrErrorSemantics,
        Self::DisjointProducerOrConsumerRoles,
        Self::DifferentExternallyOwnedProtocolObligations,
        Self::DistinctConfiguredConcepts,
        Self::IncompleteGraphOrTypeEvidence,
    ];
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticSiblingCounterevidenceState {
    Present,
    Absent,
    Unknown,
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticSiblingDisposition {
    ReviewCandidate,
    SeparateByEvidence,
    Inconclusive,
}

#[derive(JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct SemanticSiblingOmission {
    kind: SemanticSiblingOmissionKind,
    nomination: SemanticSiblingNominationKind,
    key: String,
    count: usize,
    reason: String,
}

impl SemanticSiblingOmission {
    pub(crate) fn new(
        kind: SemanticSiblingOmissionKind,
        nomination: SemanticSiblingNominationKind,
        key: String,
        count: usize,
        reason: String,
    ) -> Result<Self> {
        if key.is_empty() || count == 0 || reason.trim().is_empty() || reason.trim() != reason {
            bail!("Semantic sibling omissions require a key, positive count, and canonical reason");
        }
        Ok(Self {
            kind,
            nomination,
            key,
            count,
            reason,
        })
    }
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticSiblingOmissionKind {
    PerKeyLimit,
    ComparisonSetLimit,
}

fn complete_counterevidence(
    mut checks: Vec<SemanticSiblingCounterevidenceCheck>,
) -> Result<Vec<SemanticSiblingCounterevidenceCheck>> {
    checks.sort_by_key(|check| check.kind);
    reject_adjacent_duplicates(
        checks.iter().map(|check| check.kind),
        "semantic sibling counterevidence kind",
    )?;
    let supplied = checks
        .iter()
        .map(|check| check.kind)
        .collect::<BTreeSet<_>>();
    for kind in SemanticSiblingCounterevidenceKind::ALL {
        if !supplied.contains(&kind) {
            checks.push(SemanticSiblingCounterevidenceCheck::unknown(kind));
        }
    }
    checks.sort_by_key(|check| check.kind);
    Ok(checks)
}

fn derive_disposition(
    corroboration_count: usize,
    checks: &[SemanticSiblingCounterevidenceCheck],
) -> SemanticSiblingDisposition {
    if checks.iter().any(|check| {
        check.state == SemanticSiblingCounterevidenceState::Present
            && check.kind != SemanticSiblingCounterevidenceKind::IncompleteGraphOrTypeEvidence
    }) {
        return SemanticSiblingDisposition::SeparateByEvidence;
    }
    if checks.iter().any(|check| {
        check.state == SemanticSiblingCounterevidenceState::Unknown
            || (check.state == SemanticSiblingCounterevidenceState::Present
                && check.kind == SemanticSiblingCounterevidenceKind::IncompleteGraphOrTypeEvidence)
    }) {
        return SemanticSiblingDisposition::Inconclusive;
    }
    if corroboration_count >= 2 {
        SemanticSiblingDisposition::ReviewCandidate
    } else {
        SemanticSiblingDisposition::Inconclusive
    }
}

fn reject_adjacent_duplicates<T>(values: impl IntoIterator<Item = T>, label: &str) -> Result<()>
where
    T: Eq + std::fmt::Debug,
{
    let mut previous = None;
    for value in values {
        if previous.as_ref() == Some(&value) {
            bail!("Duplicate {label}: {value:?}");
        }
        previous = Some(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str, member_id: &str) -> SemanticSiblingTarget {
        SemanticSiblingTarget::new(
            id.to_string(),
            member_id.to_string(),
            format!("src/{member_id}.rs"),
        )
        .expect("target")
    }

    fn evidence(
        origin: SemanticSiblingEvidenceOrigin,
        id: &str,
        member_id: &str,
        fact: &str,
    ) -> SemanticSiblingEvidence {
        SemanticSiblingEvidence::new(origin, target(id, member_id), fact.to_string())
            .expect("evidence")
    }

    fn nomination() -> SemanticSiblingNomination {
        SemanticSiblingNomination::new(
            SemanticSiblingNominationKind::SameDeclaredContract,
            "crate::Adapter".to_string(),
            vec![evidence(
                SemanticSiblingEvidenceOrigin::SharedContract,
                "trait-adapter",
                "contract",
                "declared contract crate::Adapter",
            )],
        )
        .expect("nomination")
    }

    fn absent_checks() -> Vec<SemanticSiblingCounterevidenceCheck> {
        SemanticSiblingCounterevidenceKind::ALL
            .into_iter()
            .map(|kind| {
                SemanticSiblingCounterevidenceCheck::new(
                    kind,
                    SemanticSiblingCounterevidenceState::Absent,
                    "checked exact available evidence".to_string(),
                    Vec::new(),
                )
                .expect("counterevidence check")
            })
            .collect()
    }

    fn corroboration(kind: SemanticSiblingCorroborationKind) -> SemanticSiblingCorroboration {
        SemanticSiblingCorroboration::new(
            kind,
            vec![evidence(
                SemanticSiblingEvidenceOrigin::MemberLocal,
                "alpha-adapter",
                "alpha",
                "member-local callable role",
            )],
        )
        .expect("corroboration")
    }

    #[test]
    fn shared_contract_evidence_can_nominate_but_cannot_corroborate() {
        let shared = evidence(
            SemanticSiblingEvidenceOrigin::SharedContract,
            "trait-adapter",
            "contract",
            "declared contract crate::Adapter",
        );
        assert!(SemanticSiblingCorroboration::new(
            SemanticSiblingCorroborationKind::ImplementationRoleMatch,
            vec![shared]
        )
        .is_err());

        let evaluation = SemanticSiblingEvaluation::new(
            [
                target("beta-adapter", "beta"),
                target("alpha-adapter", "alpha"),
            ],
            nomination(),
            vec![
                corroboration(SemanticSiblingCorroborationKind::EffectSetMatch),
                corroboration(SemanticSiblingCorroborationKind::ImplementationRoleMatch),
            ],
            absent_checks(),
        )
        .expect("evaluation");
        assert_eq!(evaluation.corroboration_count, 2);
        assert_eq!(
            evaluation.disposition,
            SemanticSiblingDisposition::ReviewCandidate
        );
        assert_eq!(evaluation.targets[0].member_id, "alpha");
    }

    #[test]
    fn every_evaluation_has_the_ordered_mandatory_counterevidence_checklist() {
        let evaluation = SemanticSiblingEvaluation::new(
            [
                target("alpha-adapter", "alpha"),
                target("beta-adapter", "beta"),
            ],
            nomination(),
            Vec::new(),
            vec![SemanticSiblingCounterevidenceCheck::new(
                SemanticSiblingCounterevidenceKind::DistinctConfiguredConcepts,
                SemanticSiblingCounterevidenceState::Present,
                "project policy declares distinct concepts".to_string(),
                Vec::new(),
            )
            .expect("counterevidence")],
        )
        .expect("evaluation");

        assert_eq!(evaluation.counterevidence_checks.len(), 8);
        assert_eq!(
            evaluation.disposition,
            SemanticSiblingDisposition::SeparateByEvidence
        );
        assert_eq!(
            evaluation.counterevidence_checks[0].kind,
            SemanticSiblingCounterevidenceKind::ConflictingOrUnknownEffects
        );
        assert_eq!(
            evaluation.counterevidence_checks[0].state,
            SemanticSiblingCounterevidenceState::Unknown
        );
    }

    #[test]
    fn report_order_and_nomination_bounds_are_invariants() {
        let members = vec![
            SemanticSiblingMember::new("beta".to_string(), vec!["src/beta".to_string()])
                .expect("member"),
            SemanticSiblingMember::new("alpha".to_string(), vec!["src/alpha".to_string()])
                .expect("member"),
        ];
        let evaluation = SemanticSiblingEvaluation::new(
            [
                target("beta-adapter", "beta"),
                target("alpha-adapter", "alpha"),
            ],
            nomination(),
            Vec::new(),
            Vec::new(),
        )
        .expect("evaluation");
        let omission = SemanticSiblingOmission::new(
            SemanticSiblingOmissionKind::PerKeyLimit,
            SemanticSiblingNominationKind::SameDeclaredContract,
            "crate::Adapter".to_string(),
            2,
            "per-key expansion reached the configured ceiling".to_string(),
        )
        .expect("omission");
        let set = SemanticSiblingComparisonSetAnalysis::new(
            "adapters".to_string(),
            None,
            members,
            3,
            1,
            vec![evaluation],
            vec![omission],
        )
        .expect("comparison set");
        let analysis = SemanticSiblingAnalysis::new(vec![set]).expect("analysis");

        assert_eq!(analysis.comparison_sets[0].members[0].id, "alpha");
        assert_eq!(analysis.evaluation_count(), 1);
        assert_eq!(analysis.review_candidate_count(), 0);
        assert_eq!(analysis.omitted_nomination_count(), 2);

        let invalid = SemanticSiblingComparisonSetAnalysis::new(
            "over-limit".to_string(),
            None,
            vec![
                SemanticSiblingMember::new("alpha".to_string(), vec!["a".to_string()])
                    .expect("member"),
                SemanticSiblingMember::new("beta".to_string(), vec!["b".to_string()])
                    .expect("member"),
            ],
            2,
            0,
            Vec::new(),
            vec![SemanticSiblingOmission::new(
                SemanticSiblingOmissionKind::ComparisonSetLimit,
                SemanticSiblingNominationKind::ConfiguredConcept,
                "overflow".to_string(),
                3,
                "comparison-set ceiling reached".to_string(),
            )
            .expect("omission")],
        );
        assert!(invalid.is_err());
    }
}
