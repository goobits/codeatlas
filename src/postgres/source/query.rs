use super::{
    digest, discovery, ecmascript, parameters, require_project_path, source_error, CollectedQuery,
    PostgresQueryDocumentation, MAX_SQL_BYTES,
};
use crate::config::{PostgresContractConfig, ProjectConfig};
use crate::paths;
use crate::postgres::model::{PostgresEvidence, PostgresFinding, PostgresFindingSeverity};
use crate::postgres::target::query::{analyze_query, StaticQueryInput};
use crate::source_discovery::{self, SourceDiscoveryRequest};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const MAX_QUERY_DESCRIPTION_BYTES: usize = 64 * 1024;

pub(super) fn collect(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    diagnostics: &mut Vec<PostgresFinding>,
) -> Result<Vec<CollectedQuery>> {
    let paths = query_paths(project, contract, diagnostics)?;
    let mut queries = Vec::new();
    for path in paths {
        if discovery::is_sql_file(&path) {
            match collect_query_file(project, contract, &path) {
                Ok(query) => {
                    append_fuzz_policy_findings(contract, &query, diagnostics);
                    queries.push(query);
                }
                Err(error) => diagnostics.push(source_error(
                    "query-read-failed",
                    project,
                    contract,
                    &path,
                    error,
                )),
            }
            continue;
        }
        match ecmascript::extract(&project.root, std::slice::from_ref(&path)) {
            Ok(extracted) => {
                queries.extend(extracted.queries.into_iter().map(|query| {
                    collected_query(
                        &contract.id,
                        query.sql,
                        None,
                        PostgresQueryDocumentation {
                            description: None,
                            missing_reason: Some(
                                "No source-adjacent description is available for this embedded query."
                                    .to_string(),
                            ),
                        },
                        &project.config.fuzz.exclude.postgres,
                    )
                }));
            }
            Err(error) => diagnostics.push(PostgresFinding::new(
                if contract.source_complete {
                    PostgresFindingSeverity::Error
                } else {
                    PostgresFindingSeverity::Warning
                },
                "query-parse-failed",
                &contract.id,
                Some(paths::normalize_relative_path(&path, &project.root)),
                error.to_string(),
                contract.source_complete,
                None,
            )),
        }
    }
    queries.sort_by(|left, right| left.contract.id.cmp(&right.contract.id));
    queries.dedup_by(|left, right| left.contract.id == right.contract.id);
    Ok(queries)
}

fn query_paths(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    diagnostics: &mut Vec<PostgresFinding>,
) -> Result<Vec<PathBuf>> {
    let excluded_paths = resolve_query_paths(
        project,
        contract,
        &contract.query_exclude_paths,
        "query exclusion",
    )?;
    let query_roots = resolve_query_paths(project, contract, &contract.query_roots, "query root")?;
    for excluded in &excluded_paths {
        if !query_roots
            .iter()
            .any(|root| excluded == root || excluded.starts_with(root))
        {
            anyhow::bail!(
                "PostgreSQL query exclusion is outside every query root for contract {}: {}",
                contract.id,
                excluded.display()
            );
        }
    }

    let mut paths = BTreeSet::new();
    for root in query_roots {
        if is_excluded_query_path(&root, &excluded_paths) {
            continue;
        }
        if root.is_file() {
            if discovery::is_supported_source_file(&root) {
                paths.insert(root);
            }
            continue;
        }
        let discovery = source_discovery::discover(SourceDiscoveryRequest {
            root: &root,
            patterns: &[],
            excluded_roots: &excluded_paths,
            no_default_ignore: project.config.no_default_ignore,
        });
        for warning in discovery.warnings {
            diagnostics.push(PostgresFinding::new(
                PostgresFindingSeverity::Warning,
                "query-discovery-warning",
                &contract.id,
                Some(paths::normalize_relative_path(&root, &project.root)),
                warning,
                false,
                None,
            ));
        }
        paths.extend(discovery.files.into_iter().filter(|path| {
            discovery::is_supported_source_file(path)
                && !is_excluded_query_path(path, &excluded_paths)
                && !crate::source_policy::is_conventional_test_source(
                    path.strip_prefix(&root).unwrap_or(path),
                )
        }));
    }
    Ok(paths.into_iter().collect())
}

