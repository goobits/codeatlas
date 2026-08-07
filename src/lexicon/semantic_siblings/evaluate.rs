use super::model::{
    SemanticSiblingCorroboration, SemanticSiblingCorroborationKind,
    SemanticSiblingCounterevidenceCheck, SemanticSiblingCounterevidenceKind,
    SemanticSiblingCounterevidenceState, SemanticSiblingEvaluation, SemanticSiblingEvidence,
    SemanticSiblingEvidenceOrigin, SemanticSiblingNomination, SemanticSiblingNominationKind,
};
use super::{has_unknown_type, NominationSeed, SiblingFact};
use anyhow::Result;
use codeatlas_domain::source_graph::SourceVisibility;
use std::collections::BTreeSet;

pub(super) fn evaluate(
    seed: &NominationSeed,
    facts: &[SiblingFact],
    policy: &super::LexiconPolicy,
) -> Result<SemanticSiblingEvaluation> {
    let left = &facts[seed.left];
    let right = &facts[seed.right];
    let nomination_origin = if seed.kind == SemanticSiblingNominationKind::SameDeclaredContract {
        SemanticSiblingEvidenceOrigin::SharedContract
    } else {
        SemanticSiblingEvidenceOrigin::MemberLocal
    };
    let nomination = SemanticSiblingNomination::new(
        seed.kind,
        seed.key.clone(),
        pair_evidence(
            left,
            right,
            nomination_origin,
            &format!("nomination {:?} uses exact key {:?}", seed.kind, seed.key),
        )?,
    )?;
    let corroborations = collect_corroborations(seed, left, right)?;
    let counterevidence = collect_counterevidence(left, right, policy)?;
    SemanticSiblingEvaluation::new(
        [left.target.clone(), right.target.clone()],
        nomination,
        corroborations,
        counterevidence,
    )
}

fn collect_corroborations(
    seed: &NominationSeed,
    left: &SiblingFact,
    right: &SiblingFact,
) -> Result<Vec<SemanticSiblingCorroboration>> {
    let mut corroborations = Vec::new();
    if seed.kind != SemanticSiblingNominationKind::SameDeclaredContract
        && left.has_callable_type_evidence
        && right.has_callable_type_evidence
        && left.callable_role_shape == right.callable_role_shape
    {
        corroborations.push(corroboration(
            SemanticSiblingCorroborationKind::ImplementationRoleMatch,
            left,
            right,
            "member-local callable parameter and result roles match",
        )?);
    }
    if !left.has_unknown_effects
        && !right.has_unknown_effects
        && !left.effect_kinds.is_empty()
        && left.effect_kinds == right.effect_kinds
    {
        corroborations.push(corroboration(
            SemanticSiblingCorroborationKind::EffectSetMatch,
            left,
            right,
            &format!("member-local effect sets match: {:?}", left.effect_kinds),
        )?);
    }

    let mut shared_models = left
        .model_roles
        .intersection(&right.model_roles)
        .cloned()
        .collect::<BTreeSet<_>>();
    if seed.kind == SemanticSiblingNominationKind::NamedModelRole {
        shared_models.remove(&seed.key);
    }
    if !shared_models.is_empty() {
        corroborations.push(corroboration(
            SemanticSiblingCorroborationKind::ModelRoleMatch,
            left,
            right,
            &format!("additional named model roles match: {shared_models:?}"),
        )?);
    }
    if !left.producer_roles.is_empty() && left.producer_roles == right.producer_roles {
        corroborations.push(corroboration(
            SemanticSiblingCorroborationKind::ProducerPositionMatch,
            left,
            right,
            &format!("upstream dependency roles match: {:?}", left.producer_roles),
        )?);
    }
    if !left.consumer_roles.is_empty() && left.consumer_roles == right.consumer_roles {
        corroborations.push(corroboration(
            SemanticSiblingCorroborationKind::ConsumerPositionMatch,
            left,
            right,
            &format!("downstream caller roles match: {:?}", left.consumer_roles),
        )?);
    }
    if !left.external_protocols.is_empty() && left.external_protocols == right.external_protocols {
        corroborations.push(corroboration(
            SemanticSiblingCorroborationKind::DependencyRoleMatch,
            left,
            right,
            &format!(
                "independently selected external dependency roles match: {:?}",
                left.external_protocols
            ),
        )?);
    }
    if seed.kind != SemanticSiblingNominationKind::CanonicalActionObject
        && left.lifecycle_role.is_some()
        && left.lifecycle_role == right.lifecycle_role
    {
        corroborations.push(corroboration(
            SemanticSiblingCorroborationKind::LifecycleRoleMatch,
            left,
            right,
            &format!(
                "member-local lifecycle roles match: {}",
                left.lifecycle_role.as_deref().unwrap_or("unknown")
            ),
        )?);
    }
    Ok(corroborations)
}

