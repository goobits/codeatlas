use super::model::{
    SemanticSiblingNominationKind, SemanticSiblingOmission, SemanticSiblingOmissionKind,
};
use super::{NominationSeed, SiblingFact};
use anyhow::{Context, Result};
use std::collections::BTreeMap;

const MAXIMUM_NOMINATIONS_PER_KEY: usize = 32;

pub(super) struct Nominated {
    pub(super) seeds: Vec<NominationSeed>,
    pub(super) omissions: Vec<SemanticSiblingOmission>,
}

pub(super) fn collect(facts: &[SiblingFact], maximum_nominations: usize) -> Result<Nominated> {
    let mut groups = BTreeMap::<(SemanticSiblingNominationKind, String), Vec<usize>>::new();
    for (index, fact) in facts.iter().enumerate() {
        if !fact.action_object.is_empty() {
            groups
                .entry((
                    SemanticSiblingNominationKind::CanonicalActionObject,
                    fact.action_object.clone(),
                ))
                .or_default()
                .push(index);
        }
        for contract in &fact.declared_contracts {
            groups
                .entry((
                    SemanticSiblingNominationKind::SameDeclaredContract,
                    contract.clone(),
                ))
                .or_default()
                .push(index);
        }
        for model_role in &fact.model_roles {
            groups
                .entry((
                    SemanticSiblingNominationKind::NamedModelRole,
                    model_role.clone(),
                ))
                .or_default()
                .push(index);
        }
        for concept in &fact.concept_ids {
            groups
                .entry((
                    SemanticSiblingNominationKind::ConfiguredConcept,
                    concept.clone(),
                ))
                .or_default()
                .push(index);
        }
    }

    let mut seeds = Vec::new();
    let mut omissions = Vec::new();
    let mut unreported_after_limit = BTreeMap::<SemanticSiblingNominationKind, usize>::new();
    for ((kind, key), indices) in groups {
        let members = group_by_member(facts, &indices);
        if members.len() < 2 {
            continue;
        }
        let possible = count_cross_member_pairs(&members)?;
        if possible == 0 {
            continue;
        }
        let remaining = maximum_nominations.saturating_sub(seeds.len());
        if remaining == 0 {
            add_count(&mut unreported_after_limit, kind, possible)?;
            continue;
        }
        let allowed = possible.min(MAXIMUM_NOMINATIONS_PER_KEY).min(remaining);
        let generated = append_pairs(&mut seeds, &members, kind, &key, allowed);
        debug_assert_eq!(generated, allowed);
        if possible > generated {
            let comparison_limit = remaining < possible.min(MAXIMUM_NOMINATIONS_PER_KEY);
            omissions.push(SemanticSiblingOmission::new(
                if comparison_limit {
                    SemanticSiblingOmissionKind::ComparisonSetLimit
                } else {
                    SemanticSiblingOmissionKind::PerKeyLimit
                },
                kind,
                key,
                possible - generated,
                if comparison_limit {
                    "comparison-set nomination ceiling reached"
                } else {
                    "per-key cross-member expansion ceiling reached"
                }
                .to_string(),
            )?);
        }
    }
    for (kind, count) in unreported_after_limit {
        omissions.push(SemanticSiblingOmission::new(
            SemanticSiblingOmissionKind::ComparisonSetLimit,
            kind,
            "remaining_keys".to_string(),
            count,
            "comparison-set nomination ceiling reached before these keys were expanded".to_string(),
        )?);
    }
    seeds.sort();
    omissions.sort();
    Ok(Nominated { seeds, omissions })
}

fn group_by_member<'a>(
    facts: &'a [SiblingFact],
    indices: &[usize],
) -> BTreeMap<&'a str, Vec<usize>> {
    let mut members = BTreeMap::<&str, Vec<usize>>::new();
    for index in indices {
        members
            .entry(facts[*index].target.member_id())
            .or_default()
            .push(*index);
    }
    members
}

fn count_cross_member_pairs(members: &BTreeMap<&str, Vec<usize>>) -> Result<usize> {
    let groups = members.values().collect::<Vec<_>>();
    let mut count = 0usize;
    for (index, left) in groups.iter().enumerate() {
        for right in &groups[index + 1..] {
            let pairs = left
                .len()
                .checked_mul(right.len())
                .context("Semantic sibling nomination pair count overflow")?;
            count = count
                .checked_add(pairs)
                .context("Semantic sibling nomination pair count overflow")?;
        }
    }
    Ok(count)
}

fn append_pairs(
    seeds: &mut Vec<NominationSeed>,
    members: &BTreeMap<&str, Vec<usize>>,
    kind: SemanticSiblingNominationKind,
    key: &str,
    maximum: usize,
) -> usize {
    let start = seeds.len();
    let groups = members.values().collect::<Vec<_>>();
    'members: for (index, left) in groups.iter().enumerate() {
        for right in &groups[index + 1..] {
            for left in *left {
                for right in *right {
                    seeds.push(NominationSeed {
                        left: *left,
                        right: *right,
                        kind,
                        key: key.to_string(),
                    });
                    if seeds.len() - start == maximum {
                        break 'members;
                    }
                }
            }
        }
    }
    seeds.len() - start
}

fn add_count(
    counts: &mut BTreeMap<SemanticSiblingNominationKind, usize>,
    kind: SemanticSiblingNominationKind,
    count: usize,
) -> Result<()> {
    let current = counts.entry(kind).or_default();
    *current = current
        .checked_add(count)
        .context("Semantic sibling omission count overflow")?;
    Ok(())
}
