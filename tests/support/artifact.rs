use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub(crate) fn artifact_payload(schema_version: &str, body: Value) -> Value {
    json!({
        "schema_version": schema_version,
        "content_digest": digest_json(
            &format!("atlas.codeatlas.dev/artifact-payload/v1/{schema_version}"),
            &body,
        ),
        "body": body,
    })
}

pub(crate) fn write_reproducer(plan: &Value, workload: Value, path: &Path) -> Value {
    let body = json!({
        "subject": plan["subject"],
        "tool": plan["tool"],
        "parent_plan_id": plan["id"],
        "parent_plan_content_digest": plan["content_digest"],
        "evidence": plan["evidence"],
        "workload": workload,
        "execution_limits": plan["limits"],
        "fuzz_limits": plan["workload"]["body"]["limits"],
        "oracle_digest": format!("sha256:{}", "a".repeat(64)),
        "result_digest": format!("sha256:{}", "b".repeat(64)),
        "links": [{
            "kind": "plan",
            "id": plan["id"],
            "content_digest": plan["content_digest"]
        }]
    });
    let identity = json!({
        "schema_version": "codeatlas.reproducer/v1",
        "kind": "reproducer",
        "subject": body["subject"],
        "tool": body["tool"],
        "parent_plan_id": body["parent_plan_id"],
        "parent_plan_content_digest": body["parent_plan_content_digest"],
        "evidence": body["evidence"],
        "workload": body["workload"],
        "execution_limits": body["execution_limits"],
        "fuzz_limits": body["fuzz_limits"],
        "oracle_digest": body["oracle_digest"],
        "result_digest": body["result_digest"],
        "links": body["links"],
    });
    let content_digest = digest_json("atlas.codeatlas.dev/reproducer/v1", &identity);
    let document = json!({
        "schema_version": "codeatlas.reproducer/v1",
        "kind": "reproducer",
        "id": format!(
            "reproducer_{}",
            content_digest.strip_prefix("sha256:").expect("digest prefix")
        ),
        "content_digest": content_digest,
        "subject": body["subject"],
        "tool": body["tool"],
        "parent_plan_id": body["parent_plan_id"],
        "parent_plan_content_digest": body["parent_plan_content_digest"],
        "evidence": body["evidence"],
        "workload": body["workload"],
        "execution_limits": body["execution_limits"],
        "fuzz_limits": body["fuzz_limits"],
        "oracle_digest": body["oracle_digest"],
        "result_digest": body["result_digest"],
        "links": body["links"],
    });
    fs::write(
        path,
        serde_json::to_vec_pretty(&document).expect("reproducer JSON"),
    )
    .expect("write reproducer fixture");
    document
}

fn digest_json(domain: &str, value: &Value) -> String {
    let canonical = serde_json_canonicalizer::to_vec(value).expect("canonical test artifact");
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update(b"\n");
    digest.update(canonical);
    format!("sha256:{:x}", digest.finalize())
}
