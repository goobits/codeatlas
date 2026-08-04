use super::compiler::CompileResult;
use super::diagnostic::Diagnostic;
use super::digest::{digest_value, DigestKind, TypedDigest};
use super::vocabulary::{is_qualified_identifier, Vocabulary};
use super::{ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::str::FromStr;

pub(crate) fn load(path: &Path) -> Result<CompileResult, Vec<Diagnostic>> {
    let bytes = fs::read(path).map_err(|error| {
        vec![Diagnostic::error(
            "baseline.read-failed",
            format!("{}: {error}", path.display()),
        )
        .at_path(path)]
    })?;
    let compilation = serde_json::from_slice::<CompileResult>(&bytes).map_err(|error| {
        vec![Diagnostic::error(
            "baseline.decode-failed",
            format!("{}: {error}", path.display()),
        )
        .at_path(path)]
    })?;
    let diagnostics = validate_loaded(&compilation)
        .into_iter()
        .map(|diagnostic| diagnostic.at_path(path))
        .collect::<Vec<_>>();
    if diagnostics.is_empty() {
        Ok(compilation)
    } else {
        Err(diagnostics)
    }
}

fn validate_loaded(compilation: &CompileResult) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let report = &compilation.report;
    let lockfile = &compilation.lockfile;
    if report.schema_version != ARCHITECTURE_SCHEMA_VERSION
        || lockfile.schema_version != ARCHITECTURE_SCHEMA_VERSION
    {
        diagnostics.push(Diagnostic::error(
            "baseline.schema-version-unsupported",
            format!("architecture baseline requires schema version {ARCHITECTURE_SCHEMA_VERSION}"),
        ));
    }
    if report.api_version != ARCHITECTURE_API_VERSION
        || lockfile.api_version != ARCHITECTURE_API_VERSION
    {
        diagnostics.push(Diagnostic::error(
            "baseline.api-version-unsupported",
            format!("architecture baseline requires API version {ARCHITECTURE_API_VERSION}"),
        ));
    }
    if report.mode != report.graph.mode {
        diagnostics.push(Diagnostic::error(
            "baseline.mode-mismatch",
            "compilation mode and graph mode differ",
        ));
    }
    if report.roots != lockfile.roots {
        diagnostics.push(Diagnostic::error(
            "baseline.roots-mismatch",
            "compilation roots and lockfile roots differ",
        ));
    }
    if report.roots.is_empty()
        || report.roots.iter().collect::<BTreeSet<_>>().len() != report.roots.len()
    {
        diagnostics.push(Diagnostic::error(
            "baseline.roots-invalid",
            "architecture baseline roots must be non-empty and unique",
        ));
    }
    if report.vocabulary != lockfile.vocabulary {
        diagnostics.push(Diagnostic::error(
            "baseline.vocabulary-mismatch",
            "compilation and lockfile use different vocabularies",
        ));
    }
    match Vocabulary::bundled() {
        Ok(vocabulary) if report.vocabulary != vocabulary.identity() => {
            diagnostics.push(Diagnostic::error(
                "baseline.vocabulary-unsupported",
                "architecture baseline does not use the current bundled vocabulary",
            ));
        }
        Err(mut errors) => diagnostics.append(&mut errors),
        Ok(_) => {}
    }
    if !lockfile.generated || lockfile.manual_editing != "prohibited" {
        diagnostics.push(Diagnostic::error(
            "baseline.lockfile-provenance-invalid",
            "architecture lockfile must be generated and prohibit manual editing",
        ));
    }
    if report.tool_version.is_empty()
        || report.compiler_version.is_empty()
        || lockfile.generator.id.is_empty()
        || lockfile.generator.version.is_empty()
    {
        diagnostics.push(Diagnostic::error(
            "baseline.generator-invalid",
            "architecture baseline generator identities must be non-empty",
        ));
    }

    match report.graph.digest() {
        Ok(digest) if digest != report.graph_digest => diagnostics.push(Diagnostic::error(
            "baseline.graph-digest-mismatch",
            "compiled graph does not match its recorded digest",
        )),
        Err(error) => diagnostics.push(*error.diagnostic),
        Ok(_) => {}
    }

    let mut lock_modules = BTreeMap::new();
    for document in &lockfile.documents {
        if lock_modules
            .insert(&document.module_id, &document.import_closure_digest)
            .is_some()
        {
            diagnostics.push(Diagnostic::error(
                "baseline.lockfile-module-duplicate",
                format!("lockfile repeats module {}", document.module_id),
            ));
        }
        if !is_qualified_identifier(&document.module_id) || document.architecture_version == 0 {
            diagnostics.push(Diagnostic::error(
                "baseline.lockfile-module-invalid",
                format!(
                    "lockfile module {} has invalid identity",
                    document.module_id
                ),
            ));
        }
        for digest in [
            &document.source_document_digest,
            &document.canonical_module_digest,
            &document.import_closure_digest,
        ] {
            if TypedDigest::from_str(digest.as_str()).is_err() {
                diagnostics.push(Diagnostic::error(
                    "baseline.digest-invalid",
                    format!(
                        "lockfile module {} has an invalid digest",
                        document.module_id
                    ),
                ));
            }
        }
    }
    let roots_are_known = report.roots.iter().all(|root| {
        if lock_modules.contains_key(root) {
            true
        } else {
            diagnostics.push(Diagnostic::error(
                "baseline.root-unresolved",
                format!("baseline root {root} is absent from the lockfile"),
            ));
            false
        }
    });
    if roots_are_known {
        match digest_value(
            DigestKind::ArchitectureClosure,
            &json!({
                "roots": report.roots.iter().map(|id| {
                    json!({
                        "moduleId": id,
                        "importClosureDigest": lock_modules[id],
                    })
                }).collect::<Vec<_>>(),
                "vocabulary": {
                    "id": report.vocabulary.id,
                    "version": report.vocabulary.version,
                    "digest": report.vocabulary.digest,
                },
                "compilerVersion": report.compiler_version,
            }),
        ) {
            Ok(digest) if digest != report.architecture_closure_digest => {
                diagnostics.push(Diagnostic::error(
                    "baseline.architecture-closure-digest-mismatch",
                    "architecture closure does not match its recorded digest",
                ));
            }
            Err(error) => diagnostics.push(*error.diagnostic),
            Ok(_) => {}
        }
    }

    let lock_module_ids = lock_modules.keys().copied().collect::<BTreeSet<_>>();
    let mut declaration_ids = BTreeSet::new();
    for (category, declarations) in [
        ("object", &report.graph.objects),
        ("relation", &report.graph.relations),
        ("binding", &report.graph.bindings),
        ("constraint", &report.graph.constraints),
    ] {
        for (id, declaration) in declarations {
            if !is_qualified_identifier(id) || !declaration_ids.insert(id) {
                diagnostics.push(Diagnostic::error(
                    "baseline.declaration-id-invalid",
                    format!("{category} {id} has an invalid or duplicate identity"),
                ));
            }
            if !lock_module_ids.contains(&declaration.module) {
                diagnostics.push(Diagnostic::error(
                    "baseline.declaration-owner-unresolved",
                    format!(
                        "{category} {id} names unlocked module {}",
                        declaration.module
                    ),
                ));
            }
            if !declaration.declaration.is_object() {
                diagnostics.push(Diagnostic::error(
                    "baseline.declaration-invalid",
                    format!("{category} {id} is not an object declaration"),
                ));
            }
        }
    }
    for (id, binding) in &report.graph.bindings {
        let declaration = &binding.declaration;
        let target = declaration
            .get("target")
            .and_then(serde_json::Value::as_str);
        let adapter = declaration
            .pointer("/adapter/kind")
            .and_then(serde_json::Value::as_str);
        let selector = declaration
            .pointer("/selector/name")
            .and_then(serde_json::Value::as_str);
        let cardinality = declaration
            .get("cardinality")
            .and_then(serde_json::Value::as_str);
        if target.is_none_or(|target| !report.graph.objects.contains_key(target))
            || adapter.is_none()
            || selector.is_none()
            || !matches!(
                cardinality,
                Some("exactly_one" | "at_most_one" | "one_or_more" | "any")
            )
        {
            diagnostics.push(Diagnostic::error(
                "baseline.binding-invalid",
                format!("binding {id} lacks a valid target, adapter, selector, or cardinality"),
            ));
        }
    }
    for (id, object) in &report.graph.objects {
        if object
            .declaration
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .is_none()
        {
            diagnostics.push(Diagnostic::error(
                "baseline.object-invalid",
                format!("object {id} lacks a valid kind"),
            ));
        }
    }
    diagnostics
}
