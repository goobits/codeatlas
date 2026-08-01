import { IMPORTED_SCHEMA_SQL } from './schema-index.js'

declare function runMigrations(options: { bootstrapSql: readonly string[] }): void

runMigrations({ bootstrapSql: [IMPORTED_SCHEMA_SQL] })
