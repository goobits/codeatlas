use super::diagnostic::{sort_diagnostics, Diagnostic};
use super::digest::{digest_value, DigestKind, TypedDigest};
use super::documents::DocumentSet;
use super::model::valid_timestamp;
use super::vocabulary::Vocabulary;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExceptionDisposition {
    Applied,
    Stale,
    Expired,
    Irrelevant,
    Rejected,
}

#[derive(Clone, Debug)]
pub(super) struct PolicyException {
    pub id: String,
    pub declaration: Value,
}

pub(super) struct PolicySet {
    pub digest: TypedDigest,
    pub exceptions: Vec<PolicyException>,
}

pub(super) struct ExceptionContext<'a> {
    pub constraint_id: &'a str,
    pub constraint_version: u64,
    pub declaration_id: &'a str,
    pub declaring_module: &'a str,
    pub affected_closure_digest: &'a str,
    pub as_of: &'a str,
}

impl PolicySet {
    pub fn load(
        roots: &[PathBuf],
        allowed_root: &Path,
        vocabulary: &Vocabulary,
    ) -> Result<Self, Vec<Diagnostic>> {
        if roots.is_empty() {
            let digest = digest_value(
                DigestKind::PolicyClosure,
                &json!({
                    "roots": [],
                    "documents": [],
                    "vocabulary": vocabulary.identity(),
                }),
            )
            .map_err(|error| vec![*error.diagnostic])?;
            return Ok(Self {
                digest,
                exceptions: Vec::new(),
            });
        }
        let loaded = DocumentSet::load(roots, allowed_root, "ArchitecturePolicy", vocabulary)?;
        let mut diagnostics = Vec::new();
        let mut declarations = BTreeMap::<String, String>::new();
        let mut exceptions = Vec::new();
        for (policy_id, document) in &loaded.documents {
            for category in ["rules", "exceptions"] {
                for (id, declaration) in document.value[category]
                    .as_object()
                    .expect("validated policy declarations")
                {
                    if let Some(previous_policy) =
                        declarations.insert(id.clone(), policy_id.clone())
                    {
                        diagnostics.push(Diagnostic::error(
                            "policy.duplicate-declaration-id",
                            format!("{id} is declared by both {previous_policy} and {policy_id}"),
                        ));
                    }
                    if category == "exceptions" {
                        exceptions.push(PolicyException {
                            id: id.clone(),
                            declaration: declaration.clone(),
                        });
                    }
                }
            }
        }
        if !diagnostics.is_empty() {
            sort_diagnostics(&mut diagnostics);
            return Err(diagnostics);
        }
        exceptions.sort_by(|left, right| left.id.cmp(&right.id));
        let mut documents = Vec::with_capacity(loaded.documents.len());
        for (id, document) in &loaded.documents {
            documents.push(json!({
                "id": id,
                "canonicalModuleDigest": &document.canonical_digest,
                "importClosureDigest": loaded.import_closure_digest(id)?,
            }));
        }
        let digest = digest_value(
            DigestKind::PolicyClosure,
            &json!({
                "roots": &loaded.roots,
                "documents": documents,
                "vocabulary": vocabulary.identity(),
            }),
        )
        .map_err(|error| vec![*error.diagnostic])?;
        Ok(Self { digest, exceptions })
    }
}

