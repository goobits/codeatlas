use super::psql::{Connection, Psql};
use crate::postgres::model::{
    PostgresCatalogColumn, PostgresCatalogConstraint, PostgresCatalogIndex,
    PostgresCatalogInventory, PostgresCatalogTable, PostgresTypeName, PostgresTypeShape,
};
use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const CATALOG_SQL: &str = r#"
SELECT json_build_object(
    'server_version', current_setting('server_version'),
    'server_version_num', current_setting('server_version_num')::integer,
    'tables', COALESCE((
        SELECT json_agg(json_build_object(
            'schema', namespace.nspname,
            'name', relation.relname,
            'kind', relation.relkind::text
        ) ORDER BY namespace.nspname, relation.relname)
        FROM pg_class relation
        JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname !~ '^pg_'
          AND namespace.nspname <> 'information_schema'
          AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
    ), '[]'::json),
    'columns', COALESCE((
        SELECT json_agg(json_build_object(
            'schema', namespace.nspname,
            'table', relation.relname,
            'name', attribute.attname,
            'position', attribute.attnum,
            'type_oid', attribute.atttypid::bigint,
            'type_schema', type_namespace.nspname,
            'type_name', type_row.typname,
            'formatted_type', format_type(attribute.atttypid, attribute.atttypmod),
            'base_type_schema', base_type_namespace.nspname,
            'base_type_name', base_type.typname,
            'enum_values', COALESCE((
                SELECT json_agg(enum_row.enumlabel ORDER BY enum_row.enumsortorder)
                FROM pg_enum enum_row
                WHERE enum_row.enumtypid = CASE
                    WHEN type_row.typtype = 'd' THEN type_row.typbasetype
                    ELSE type_row.oid
                END
            ), '[]'::json),
            'max_length', information_column.character_maximum_length,
            'numeric_precision', information_column.numeric_precision,
            'numeric_scale', information_column.numeric_scale,
            'nullable', NOT attribute.attnotnull,
            'default_expression', pg_get_expr(default_value.adbin, default_value.adrelid)
        ) ORDER BY namespace.nspname, relation.relname, attribute.attnum)
        FROM pg_attribute attribute
        JOIN pg_class relation ON relation.oid = attribute.attrelid
        JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
        JOIN pg_type type_row ON type_row.oid = attribute.atttypid
        JOIN pg_namespace type_namespace ON type_namespace.oid = type_row.typnamespace
        LEFT JOIN pg_type base_type
          ON type_row.typtype = 'd'
         AND base_type.oid = type_row.typbasetype
        LEFT JOIN pg_namespace base_type_namespace
          ON base_type_namespace.oid = base_type.typnamespace
        LEFT JOIN pg_attrdef default_value
          ON default_value.adrelid = attribute.attrelid
         AND default_value.adnum = attribute.attnum
        LEFT JOIN information_schema.columns information_column
          ON information_column.table_schema = namespace.nspname
         AND information_column.table_name = relation.relname
         AND information_column.column_name = attribute.attname
        WHERE namespace.nspname !~ '^pg_'
          AND namespace.nspname <> 'information_schema'
          AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
          AND attribute.attnum > 0
          AND NOT attribute.attisdropped
    ), '[]'::json),
    'constraints', COALESCE((
        SELECT json_agg(json_build_object(
            'schema', namespace.nspname,
            'table', relation.relname,
            'name', constraint_row.conname,
            'kind', constraint_row.contype::text,
            'columns', COALESCE((
                SELECT json_agg(column_attribute.attname ORDER BY key_column.ordinality)
                FROM unnest(constraint_row.conkey) WITH ORDINALITY key_column(attnum, ordinality)
                JOIN pg_attribute column_attribute
                  ON column_attribute.attrelid = constraint_row.conrelid
                 AND column_attribute.attnum = key_column.attnum
            ), '[]'::json),
            'definition', pg_get_constraintdef(constraint_row.oid, true)
        ) ORDER BY namespace.nspname, relation.relname, constraint_row.conname)
        FROM pg_constraint constraint_row
        JOIN pg_class relation ON relation.oid = constraint_row.conrelid
        JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname !~ '^pg_'
          AND namespace.nspname <> 'information_schema'
    ), '[]'::json),
    'indexes', COALESCE((
        SELECT json_agg(json_build_object(
            'schema', namespace.nspname,
            'table', relation.relname,
            'name', index_relation.relname,
            'unique', index_row.indisunique,
            'valid', index_row.indisvalid,
            'definition', pg_get_indexdef(index_row.indexrelid, 0, true)
        ) ORDER BY namespace.nspname, relation.relname, index_relation.relname)
        FROM pg_index index_row
        JOIN pg_class relation ON relation.oid = index_row.indrelid
        JOIN pg_class index_relation ON index_relation.oid = index_row.indexrelid
        JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname !~ '^pg_'
          AND namespace.nspname <> 'information_schema'
          AND NOT EXISTS (
              SELECT 1
              FROM pg_constraint owning_constraint
              WHERE owning_constraint.conindid = index_row.indexrelid
          )
    ), '[]'::json)
)::text;
"#;

