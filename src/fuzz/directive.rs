use crate::domain::{FuzzDenial, FuzzDirectiveIssue, FuzzDirectiveIssueKind, FuzzPolicyEvidence};

pub(crate) const FUZZ_DIRECTIVE_MARKER: &str = "@codeatlas-fuzz";
pub(crate) const MAX_FUZZ_DIRECTIVE_REASON_BYTES: usize = 256;

pub(crate) fn parse_directive_lines(
    lines: impl IntoIterator<Item = (u32, String)>,
) -> Option<FuzzPolicyEvidence> {
    let mut denials = Vec::new();
    let mut issues = Vec::new();

    for (line, value) in lines {
        let value = value.trim();
        let Some(payload) = value.strip_prefix(FUZZ_DIRECTIVE_MARKER) else {
            continue;
        };
        if payload
            .chars()
            .next()
            .is_some_and(|character| !character.is_whitespace())
        {
            continue;
        }
        let payload = payload.trim_start();
        let Some((action, reason)) = payload.split_once(':') else {
            issues.push(issue(
                line,
                FuzzDirectiveIssueKind::Malformed,
                "Fuzz directive must use `@codeatlas-fuzz deny: <reason>`",
            ));
            continue;
        };
        if action != action.trim() || action.is_empty() {
            issues.push(issue(
                line,
                FuzzDirectiveIssueKind::Malformed,
                "Fuzz directive action must be followed immediately by `:`",
            ));
            continue;
        }
        if action != "deny" {
            issues.push(issue(
                line,
                FuzzDirectiveIssueKind::UnsupportedAction,
                "Only the subtractive `deny` fuzz directive is supported",
            ));
            continue;
        }
        let reason = reason.trim();
        if reason.is_empty() {
            issues.push(issue(
                line,
                FuzzDirectiveIssueKind::EmptyReason,
                "Fuzz denial needs a maintainer reason",
            ));
            continue;
        }
        if reason.len() > MAX_FUZZ_DIRECTIVE_REASON_BYTES {
            issues.push(issue(
                line,
                FuzzDirectiveIssueKind::ReasonTooLong,
                "Fuzz denial reason exceeds the 256-byte limit",
            ));
            continue;
        }
        if reason.chars().any(char::is_control) {
            issues.push(issue(
                line,
                FuzzDirectiveIssueKind::Malformed,
                "Fuzz denial reason may not contain control characters",
            ));
            continue;
        }
        denials.push(FuzzDenial {
            line: line.max(1),
            reason: reason.to_string(),
        });
    }

    if denials.is_empty() && issues.is_empty() {
        return None;
    }

    denials.sort();
    let denial = match denials.as_slice() {
        [] => None,
        [denial] => Some(denial.clone()),
        [first, rest @ ..] if rest.iter().all(|denial| denial.reason == first.reason) => {
            for duplicate in rest {
                issues.push(issue(
                    duplicate.line,
                    FuzzDirectiveIssueKind::Duplicate,
                    "Fuzz denial is repeated for the same target",
                ));
            }
            Some(first.clone())
        }
        [first, rest @ ..] => {
            issues.push(issue(
                first.line,
                FuzzDirectiveIssueKind::Conflicting,
                "Fuzz target has conflicting denial reasons",
            ));
            for conflict in rest {
                issues.push(issue(
                    conflict.line,
                    FuzzDirectiveIssueKind::Conflicting,
                    "Fuzz target has conflicting denial reasons",
                ));
            }
            None
        }
    };
    issues.sort();
    issues.dedup();
    Some(FuzzPolicyEvidence { denial, issues })
}

pub(crate) fn merge_policy(
    current: &mut Option<FuzzPolicyEvidence>,
    incoming: Option<FuzzPolicyEvidence>,
) {
    let Some(mut incoming) = incoming else {
        return;
    };
    let Some(existing) = current else {
        *current = Some(incoming);
        return;
    };
    existing.issues.append(&mut incoming.issues);
    match (&existing.denial, incoming.denial) {
        (None, Some(denial)) => existing.denial = Some(denial),
        (Some(first), Some(second)) if first.reason == second.reason => {
            existing.issues.push(issue(
                second.line,
                FuzzDirectiveIssueKind::Duplicate,
                "Fuzz denial is repeated for the same target",
            ))
        }
        (Some(first), Some(second)) => {
            existing.issues.push(issue(
                first.line,
                FuzzDirectiveIssueKind::Conflicting,
                "Fuzz target has conflicting denial reasons",
            ));
            existing.issues.push(issue(
                second.line,
                FuzzDirectiveIssueKind::Conflicting,
                "Fuzz target has conflicting denial reasons",
            ));
            existing.denial = None;
        }
        _ => {}
    }
    existing.issues.sort();
    existing.issues.dedup();
}

fn issue(line: u32, kind: FuzzDirectiveIssueKind, message: &str) -> FuzzDirectiveIssue {
    FuzzDirectiveIssue {
        line: line.max(1),
        kind,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{merge_policy, parse_directive_lines, MAX_FUZZ_DIRECTIVE_REASON_BYTES};
    use crate::domain::FuzzDirectiveIssueKind;

    #[test]
    fn deny_is_the_only_bounded_subtractive_directive() {
        let policy = parse_directive_lines([
            (
                3,
                "@codeatlas-fuzz deny: calls the real provider".to_string(),
            ),
            (4, "@codeatlas-fuzz allow: fixture".to_string()),
            (
                5,
                format!(
                    "@codeatlas-fuzz deny: {}",
                    "x".repeat(MAX_FUZZ_DIRECTIVE_REASON_BYTES + 1)
                ),
            ),
        ])
        .expect("directive evidence");

        assert_eq!(
            policy.denial.as_ref().map(|denial| denial.reason.as_str()),
            Some("calls the real provider")
        );
        assert_eq!(
            policy
                .issues
                .iter()
                .map(|issue| issue.kind)
                .collect::<Vec<_>>(),
            [
                FuzzDirectiveIssueKind::UnsupportedAction,
                FuzzDirectiveIssueKind::ReasonTooLong,
            ]
        );
    }

    #[test]
    fn duplicate_and_conflicting_directives_fail_closed() {
        let duplicate = parse_directive_lines([
            (2, "@codeatlas-fuzz deny: real provider".to_string()),
            (3, "@codeatlas-fuzz deny: real provider".to_string()),
        ])
        .expect("duplicate evidence");
        assert!(duplicate.denial.is_some());
        assert_eq!(duplicate.issues[0].kind, FuzzDirectiveIssueKind::Duplicate);

        let conflicting = parse_directive_lines([
            (2, "@codeatlas-fuzz deny: real provider".to_string()),
            (3, "@codeatlas-fuzz deny: production credential".to_string()),
        ])
        .expect("conflicting evidence");
        assert!(conflicting.denial.is_none());
        assert!(conflicting
            .issues
            .iter()
            .all(|issue| issue.kind == FuzzDirectiveIssueKind::Conflicting));

        let mut merged = Some(
            parse_directive_lines([(1, "@codeatlas-fuzz allow: stale".to_string())])
                .expect("malformed overload policy"),
        );
        merge_policy(
            &mut merged,
            parse_directive_lines([(2, "@codeatlas-fuzz deny: real provider".to_string())]),
        );
        let merged = merged.expect("merged overload policy");
        assert_eq!(
            merged.denial.as_ref().map(|denial| denial.reason.as_str()),
            Some("real provider")
        );
        assert_eq!(
            merged.issues[0].kind,
            FuzzDirectiveIssueKind::UnsupportedAction
        );
    }
}