pub(super) fn evaluate_exception(
    exception: &PolicyException,
    context: &ExceptionContext<'_>,
) -> Option<ExceptionDisposition> {
    let declaration = &exception.declaration;
    let affected = declaration["affectedIds"]
        .as_array()
        .expect("validated affected IDs")
        .iter()
        .filter_map(Value::as_str)
        .any(|id| id == context.declaration_id);
    let scoped = declaration["scope"]["module"].as_str() == Some(context.declaring_module);
    let constraint_matches = declaration["constraint"]["id"].as_str()
        == Some(context.constraint_id)
        && declaration["constraint"]["version"].as_u64() == Some(context.constraint_version);
    if !affected && !scoped && !constraint_matches {
        return None;
    }
    if !affected || !scoped || !constraint_matches {
        return Some(ExceptionDisposition::Irrelevant);
    }
    if declaration["decision"]["status"].as_str() != Some("accepted")
        || !matches!(
            declaration["approval"]["status"].as_str(),
            Some("granted" | "not_required")
        )
        || declaration["decision"]["authority"]["governing"]
            .as_array()
            .is_none_or(Vec::is_empty)
    {
        return Some(ExceptionDisposition::Rejected);
    }
    if declaration["baseClosureDigest"].as_str() != Some(context.affected_closure_digest) {
        return Some(ExceptionDisposition::Stale);
    }
    let expires_at = declaration["expiresAt"]
        .as_str()
        .expect("validated expiration");
    if !valid_timestamp(context.as_of) || !valid_timestamp(expires_at) {
        return Some(ExceptionDisposition::Rejected);
    }
    if context.as_of >= expires_at {
        return Some(ExceptionDisposition::Expired);
    }
    Some(ExceptionDisposition::Applied)
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_exception, ExceptionContext, ExceptionDisposition, PolicyException, PolicySet,
    };
    use crate::architecture::vocabulary::Vocabulary;
    use serde_json::json;
    use std::path::PathBuf;

    fn exception() -> PolicyException {
        PolicyException {
            id: "goobits.exception.example".to_owned(),
            declaration: json!({
                "constraint": {
                    "id": "goobits.constraint.example",
                    "version": 1
                },
                "scope": {"module": "goobits.module.example"},
                "affectedIds": ["goobits.object.example"],
                "baseClosureDigest": format!("sha256:{}", "a".repeat(64)),
                "expiresAt": "2026-09-01T00:00:00Z",
                "decision": {
                    "status": "accepted",
                    "authority": {
                        "governing": [{
                            "kind": "accepted-adr",
                            "artifact": {
                                "id": "goobits.adr.example",
                                "version": 1
                            }
                        }],
                        "supporting": []
                    }
                },
                "approval": {"status": "granted"}
            }),
        }
    }

    fn context<'a>(closure: &'a str, as_of: &'a str) -> ExceptionContext<'a> {
        ExceptionContext {
            constraint_id: "goobits.constraint.example",
            constraint_version: 1,
            declaration_id: "goobits.object.example",
            declaring_module: "goobits.module.example",
            affected_closure_digest: closure,
            as_of,
        }
    }

    #[test]
    fn exact_closure_and_recorded_time_control_exception_disposition() {
        let accepted = exception();
        let matching = format!("sha256:{}", "a".repeat(64));
        let changed = format!("sha256:{}", "b".repeat(64));
        assert_eq!(
            evaluate_exception(&accepted, &context(&matching, "2026-08-01T00:00:00Z")),
            Some(ExceptionDisposition::Applied)
        );
        assert_eq!(
            evaluate_exception(&accepted, &context(&changed, "2026-08-01T00:00:00Z")),
            Some(ExceptionDisposition::Stale)
        );
        assert_eq!(
            evaluate_exception(&accepted, &context(&matching, "2026-09-01T00:00:00Z")),
            Some(ExceptionDisposition::Expired)
        );
    }

    #[test]
    fn unrelated_exceptions_do_not_bloat_results() {
        let accepted = exception();
        let matching = format!("sha256:{}", "a".repeat(64));
        let mut unrelated = context(&matching, "2026-08-01T00:00:00Z");
        unrelated.constraint_id = "goobits.constraint.unrelated";
        unrelated.declaration_id = "goobits.object.unrelated";
        unrelated.declaring_module = "goobits.module.unrelated";
        assert_eq!(evaluate_exception(&accepted, &unrelated), None);
    }

    #[test]
    fn accepted_policy_example_compiles_to_a_deterministic_closure() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let paths = vec![root.join(
            "spec/architecture/v0.1/examples/policy-exception/architecture-policy.atlas.yaml",
        )];
        let vocabulary = Vocabulary::bundled().expect("vocabulary");
        let first = PolicySet::load(&paths, &root, &vocabulary).expect("first policy load");
        let second = PolicySet::load(&paths, &root, &vocabulary).expect("second policy load");
        assert_eq!(first.digest, second.digest);
        assert_eq!(first.exceptions.len(), 1);
    }
}
