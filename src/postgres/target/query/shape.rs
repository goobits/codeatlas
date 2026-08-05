use super::lexer::{
    find_top_level_word, find_word, identifier, is_reserved_word, parenthesized_segments,
    qualified_identifier, qualified_identifier_backwards, split_top_level, update_depth, Token,
};
use crate::postgres::model::{
    PostgresCatalogColumn, PostgresCatalogInventory, PostgresColumnBinding,
    PostgresConstraintEvidence, PostgresObjectKind, PostgresObjectReference, PostgresParameterRole,
    PostgresQueryParameter, PostgresQueryResultColumn, PostgresQueryResultShape,
    PostgresStatementClass, PostgresTypeShape,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn collect_table_references(
    tokens: &[Token],
    statement_start: usize,
) -> Vec<PostgresObjectReference> {
    let mut tables = Vec::new();
    let mut index = statement_start;
    while index < tokens.len() {
        let Some(word) = identifier(tokens.get(index)) else {
            index += 1;
            continue;
        };
        let candidate = match word {
            "from" | "join" | "update" => Some(index + 1),
            "into" if index > 0 && identifier(tokens.get(index - 1)) == Some("insert") => {
                Some(index + 1)
            }
            _ => None,
        };
        if let Some(mut start) = candidate {
            if identifier(tokens.get(start)) == Some("only") {
                start += 1;
            }
            if let Some((parts, end)) = qualified_identifier(tokens, start) {
                tables.push(table_reference(&parts));
                index = end;
                continue;
            }
        }
        index += 1;
    }
    tables.sort();
    tables.dedup();
    tables
}

pub(super) fn collect_table_aliases(
    tokens: &[Token],
    tables: &[PostgresObjectReference],
) -> BTreeMap<String, PostgresObjectReference> {
    let mut aliases = BTreeMap::new();
    let mut index = 0;
    while index < tokens.len() {
        let Some(word) = identifier(tokens.get(index)) else {
            index += 1;
            continue;
        };
        if !matches!(word, "from" | "join" | "update" | "into") {
            index += 1;
            continue;
        }
        let start = index + 1;
        let Some((parts, mut end)) = qualified_identifier(tokens, start) else {
            index += 1;
            continue;
        };
        let raw = table_reference(&parts);
        let table = tables
            .iter()
            .find(|candidate| candidate.name == raw.name && candidate.schema == raw.schema)
            .cloned()
            .unwrap_or(raw);
        if identifier(tokens.get(end)) == Some("as") {
            end += 1;
        }
        if let Some(alias) = identifier(tokens.get(end)) {
            if !is_reserved_word(alias) {
                aliases.insert(alias.to_string(), table);
            }
        }
        index = end.max(index + 1);
    }
    aliases
}

fn table_reference(parts: &[String]) -> PostgresObjectReference {
    let (schema, name) = if parts.len() >= 2 {
        (
            Some(parts[parts.len() - 2].clone()),
            parts[parts.len() - 1].clone(),
        )
    } else {
        (None, parts.first().cloned().unwrap_or_default())
    };
    PostgresObjectReference {
        kind: PostgresObjectKind::Table,
        schema,
        relation: None,
        name,
        resolved: false,
    }
}

pub(super) fn resolve_table_references(
    tables: &mut [PostgresObjectReference],
    catalog: Option<&PostgresCatalogInventory>,
) {
    let Some(catalog) = catalog else {
        return;
    };
    for table in tables {
        let matches = catalog
            .tables
            .iter()
            .filter(|candidate| {
                candidate.name == table.name
                    && table
                        .schema
                        .as_ref()
                        .is_none_or(|schema| schema == &candidate.schema)
            })
            .collect::<Vec<_>>();
        if let [candidate] = matches.as_slice() {
            table.schema = Some(candidate.schema.clone());
            table.resolved = true;
        }
    }
}

pub(super) fn query_parameters(tokens: &[Token]) -> Vec<PostgresQueryParameter> {
    let mut occurrences = BTreeMap::<u32, u32>::new();
    for token in tokens {
        if let Token::Parameter(position) = token {
            *occurrences.entry(*position).or_default() += 1;
        }
    }
    occurrences
        .into_iter()
        .map(|(position, occurrence_count)| PostgresQueryParameter {
            position,
            occurrence_count,
            roles: Vec::new(),
            data_type: inline_parameter_type(tokens, position),
            bindings: Vec::new(),
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn bind_parameters(
    parameters: &mut [PostgresQueryParameter],
    tokens: &[Token],
    statement_class: PostgresStatementClass,
    statement_start: usize,
    tables: &[PostgresObjectReference],
    aliases: &BTreeMap<String, PostgresObjectReference>,
    catalog: Option<&PostgresCatalogInventory>,
) {
    let insert_bindings = insert_parameter_bindings(tokens, statement_start, tables, aliases);
    for (index, token) in tokens.iter().enumerate() {
        let Token::Parameter(position) = token else {
            continue;
        };
        let Some(parameter) = parameters
            .iter_mut()
            .find(|parameter| parameter.position == *position)
        else {
            continue;
        };
        let mut bindings = insert_bindings
            .get(position)
            .cloned()
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(parts) = comparison_column(tokens, index) {
            let role = if statement_class == PostgresStatementClass::Update
                && parameter_is_in_update_set(tokens, statement_start, index)
            {
                PostgresParameterRole::UpdateValue
            } else {
                PostgresParameterRole::Predicate
            };
            bindings.push(PostgresColumnBinding {
                role,
                column: column_reference(&parts, tables, aliases),
                data_type: None,
                column_nullable: None,
                constraints: Vec::new(),
            });
        }
        parameter.roles = if bindings.is_empty() {
            vec![PostgresParameterRole::Expression]
        } else {
            bindings
                .iter()
                .map(|binding| binding.role)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        for mut binding in bindings {
            resolve_column_binding(&mut binding, tables, catalog);
            parameter.bindings.push(binding);
        }
        parameter.bindings.sort();
        parameter.bindings.dedup();
        let resolved_types = parameter
            .bindings
            .iter()
            .filter_map(|binding| binding.data_type.as_ref())
            .collect::<BTreeSet<_>>();
        if let [resolved] = resolved_types.into_iter().collect::<Vec<_>>().as_slice() {
            if parameter
                .data_type
                .as_ref()
                .is_none_or(|inline| type_names_match(inline, resolved))
            {
                parameter.data_type = Some((*resolved).clone());
            }
        }
    }
}

fn inline_parameter_type(tokens: &[Token], position: u32) -> Option<PostgresTypeShape> {
    for (index, token) in tokens.iter().enumerate() {
        if token != &Token::Parameter(position) {
            continue;
        }
        if tokens.get(index + 1) == Some(&Token::Operator("::".to_string())) {
            if let Some((parts, _)) = qualified_identifier(tokens, index + 2) {
                return Some(inline_type_shape(&parts));
            }
        }
        if index >= 2
            && tokens.get(index - 1) == Some(&Token::Symbol('('))
            && identifier(tokens.get(index - 2)) == Some("cast")
            && identifier(tokens.get(index + 1)) == Some("as")
        {
            if let Some((parts, _)) = qualified_identifier(tokens, index + 2) {
                return Some(inline_type_shape(&parts));
            }
        }
    }
    None
}

fn inline_type_shape(parts: &[String]) -> PostgresTypeShape {
    let name = parts.last().cloned().unwrap_or_default();
    PostgresTypeShape {
        oid: None,
        schema: (parts.len() > 1).then(|| parts[..parts.len() - 1].join(".")),
        formatted: parts.join("."),
        name,
        base_type: None,
        enum_values: Vec::new(),
        max_length: None,
        numeric_precision: None,
        numeric_scale: None,
    }
}

fn comparison_column(tokens: &[Token], parameter_index: usize) -> Option<Vec<String>> {
    if parameter_index >= 2 && is_comparison(tokens.get(parameter_index - 1)) {
        return qualified_identifier_backwards(tokens, parameter_index - 2);
    }
    if is_comparison(tokens.get(parameter_index + 1)) {
        return qualified_identifier(tokens, parameter_index + 2).map(|(parts, _)| parts);
    }
    None
}

fn is_comparison(token: Option<&Token>) -> bool {
    matches!(
        token,
        Some(Token::Operator(operator))
            if matches!(operator.as_str(), "=" | "<>" | "!=" | "<" | ">" | "<=" | ">=")
    )
}

fn parameter_is_in_update_set(tokens: &[Token], start: usize, parameter_index: usize) -> bool {
    let mut last_clause = None;
    let mut depth = 0_u32;
    for token in tokens.iter().take(parameter_index).skip(start) {
        update_depth(token, &mut depth);
        if depth == 0 {
            if let Some(word) = identifier(Some(token)) {
                if matches!(word, "set" | "where" | "returning") {
                    last_clause = Some(word);
                }
            }
        }
    }
    last_clause == Some("set")
}

fn insert_parameter_bindings(
    tokens: &[Token],
    start: usize,
    tables: &[PostgresObjectReference],
    aliases: &BTreeMap<String, PostgresObjectReference>,
) -> BTreeMap<u32, PostgresColumnBinding> {
    let mut bindings = BTreeMap::new();
    if identifier(tokens.get(start)) != Some("insert") {
        return bindings;
    }
    let Some(into) = find_word(tokens, start + 1, "into") else {
        return bindings;
    };
    let Some((_, table_end)) = qualified_identifier(tokens, into + 1) else {
        return bindings;
    };
    let Some(Token::Symbol('(')) = tokens.get(table_end) else {
        return bindings;
    };
    let Some((columns, after_columns)) = comma_separated_identifiers(tokens, table_end) else {
        return bindings;
    };
    let Some(values) = find_word(tokens, after_columns, "values") else {
        return bindings;
    };
    let Some(Token::Symbol('(')) = tokens.get(values + 1) else {
        return bindings;
    };
    let Some((segments, _)) = parenthesized_segments(tokens, values + 1) else {
        return bindings;
    };
    for (column, segment) in columns.into_iter().zip(segments) {
        let direct = segment.iter().find_map(|token| match token {
            Token::Parameter(position) => Some(*position),
            _ => None,
        });
        if let Some(position) = direct {
            bindings.insert(
                position,
                PostgresColumnBinding {
                    role: PostgresParameterRole::InsertValue,
                    column: column_reference(&column, tables, aliases),
                    data_type: None,
                    column_nullable: None,
                    constraints: Vec::new(),
                },
            );
        }
    }
    bindings
}

fn comma_separated_identifiers(tokens: &[Token], open: usize) -> Option<(Vec<Vec<String>>, usize)> {
    let (segments, end) = parenthesized_segments(tokens, open)?;
    let identifiers = segments
        .into_iter()
        .map(|segment| qualified_identifier(&segment, 0).map(|(parts, _)| parts))
        .collect::<Option<Vec<_>>>()?;
    Some((identifiers, end))
}

fn column_reference(
    parts: &[String],
    tables: &[PostgresObjectReference],
    aliases: &BTreeMap<String, PostgresObjectReference>,
) -> PostgresObjectReference {
    let name = parts.last().cloned().unwrap_or_default();
    let qualifier = (parts.len() >= 2).then(|| parts[parts.len() - 2].clone());
    let mut schema = (parts.len() >= 3).then(|| parts[parts.len() - 3].clone());
    let mut relation = qualifier.clone();
    if let Some(table) = qualifier
        .as_ref()
        .and_then(|qualifier| aliases.get(qualifier))
    {
        schema = table.schema.clone();
        relation = Some(table.name.clone());
    } else if qualifier.is_none() && tables.len() == 1 {
        schema = tables[0].schema.clone();
        relation = Some(tables[0].name.clone());
    }
    PostgresObjectReference {
        kind: PostgresObjectKind::Column,
        schema,
        relation,
        name,
        resolved: false,
    }
}

fn resolve_column_binding(
    binding: &mut PostgresColumnBinding,
    tables: &[PostgresObjectReference],
    catalog: Option<&PostgresCatalogInventory>,
) {
    let Some(column) = resolve_column(&mut binding.column, tables, catalog) else {
        return;
    };
    binding.data_type = Some(column.data_type.clone());
    binding.column_nullable = Some(column.nullable);
    binding.constraints = catalog
        .into_iter()
        .flat_map(|catalog| &catalog.constraints)
        .filter(|constraint| {
            constraint.schema == column.schema
                && constraint.table == column.table
                && constraint.columns.iter().any(|name| name == &column.name)
        })
        .map(|constraint| PostgresConstraintEvidence {
            name: constraint.name.clone(),
            kind: constraint.kind.clone(),
            definition_digest: constraint.definition_digest.clone(),
        })
        .collect();
    binding.constraints.sort();
}

fn resolve_column<'a>(
    reference: &mut PostgresObjectReference,
    tables: &[PostgresObjectReference],
    catalog: Option<&'a PostgresCatalogInventory>,
) -> Option<&'a PostgresCatalogColumn> {
    let catalog = catalog?;
    let matches = catalog
        .columns
        .iter()
        .filter(|column| {
            column.name == reference.name
                && reference
                    .schema
                    .as_ref()
                    .is_none_or(|schema| schema == &column.schema)
                && reference
                    .relation
                    .as_ref()
                    .is_none_or(|relation| relation == &column.table)
                && (reference.relation.is_some()
                    || tables.iter().any(|table| {
                        table.name == column.table
                            && table
                                .schema
                                .as_ref()
                                .is_none_or(|schema| schema == &column.schema)
                    }))
        })
        .collect::<Vec<_>>();
    let [column] = matches.as_slice() else {
        return None;
    };
    reference.schema = Some(column.schema.clone());
    reference.relation = Some(column.table.clone());
    reference.resolved = true;
    Some(*column)
}

fn type_names_match(left: &PostgresTypeShape, right: &PostgresTypeShape) -> bool {
    left.name == right.name
        && left
            .schema
            .as_ref()
            .is_none_or(|schema| right.schema.as_ref() == Some(schema))
}

pub(super) fn query_result(
    tokens: &[Token],
    statement_class: PostgresStatementClass,
    statement_start: usize,
    tables: &[PostgresObjectReference],
    aliases: &BTreeMap<String, PostgresObjectReference>,
    catalog: Option<&PostgresCatalogInventory>,
) -> PostgresQueryResultShape {
    let (start, end) = if statement_class == PostgresStatementClass::Select {
        let start = statement_start + 1;
        let end = find_top_level_word(tokens, start, "from").unwrap_or(tokens.len());
        (start, end)
    } else if let Some(returning) = find_top_level_word(tokens, statement_start, "returning") {
        (returning + 1, tokens.len())
    } else {
        return PostgresQueryResultShape {
            complete: true,
            columns: Vec::new(),
        };
    };
    let mut columns = split_top_level(&tokens[start..end])
        .into_iter()
        .enumerate()
        .map(|(index, expression)| {
            let (expression, alias) = expression_alias(expression);
            let source =
                exact_identifier(expression).map(|parts| column_reference(&parts, tables, aliases));
            let name = alias.or_else(|| source.as_ref().map(|source| source.name.clone()));
            let mut column = PostgresQueryResultColumn {
                position: u32::try_from(index + 1).unwrap_or(u32::MAX),
                name,
                source,
                data_type: None,
                nullable: None,
            };
            if let Some(source) = &mut column.source {
                if let Some(catalog_column) = resolve_column(source, tables, catalog) {
                    column.data_type = Some(catalog_column.data_type.clone());
                    column.nullable = Some(catalog_column.nullable);
                }
            }
            column
        })
        .collect::<Vec<_>>();
    columns.retain(|column| column.name.is_some() || column.source.is_some() || start < end);
    PostgresQueryResultShape {
        complete: columns.iter().all(|column| column.data_type.is_some()),
        columns,
    }
}

fn expression_alias(expression: &[Token]) -> (&[Token], Option<String>) {
    let mut depth = 0_u32;
    for (index, token) in expression.iter().enumerate() {
        update_depth(token, &mut depth);
        if depth == 0 && identifier(Some(token)) == Some("as") {
            return (
                &expression[..index],
                identifier(expression.get(index + 1)).map(str::to_string),
            );
        }
    }
    if expression.len() > 1 {
        if let Some(alias) = expression.last().and_then(|token| identifier(Some(token))) {
            if expression.get(expression.len() - 2) != Some(&Token::Symbol('.'))
                && !matches!(
                    expression.get(expression.len() - 2),
                    Some(Token::Operator(operator)) if operator == "::"
                )
                && !is_reserved_word(alias)
            {
                return (&expression[..expression.len() - 1], Some(alias.to_string()));
            }
        }
    }
    (expression, None)
}

fn exact_identifier(tokens: &[Token]) -> Option<Vec<String>> {
    let (parts, end) = qualified_identifier(tokens, 0)?;
    (end == tokens.len()).then_some(parts)
}