fn resolve_query_paths(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    configured_paths: &[PathBuf],
    label: &str,
) -> Result<Vec<PathBuf>> {
    let mut paths = BTreeSet::new();
    for configured in configured_paths {
        if configured.as_os_str().is_empty() {
            anyhow::bail!("PostgreSQL contract {} has an empty {label}", contract.id);
        }
        let unresolved = if configured.is_absolute() {
            configured.clone()
        } else {
            project.config_base().join(configured)
        };
        let path = unresolved.canonicalize().with_context(|| {
            format!(
                "PostgreSQL {label} does not exist: {}",
                unresolved.display()
            )
        })?;
        require_project_path(&path, &project.root, &contract.id)?;
        paths.insert(path);
    }
    Ok(paths.into_iter().collect())
}

fn is_excluded_query_path(path: &Path, excluded_paths: &[PathBuf]) -> bool {
    excluded_paths
        .iter()
        .any(|excluded| path == excluded || path.starts_with(excluded))
}

fn collect_query_file(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    path: &Path,
) -> Result<CollectedQuery> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("Could not inspect PostgreSQL query {}", path.display()))?;
    if metadata.len() > MAX_SQL_BYTES {
        anyhow::bail!(
            "PostgreSQL query {} is {} bytes; the per-file limit is {} bytes",
            path.display(),
            metadata.len(),
            MAX_SQL_BYTES
        );
    }
    let text = std::fs::read_to_string(path).with_context(|| {
        format!(
            "Could not read PostgreSQL query {} as UTF-8",
            path.display()
        )
    })?;
    let leading = leading_sql_evidence(&text);
    Ok(collected_query(
        &contract.id,
        ecmascript::StaticSql {
            text,
            path: paths::normalize_relative_path(path, &project.root),
            line: 1,
            column: 1,
            dynamic: false,
        },
        leading.fuzz_policy,
        leading.documentation,
        &project.config.fuzz.exclude.postgres,
    ))
}

fn collected_query(
    contract_id: &str,
    sql: ecmascript::StaticSql,
    fuzz_policy: Option<codeatlas_domain::FuzzPolicyEvidence>,
    documentation: PostgresQueryDocumentation,
    fuzz_exclusions: &[String],
) -> CollectedQuery {
    let parameters = parameters::analyze(&sql.text);
    let dynamic = sql.dynamic || parameters.dynamic;
    let sha256 = digest(&sql.text);
    let contract = analyze_query(
        StaticQueryInput {
            contract_id,
            path: &sql.path,
            line: sql.line,
            column: sql.column,
            sha256: &sha256,
            sql: &parameters.sql,
            dynamic,
            fuzz_policy: fuzz_policy.as_ref(),
            fuzz_exclusions,
        },
        None,
        None,
    );
    CollectedQuery {
        contract_id: contract_id.to_string(),
        sql: (!contract.dynamic).then_some(parameters.sql),
        contract,
        documentation,
    }
}

struct LeadingSqlEvidence {
    fuzz_policy: Option<codeatlas_domain::FuzzPolicyEvidence>,
    documentation: PostgresQueryDocumentation,
}

