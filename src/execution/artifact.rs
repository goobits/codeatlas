mod identity;
mod store;

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;

#[cfg(test)]
use crate::execution::model::{ArtifactPayload, ExecutionPlan, ExecutionPlanBody};
#[cfg(test)]
use serde_json::json;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[cfg(test)]
pub(crate) use identity::is_namespaced_artifact_version;
pub(crate) use identity::{
    digest_bytes, digest_file, digest_value, validate_artifact_id, validate_artifact_links,
    validate_digest, validate_execution_limits, validate_tool_identity,
};
pub(crate) use store::{ArtifactRef, ArtifactStore};

pub(crate) trait ManagedArtifact: Serialize + DeserializeOwned {
    const DIRECTORY: &'static str;
    const PREFIX: &'static str;
    const LABEL: &'static str;

    fn artifact_id(&self) -> &str;
    fn verify_identity(&self) -> Result<()>;
}

#[cfg(unix)]
pub(super) fn has_file_metadata_changed(
    before: &std::fs::Metadata,
    after: &std::fs::Metadata,
) -> bool {
    before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
}

#[cfg(not(unix))]
pub(super) fn has_file_metadata_changed(
    before: &std::fs::Metadata,
    after: &std::fs::Metadata,
) -> bool {
    before.len() != after.len() || before.modified().ok() != after.modified().ok()
}

#[cfg(test)]
pub(crate) fn sample_plan() -> ExecutionPlan {
    let tool_digest = format!("sha256:{}", "1".repeat(64));
    let engine_digest = format!("sha256:{}", "2".repeat(64));
    let body: ExecutionPlanBody = serde_json::from_value(json!({
        "subject": "http",
        "operation": "fuzz",
        "tool": {"name": "codeatlas", "version": "1.0.0", "digest": tool_digest.clone()},
        "engine": {"name": "fixture", "version": "2.0.0", "digest": engine_digest.clone()},
        "evidence": {
            "workspace": format!("sha256:{}", "3".repeat(64)),
            "config": format!("sha256:{}", "4".repeat(64)),
            "target": format!("sha256:{}", "5".repeat(64)),
            "contract": format!("sha256:{}", "6".repeat(64)),
            "tool": tool_digest,
            "engine": engine_digest,
            "policy": format!("sha256:{}", "7".repeat(64))
        },
        "target": {
            "id": "fixture",
            "class": "local_disposable",
            "secret_references": []
        },
        "workload": ArtifactPayload::from_serializable(
            "codeatlas.fixture-workload/v1",
            &json!({"nested": {"β": 2, "a": "€"}})
        ).expect("fixture payload"),
        "effects": ["filesystem_scratch", "network_target_call"],
        "required_capabilities": ["cleanup_verification", "network_allowlist"],
        "destinations": [{"scheme": "http", "host": "127.0.0.1", "port": 8080}],
        "managed_commands": [],
        "managed_images": [],
        "expected_calls": [],
        "writable_scratch_roots": [{
            "logical_name": "execution_scratch",
            "owner": "execution_kernel"
        }],
        "limits": {
            "max_calls": 2,
            "calls_per_second": 1,
            "max_concurrency": 1,
            "run_timeout_ms": 1000,
            "max_cpu_time_ms": 900,
            "max_rss_bytes": 1048576,
            "max_processes": 2,
            "max_open_files": 16,
            "max_call_result_bytes": 1024,
            "max_output_bytes": 2048,
            "max_artifact_bytes": 4096
        },
        "isolation": {
            "backend": "container",
            "filesystem": "scratch_only",
            "network": "proxy_only",
            "processes": "planned_only"
        },
        "authorization": {
            "class": "local_disposable",
            "disposition": "reviewed_plan_required",
            "reasons": ["explicit review fixture"]
        }
    }))
    .expect("fixture plan body");
    ExecutionPlan::new(body).expect("fixture plan")
}

#[cfg(test)]
mod tests {
    use super::{digest_value, sample_plan, ArtifactRef, ArtifactStore};
    use crate::execution::model::{
        ArtifactLink, ExecutionPlan, ManagedCommandEvidence, ManagedImageEvidence,
    };
    use serde_json::json;

    #[test]
    fn rfc_8785_vectors_and_integer_guard_are_exact() {
        let value = json!({
            "numbers": [333_333_333.333_333_3_f64, 1E30_f64, 4.50_f64, 2e-3_f64, 1e-27_f64],
            "string": "€$\u{000f}\nA'B\"\\\\\"/"
        });
        let canonical = serde_json_canonicalizer::to_string(&value).expect("RFC 8785 vector");
        assert_eq!(
            canonical,
            "{\"numbers\":[333333333.3333333,1e+30,4.5,0.002,1e-27],\"string\":\"€$\\u000f\\nA'B\\\"\\\\\\\\\\\"/\"}"
        );
        assert!(digest_value(
            "atlas.codeatlas.dev/test/v1",
            &json!({"unsafe": 9_007_199_254_740_992_u64})
        )
        .is_err());
    }

    #[test]
    fn codeatlas_plan_domain_and_identity_body_have_one_exact_vector() {
        let plan = sample_plan();
        assert_eq!(
            plan.id,
            "plan_bcc4125edebc92b104807e3f3cd823c019f544dec28c8d66671106bc188f9856"
        );
        let mut changed = plan.body.clone();
        changed.operation = "fuzz-changed".to_string();
        assert_ne!(
            ExecutionPlan::new(changed).expect("changed plan").id,
            plan.id
        );
    }

