use super::callable_shape::{
    project_callable_shape_exact, project_callable_shape_semantic_roles, CallableShape,
};
use super::model::{CallableCandidate, CallableCandidateKind};
use super::symbols::{
    collect_identifier_concept_terms, project_symbol, resolve_semantic_scope, sort_symbols,
};
use crate::domain::{EvidenceClass, Language, Symbol};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn find_callable_candidates<'a>(
    symbols: impl IntoIterator<Item = &'a Symbol>,
) -> Vec<CallableCandidate> {
    let symbols = symbols.into_iter().collect::<Vec<_>>();
    let mut exact_groups = BTreeMap::<(u8, String, CallableShape), Vec<&Symbol>>::new();
    let mut structural_groups = BTreeMap::<(u8, CallableShape, String), Vec<&Symbol>>::new();
    for symbol in &symbols {
        let Some(contract) = &symbol.callable else {
            continue;
        };
        exact_groups
            .entry((
                rank_language(symbol.language),
                symbol.name.clone(),
                project_callable_shape_exact(contract),
            ))
            .or_default()
            .push(symbol);

        let shape = project_callable_shape_semantic_roles(contract);
        if !shape.has_type_evidence() {
            continue;
        }
        let Some(scope) = resolve_semantic_scope(&symbol.file_path) else {
            continue;
        };
        structural_groups
            .entry((rank_language(symbol.language), shape, scope))
            .or_default()
            .push(symbol);
    }

    let mut candidates = exact_groups
        .into_iter()
        .filter_map(|((_language, name, shape), symbols)| {
            has_multiple_files(&symbols).then(|| {
                project_candidate(
                    CallableCandidateKind::ExactCallableShape,
                    EvidenceClass::Direct,
                    shape.format_shape(),
                    resolve_common_scope(&symbols),
                    BTreeSet::new(),
                    BTreeSet::from([name]),
                    symbols,
                )
            })
        })
        .collect::<Vec<_>>();
    for ((_language, callable_shape, scope), symbols) in structural_groups {
        for symbols in collect_related_components(symbols) {
            let names = symbols
                .iter()
                .map(|symbol| symbol.name.clone())
                .collect::<BTreeSet<_>>();
            if !has_multiple_files(&symbols) || names.len() < 2 {
                continue;
            }
            candidates.push(project_candidate(
                CallableCandidateKind::SharedCallableRoleShape,
                EvidenceClass::Inferred,
                callable_shape.format_shape(),
                Some(scope.clone()),
                collect_shared_identifier_terms(&symbols),
                names,
                symbols,
            ));
        }
    }
    candidates.sort_by(|left, right| {
        left.evidence_class
            .cmp(&right.evidence_class)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.scope.cmp(&right.scope))
            .then_with(|| left.names.cmp(&right.names))
            .then_with(|| left.shared_terms.cmp(&right.shared_terms))
            .then_with(|| left.callable_shape.cmp(&right.callable_shape))
    });
    candidates
}

fn project_candidate(
    kind: CallableCandidateKind,
    evidence_class: EvidenceClass,
    callable_shape: String,
    scope: Option<String>,
    shared_terms: BTreeSet<String>,
    names: BTreeSet<String>,
    symbols: Vec<&Symbol>,
) -> CallableCandidate {
    let mut projected = symbols.into_iter().map(project_symbol).collect::<Vec<_>>();
    sort_symbols(&mut projected);
    CallableCandidate {
        kind,
        evidence_class,
        callable_shape,
        scope,
        shared_terms: shared_terms.into_iter().collect(),
        names: names.into_iter().collect(),
        symbols: projected,
    }
}

fn collect_related_components(mut symbols: Vec<&Symbol>) -> Vec<Vec<&Symbol>> {
    symbols.sort_by(|left, right| left.id.cmp(&right.id));
    let terms = symbols
        .iter()
        .map(|symbol| collect_identifier_concept_terms(&symbol.name))
        .collect::<Vec<_>>();
    let mut visited = vec![false; symbols.len()];
    let mut components = Vec::new();

    for start in 0..symbols.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut pending = vec![start];
        let mut component = Vec::new();
        while let Some(current) = pending.pop() {
            component.push(symbols[current]);
            for candidate in 0..symbols.len() {
                if !visited[candidate] && !terms[current].is_disjoint(&terms[candidate]) {
                    visited[candidate] = true;
                    pending.push(candidate);
                }
            }
        }
        components.push(component);
    }
    components
}

fn collect_shared_identifier_terms(symbols: &[&Symbol]) -> BTreeSet<String> {
    let mut names_by_term = BTreeMap::<String, BTreeSet<String>>::new();
    for symbol in symbols {
        for term in collect_identifier_concept_terms(&symbol.name) {
            names_by_term
                .entry(term)
                .or_default()
                .insert(symbol.name.clone());
        }
    }
    names_by_term
        .into_iter()
        .filter_map(|(term, names)| (names.len() >= 2).then_some(term))
        .collect()
}

fn has_multiple_files(symbols: &[&Symbol]) -> bool {
    symbols
        .iter()
        .map(|symbol| symbol.file_path.as_str())
        .collect::<BTreeSet<_>>()
        .len()
        >= 2
}

