use crate::{digest_value, Diagnostic, DigestKind, TypedDigest, Vocabulary};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileMode {
    Governing,
    Review,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphDeclaration {
    pub module: String,
    pub declaration: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompiledGraph {
    pub mode: CompileMode,
    pub objects: BTreeMap<String, GraphDeclaration>,
    pub relations: BTreeMap<String, GraphDeclaration>,
    pub bindings: BTreeMap<String, GraphDeclaration>,
    pub constraints: BTreeMap<String, GraphDeclaration>,
    #[serde(skip)]
    pub reserved_ids: BTreeSet<String>,
}

impl CompiledGraph {
    pub fn digest(&self) -> Result<TypedDigest, crate::ValidationError> {
        let kind = match self.mode {
            CompileMode::Governing => DigestKind::GoverningGraph,
            CompileMode::Review => DigestKind::ReviewGraph,
        };
        let value = serde_json::to_value(self).map_err(|error| {
            crate::ValidationError::new(
                "graph.serialization-failed",
                format!("cannot serialize graph: {error}"),
            )
        })?;
        digest_value(kind, &value)
    }
}

pub fn compile_modules(
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
                "graph compilation accepts ArchitectureModule documents only",
            ));
            continue;
        }
        let id = document["metadata"]["id"]
            .as_str()
            .expect("schema validated module ID")
            .to_owned();
        if modules.insert(id.clone(), document).is_some() {
            diagnostics.push(Diagnostic::error(
                "module.duplicate-id",
                format!("duplicate module ID {id}"),
            ));
        }
    }
    if !diagnostics.is_empty() {
        return Err(sorted(diagnostics));
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
        return Err(sorted(diagnostics));
    }

    let mut graph = CompiledGraph {
        mode,
        objects: BTreeMap::new(),
        relations: BTreeMap::new(),
        bindings: BTreeMap::new(),
        constraints: BTreeMap::new(),
        reserved_ids,
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

    for (id, entry) in &graph.relations {
        diagnostics.extend(vocabulary.validate_relation(id, &entry.declaration, &object_kinds));
        diagnostics.extend(validate_reference_visibility(
            id,
            entry,
            &entry.declaration["subject"],
            &modules,
            &owners,
        ));
        diagnostics.extend(validate_reference_visibility(
            id,
            entry,
            &entry.declaration["object"],
            &modules,
            &owners,
        ));
    }
    for (id, entry) in &graph.bindings {
        let target = &entry.declaration["target"];
        diagnostics.extend(validate_reference_visibility(
            id, entry, target, &modules, &owners,
        ));
        if target
            .as_str()
            .is_some_and(|target| !graph.objects.contains_key(target))
        {
            diagnostics.push(Diagnostic::error(
                "binding.target-not-active",
                format!("{id} targets a declaration absent from the {mode:?} graph"),
            ));
        }
    }

    diagnostics.extend(evaluate_constraints(&graph));
    diagnostics.extend(validate_forbidden_predicate_cycles(&graph, vocabulary));
    if diagnostics.is_empty() {
        Ok(graph)
    } else {
        Err(sorted(diagnostics))
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
    fn visit(
        module_id: &str,
        modules: &BTreeMap<String, &Value>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        if visited.contains(module_id) {
            return None;
        }
        if !visiting.insert(module_id.to_owned()) {
            return Some(vec![module_id.to_owned()]);
        }
        let document = modules.get(module_id)?;
        for import in document["imports"].as_array()? {
            let imported_id = import["module"].as_str()?;
            if let Some(mut cycle) = visit(imported_id, modules, visiting, visited) {
                cycle.push(module_id.to_owned());
                return Some(cycle);
            }
        }
        visiting.remove(module_id);
        visited.insert(module_id.to_owned());
        None
    }

    let mut visited = BTreeSet::new();
    for module_id in modules.keys() {
        if let Some(mut cycle) = visit(module_id, modules, &mut BTreeSet::new(), &mut visited) {
            cycle.reverse();
            return vec![Diagnostic::error(
                "import.cycle",
                format!("module import cycle: {}", cycle.join(" -> ")),
            )];
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
                    .is_some_and(Vec::is_empty)
                && matches!(
                    value["approval"]["status"].as_str(),
                    Some("granted" | "not_required")
                )
        }
        CompileMode::Review => {
            matches!(status, Some("accepted" | "proposed" | "unresolved"))
        }
    }
}

fn validate_reference_visibility(
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
    let imports_owner = module["imports"]
        .as_array()
        .expect("schema validated imports")
        .iter()
        .any(|import| import["module"].as_str() == Some(&owner.module));
    if !imports_owner {
        return vec![Diagnostic::error(
            "reference.module-not-imported",
            format!(
                "{} references {reference_id} without importing {}",
                declaration.module, owner.module
            ),
        )];
    }

    let owner_module = modules.get(&owner.module).expect("owner module exists");
    let exported = owner_module["exports"][&owner.category]
        .as_array()
        .expect("schema validated exports")
        .iter()
        .any(|id| id.as_str() == Some(reference_id));
    if !exported {
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

fn evaluate_constraints(graph: &CompiledGraph) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, entry) in &graph.constraints {
        let declaration = &entry.declaration;
        let rule = declaration["rule"]
            .as_str()
            .expect("schema validated constraint rule");
        let arguments = &declaration["arguments"];
        match rule {
            "requires_object" => {
                let target = arguments["target"]
                    .as_str()
                    .expect("semantically validated target");
                if !graph.objects.contains_key(target) {
                    diagnostics.push(constraint_failed(
                        id,
                        format!("required object {target} is absent"),
                    ));
                }
            }
            "exactly_one_incoming" => {
                let target = arguments["target"]
                    .as_str()
                    .expect("semantically validated target");
                let predicate = arguments["predicate"]
                    .as_str()
                    .expect("semantically validated predicate");
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
                let predicate = arguments["predicate"].as_str();
                let subject = arguments["subject"].as_str();
                let object = arguments["object"].as_str();
                if graph.relations.values().any(|relation| {
                    relation.declaration["predicate"].as_str() == predicate
                        && relation.declaration["subject"].as_str() == subject
                        && relation.declaration["object"].as_str() == object
                }) {
                    diagnostics.push(constraint_failed(
                        id,
                        "forbidden relation is present".to_owned(),
                    ));
                }
            }
            "no_path" => {
                let from = arguments["from"]
                    .as_str()
                    .expect("semantically validated from");
                let to = arguments["to"].as_str().expect("semantically validated to");
                let predicates = arguments["via"]
                    .as_array()
                    .expect("semantically validated via")
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<BTreeSet<_>>();
                if path_exists(graph, from, to, &predicates) {
                    diagnostics.push(constraint_failed(
                        id,
                        format!("a prohibited path exists from {from} to {to}"),
                    ));
                }
            }
            "must_reference_contract" => {
                let object_kind = arguments["objectKind"].as_str();
                let visibility = arguments.get("visibility").and_then(Value::as_str);
                for (object_id, object) in &graph.objects {
                    if object.declaration["kind"].as_str() != object_kind {
                        continue;
                    }
                    if visibility.is_some()
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

fn validate_forbidden_predicate_cycles(
    graph: &CompiledGraph,
    vocabulary: &Vocabulary,
) -> Vec<Diagnostic> {
    for (predicate, definition) in &vocabulary.predicates {
        if definition.cycles != crate::semantic_validation::CyclePolicy::Forbidden {
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
                        .expect("schema validated subject"),
                    relation.declaration["object"]
                        .as_str()
                        .expect("schema validated object"),
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
                    .expect("schema validated subject"),
                relation.declaration["object"]
                    .as_str()
                    .expect("schema validated object"),
            )
        })
        .collect::<Vec<_>>();
    edge_path_exists(&edges, from, to, false)
}

fn edge_path_exists(edges: &[(&str, &str)], from: &str, to: &str, require_edge: bool) -> bool {
    let mut pending = vec![from];
    let mut visited = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if current == to && (!require_edge || !visited.is_empty()) {
            return true;
        }
        if !visited.insert(current) {
            continue;
        }
        for (_, next) in edges.iter().filter(|(source, _)| *source == current) {
            if *next == to {
                return true;
            }
            pending.push(next);
        }
    }
    false
}

fn constraint_failed(id: &str, message: String) -> Diagnostic {
    Diagnostic::error("constraint.failed", format!("{id}: {message}"))
}

fn sorted(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::{compile_modules, CompileMode, CompiledGraph};
    use crate::{digest_value, parse_restricted_yaml, DigestKind, ParseLimits, Vocabulary};
    use serde_json::{json, Value};
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn empty_graph_has_a_typed_reproducible_digest() {
        let graph = CompiledGraph {
            mode: CompileMode::Governing,
            objects: BTreeMap::new(),
            relations: BTreeMap::new(),
            bindings: BTreeMap::new(),
            constraints: BTreeMap::new(),
            reserved_ids: BTreeSet::new(),
        };

        assert_eq!(
            graph.digest().expect("first"),
            graph.digest().expect("second")
        );
    }

    #[test]
    fn governing_and_review_graphs_filter_decision_status() {
        let vocabulary = core_vocabulary();
        let module = module(
            "goobits.module.status-example",
            &vocabulary,
            json!({
                "goobits.app.accepted": object("accepted"),
                "goobits.app.proposed": object("proposed"),
                "goobits.app.unresolved": object("unresolved"),
                "goobits.app.rejected": object("rejected"),
                "goobits.app.superseded": object("superseded")
            }),
        );

        let governing = compile_modules(
            std::slice::from_ref(&module),
            &vocabulary,
            CompileMode::Governing,
        )
        .expect("governing graph");
        assert_eq!(
            governing.objects.keys().collect::<Vec<_>>(),
            [&"goobits.app.accepted"]
        );

        let review =
            compile_modules(&[module], &vocabulary, CompileMode::Review).expect("review graph");
        assert_eq!(
            review.objects.keys().collect::<Vec<_>>(),
            [
                &"goobits.app.accepted",
                &"goobits.app.proposed",
                &"goobits.app.unresolved"
            ]
        );
        assert_ne!(
            governing.digest().expect("governing digest"),
            review.digest().expect("review digest")
        );
    }

    #[test]
    fn duplicate_and_retired_ids_never_compile() {
        let vocabulary = core_vocabulary();
        let first = module(
            "goobits.module.first",
            &vocabulary,
            json!({"goobits.app.shared": object("accepted")}),
        );
        let second = module(
            "goobits.module.second",
            &vocabulary,
            json!({"goobits.app.shared": object("accepted")}),
        );
        let duplicate = compile_modules(&[first, second], &vocabulary, CompileMode::Governing)
            .expect_err("duplicate ID");
        assert!(duplicate
            .iter()
            .any(|diagnostic| diagnostic.code == "declaration.duplicate-id"));

        let mut retired = module(
            "goobits.module.retired",
            &vocabulary,
            json!({"goobits.app.retired": object("accepted")}),
        );
        retired["retired"] = json!({
            "goobits.app.retired": {
                "retiredInArchitectureVersion": 1,
                "supersededBy": ["goobits.app.successor"],
                "authority": authority()
            }
        });
        let reused = compile_modules(&[retired], &vocabulary, CompileMode::Governing)
            .expect_err("retired ID reuse");
        assert!(reused
            .iter()
            .any(|diagnostic| diagnostic.code == "retired.id-reused"));
    }

    #[test]
    fn exact_import_digest_and_private_exports_are_enforced() {
        let vocabulary = core_vocabulary();
        let mut base = module(
            "goobits.module.base",
            &vocabulary,
            json!({"goobits.app.private": object("accepted")}),
        );
        base["exports"]["objects"] = json!([]);
        let base_digest = digest_value(DigestKind::CanonicalModule, &base).expect("base digest");
        let mut consumer = module(
            "goobits.module.consumer",
            &vocabulary,
            json!({"goobits.app.consumer": object("accepted")}),
        );
        consumer["imports"] = json!([{
            "module": "goobits.module.base",
            "architectureVersion": 1,
            "digest": base_digest.as_str(),
            "source": "base.atlas.yaml"
        }]);
        consumer["relations"] = json!({
            "goobits.relation.consumer-depends-private": {
                "predicate": "depends_on",
                "subject": "goobits.app.consumer",
                "object": "goobits.app.private",
                "decision": lifecycle("accepted")["decision"].clone(),
                "approval": lifecycle("accepted")["approval"].clone(),
                "changeControl": lifecycle("accepted")["changeControl"].clone()
            }
        });

        let diagnostics = compile_modules(
            &[base.clone(), consumer.clone()],
            &vocabulary,
            CompileMode::Governing,
        )
        .expect_err("private reference");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "reference.private-cross-module"));

        consumer["imports"][0]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
        let diagnostics = compile_modules(&[base, consumer], &vocabulary, CompileMode::Governing)
            .expect_err("digest mismatch");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "import.digest-mismatch"));
    }

    fn core_vocabulary() -> Vocabulary {
        let document = parse_restricted_yaml(
            include_bytes!("../../vocabularies/core.v0.1.atlas.yaml"),
            ParseLimits::default(),
        )
        .expect("parse vocabulary");
        Vocabulary::from_document(&document.value).expect("vocabulary")
    }

    fn module(id: &str, vocabulary: &Vocabulary, objects: Value) -> Value {
        let mut exports = objects
            .as_object()
            .expect("object map")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        exports.sort();
        json!({
            "apiVersion": "atlas.codeatlas.dev/v0.1",
            "kind": "ArchitectureModule",
            "metadata": {
                "id": id,
                "name": id,
                "architectureVersion": 1
            },
            "vocabulary": {
                "id": vocabulary.id,
                "version": vocabulary.version,
                "digest": vocabulary.digest.as_str()
            },
            "decision": lifecycle("accepted")["decision"].clone(),
            "approval": lifecycle("accepted")["approval"].clone(),
            "changeControl": lifecycle("accepted")["changeControl"].clone(),
            "imports": [],
            "exports": {
                "objects": exports,
                "relations": [],
                "bindings": [],
                "constraints": []
            },
            "objects": objects,
            "relations": {},
            "bindings": {},
            "constraints": {},
            "retired": {}
        })
    }

    fn object(status: &str) -> Value {
        json!({
            "kind": "app",
            "name": status,
            "attributes": {},
            "decision": lifecycle(status)["decision"].clone(),
            "approval": lifecycle(status)["approval"].clone(),
            "changeControl": lifecycle(status)["changeControl"].clone()
        })
    }

    fn lifecycle(status: &str) -> Value {
        let governing = if status == "accepted" {
            json!([{
                "kind": "owner-decision",
                "artifact": {
                    "id": "goobits.decision.test-architecture",
                    "version": 1
                }
            }])
        } else {
            json!([])
        };
        let supporting = if status == "proposed" {
            json!([{
                "kind": "owner-direction",
                "artifact": {
                    "id": "goobits.direction.test-architecture",
                    "version": 1
                }
            }])
        } else {
            json!([])
        };
        json!({
            "decision": {
                "status": status,
                "authority": {
                    "governing": governing,
                    "supporting": supporting
                }
            },
            "approval": {
                "status": if status == "accepted" { "granted" } else { "required" }
            },
            "changeControl": {
                "policy": "owner_approval_required"
            }
        })
    }

    fn authority() -> Value {
        json!({
            "governing": [{
                "kind": "accepted-adr",
                "artifact": {
                    "id": "goobits.adr.test-retirement",
                    "version": 1
                }
            }],
            "supporting": []
        })
    }
}
