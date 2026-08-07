use crate::config::RepositoryMember;
use crate::postgres::model::PostgresEvidence;
use crate::postgres::source::CollectedPostgres;
use crate::postgres::target::query::lexer::{
    identifier, lex_sql, parenthesized_segments, qualified_identifier, Token,
};
use std::collections::{BTreeMap, BTreeSet};

const MAX_COMMENT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum StaticSchemaObjectKind {
    Table,
    Column,
    Constraint,
    Index,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct StaticSchemaObjectIdentity {
    pub(crate) kind: StaticSchemaObjectKind,
    pub(crate) schema: Option<String>,
    pub(crate) relation: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) subject: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum StaticSchemaSourceKind {
    Bootstrap,
    Migration,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct StaticSchemaDefinition {
    pub(crate) source_kind: StaticSchemaSourceKind,
    pub(crate) source_name: String,
    pub(crate) evidence: PostgresEvidence,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct StaticSchemaObject {
    pub(crate) contract: String,
    pub(crate) identity: StaticSchemaObjectIdentity,
    pub(crate) definition: StaticSchemaDefinition,
    pub(crate) detail: Option<String>,
    pub(crate) description: Option<String>,
}

struct StaticSchemaComment {
    contract: String,
    object: StaticSchemaObjectIdentity,
    description: Option<String>,
}

pub(crate) struct StaticSchemaDiscovery {
    pub(crate) objects: Vec<StaticSchemaObject>,
    pub(crate) complete_by_contract: BTreeMap<String, bool>,
    pub(crate) reasons_by_contract: BTreeMap<String, BTreeSet<String>>,
}

pub(crate) fn discover(
    member: &RepositoryMember,
    collected: &CollectedPostgres,
) -> StaticSchemaDiscovery {
    let mut discovery = StaticSchemaDiscovery {
        objects: Vec::new(),
        complete_by_contract: collected
            .report
            .contracts
            .iter()
            .map(|contract| (contract.id.clone(), true))
            .collect(),
        reasons_by_contract: BTreeMap::new(),
    };
    let mut comments = Vec::new();
    for (kind, source) in collected
        .bootstraps
        .iter()
        .map(|source| (StaticSchemaSourceKind::Bootstrap, source))
        .chain(
            collected
                .migrations
                .iter()
                .map(|source| (StaticSchemaSourceKind::Migration, source)),
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
        let definition = StaticSchemaDefinition {
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
            .extend(parsed.objects.into_iter().map(|object| StaticSchemaObject {
                contract: source.contract_id.clone(),
                identity: object.identity,
                definition: definition.clone(),
                detail: object.detail,
                description: None,
            }));
        comments.extend(
            parsed
                .comments
                .into_iter()
                .map(|comment| StaticSchemaComment {
                    contract: source.contract_id.clone(),
                    object: comment.object,
                    description: comment.description,
                }),
        );
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
    for comment in &comments {
        for object in discovery.objects.iter_mut().filter(|object| {
            object.contract == comment.contract && object.identity == comment.object
        }) {
            object.description.clone_from(&comment.description);
        }
    }
    discovery.objects.sort();
    discovery
}

struct ParsedSchemaSource {
    objects: Vec<ParsedSchemaObject>,
    comments: Vec<ParsedSchemaComment>,
    complete: bool,
    reasons: BTreeSet<String>,
}

#[derive(Clone)]
struct ParsedSchemaObject {
    identity: StaticSchemaObjectIdentity,
    detail: Option<String>,
}

struct ParsedSchemaComment {
    object: StaticSchemaObjectIdentity,
    description: Option<String>,
}

fn parse_schema_source(source: &str) -> ParsedSchemaSource {
    let lexed = lex_sql(source);
    let mut parsed = ParsedSchemaSource {
        objects: Vec::new(),
        comments: Vec::new(),
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
        if let Some(object) = create_index_object(statement) {
            parsed.objects.push(object);
            continue;
        }
        if let Some(comment) = comment_on_object(statement, &mut parsed.reasons) {
            parsed.comments.push(comment);
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
    parsed
        .objects
        .sort_by(|left, right| left.identity.cmp(&right.identity));
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

fn create_table_objects(tokens: &[Token]) -> Option<Vec<ParsedSchemaObject>> {
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
    let mut objects = vec![ParsedSchemaObject {
        identity: StaticSchemaObjectIdentity {
            kind: StaticSchemaObjectKind::Table,
            schema: schema.clone(),
            relation: None,
            name: Some(table.clone()),
            subject: None,
        },
        detail: None,
    }];
    for segment in columns {
        let Some(first) = identifier(segment.first()) else {
            continue;
        };
        if is_table_constraint(first) {
            if let Some(constraint) = constraint_object(&segment, &schema, &table, None) {
                objects.push(constraint);
            }
            continue;
        }
        let column = first.to_string();
        objects.push(ParsedSchemaObject {
            identity: StaticSchemaObjectIdentity {
                kind: StaticSchemaObjectKind::Column,
                schema: schema.clone(),
                relation: Some(table.clone()),
                name: Some(column.clone()),
                subject: None,
            },
            detail: None,
        });
        objects.extend(column_constraint_objects(
            &segment, &schema, &table, &column,
        ));
    }
    Some(objects)
}

fn constraint_object(
    tokens: &[Token],
    schema: &Option<String>,
    table: &str,
    subject: Option<&str>,
) -> Option<ParsedSchemaObject> {
    let (name, kind_index) = if identifier(tokens.first()) == Some("constraint") {
        (identifier(tokens.get(1)).map(str::to_string), 2)
    } else {
        (None, 0)
    };
    let kind = constraint_kind(tokens, kind_index)?;
    Some(ParsedSchemaObject {
        identity: StaticSchemaObjectIdentity {
            kind: StaticSchemaObjectKind::Constraint,
            schema: schema.clone(),
            relation: Some(table.to_string()),
            name,
            subject: subject.map(str::to_string),
        },
        detail: Some(kind.to_string()),
    })
}

fn column_constraint_objects(
    tokens: &[Token],
    schema: &Option<String>,
    table: &str,
    column: &str,
) -> Vec<ParsedSchemaObject> {
    let mut objects = Vec::new();
    let mut index = 1;
    while index < tokens.len() {
        if identifier(tokens.get(index)) == Some("constraint") {
            if let Some(object) = constraint_object(&tokens[index..], schema, table, Some(column)) {
                objects.push(object);
            }
            index += 2;
        } else if let Some(kind) = constraint_kind(tokens, index) {
            objects.push(ParsedSchemaObject {
                identity: StaticSchemaObjectIdentity {
                    kind: StaticSchemaObjectKind::Constraint,
                    schema: schema.clone(),
                    relation: Some(table.to_string()),
                    name: None,
                    subject: Some(column.to_string()),
                },
                detail: Some(kind.to_string()),
            });
        }
        index += 1;
    }
    objects.sort_by(|left, right| {
        (&left.identity, &left.detail).cmp(&(&right.identity, &right.detail))
    });
    objects
}

fn constraint_kind(tokens: &[Token], index: usize) -> Option<&'static str> {
    match identifier(tokens.get(index))? {
        "primary" if identifier(tokens.get(index + 1)) == Some("key") => Some("primary_key"),
        "foreign" if identifier(tokens.get(index + 1)) == Some("key") => Some("foreign_key"),
        "not" if identifier(tokens.get(index + 1)) == Some("null") => Some("not_null"),
        "unique" => Some("unique"),
        "check" => Some("check"),
        "exclude" => Some("exclude"),
        "references" => Some("references"),
        _ => None,
    }
}

fn create_index_object(tokens: &[Token]) -> Option<ParsedSchemaObject> {
    if identifier(tokens.first()) != Some("create") {
        return None;
    }
    let mut index = 1;
    let unique = if identifier(tokens.get(index)) == Some("unique") {
        index += 1;
        true
    } else {
        false
    };
    if identifier(tokens.get(index)) != Some("index") {
        return None;
    }
    index += 1;
    if identifier(tokens.get(index)) == Some("concurrently") {
        index += 1;
    }
    if identifier(tokens.get(index)) == Some("if")
        && identifier(tokens.get(index + 1)) == Some("not")
        && identifier(tokens.get(index + 2)) == Some("exists")
    {
        index += 3;
    }
    let (index_parts, after_name) = qualified_identifier(tokens, index)?;
    let on = tokens
        .iter()
        .enumerate()
        .skip(after_name)
        .find_map(|(position, token)| {
            (identifier(Some(token)) == Some("on")).then_some(position)
        })?;
    let mut relation_index = on + 1;
    if identifier(tokens.get(relation_index)) == Some("only") {
        relation_index += 1;
    }
    let (relation_parts, _) = qualified_identifier(tokens, relation_index)?;
    let (relation_schema, relation) = relation_name(&relation_parts)?;
    let (index_schema, name) = relation_name(&index_parts)?;
    Some(ParsedSchemaObject {
        identity: StaticSchemaObjectIdentity {
            kind: StaticSchemaObjectKind::Index,
            schema: index_schema.or(relation_schema),
            relation: Some(relation),
            name: Some(name),
            subject: None,
        },
        detail: Some(if unique { "unique" } else { "non_unique" }.to_string()),
    })
}

fn comment_on_object(
    tokens: &[Token],
    reasons: &mut BTreeSet<String>,
) -> Option<ParsedSchemaComment> {
    if identifier(tokens.first()) != Some("comment") || identifier(tokens.get(1)) != Some("on") {
        return None;
    }
    let kind = match identifier(tokens.get(2))? {
        "table" => StaticSchemaObjectKind::Table,
        "column" => StaticSchemaObjectKind::Column,
        _ => return None,
    };
    let (parts, after_target) = qualified_identifier(tokens, 3)?;
    if identifier(tokens.get(after_target)) != Some("is") {
        return None;
    }
    let description = match tokens.get(after_target + 1) {
        Some(Token::StringLiteral(value)) if value.len() <= MAX_COMMENT_BYTES => {
            Some(value.clone())
        }
        Some(Token::StringLiteral(_)) => {
            reasons.insert(format!(
                "A static database comment exceeds {MAX_COMMENT_BYTES} UTF-8 bytes."
            ));
            None
        }
        token if identifier(token) == Some("null") => None,
        _ => return None,
    };
    let object = match kind {
        StaticSchemaObjectKind::Table => {
            let (schema, table) = relation_name(&parts)?;
            StaticSchemaObjectIdentity {
                kind,
                schema,
                relation: None,
                name: Some(table),
                subject: None,
            }
        }
        StaticSchemaObjectKind::Column => {
            if parts.len() < 2 {
                return None;
            }
            let name = parts.last()?.clone();
            let relation = parts.get(parts.len() - 2)?.clone();
            let schema = (parts.len() > 2).then(|| parts[parts.len() - 3].clone());
            StaticSchemaObjectIdentity {
                kind,
                schema,
                relation: Some(relation),
                name: Some(name),
                subject: None,
            }
        }
        StaticSchemaObjectKind::Constraint | StaticSchemaObjectKind::Index => return None,
    };
    Some(ParsedSchemaComment {
        object,
        description,
    })
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
    use super::{parse_schema_source, StaticSchemaObjectKind};

    #[test]
    fn static_schema_extraction_finds_objects_constraints_indexes_and_comments() {
        let parsed = parse_schema_source(
            "CREATE TABLE IF NOT EXISTS app.users (id bigint PRIMARY KEY, email text, CONSTRAINT email_ok CHECK (email <> ''));\n\
             CREATE UNIQUE INDEX users_email_idx ON app.users(email);\n\
             COMMENT ON TABLE app.users IS 'Application users.';\n\
             COMMENT ON COLUMN app.users.email IS 'Primary email.';",
        );
        assert!(parsed.complete);
        assert_eq!(
            parsed
                .objects
                .iter()
                .filter(|object| object.identity.kind == StaticSchemaObjectKind::Table)
                .count(),
            1
        );
        assert_eq!(
            parsed
                .objects
                .iter()
                .filter(|object| object.identity.kind == StaticSchemaObjectKind::Column)
                .count(),
            2
        );
        assert!(parsed.objects.iter().any(|object| object.identity.kind
            == StaticSchemaObjectKind::Constraint
            && object.identity.name.as_deref() == Some("email_ok")));
        assert!(parsed.objects.iter().any(|object| object.identity.kind
            == StaticSchemaObjectKind::Index
            && object.identity.name.as_deref() == Some("users_email_idx")));
        assert_eq!(parsed.comments.len(), 2);
        assert_eq!(
            parsed
                .comments
                .iter()
                .find(|comment| comment.object.kind == StaticSchemaObjectKind::Table)
                .and_then(|comment| comment.description.as_deref()),
            Some("Application users.")
        );
        assert_eq!(
            parsed
                .comments
                .iter()
                .find(|comment| comment.object.kind == StaticSchemaObjectKind::Column)
                .and_then(|comment| comment.description.as_deref()),
            Some("Primary email.")
        );
    }

    #[test]
    fn distinct_unnamed_constraints_are_not_collapsed() {
        let parsed = parse_schema_source(
            "CREATE TABLE values (value integer, CHECK (value > 0), CHECK (value < 100));",
        );

        assert_eq!(
            parsed
                .objects
                .iter()
                .filter(|object| object.identity.kind == StaticSchemaObjectKind::Constraint)
                .count(),
            2
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
