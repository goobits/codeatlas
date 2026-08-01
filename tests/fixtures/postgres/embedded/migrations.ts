import { IMPORTED_SCHEMA_SQL } from './schema.js'

export const migrations = [
	{
		name: '001_inline.sql',
		sql: `CREATE TABLE inline_users (id BIGINT PRIMARY KEY);`
	},
	{
		name: '002_imported.sql',
		sql: IMPORTED_SCHEMA_SQL
	}
]
