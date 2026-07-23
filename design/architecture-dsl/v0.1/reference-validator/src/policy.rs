use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExceptionDisposition {
    Applied,
    Stale,
    Expired,
    Irrelevant,
    Rejected,
}

#[derive(Clone, Debug)]
pub struct ExceptionContext<'a> {
    pub constraint_id: &'a str,
    pub constraint_version: u64,
    pub affected_ids: &'a BTreeSet<String>,
    pub affected_closure_digest: &'a str,
    pub as_of: &'a str,
}

pub fn evaluate_exception(
    exception: &Value,
    context: &ExceptionContext<'_>,
) -> ExceptionDisposition {
    if exception["decision"]["status"].as_str() != Some("accepted")
        || !matches!(
            exception["approval"]["status"].as_str(),
            Some("granted" | "not_required")
        )
        || exception["decision"]["authority"]["governing"]
            .as_array()
            .is_none_or(Vec::is_empty)
    {
        return ExceptionDisposition::Rejected;
    }

    if exception["constraint"]["id"].as_str() != Some(context.constraint_id)
        || exception["constraint"]["version"].as_u64() != Some(context.constraint_version)
        || !exception["affectedIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|id| context.affected_ids.contains(id))
    {
        return ExceptionDisposition::Irrelevant;
    }

    if exception["baseClosureDigest"].as_str() != Some(context.affected_closure_digest) {
        return ExceptionDisposition::Stale;
    }

    let Some(expires_at) = exception["expiresAt"].as_str() else {
        return ExceptionDisposition::Rejected;
    };
    if !valid_timestamp(context.as_of) || !valid_timestamp(expires_at) {
        return ExceptionDisposition::Rejected;
    }
    if context.as_of >= expires_at {
        return ExceptionDisposition::Expired;
    }

    ExceptionDisposition::Applied
}

fn valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
}

#[cfg(test)]
mod tests {
    use super::{evaluate_exception, ExceptionContext, ExceptionDisposition};
    use serde_json::json;
    use std::collections::BTreeSet;

    fn accepted_exception() -> serde_json::Value {
        json!({
            "constraint": {
                "id": "goobits.constraint.no-workshop-shelly-path",
                "version": 1
            },
            "affectedIds": ["goobits.app.workshop"],
            "baseClosureDigest": format!("sha256:{}", "a".repeat(64)),
            "expiresAt": "2026-09-01T00:00:00Z",
            "decision": {
                "status": "accepted",
                "authority": {
                    "governing": [{
                        "kind": "accepted-adr",
                        "artifact": {
                            "id": "goobits.adr.temporary-shell-import",
                            "version": 1
                        }
                    }],
                    "supporting": []
                }
            },
            "approval": {"status": "granted"}
        })
    }

    fn context<'a>(
        affected_ids: &'a BTreeSet<String>,
        closure: &'a str,
        as_of: &'a str,
    ) -> ExceptionContext<'a> {
        ExceptionContext {
            constraint_id: "goobits.constraint.no-workshop-shelly-path",
            constraint_version: 1,
            affected_ids,
            affected_closure_digest: closure,
            as_of,
        }
    }

    #[test]
    fn exceptions_apply_without_mutating_architecture_identity() {
        let ids = BTreeSet::from(["goobits.app.workshop".to_owned()]);
        let closure = format!("sha256:{}", "a".repeat(64));

        assert_eq!(
            evaluate_exception(
                &accepted_exception(),
                &context(&ids, &closure, "2026-08-01T00:00:00Z")
            ),
            ExceptionDisposition::Applied
        );
    }

    #[test]
    fn relevant_closure_changes_make_exceptions_stale() {
        let ids = BTreeSet::from(["goobits.app.workshop".to_owned()]);
        let changed = format!("sha256:{}", "b".repeat(64));

        assert_eq!(
            evaluate_exception(
                &accepted_exception(),
                &context(&ids, &changed, "2026-08-01T00:00:00Z")
            ),
            ExceptionDisposition::Stale
        );
    }

    #[test]
    fn recorded_as_of_controls_expiration() {
        let ids = BTreeSet::from(["goobits.app.workshop".to_owned()]);
        let closure = format!("sha256:{}", "a".repeat(64));
        let exception = accepted_exception();

        assert_eq!(
            evaluate_exception(&exception, &context(&ids, &closure, "2026-08-31T23:59:59Z")),
            ExceptionDisposition::Applied
        );
        assert_eq!(
            evaluate_exception(&exception, &context(&ids, &closure, "2026-09-01T00:00:00Z")),
            ExceptionDisposition::Expired
        );
    }

    #[test]
    fn owner_direction_does_not_promote_a_proposal() {
        let ids = BTreeSet::from(["goobits.app.workshop".to_owned()]);
        let closure = format!("sha256:{}", "a".repeat(64));
        let mut exception = accepted_exception();
        exception["decision"]["status"] = json!("proposed");
        exception["decision"]["authority"]["governing"] = json!([]);
        exception["decision"]["authority"]["supporting"] = json!([{
            "kind": "owner-direction",
            "artifact": {
                "id": "goobits.owner-direction.example",
                "version": 1
            }
        }]);

        assert_eq!(
            evaluate_exception(&exception, &context(&ids, &closure, "2026-08-01T00:00:00Z")),
            ExceptionDisposition::Rejected
        );
    }
}