fn collect_counterevidence(
    left: &SiblingFact,
    right: &SiblingFact,
    policy: &super::LexiconPolicy,
) -> Result<Vec<SemanticSiblingCounterevidenceCheck>> {
    let mut checks = Vec::new();

    let (state, reason) = if left.has_unknown_effects || right.has_unknown_effects {
        (
            SemanticSiblingCounterevidenceState::Unknown,
            "at least one target has an unresolved effect boundary".to_string(),
        )
    } else if left.effect_kinds == right.effect_kinds {
        (
            SemanticSiblingCounterevidenceState::Absent,
            format!("known effect sets agree: {:?}", left.effect_kinds),
        )
    } else {
        (
            SemanticSiblingCounterevidenceState::Present,
            format!(
                "known effect sets differ: {:?} versus {:?}",
                left.effect_kinds, right.effect_kinds
            ),
        )
    };
    checks.push(counterevidence(
        SemanticSiblingCounterevidenceKind::ConflictingOrUnknownEffects,
        state,
        reason,
        left,
        right,
    )?);

    let (state, reason) = if matches!(left.visibility, SourceVisibility::Unknown)
        || matches!(right.visibility, SourceVisibility::Unknown)
    {
        (
            SemanticSiblingCounterevidenceState::Unknown,
            "at least one target has unknown source visibility".to_string(),
        )
    } else if left.project == right.project && left.visibility == right.visibility {
        (
            SemanticSiblingCounterevidenceState::Absent,
            "targets share the same indexed project and visibility boundary".to_string(),
        )
    } else {
        (
            SemanticSiblingCounterevidenceState::Present,
            format!(
                "indexed project or visibility boundaries differ: {}/ {:?} versus {}/ {:?}",
                left.project, left.visibility, right.project, right.visibility
            ),
        )
    };
    checks.push(counterevidence(
        SemanticSiblingCounterevidenceKind::DifferentAuthorityOrSecurityBoundaries,
        state,
        reason,
        left,
        right,
    )?);

    let (state, reason) = match (&left.lifecycle_role, &right.lifecycle_role) {
        (Some(left), Some(right)) if left == right => (
            SemanticSiblingCounterevidenceState::Absent,
            format!("known lifecycle roles agree: {left}"),
        ),
        (Some(left), Some(right)) => (
            SemanticSiblingCounterevidenceState::Present,
            format!("known lifecycle roles differ: {left} versus {right}"),
        ),
        _ => (
            SemanticSiblingCounterevidenceState::Unknown,
            "lifecycle or cleanup ownership is not explicit for both targets".to_string(),
        ),
    };
    checks.push(counterevidence(
        SemanticSiblingCounterevidenceKind::DifferentLifecycleOrCleanupOwnership,
        state,
        reason,
        left,
        right,
    )?);

    let result_unknown = left.result_types.is_empty()
        || right.result_types.is_empty()
        || left.result_types.iter().any(has_unknown_type)
        || right.result_types.iter().any(has_unknown_type);
    let (state, reason) = if result_unknown {
        (
            SemanticSiblingCounterevidenceState::Unknown,
            "result or error type evidence is incomplete".to_string(),
        )
    } else if left.result_types == right.result_types {
        (
            SemanticSiblingCounterevidenceState::Absent,
            "structured result and error types agree".to_string(),
        )
    } else {
        (
            SemanticSiblingCounterevidenceState::Present,
            "structured result or error types differ".to_string(),
        )
    };
    checks.push(counterevidence(
        SemanticSiblingCounterevidenceKind::IncompatibleResultOrErrorSemantics,
        state,
        reason,
        left,
        right,
    )?);

    let left_positions = left
        .producer_roles
        .union(&left.consumer_roles)
        .cloned()
        .collect::<BTreeSet<_>>();
    let right_positions = right
        .producer_roles
        .union(&right.consumer_roles)
        .cloned()
        .collect::<BTreeSet<_>>();
    let (state, reason) = if left.graph_incomplete || right.graph_incomplete {
        (
            SemanticSiblingCounterevidenceState::Unknown,
            "graph boundaries prevent a complete producer/consumer comparison".to_string(),
        )
    } else if left_positions.is_empty() || right_positions.is_empty() {
        (
            SemanticSiblingCounterevidenceState::Unknown,
            "one or both targets have no resolved producer/consumer position".to_string(),
        )
    } else if left_positions.is_disjoint(&right_positions) {
        (
            SemanticSiblingCounterevidenceState::Present,
            "resolved producer and consumer roles are disjoint".to_string(),
        )
    } else {
        (
            SemanticSiblingCounterevidenceState::Absent,
            "resolved producer or consumer roles overlap".to_string(),
        )
    };
    checks.push(counterevidence(
        SemanticSiblingCounterevidenceKind::DisjointProducerOrConsumerRoles,
        state,
        reason,
        left,
        right,
    )?);

    let (state, reason) = if left.graph_incomplete || right.graph_incomplete {
        (
            SemanticSiblingCounterevidenceState::Unknown,
            "graph boundaries prevent a complete external-protocol comparison".to_string(),
        )
    } else if left.external_protocols == right.external_protocols {
        (
            SemanticSiblingCounterevidenceState::Absent,
            format!(
                "known externally owned protocol obligations agree: {:?}",
                left.external_protocols
            ),
        )
    } else {
        (
            SemanticSiblingCounterevidenceState::Present,
            format!(
                "known externally owned protocol obligations differ: {:?} versus {:?}",
                left.external_protocols, right.external_protocols
            ),
        )
    };
    checks.push(counterevidence(
        SemanticSiblingCounterevidenceKind::DifferentExternallyOwnedProtocolObligations,
        state,
        reason,
        left,
        right,
    )?);

    let distinct_reason = policy.distinct_concept_reason(&left.concept_ids, &right.concept_ids);
    checks.push(counterevidence(
        SemanticSiblingCounterevidenceKind::DistinctConfiguredConcepts,
        if distinct_reason.is_some() {
            SemanticSiblingCounterevidenceState::Present
        } else {
            SemanticSiblingCounterevidenceState::Absent
        },
        distinct_reason.unwrap_or_else(|| {
            "project policy declares no distinct concept relation for the matched concepts"
                .to_string()
        }),
        left,
        right,
    )?);

    let incomplete = left.graph_incomplete
        || right.graph_incomplete
        || left.has_unknown_types
        || right.has_unknown_types;
    checks.push(counterevidence(
        SemanticSiblingCounterevidenceKind::IncompleteGraphOrTypeEvidence,
        if incomplete {
            SemanticSiblingCounterevidenceState::Present
        } else {
            SemanticSiblingCounterevidenceState::Absent
        },
        if incomplete {
            "at least one target has incomplete graph or structured type evidence"
        } else {
            "graph and structured type evidence are complete for both targets"
        }
        .to_string(),
        left,
        right,
    )?);

    Ok(checks)
}

