use super::{
    ObjectKey, PostgresObjectDefinition, PostgresSchemaSourceKind, PostgresUsageObjectIdentity,
};
use crate::config::RepositoryMember;
use crate::postgres::model::{PostgresEvidence, PostgresObjectKind};
use crate::postgres::source::CollectedPostgres;
use crate::postgres::target::query::lexer::{
    identifier, lex_sql, parenthesized_segments, qualified_identifier, Token,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct DiscoveredObject {
    pub(super) key: ObjectKey,
    pub(super) definition: PostgresObjectDefinition,
}

pub(super) struct SchemaDiscovery {
    pub(super) objects: Vec<DiscoveredObject>,
    pub(super) complete_by_contract: BTreeMap<String, bool>,
    pub(super) reasons_by_contract: BTreeMap<String, BTreeSet<String>>,
}

pub(super) fn discover_schema_objects(
    member: &RepositoryMember,
    collected: &CollectedPostgres,
) -> SchemaDiscovery {
    let mut discovery = SchemaDiscovery {
        objects: Vec::new(),
        complete_by_contract: collected
            .report
            .contracts
            .iter()
            .map(|contract| (contract.id.clone(), true))
            .collect(),
        reasons_by_contract: BTreeMap::new(),
    };
    for (kind, source) in collected
        .bootstraps
        .iter()
        .map(|source| (PostgresSchemaSourceKind::Bootstrap, source))
        .chain(
            collected
                .migrations
                .iter()
                .map(|source| (PostgresSchemaSourceKind::Migration, source)),
        )
    {
        let parsed = parse_schema_source(&source.lint_sql);
        if !parsed.complete {
            discovery
                .complete_by_contract
                .insert(source.contract_id.clone(), false);
        }
        discovery
            .reasons_by_contract
            .entry(source.contract_id.clone())
            .or_default()
            .extend(parsed.reasons);
        let definition = PostgresObjectDefinition {
            source_kind: kind,
            source_name: source.inventory.name.clone(),
            evidence: PostgresEvidence {
                path: crate::paths::repository_path(&member.report_root, &source.inventory.path),
                line: source.source_line,
                column: Some(source.source_column),
            },
        };
        discovery
            .objects
            .extend(parsed.objects.into_iter().map(|object| DiscoveredObject {
                key: ObjectKey {
                    contract: source.contract_id.clone(),
                    object,
                },
                definition: definition.clone(),
            }));
    }
    for contract in &collected.report.contracts {
        if contract.bootstraps.is_empty() && contract.migrations.is_empty() {
            discovery
                .complete_by_contract
                .insert(contract.id.clone(), false);
            discovery
                .reasons_by_contract
                .entry(contract.id.clone())
                .or_default()
                .insert(
                    "No static bootstrap or migration source defines schema objects.".to_string(),
                );
        }
    }
    discovery
}

struct ParsedSchemaSource {
    objects: Vec<PostgresUsageObjectIdentity>,
    complete: bool,
    reasons: BTreeSet<String>,
}

fn parse_schema_source(source: &str) -> ParsedSchemaSource {
    let lexed = lex_sql(source);
    let mut parsed = ParsedSchemaSource {
        objects: Vec::new(),
        complete: lexed.complete,
        reasons: BTreeSet::new(),
    };
    if !lexed.complete {
        parsed
            .reasons
            .insert("A static schema source has incomplete SQL syntax.".to_string());
    }
    for statement in sql_statements(&lexed.tokens) {
        if statement.is_empty() {
            continue;
        }
        if let Some(mut objects) = create_table_objects(statement) {
            parsed.objects.append(&mut objects);
            continue;
        }
        if statement_changes_schema(statement) {
            parsed.complete = false;
            parsed.reasons.insert(format!(
                "Static schema-object extraction does not model {} statements.",
                statement_label(statement)
            ));
        }
    }
    parsed.objects.sort();
    parsed.objects.dedup();
    parsed
}

fn sql_statements(tokens: &[Token]) -> Vec<&[Token]> {
    let mut statements = Vec::new();
    let mut start = 0;
    let mut depth = 0_u32;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::Symbol('(') => depth = depth.saturating_add(1),
            Token::Symbol(')') => depth = depth.saturating_sub(1),
            Token::Symbol(';') if depth == 0 => {
                statements.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < tokens.len() {
        statements.push(&tokens[start..]);
    }
    statements
}

fn create_table_objects(tokens: &[Token]) -> Option<Vec<PostgresUsageObjectIdentity>> {
    if identifier(tokens.first()) != Some("create") {
        return None;
    }
    let mut index = 1;
    if matches!(
        identifier(tokens.get(index)),
        Some("temporary" | "temp" | "unlogged")
    ) {
        index += 1;
    }
    if identifier(tokens.get(index)) != Some("table") {
        return None;
    }
    index += 1;
    if identifier(tokens.get(index)) == Some("if")
        && identifier(tokens.get(index + 1)) == Some("not")
        && identifier(tokens.get(index + 2)) == Some("exists")
    {
        index += 3;
    }
    let (parts, open) = qualified_identifier(tokens, index)?;
    let (schema, table) = relation_name(&parts)?;
    let (columns, _) = parenthesized_segments(tokens, open)?;
    let mut objects = vec![PostgresUsageObjectIdentity {
        kind: PostgresObjectKind::Table,
        schema: schema.clone(),
        relation: None,
        name: table.clone(),
    }];
    for column in columns {
        let Some(name) = identifier(column.first()) else {
            continue;
        };
        if is_table_constraint(name) {
            continue;
        }
        objects.push(PostgresUsageObjectIdentity {
            kind: PostgresObjectKind::Column,
            schema: schema.clone(),
            relation: Some(table.clone()),
            name: name.to_string(),
        });
    }
    Some(objects)
}

fn relation_name(parts: &[String]) -> Option<(Option<String>, String)> {
    let table = parts.last()?.clone();
    let schema = (parts.len() > 1).then(|| parts[parts.len() - 2].clone());
    Some((schema, table))
}

fn is_table_constraint(value: &str) -> bool {
    matches!(
        value,
        "check" | "constraint" | "exclude" | "foreign" | "primary" | "unique"
    )
}

fn statement_changes_schema(tokens: &[Token]) -> bool {
    matches!(
        identifier(tokens.first()),
        Some("alter" | "comment" | "create" | "drop" | "grant" | "revoke")
    )
}

fn statement_label(tokens: &[Token]) -> String {
    tokens
        .iter()
        .filter_map(|token| identifier(Some(token)))
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{parse_schema_source, PostgresObjectKind, PostgresUsageObjectIdentity};

    #[test]
    fn static_schema_extraction_finds_tables_and_columns_without_counting_ddl_as_usage() {
        let parsed = parse_schema_source(
            "CREATE TABLE IF NOT EXISTS app.users (id bigint PRIMARY KEY, email text, CONSTRAINT email_ok CHECK (email <> ''));",
        );
        assert!(parsed.complete);
        assert_eq!(
            parsed.objects,
            [
                PostgresUsageObjectIdentity {
                    kind: PostgresObjectKind::Table,
                    schema: Some("app".to_string()),
                    relation: None,
                    name: "users".to_string(),
                },
                PostgresUsageObjectIdentity {
                    kind: PostgresObjectKind::Column,
                    schema: Some("app".to_string()),
                    relation: Some("users".to_string()),
                    name: "email".to_string(),
                },
                PostgresUsageObjectIdentity {
                    kind: PostgresObjectKind::Column,
                    schema: Some("app".to_string()),
                    relation: Some("users".to_string()),
                    name: "id".to_string(),
                },
            ]
        );
    }

    #[test]
    fn unsupported_schema_changes_make_negative_evidence_incomplete() {
        let parsed = parse_schema_source(
            "CREATE TABLE users (id bigint); ALTER TABLE users ADD COLUMN email text;",
        );
        assert!(!parsed.complete);
        assert!(parsed
            .reasons
            .iter()
            .any(|reason| reason.contains("alter table")));
    }
}
