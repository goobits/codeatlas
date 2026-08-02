import { COMPOSED_EXTENSION_SQL, IMPORTED_SCHEMA_SQL } from './schema.js'

const AUDIT_SCHEMA_SQL = `CREATE TABLE composed_audit (id BIGINT PRIMARY KEY);`

export const migrations = [
	{
		name: '001_inline.sql',
		sql: `CREATE TABLE inline_users (id BIGINT PRIMARY KEY);`
	},
	{
		name: '002_imported.sql',
		sql: IMPORTED_SCHEMA_SQL
	},
	{
		name: '003_composed.sql',
		sql: `${COMPOSED_EXTENSION_SQL}\n${AUDIT_SCHEMA_SQL}`
	}
]