fn leading_sql_evidence(source: &str) -> LeadingSqlEvidence {
    let mut index = 0;
    let mut line = 1_u32;
    let bytes = source.as_bytes();
    let mut documentation = Vec::new();
    loop {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            if bytes[index] == b'\n' {
                line = line.saturating_add(1);
            }
            index += 1;
        }
        if source[index..].starts_with("--") {
            let start_line = line;
            let end = source[index..]
                .find('\n')
                .map_or(bytes.len(), |offset| index + offset);
            documentation.push((start_line, source[index + 2..end].trim().to_string()));
            index = end;
            continue;
        }
        if source[index..].starts_with("/*") {
            let start_line = line;
            let Some(offset) = source[index + 2..].find("*/") else {
                break;
            };
            let end = index + 2 + offset;
            for (line_offset, value) in source[index + 2..end].lines().enumerate() {
                let value = value
                    .trim()
                    .strip_prefix('*')
                    .unwrap_or(value.trim())
                    .trim();
                documentation.push((
                    start_line.saturating_add(line_offset as u32),
                    value.to_string(),
                ));
            }
            line = line.saturating_add(
                source[index..end + 2]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count() as u32,
            );
            index = end + 2;
            continue;
        }
        break;
    }
    let fuzz_policy = crate::fuzz::directive::parse_directive_lines(documentation.clone());
    let description = documentation
        .into_iter()
        .map(|(_, value)| value)
        .filter(|value| !is_fuzz_directive(value))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    let documentation = if description.is_empty() {
        PostgresQueryDocumentation {
            description: None,
            missing_reason: Some(
                "The static SQL file has no leading non-directive description comment.".to_string(),
            ),
        }
    } else if description.len() > MAX_QUERY_DESCRIPTION_BYTES {
        PostgresQueryDocumentation {
            description: None,
            missing_reason: Some(format!(
                "The leading SQL description exceeds the {MAX_QUERY_DESCRIPTION_BYTES}-byte documentation limit."
            )),
        }
    } else {
        PostgresQueryDocumentation {
            description: Some(description),
            missing_reason: None,
        }
    };
    LeadingSqlEvidence {
        fuzz_policy,
        documentation,
    }
}

fn is_fuzz_directive(value: &str) -> bool {
    value
        .trim()
        .strip_prefix(crate::fuzz::directive::FUZZ_DIRECTIVE_MARKER)
        .is_some_and(|payload| {
            payload.is_empty() || payload.chars().next().is_some_and(char::is_whitespace)
        })
}

fn append_fuzz_policy_findings(
    contract: &PostgresContractConfig,
    query: &CollectedQuery,
    diagnostics: &mut Vec<PostgresFinding>,
) {
    let Some(policy) = &query.contract.fuzz_policy else {
        return;
    };
    for issue in &policy.issues {
        diagnostics.push(PostgresFinding::new(
            PostgresFindingSeverity::Error,
            "fuzz-directive-invalid",
            &contract.id,
            Some(query.contract.id.clone()),
            issue.message.clone(),
            true,
            Some(PostgresEvidence {
                path: query.contract.path.clone(),
                line: issue.line,
                column: None,
            }),
        ));
    }
}

#[cfg(test)]
mod fuzz_directive_tests {
    use super::leading_sql_evidence;
    use codeatlas_domain::FuzzDirectiveIssueKind;

    #[test]
    fn sql_directive_is_leading_comment_convenience_only() {
        let evidence = leading_sql_evidence(
            "-- @codeatlas-fuzz deny: invokes an extension that sends real email\nWITH value AS (SELECT 1) SELECT * FROM value",
        );
        let policy = evidence.fuzz_policy.expect("leading SQL directive");
        assert_eq!(
            policy.denial.as_ref().map(|denial| denial.reason.as_str()),
            Some("invokes an extension that sends real email")
        );

        assert!(
            leading_sql_evidence("SELECT 1; -- @codeatlas-fuzz deny: too late")
                .fuzz_policy
                .is_none()
        );
        let unsupported = leading_sql_evidence("/* @codeatlas-fuzz allow: stale */\nSELECT 1")
            .fuzz_policy
            .expect("unsupported directive evidence");
        assert_eq!(
            unsupported.issues[0].kind,
            FuzzDirectiveIssueKind::UnsupportedAction
        );
    }

    #[test]
    fn sql_description_excludes_the_fuzz_directive() {
        let evidence = leading_sql_evidence(
            "-- Load the account visible to the current tenant.\n-- @codeatlas-fuzz deny: invokes the real audit provider\nSELECT * FROM accounts",
        );

        assert_eq!(
            evidence.documentation.description.as_deref(),
            Some("Load the account visible to the current tenant.")
        );
        assert!(!evidence
            .documentation
            .description
            .as_deref()
            .unwrap_or_default()
            .contains("codeatlas-fuzz"));
    }
}
