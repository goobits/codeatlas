use super::diagnostic::{sort_diagnostics, Diagnostic};
use super::digest::{digest_value, DigestKind, TypedDigest};
use super::vocabulary::{CyclePolicy, Vocabulary};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompileMode {
    Governing,
    Review,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphDeclaration {
    pub module: String,
    pub declaration: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompiledGraph {
    pub mode: CompileMode,
    pub objects: BTreeMap<String, GraphDeclaration>,
    pub relations: BTreeMap<String, GraphDeclaration>,
    pub bindings: BTreeMap<String, GraphDeclaration>,
    pub constraints: BTreeMap<String, GraphDeclaration>,
}

impl CompiledGraph {
    pub(crate) fn digest(&self) -> Result<TypedDigest, super::diagnostic::ArchitectureError> {
        let kind = match self.mode {
            CompileMode::Governing => DigestKind::GoverningGraph,
            CompileMode::Review => DigestKind::ReviewGraph,
        };
        let value = serde_json::to_value(self).map_err(|error| {
            super::diagnostic::ArchitectureError::new(
                "graph.serialization-failed",
                format!("cannot serialize graph: {error}"),
            )
        })?;
        digest_value(kind, &value)
    }
}

pub(crate) fn compile(
    documents: &[Value],
    vocabulary: &Vocabulary,
    mode: CompileMode,
) -> Result<CompiledGraph, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let mut modules = BTreeMap::new();
    for document in documents {
        diagnostics.extend(vocabulary.validate_document(document));
        if document.get("kind").and_then(Value::as_str) != Some("ArchitectureModule") {
            diagnostics.push(Diagnostic::error(
                "graph.non-module-input",
                "architecture graph compilation accepts ArchitectureModule documents only",
            ));
            continue;
        }
        let Some(id) = document["metadata"]["id"].as_str() else {
            continue;
        };
        if modules.insert(id.to_owned(), document).is_some() {
            diagnostics.push(Diagnostic::error(
                "module.duplicate-id",
                format!("duplicate module ID {id}"),
            ));
        }
    }
    if !diagnostics.is_empty() {
        sort_diagnostics(&mut diagnostics);
        return Err(diagnostics);
    }

    diagnostics.extend(validate_imports(&modules));
    diagnostics.extend(validate_import_cycles(&modules));

    let mut owners = BTreeMap::<String, DeclarationOwner>::new();
    let mut reserved_ids = BTreeSet::new();
    for (module_id, document) in &modules {
        for category in ["objects", "relations", "bindings", "constraints"] {
            for id in document[category]
                .as_object()
                .expect("schema validated declaration map")
                .keys()
            {
                let owner = DeclarationOwner {
                    module: module_id.clone(),
                    category: category.to_owned(),
                };
                if let Some(previous) = owners.insert(id.clone(), owner) {
                    diagnostics.push(Diagnostic::error(
                        "declaration.duplicate-id",
                        format!(
                            "{id} is declared by both {} {} and {module_id} {category}",
                            previous.module, previous.category
                        ),
                    ));
                }
            }
        }
        for id in document["retired"]
            .as_object()
            .expect("schema validated retired map")
            .keys()
        {
            if !reserved_ids.insert(id.clone()) {
                diagnostics.push(Diagnostic::error(
                    "retired.duplicate-id",
                    format!("retired ID {id} appears in more than one module"),
                ));
            }
        }
    }
    for id in &reserved_ids {
        if owners.contains_key(id) {
            diagnostics.push(Diagnostic::error(
                "retired.id-reused",
                format!("retired ID {id} is reused by an active declaration"),
            ));
        }
    }
    diagnostics.extend(validate_exports(&modules, &owners));
    if !diagnostics.is_empty() {
        sort_diagnostics(&mut diagnostics);
        return Err(diagnostics);
    }

    let mut graph = CompiledGraph {
        mode,
        objects: BTreeMap::new(),
        relations: BTreeMap::new(),
        bindings: BTreeMap::new(),
        constraints: BTreeMap::new(),
    };
    for (module_id, document) in &modules {
        if !eligible(document, mode) {
            continue;
        }
        collect_eligible(module_id, &document["objects"], mode, &mut graph.objects);
        collect_eligible(
            module_id,
            &document["relations"],
            mode,
            &mut graph.relations,
        );
        collect_eligible(module_id, &document["bindings"], mode, &mut graph.bindings);
        collect_eligible(
            module_id,
            &document["constraints"],
            mode,
            &mut graph.constraints,
        );
    }

    let object_kinds = graph
        .objects
        .iter()
        .map(|(id, entry)| {
            (
                id.clone(),
                entry.declaration["kind"]
                    .as_str()
                    .expect("schema validated object kind")
                    .to_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (id, entry) in &graph.objects {
        for reference in vocabulary.object_reference_ids(&entry.declaration) {
            let reference_diagnostics = validate_reference(
                id,
                entry,
                &Value::String(reference.clone()),
                &modules,
                &owners,
            );
            let reference_is_valid = reference_diagnostics.is_empty();
            diagnostics.extend(reference_diagnostics);
            if reference_is_valid && !graph.objects.contains_key(&reference) {
                diagnostics.push(Diagnostic::error(
                    "object.reference-not-active",
                    format!("{id} references {reference}, which is absent from the {mode:?} graph"),
                ));
            }
        }
    }
    for (id, entry) in &graph.relations {
        diagnostics.extend(vocabulary.validate_relation(id, &entry.declaration, &object_kinds));
        diagnostics.extend(validate_reference(
            id,
            entry,
            &entry.declaration["subject"],
            &modules,
            &owners,
        ));
        diagnostics.extend(validate_reference(
            id,
            entry,
            &entry.declaration["object"],
            &modules,
            &owners,
        ));
    }
    for (id, entry) in &graph.bindings {
        diagnostics.extend(validate_reference(
            id,
            entry,
            &entry.declaration["target"],
            &modules,
            &owners,
        ));
        if entry.declaration["target"]
            .as_str()
            .is_some_and(|target| !graph.objects.contains_key(target))
        {
            diagnostics.push(Diagnostic::error(
                "binding.target-not-active",
                format!("{id} targets a declaration absent from the {mode:?} graph"),
            ));
        }
    }
    diagnostics.extend(evaluate_constraints(&graph, vocabulary));
    diagnostics.extend(validate_predicate_cycles(&graph, vocabulary));
    if diagnostics.is_empty() {
        Ok(graph)
    } else {
        sort_diagnostics(&mut diagnostics);
        Err(diagnostics)
    }
}

#[derive(Clone, Debug)]
struct DeclarationOwner {
    module: String,
    category: String,
}

fn validate_imports(modules: &BTreeMap<String, &Value>) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (module_id, document) in modules {
        for import in document["imports"]
            .as_array()
            .expect("schema validated imports")
        {
            let imported_id = import["module"]
                .as_str()
                .expect("schema validated import module");
            let Some(imported) = modules.get(imported_id) else {
                diagnostics.push(Diagnostic::error(
                    "import.module-unresolved",
                    format!("{module_id} imports missing module {imported_id}"),
                ));
                continue;
            };
            if import["architectureVersion"].as_u64()
                != imported["metadata"]["architectureVersion"].as_u64()
            {
                diagnostics.push(Diagnostic::error(
                    "import.architecture-version-mismatch",
                    format!("{module_id} pins the wrong architecture version for {imported_id}"),
                ));
            }
            match digest_value(DigestKind::CanonicalModule, imported) {
                Ok(digest) if import["digest"].as_str() == Some(digest.as_str()) => {}
                Ok(_) => diagnostics.push(Diagnostic::error(
                    "import.digest-mismatch",
                    format!("{module_id} pins the wrong digest for {imported_id}"),
                )),
                Err(error) => diagnostics.push(*error.diagnostic),
            }
        }
    }
    diagnostics
}

fn validate_import_cycles(modules: &BTreeMap<String, &Value>) -> Vec<Diagnostic> {
    let mut visited = BTreeSet::new();
    for module_id in modules.keys() {
        let mut visiting = BTreeSet::new();
        let mut pending = vec![(module_id.as_str(), false)];
        let mut path = Vec::<String>::new();
        while let Some((current, exiting)) = pending.pop() {
            if exiting {
                visiting.remove(current);
                visited.insert(current.to_owned());
                path.pop();
                continue;
            }
            if visited.contains(current) {
                continue;
            }
            if !visiting.insert(current.to_owned()) {
                path.push(current.to_owned());
                return vec![Diagnostic::error(
                    "import.cycle",
                    format!("module import cycle: {}", path.join(" -> ")),
                )];
            }
            path.push(current.to_owned());
            pending.push((current, true));
            if let Some(document) = modules.get(current) {
                for import in document["imports"]
                    .as_array()
                    .expect("schema validated imports")
                    .iter()
                    .rev()
                {
                    if let Some(imported) = import["module"].as_str() {
                        pending.push((imported, false));
                    }
                }
            }
        }
    }
    Vec::new()
}

fn validate_exports(
    modules: &BTreeMap<String, &Value>,
    owners: &BTreeMap<String, DeclarationOwner>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (module_id, document) in modules {
        for category in ["objects", "relations", "bindings", "constraints"] {
            for exported in document["exports"][category]
                .as_array()
                .expect("schema validated export set")
            {
                let id = exported.as_str().expect("schema validated export ID");
                match owners.get(id) {
                    Some(owner) if owner.module == *module_id && owner.category == category => {}
                    Some(_) => diagnostics.push(Diagnostic::error(
                        "export.not-owned",
                        format!("{module_id} cannot export declaration {id} it does not own"),
                    )),
                    None => diagnostics.push(Diagnostic::error(
                        "export.declaration-unresolved",
                        format!("{module_id} exports unknown declaration {id}"),
                    )),
                }
            }
        }
    }
    diagnostics
}

fn collect_eligible(
    module_id: &str,
    declarations: &Value,
    mode: CompileMode,
    output: &mut BTreeMap<String, GraphDeclaration>,
) {
    for (id, declaration) in declarations
        .as_object()
        .expect("schema validated declaration map")
    {
        if eligible(declaration, mode) {
            output.insert(
                id.clone(),
                GraphDeclaration {
                    module: module_id.to_owned(),
                    declaration: declaration.clone(),
                },
            );
        }
    }
}

fn eligible(value: &Value, mode: CompileMode) -> bool {
    let status = value["decision"]["status"].as_str();
    match mode {
        CompileMode::Governing => {
            status == Some("accepted")
                && !value["decision"]["authority"]["governing"]
                    .as_array()
                    .is_none_or(Vec::is_empty)
                && matches!(
                    value["approval"]["status"].as_str(),
                    Some("granted" | "not_required")
                )
        }
        CompileMode::Review => matches!(status, Some("accepted" | "proposed" | "unresolved")),
    }
}

fn validate_reference(
    declaration_id: &str,
    declaration: &GraphDeclaration,
    reference: &Value,
    modules: &BTreeMap<String, &Value>,
    owners: &BTreeMap<String, DeclarationOwner>,
) -> Vec<Diagnostic> {
    let Some(reference_id) = reference.as_str() else {
        return vec![Diagnostic::error(
            "reference.invalid",
            format!("{declaration_id} contains a non-string reference"),
        )];
    };
    let Some(owner) = owners.get(reference_id) else {
        return vec![Diagnostic::error(
            "reference.unresolved",
            format!("{declaration_id} references unknown ID {reference_id}"),
        )];
    };
    if owner.module == declaration.module {
        return Vec::new();
    }
    let module = modules
        .get(&declaration.module)
        .expect("declaring module exists");
    if !module["imports"]
        .as_array()
        .expect("schema validated imports")
        .iter()
        .any(|import| import["module"].as_str() == Some(&owner.module))
    {
        return vec![Diagnostic::error(
            "reference.module-not-imported",
            format!(
                "{} references {reference_id} without importing {}",
                declaration.module, owner.module
            ),
        )];
    }
    let owner_module = modules.get(&owner.module).expect("owner module exists");
    if !owner_module["exports"][&owner.category]
        .as_array()
        .expect("schema validated exports")
        .iter()
        .any(|id| id.as_str() == Some(reference_id))
    {
        return vec![Diagnostic::error(
            "reference.private-cross-module",
            format!(
                "{} references private declaration {reference_id} from {}",
                declaration.module, owner.module
            ),
        )];
    }
    Vec::new()
}

fn evaluate_constraints(graph: &CompiledGraph, vocabulary: &Vocabulary) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, entry) in &graph.constraints {
        let declaration = &entry.declaration;
        let rule = declaration["rule"]
            .as_str()
            .expect("schema validated constraint rule");
        let arguments = &declaration["arguments"];
        match rule {
            "requires_object" => {
                let target = arguments["target"].as_str().expect("validated target");
                if !graph.objects.contains_key(target) {
                    diagnostics.push(constraint_failed(
                        id,
                        format!("required object {target} is absent"),
                    ));
                }
            }
            "exactly_one_incoming" => {
                let target = arguments["target"].as_str().expect("validated target");
                let predicate = arguments["predicate"]
                    .as_str()
                    .expect("validated predicate");
                if !vocabulary.has_predicate(predicate) {
                    diagnostics.push(Diagnostic::error(
                        "constraint.predicate-unknown",
                        format!("{id} references unknown predicate {predicate}"),
                    ));
                    continue;
                }
                let count = graph
                    .relations
                    .values()
                    .filter(|relation| {
                        relation.declaration["predicate"].as_str() == Some(predicate)
                            && relation.declaration["object"].as_str() == Some(target)
                    })
                    .count();
                if count != 1 {
                    diagnostics.push(constraint_failed(
                        id,
                        format!(
                            "expected exactly one incoming {predicate} relation to {target}, found {count}"
                        ),
                    ));
                }
            }
            "forbids_relation" => {
                let predicate = arguments["predicate"]
                    .as_str()
                    .expect("validated predicate");
                if !vocabulary.has_predicate(predicate) {
                    diagnostics.push(Diagnostic::error(
                        "constraint.predicate-unknown",
                        format!("{id} references unknown predicate {predicate}"),
                    ));
                    continue;
                }
                diagnostics.extend(
                    graph
                        .relations
                        .values()
                        .any(|relation| {
                            relation.declaration["predicate"].as_str() == Some(predicate)
                                && relation.declaration["subject"].as_str()
                                    == arguments["subject"].as_str()
                                && relation.declaration["object"].as_str()
                                    == arguments["object"].as_str()
                        })
                        .then(|| constraint_failed(id, "forbidden relation is present".to_owned())),
                );
            }
            "no_path" => {
                let predicates = arguments["via"]
                    .as_array()
                    .expect("validated path predicates")
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<BTreeSet<_>>();
                let mut unknown_predicate = false;
                for predicate in &predicates {
                    if !vocabulary.has_predicate(predicate) {
                        unknown_predicate = true;
                        diagnostics.push(Diagnostic::error(
                            "constraint.predicate-unknown",
                            format!("{id} references unknown predicate {predicate}"),
                        ));
                    }
                }
                if unknown_predicate {
                    continue;
                }
                if path_exists(
                    graph,
                    arguments["from"].as_str().expect("validated from"),
                    arguments["to"].as_str().expect("validated to"),
                    &predicates,
                ) {
                    diagnostics.push(constraint_failed(
                        id,
                        "a prohibited dependency path exists".to_owned(),
                    ));
                }
            }
            "must_reference_contract" => {
                let object_kind = arguments["objectKind"]
                    .as_str()
                    .expect("validated object kind");
                if !vocabulary.has_object_kind(object_kind) {
                    diagnostics.push(Diagnostic::error(
                        "constraint.object-kind-unknown",
                        format!("{id} references unknown object kind {object_kind}"),
                    ));
                    continue;
                }
                let visibility = arguments.get("visibility").and_then(Value::as_str);
                for (object_id, object) in &graph.objects {
                    if object.declaration["kind"].as_str() != Some(object_kind)
                        || visibility.is_some()
                            && object.declaration["attributes"]["visibility"].as_str() != visibility
                    {
                        continue;
                    }
                    if object.declaration["attributes"].get("contract").is_none() {
                        diagnostics.push(constraint_failed(
                            id,
                            format!("{object_id} does not reference a contract"),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    diagnostics
}

fn validate_predicate_cycles(graph: &CompiledGraph, vocabulary: &Vocabulary) -> Vec<Diagnostic> {
    for (predicate, definition) in &vocabulary.predicates {
        if definition.cycles != CyclePolicy::Forbidden {
            continue;
        }
        let edges = graph
            .relations
            .values()
            .filter(|relation| relation.declaration["predicate"].as_str() == Some(predicate))
            .map(|relation| {
                (
                    relation.declaration["subject"]
                        .as_str()
                        .expect("validated subject"),
                    relation.declaration["object"]
                        .as_str()
                        .expect("validated object"),
                )
            })
            .collect::<Vec<_>>();
        for (from, _) in &edges {
            if edge_path_exists(&edges, from, from, true) {
                return vec![Diagnostic::error(
                    "relation.cycle-forbidden",
                    format!("predicate {predicate} contains a forbidden cycle at {from}"),
                )];
            }
        }
    }
    Vec::new()
}

fn path_exists(graph: &CompiledGraph, from: &str, to: &str, predicates: &BTreeSet<&str>) -> bool {
    let edges = graph
        .relations
        .values()
        .filter(|relation| {
            relation.declaration["predicate"]
                .as_str()
                .is_some_and(|predicate| predicates.contains(predicate))
        })
        .map(|relation| {
            (
                relation.declaration["subject"]
                    .as_str()
                    .expect("validated subject"),
                relation.declaration["object"]
                    .as_str()
                    .expect("validated object"),
            )
        })
        .collect::<Vec<_>>();
    edge_path_exists(&edges, from, to, false)
}

fn edge_path_exists(edges: &[(&str, &str)], from: &str, to: &str, require_edge: bool) -> bool {
    let mut pending = vec![(from, false)];
    let mut visited = BTreeSet::new();
    while let Some((current, traversed)) = pending.pop() {
        if current == to && (!require_edge || traversed) {
            return true;
        }
        if !visited.insert(current) {
            continue;
        }
        for (_, next) in edges.iter().filter(|(source, _)| *source == current) {
            pending.push((next, true));
        }
    }
    false
}

fn constraint_failed(id: &str, message: String) -> Diagnostic {
    Diagnostic::error("constraint.failed", format!("{id}: {message}"))
}

#[cfg(test)]
mod tests {
    use super::{compile, CompileMode};
    use crate::architecture::vocabulary::Vocabulary;
    use crate::architecture::yaml::{parse, ParseLimits};
    use serde_json::json;

    fn example(path: &str) -> serde_json::Value {
        parse(path.as_bytes(), ParseLimits::default())
            .expect("parse example")
            .value
    }

    #[test]
    fn governing_graph_excludes_proposals() {
        let vocabulary = Vocabulary::bundled().expect("vocabulary");
        let module = example(include_str!(
            "../../spec/architecture/v0.1/examples/tabby-shelly/architecture.atlas.yaml"
        ));
        let graph = compile(&[module], &vocabulary, CompileMode::Governing).expect("graph");
        assert!(graph.objects.contains_key("goobits.app.tabby"));
        assert!(!graph.objects.contains_key("goobits.runtime.tab-root-space"));
    }

    #[test]
    fn typed_relations_reject_unknown_predicates_and_endpoint_kinds() {
        let vocabulary = Vocabulary::bundled().expect("vocabulary");
        let source = include_str!(
            "../../spec/architecture/v0.1/examples/tabby-shelly/architecture.atlas.yaml"
        );

        let mut module = example(source);
        module["relations"]["goobits.relation.tabby-provides-tab-host"]["predicate"] =
            json!("uses");
        let diagnostics =
            compile(&[module], &vocabulary, CompileMode::Governing).expect_err("predicate");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "vocabulary.unknown-predicate"));

        let mut module = example(source);
        module["relations"]["goobits.relation.tabby-provides-tab-host"]["object"] =
            json!("goobits.app.shelly");
        let diagnostics =
            compile(&[module], &vocabulary, CompileMode::Governing).expect_err("kind");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "relation.invalid-object-kind"));
    }

    #[test]
    fn typed_object_references_must_resolve_to_active_objects() {
        let vocabulary = Vocabulary::bundled().expect("vocabulary");
        let mut module = example(include_str!(
            "../../spec/architecture/v0.1/examples/workshop-codeatlas/architecture.atlas.yaml"
        ));
        module["objects"]["codeatlas.capability.context-slice"]["attributes"]["contract"] =
            json!("codeatlas.contract.missing");
        let diagnostics =
            compile(&[module], &vocabulary, CompileMode::Governing).expect_err("reference");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "reference.unresolved"));
    }
}
