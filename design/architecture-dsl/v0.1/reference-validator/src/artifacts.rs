use crate::{
    compile_modules, digest_value, parse_restricted_yaml, CompileMode, DigestKind, ParseLimits,
    ValidationError, Vocabulary,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const GENERATOR_ID: &str = "codeatlas.tool.reference-validator";
const GENERATOR_VERSION: &str = "0.1.0";
const GENERATED_AT: &str = "2026-07-23T00:00:00Z";
const GENERATION_COMMAND: &str = "cargo run --locked --jobs 1 --manifest-path \
reference-validator/Cargo.toml --bin generate_artifacts -- --write";

pub fn write_generated_artifacts(design_root: &Path) -> Result<(), ValidationError> {
    for (relative_path, bytes) in generated_artifacts(design_root)? {
        write_atomic(&design_root.join(relative_path), &bytes)?;
    }
    let manifest = manifest_bytes(design_root)?;
    write_atomic(&design_root.join("MANIFEST.sha256"), &manifest)?;
    Ok(())
}

pub fn check_generated_artifacts(design_root: &Path) -> Result<(), ValidationError> {
    for (relative_path, expected) in generated_artifacts(design_root)? {
        let path = design_root.join(&relative_path);
        let actual =
            fs::read(&path).map_err(|error| io_error("generated.read-failed", &path, error))?;
        if actual != expected {
            return Err(ValidationError::new(
                "generated.stale",
                format!(
                    "{} is stale, regenerate with the documented command",
                    relative_path.display()
                ),
            ));
        }
    }
    let expected = manifest_bytes(design_root)?;
    let path = design_root.join("MANIFEST.sha256");
    let actual =
        fs::read(&path).map_err(|error| io_error("generated.read-failed", &path, error))?;
    if actual != expected {
        return Err(ValidationError::new(
            "generated.manifest-stale",
            "MANIFEST.sha256 is stale, regenerate with the documented command",
        ));
    }
    Ok(())
}

fn generated_artifacts(design_root: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, ValidationError> {
    let vocabulary_document = read_yaml(&design_root.join("vocabularies/core.v0.1.atlas.yaml"))?;
    let vocabulary = Vocabulary::from_document(&vocabulary_document)
        .map_err(|diagnostics| diagnostics_error("vocabulary.invalid", diagnostics))?;
    let tabby_document =
        read_yaml(&design_root.join("examples/tabby-shelly/architecture.atlas.yaml"))?;
    let policy_document =
        read_yaml(&design_root.join("examples/policy-exception/architecture-policy.atlas.yaml"))?;
    let source_facts = read_yaml(&design_root.join("examples/observation/source-facts.yaml"))?;

    let governing = compile_modules(
        std::slice::from_ref(&tabby_document),
        &vocabulary,
        CompileMode::Governing,
    )
    .map_err(|diagnostics| diagnostics_error("graph.invalid", diagnostics))?;
    let governing_digest = governing.digest()?;
    let module_digest = digest_value(DigestKind::CanonicalModule, &tabby_document)?;
    let architecture_closure = json!({
        "roots": ["goobits.product.tabby-shelly"],
        "modules": [{
            "id": "goobits.product.tabby-shelly",
            "canonicalModuleDigest": module_digest.as_str()
        }],
        "vocabulary": {
            "id": vocabulary.id,
            "version": vocabulary.version,
            "digest": vocabulary.digest.as_str()
        },
        "validatorVersion": format!("{GENERATOR_ID}/{GENERATOR_VERSION}")
    });
    let architecture_closure_digest =
        digest_value(DigestKind::ArchitectureClosure, &architecture_closure)?;
    let policy_closure_digest = digest_value(DigestKind::PolicyClosure, &policy_document)?;

    let observation_content = json!({
        "coverage": source_facts["coverage"].clone(),
        "facts": source_facts["facts"].clone()
    });
    let observation_content_digest =
        digest_value(DigestKind::ObservationContent, &observation_content)?;
    let observation_envelope = json!({
        "repository": source_facts["repository"].clone(),
        "sourceCommit": source_facts["sourceCommit"].clone(),
        "observationContentDigest": observation_content_digest.as_str(),
        "generator": {
            "id": GENERATOR_ID,
            "version": GENERATOR_VERSION
        },
        "generatedAt": GENERATED_AT,
        "sourceInputs": ["examples/observation/source-facts.yaml"],
        "generationCommand": GENERATION_COMMAND
    });
    let observation_envelope_digest =
        digest_value(DigestKind::ObservationEnvelope, &observation_envelope)?;

    let observation = json!({
        "apiVersion": "atlas.codeatlas.dev/v0.1",
        "kind": "ArchitectureObservation",
        "metadata": generated_metadata(
            "codeatlas.observation.tabby-example",
            "Tabby example observation",
            &["examples/observation/source-facts.yaml"]
        ),
        "vocabulary": vocabulary_reference(&vocabulary),
        "repository": source_facts["repository"].clone(),
        "sourceCommit": source_facts["sourceCommit"].clone(),
        "coverage": source_facts["coverage"].clone(),
        "facts": source_facts["facts"].clone(),
        "digests": {
            "observationContentDigest": observation_content_digest.as_str(),
            "observationEnvelopeDigest": observation_envelope_digest.as_str()
        }
    });

    let conformance_inputs = json!({
        "governingGraphDigest": governing_digest.as_str(),
        "architectureClosureDigest": architecture_closure_digest.as_str(),
        "policyClosureDigest": policy_closure_digest.as_str(),
        "observationContentDigest": observation_content_digest.as_str(),
        "vocabularyDigest": vocabulary.digest.as_str(),
        "validatorVersion": format!("{GENERATOR_ID}/{GENERATOR_VERSION}"),
        "asOf": GENERATED_AT
    });
    let results = json!({
        "codeatlas.conformance.tabby-package": {
            "declarationId": "goobits.app.tabby",
            "state": "matched",
            "severity": "advisory",
            "reasonCode": "binding.exact-match",
            "evidence": {
                "factIds": ["codeatlas.fact.tabby-package"],
                "coverageIds": ["codeatlas.coverage.npm-packages"]
            },
            "exceptions": empty_exception_dispositions(),
            "explanation": "The accepted Tabby package binding matched deterministic evidence."
        },
        "codeatlas.conformance.shell-create": {
            "declarationId": "goobits.capability.shell-create",
            "state": "unobserved",
            "severity": "advisory",
            "reasonCode": "coverage.unsupported",
            "evidence": {
                "factIds": [],
                "coverageIds": []
            },
            "exceptions": empty_exception_dispositions(),
            "explanation": "No extractor coverage evaluates the Shell creation capability."
        },
        "codeatlas.conformance.tab-host-candidate": {
            "declarationId": "goobits.capability.tab-host",
            "state": "ambiguous",
            "severity": "advisory",
            "reasonCode": "evidence.inferred-review-only",
            "evidence": {
                "factIds": ["codeatlas.fact.tab-host-candidate"],
                "coverageIds": ["codeatlas.coverage.tab-composition"]
            },
            "exceptions": empty_exception_dispositions(),
            "explanation": "Inferred partial evidence cannot establish the accepted tab-host implementation."
        }
    });
    let result_payload = json!({
        "conformanceInputs": conformance_inputs.clone(),
        "result": results.clone()
    });
    let result_digest = digest_value(DigestKind::ConformanceResult, &result_payload)?;
    let conformance = json!({
        "apiVersion": "atlas.codeatlas.dev/v0.1",
        "kind": "ArchitectureConformance",
        "metadata": generated_metadata(
            "codeatlas.conformance.tabby-example",
            "Tabby example conformance",
            &[
                "examples/tabby-shelly/architecture.atlas.yaml",
                "examples/observation/architecture-observation.generated.yaml",
                "examples/policy-exception/architecture-policy.atlas.yaml"
            ]
        ),
        "vocabulary": vocabulary_reference(&vocabulary),
        "conformanceInputs": conformance_inputs,
        "results": results,
        "conformanceResultDigest": result_digest.as_str()
    });

    Ok(vec![
        (
            PathBuf::from("examples/observation/architecture-observation.generated.yaml"),
            yaml_bytes(&observation)?,
        ),
        (
            PathBuf::from("examples/conformance/architecture-conformance.generated.yaml"),
            yaml_bytes(&conformance)?,
        ),
    ])
}

fn generated_metadata(id: &str, name: &str, source_inputs: &[&str]) -> Value {
    json!({
        "id": id,
        "name": name,
        "architectureVersion": 1,
        "generated": true,
        "generator": {
            "id": GENERATOR_ID,
            "version": GENERATOR_VERSION
        },
        "generatedAt": GENERATED_AT,
        "sourceInputs": source_inputs,
        "generationCommand": GENERATION_COMMAND,
        "manualEditing": "prohibited"
    })
}

fn vocabulary_reference(vocabulary: &Vocabulary) -> Value {
    json!({
        "id": vocabulary.id,
        "version": vocabulary.version,
        "digest": vocabulary.digest.as_str()
    })
}

fn empty_exception_dispositions() -> Value {
    json!({
        "applied": [],
        "stale": [],
        "expired": [],
        "irrelevant": [],
        "rejected": []
    })
}

fn read_yaml(path: &Path) -> Result<Value, ValidationError> {
    let bytes =
        fs::read(path).map_err(|error| io_error("generated.input-read-failed", path, error))?;
    parse_restricted_yaml(&bytes, ParseLimits::default())
        .map(|document| document.value)
        .map_err(|error| error.at_path(path))
}

fn yaml_bytes(value: &Value) -> Result<Vec<u8>, ValidationError> {
    let mut output = serde_yaml::to_string(value).map_err(|error| {
        ValidationError::new("generated.yaml-serialization-failed", error.to_string())
    })?;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output.into_bytes())
}

fn manifest_bytes(design_root: &Path) -> Result<Vec<u8>, ValidationError> {
    let mut files = Vec::new();
    collect_manifest_files(design_root, design_root, &mut files)?;
    files.sort();

    let mut output = String::from(
        "# generated: true\n\
# generator: codeatlas.tool.reference-validator/0.1.0\n\
# command: cargo run --locked --jobs 1 --manifest-path reference-validator/Cargo.toml --bin generate_artifacts -- --write\n\
# manual-editing: prohibited\n\
# excludes: MANIFEST.sha256 and reference-validator/target/\n",
    );
    for relative_path in files {
        let bytes = fs::read(design_root.join(&relative_path)).map_err(|error| {
            io_error(
                "generated.manifest-input-read-failed",
                &relative_path,
                error,
            )
        })?;
        let digest = Sha256::digest(bytes);
        output.push_str(&format!(
            "{digest:x}  {}\n",
            relative_path.to_string_lossy().replace('\\', "/")
        ));
    }
    Ok(output.into_bytes())
}

fn collect_manifest_files(
    design_root: &Path,
    directory: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), ValidationError> {
    let entries = fs::read_dir(directory)
        .map_err(|error| io_error("generated.manifest-read-directory", directory, error))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| io_error("generated.manifest-read-entry", directory, error))?;
        let path = entry.path();
        let relative = path.strip_prefix(design_root).map_err(|error| {
            ValidationError::new(
                "generated.manifest-path-error",
                format!("{}: {error}", path.display()),
            )
        })?;
        if relative == Path::new("MANIFEST.sha256")
            || relative.starts_with("reference-validator/target")
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("generated.manifest-file-type", &path, error))?;
        if file_type.is_symlink() {
            return Err(ValidationError::new(
                "generated.manifest-symlink-prohibited",
                format!("manifest input cannot be a symlink: {}", path.display()),
            ));
        }
        if file_type.is_dir() {
            collect_manifest_files(design_root, &path, files)?;
        } else if file_type.is_file() {
            files.push(relative.to_path_buf());
        }
    }
    Ok(())
}