fn resolve_common_scope(symbols: &[&Symbol]) -> Option<String> {
    let first = resolve_semantic_scope(&symbols.first()?.file_path)?;
    symbols
        .iter()
        .all(|symbol| resolve_semantic_scope(&symbol.file_path).as_deref() == Some(first.as_str()))
        .then_some(first)
}

fn rank_language(language: Language) -> u8 {
    match language {
        Language::TypeScript => 0,
        Language::Python => 1,
        Language::Rust => 2,
        Language::Unknown => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::find_callable_candidates;
    use crate::domain::{
        CallableBody, CallableContract, CallableKind, CallableParameter, CallableSignature,
        EvidenceClass, Language, ParameterRequirement, ParameterRole, ReceiverContract,
        SemanticType, Symbol, SymbolKind, Visibility,
    };
    use crate::lexicon::CallableCandidateKind;

    fn function(
        file_path: &str,
        name: &str,
        signature: &str,
        language: Language,
        parameter_name: &str,
        parameter_type: SemanticType,
        result: SemanticType,
    ) -> Symbol {
        let constructibility = parameter_type.constructibility();
        Symbol {
            id: format!("{language:?}:{file_path}:fn#{name}"),
            name: name.to_string(),
            kind: SymbolKind::Function,
            visibility: Visibility::Internal,
            language,
            file_path: file_path.to_string(),
            span: None,
            signature: signature.to_string(),
            callable: Some(CallableContract::new(
                [CallableSignature {
                    kind: CallableKind::Function,
                    body: CallableBody::Present,
                    is_async: false,
                    receiver: ReceiverContract::none(),
                    type_parameters: Vec::new(),
                    parameters: vec![CallableParameter {
                        position: 0,
                        name: Some(parameter_name.to_string()),
                        role: ParameterRole::Positional,
                        requirement: ParameterRequirement::Required,
                        semantic_type: parameter_type,
                        constructibility,
                    }],
                    result,
                }],
                [],
            )),
            fuzz_policy: None,
            docs: None,
            export_paths: Vec::new(),
            referenced: false,
            package: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn separates_exact_and_differently_named_typed_candidates() {
        let symbols = [
            function(
                "src/a.rs",
                "is_record",
                "display text is not policy evidence",
                Language::Rust,
                "value",
                SemanticType::Named {
                    identity: "Value".to_string(),
                    arguments: Vec::new(),
                },
                SemanticType::Boolean,
            ),
            function(
                "src/b.rs",
                "is_record",
                "different display text with the same contract",
                Language::Rust,
                "value",
                SemanticType::Named {
                    identity: "Value".to_string(),
                    arguments: Vec::new(),
                },
                SemanticType::Boolean,
            ),
            function(
                "src/path/normalize.rs",
                "normalize_path",
                "fn normalize_path(value: &Path) -> PathBuf",
                Language::Rust,
                "value",
                SemanticType::Named {
                    identity: "Path".to_string(),
                    arguments: Vec::new(),
                },
                SemanticType::Named {
                    identity: "PathBuf".to_string(),
                    arguments: Vec::new(),
                },
            ),
            function(
                "src/path/location.rs",
                "canonicalize_path",
                "fn canonicalize_path(path: &Path) -> PathBuf",
                Language::Rust,
                "path",
                SemanticType::Named {
                    identity: "Path".to_string(),
                    arguments: Vec::new(),
                },
                SemanticType::Named {
                    identity: "PathBuf".to_string(),
                    arguments: Vec::new(),
                },
            ),
            function(
                "src/path/xml.rs",
                "escape_xml",
                "fn escape_xml(value: &Path) -> PathBuf",
                Language::Rust,
                "value",
                SemanticType::Named {
                    identity: "Path".to_string(),
                    arguments: Vec::new(),
                },
                SemanticType::Named {
                    identity: "PathBuf".to_string(),
                    arguments: Vec::new(),
                },
            ),
            function(
                "src/run.py",
                "start",
                "def start(value)",
                Language::Python,
                "value",
                SemanticType::unknown(crate::domain::TypeUnknownReason::MissingAnnotation, "value"),
                SemanticType::unknown(
                    crate::domain::TypeUnknownReason::MissingAnnotation,
                    "return",
                ),
            ),
            function(
                "src/launch.py",
                "launch",
                "def launch(item)",
                Language::Python,
                "item",
                SemanticType::unknown(crate::domain::TypeUnknownReason::MissingAnnotation, "item"),
                SemanticType::unknown(
                    crate::domain::TypeUnknownReason::MissingAnnotation,
                    "return",
                ),
            ),
        ];

        let candidates = find_callable_candidates(symbols.iter());

        assert_eq!(candidates.len(), 2);
        assert_eq!(
            candidates[0].kind,
            CallableCandidateKind::ExactCallableShape
        );
        assert_eq!(candidates[0].evidence_class, EvidenceClass::Direct);
        assert_eq!(
            candidates[1].kind,
            CallableCandidateKind::SharedCallableRoleShape
        );
        assert_eq!(candidates[1].evidence_class, EvidenceClass::Inferred);
        assert_eq!(candidates[1].names, ["canonicalize_path", "normalize_path"]);
        assert_eq!(candidates[1].shared_terms, ["path"]);
        assert!(candidates[1].callable_shape.contains("$arg0"));
        assert!(candidates[1].callable_shape.contains("named<Path>"));
    }
}