fn corroboration(
    kind: SemanticSiblingCorroborationKind,
    left: &SiblingFact,
    right: &SiblingFact,
    fact: &str,
) -> Result<SemanticSiblingCorroboration> {
    SemanticSiblingCorroboration::new(
        kind,
        pair_evidence(
            left,
            right,
            SemanticSiblingEvidenceOrigin::MemberLocal,
            fact,
        )?,
    )
}

fn counterevidence(
    kind: SemanticSiblingCounterevidenceKind,
    state: SemanticSiblingCounterevidenceState,
    reason: String,
    left: &SiblingFact,
    right: &SiblingFact,
) -> Result<SemanticSiblingCounterevidenceCheck> {
    SemanticSiblingCounterevidenceCheck::new(
        kind,
        state,
        reason.clone(),
        pair_evidence(
            left,
            right,
            SemanticSiblingEvidenceOrigin::MemberLocal,
            &reason,
        )?,
    )
}

fn pair_evidence(
    left: &SiblingFact,
    right: &SiblingFact,
    origin: SemanticSiblingEvidenceOrigin,
    fact: &str,
) -> Result<Vec<SemanticSiblingEvidence>> {
    [left, right]
        .into_iter()
        .map(|source| SemanticSiblingEvidence::new(origin, source.target.clone(), fact.to_string()))
        .collect()
}