fn write_atomic(destination: &Path, bytes: &[u8]) -> Result<(), ValidationError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| io_error("generated.create-directory", parent, error))?;
    }
    let temporary = destination.with_extension("generated.tmp");
    fs::write(&temporary, bytes)
        .map_err(|error| io_error("generated.write-failed", &temporary, error))?;
    fs::rename(&temporary, destination)
        .map_err(|error| io_error("generated.replace-failed", destination, error))
}

fn diagnostics_error(code: &str, diagnostics: Vec<crate::Diagnostic>) -> ValidationError {
    ValidationError::new(
        code,
        diagnostics
            .into_iter()
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

fn io_error(code: &str, path: &Path, error: std::io::Error) -> ValidationError {
    ValidationError::new(code, format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{generated_artifacts, GENERATOR_ID};
    use crate::{parse_restricted_yaml, validate_document_schema, ParseLimits};
    use std::path::Path;

    #[test]
    fn generated_examples_are_schema_valid_and_identify_their_generator() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("design root");
        let artifacts = generated_artifacts(root).expect("generate");
        assert_eq!(artifacts.len(), 2);

        for (_, bytes) in artifacts {
            let document =
                parse_restricted_yaml(&bytes, ParseLimits::default()).expect("parse generated");
            let diagnostics = validate_document_schema(&document.value);
            assert!(diagnostics.is_empty(), "{diagnostics:#?}");
            assert_eq!(
                document.value["metadata"]["generator"]["id"].as_str(),
                Some(GENERATOR_ID)
            );
            assert_eq!(
                document.value["metadata"]["manualEditing"].as_str(),
                Some("prohibited")
            );
        }
    }
}