pub(super) struct CatalogSnapshot {
    pub server_version: String,
    pub server_version_num: u32,
    pub inventory: PostgresCatalogInventory,
}

pub(super) fn collect(psql: &Psql, connection: &Connection) -> Result<CatalogSnapshot> {
    let output = psql.run(connection, CATALOG_SQL, false)?;
    if !output.success {
        anyhow::bail!("Could not inventory PostgreSQL catalog: {}", output.error);
    }
    let raw: RawCatalog = serde_json::from_str(output.stdout.trim())
        .context("PostgreSQL catalog query returned invalid JSON")?;
    let tables = raw
        .tables
        .into_iter()
        .map(|table| PostgresCatalogTable {
            schema: table.schema,
            name: table.name,
            kind: table_kind(&table.kind).to_string(),
        })
        .collect::<Vec<_>>();
    let columns = raw
        .columns
        .into_iter()
        .map(|column| PostgresCatalogColumn {
            schema: column.schema,
            table: column.table,
            name: column.name,
            position: column.position,
            data_type: PostgresTypeShape {
                oid: Some(column.type_oid),
                schema: Some(column.type_schema),
                name: column.type_name,
                formatted: column.formatted_type,
                base_type: column
                    .base_type_schema
                    .zip(column.base_type_name)
                    .map(|(schema, name)| PostgresTypeName { schema, name }),
                enum_values: column.enum_values,
                max_length: column.max_length,
                numeric_precision: column.numeric_precision,
                numeric_scale: column.numeric_scale,
            },
            nullable: column.nullable,
            default_digest: column.default_expression.as_deref().map(digest),
        })
        .collect::<Vec<_>>();
    let constraints = raw
        .constraints
        .into_iter()
        .map(|constraint| PostgresCatalogConstraint {
            schema: constraint.schema,
            table: constraint.table,
            name: constraint.name,
            kind: constraint_kind(&constraint.kind).to_string(),
            columns: constraint.columns,
            definition_digest: digest(&constraint.definition),
        })
        .collect::<Vec<_>>();
    let indexes = raw
        .indexes
        .into_iter()
        .map(|index| PostgresCatalogIndex {
            schema: index.schema,
            table: index.table,
            name: index.name,
            unique: index.unique,
            valid: index.valid,
            definition_digest: digest(&index.definition),
        })
        .collect::<Vec<_>>();
    let digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&(
            &tables,
            &columns,
            &constraints,
            &indexes
        ))?)
    );
    Ok(CatalogSnapshot {
        server_version: raw.server_version,
        server_version_num: raw.server_version_num,
        inventory: PostgresCatalogInventory {
            digest,
            tables,
            columns,
            constraints,
            indexes,
        },
    })
}

fn digest(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

fn table_kind(kind: &str) -> &str {
    match kind {
        "r" => "table",
        "p" => "partitioned_table",
        "v" => "view",
        "m" => "materialized_view",
        "f" => "foreign_table",
        _ => "unknown",
    }
}

fn constraint_kind(kind: &str) -> &str {
    match kind {
        "c" => "check",
        "f" => "foreign_key",
        "p" => "primary_key",
        "u" => "unique",
        "x" => "exclusion",
        _ => "unknown",
    }
}

#[derive(Deserialize)]
struct RawCatalog {
    server_version: String,
    server_version_num: u32,
    #[serde(default)]
    tables: Vec<RawTable>,
    #[serde(default)]
    columns: Vec<RawColumn>,
    #[serde(default)]
    constraints: Vec<RawConstraint>,
    #[serde(default)]
    indexes: Vec<RawIndex>,
}

#[derive(Deserialize)]
struct RawTable {
    schema: String,
    name: String,
    kind: String,
}

#[derive(Deserialize)]
struct RawColumn {
    schema: String,
    table: String,
    name: String,
    position: u32,
    type_oid: u32,
    type_schema: String,
    type_name: String,
    formatted_type: String,
    base_type_schema: Option<String>,
    base_type_name: Option<String>,
    #[serde(default)]
    enum_values: Vec<String>,
    max_length: Option<u32>,
    numeric_precision: Option<u32>,
    numeric_scale: Option<u32>,
    nullable: bool,
    default_expression: Option<String>,
}

#[derive(Deserialize)]
struct RawConstraint {
    schema: String,
    table: String,
    name: String,
    kind: String,
    #[serde(default)]
    columns: Vec<String>,
    definition: String,
}

#[derive(Deserialize)]
struct RawIndex {
    schema: String,
    table: String,
    name: String,
    unique: bool,
    valid: bool,
    definition: String,
}

#[cfg(test)]
mod tests {
    use super::{constraint_kind, table_kind};

    #[test]
    fn catalog_kinds_are_stable_public_names() {
        assert_eq!(table_kind("p"), "partitioned_table");
        assert_eq!(constraint_kind("f"), "foreign_key");
        assert_eq!(constraint_kind("?"), "unknown");
    }
}