    #[test]
    fn plan_links_and_managed_command_owners_are_unambiguous() {
        let mut duplicate_links = sample_plan().body;
        duplicate_links.links = vec![
            ArtifactLink {
                kind: "report".to_string(),
                id: format!("report_{}", "a".repeat(64)),
                content_digest: format!("sha256:{}", "1".repeat(64)),
            },
            ArtifactLink {
                kind: "report".to_string(),
                id: format!("report_{}", "a".repeat(64)),
                content_digest: format!("sha256:{}", "2".repeat(64)),
            },
        ];
        assert!(ExecutionPlan::new(duplicate_links).is_err());

        let mut duplicate_owners = sample_plan().body;
        duplicate_owners.managed_commands = vec![
            ManagedCommandEvidence {
                owner: "engine".to_string(),
                digest: format!("sha256:{}", "1".repeat(64)),
            },
            ManagedCommandEvidence {
                owner: "engine".to_string(),
                digest: format!("sha256:{}", "2".repeat(64)),
            },
        ];
        assert!(ExecutionPlan::new(duplicate_owners).is_err());
    }

    #[test]
    fn managed_image_evidence_is_digest_pinned_and_has_one_owner() {
        let image = ManagedImageEvidence {
            owner: "workload".to_string(),
            reference: format!("example.invalid/workload@sha256:{}", "a".repeat(64)),
            manifest_digest: format!("sha256:{}", "a".repeat(64)),
        };
        let mut mismatched = sample_plan().body;
        mismatched.managed_images = vec![ManagedImageEvidence {
            manifest_digest: format!("sha256:{}", "b".repeat(64)),
            ..image.clone()
        }];
        assert!(ExecutionPlan::new(mismatched).is_err());

        let mut duplicate_owners = sample_plan().body;
        duplicate_owners.managed_images = vec![image.clone(), image];
        assert!(ExecutionPlan::new(duplicate_owners).is_err());
    }

    #[test]
    fn artifact_references_distinguish_strict_ids_from_paths() {
        let id = format!("plan_{}", "a".repeat(64));
        assert_eq!(
            ArtifactRef::parse(&id).expect("plan ID"),
            ArtifactRef::Id(id)
        );
        assert!(ArtifactRef::parse("plan_bad").is_err());
        assert_eq!(
            ArtifactRef::parse("artifacts/plan.json").expect("artifact path"),
            ArtifactRef::Path("artifacts/plan.json".into())
        );
    }

    #[test]
    fn artifact_store_refuses_a_workspace_owned_root() {
        let workspace =
            std::env::temp_dir().join(format!("codeatlas-artifact-overlap-{}", std::process::id()));
        std::fs::create_dir_all(&workspace).expect("workspace fixture");
        let state = workspace.join("state");
        assert!(ArtifactStore::for_tests(state.clone(), &workspace, 1024).is_err());
        assert!(!state.exists(), "rejected state root must not be created");
        std::fs::remove_dir_all(workspace).expect("remove workspace fixture");
    }

    #[test]
    fn artifact_store_round_trips_exact_identity_and_rejects_tampering() {
        let fixture = std::env::temp_dir().join(format!(
            "codeatlas-artifact-round-trip-{}",
            std::process::id()
        ));
        let workspace = fixture.join("workspace");
        let state = fixture.join("state");
        std::fs::create_dir_all(&workspace).expect("workspace fixture");
        let store = ArtifactStore::for_tests(state, &workspace, 1024 * 1024).expect("store");
        let plan = sample_plan();
        let path = store.persist(&plan).expect("persist plan");
        let loaded: ExecutionPlan = store
            .load(&ArtifactRef::Id(plan.id.clone()))
            .expect("load exact plan");
        assert_eq!(loaded, plan);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                .expect("weaken fixture permissions");
            assert!(store
                .load::<ExecutionPlan>(&ArtifactRef::Id(plan.id.clone()))
                .is_err());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("restore fixture permissions");
        }

        let mut tampered = serde_json::to_value(&plan).expect("plan JSON");
        tampered["operation"] = json!("tampered");
        std::fs::write(&path, serde_json::to_vec(&tampered).expect("tampered JSON"))
            .expect("tamper fixture");
        assert!(store
            .load::<ExecutionPlan>(&ArtifactRef::Id(plan.id.clone()))
            .is_err());
        assert!(store.persist(&plan).is_err());
        std::fs::remove_dir_all(fixture).expect("remove artifact fixture");
    }

    #[cfg(unix)]
    #[test]
    fn artifact_store_refuses_a_symlinked_kind_directory() {
        use std::os::unix::fs::symlink;

        let fixture =
            std::env::temp_dir().join(format!("codeatlas-artifact-symlink-{}", std::process::id()));
        let workspace = fixture.join("workspace");
        let state = fixture.join("state");
        let outside = fixture.join("outside");
        std::fs::create_dir_all(&workspace).expect("workspace fixture");
        std::fs::create_dir_all(&outside).expect("outside fixture");
        let store = ArtifactStore::for_tests(state.clone(), &workspace, 1024 * 1024)
            .expect("artifact store");
        symlink(&outside, state.join("plans")).expect("symlinked plans directory");

        assert!(store.persist(&sample_plan()).is_err());
        assert_eq!(
            std::fs::read_dir(&outside)
                .expect("outside directory")
                .count(),
            0
        );
        std::fs::remove_dir_all(fixture).expect("remove artifact fixture");
    }
}
