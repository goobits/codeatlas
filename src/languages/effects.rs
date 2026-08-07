//! Shared pure matching primitives for adapter-owned effect evidence.

use codeatlas_domain::{CallableEffect, EffectKind, EvidenceClass};
use std::collections::BTreeSet;

pub(super) fn record_direct_effect(effects: &mut BTreeSet<CallableEffect>, kind: EffectKind) {
    effects.insert(CallableEffect::new_direct(
        kind,
        EvidenceClass::BoundaryLimited,
        None,
    ));
}

pub(super) fn has_qualified_action(
    path: &str,
    separator: &str,
    namespaces: &[&str],
    actions: &[&str],
) -> bool {
    let Some((namespace, action)) = path.rsplit_once(separator) else {
        return false;
    };
    actions.contains(&action)
        && namespaces.iter().any(|candidate| {
            namespace == *candidate
                || namespace
                    .strip_prefix(candidate)
                    .is_some_and(|suffix| suffix.starts_with(separator))
        })
}

#[cfg(test)]
mod tests {
    use super::{has_qualified_action, record_direct_effect};
    use codeatlas_domain::{EffectKind, EffectProvenance, EvidenceClass};
    use std::collections::BTreeSet;

    #[test]
    fn qualified_actions_require_namespace_boundaries_and_exact_actions() {
        for (path, separator, expected) in [
            ("fs.readFile", ".", true),
            ("fs.promises.readFile", ".", true),
            ("filesystem.readFile", ".", false),
            ("fs.readFileSync", ".", false),
            ("std::fs::read", "::", true),
            ("std::filesystem::read", "::", false),
        ] {
            assert_eq!(
                has_qualified_action(path, separator, &["fs", "std::fs"], &["readFile", "read"]),
                expected,
                "{path}"
            );
        }
    }

    #[test]
    fn direct_effect_recording_has_one_cross_language_evidence_contract() {
        let mut effects = BTreeSet::new();
        record_direct_effect(&mut effects, EffectKind::FilesystemRead);
        record_direct_effect(&mut effects, EffectKind::FilesystemRead);

        let effect = effects.iter().next().expect("one deduplicated effect");
        assert_eq!(effects.len(), 1);
        assert_eq!(effect.kind, EffectKind::FilesystemRead);
        assert_eq!(effect.evidence, EvidenceClass::BoundaryLimited);
        assert_eq!(effect.provenance, EffectProvenance::Direct);
        assert!(effect.span.is_none());
    }
}
