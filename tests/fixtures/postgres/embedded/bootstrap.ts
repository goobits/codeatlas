export const BOOTSTRAP_SQL = `
CREATE TABLE bootstrap_settings (
	key TEXT PRIMARY KEY,
	value JSONB NOT NULL
);
`

export const migrations = [
	{
		name: '000_bootstrap_audit.sql',
		sql: `CREATE TABLE bootstrap_audit (id BIGINT PRIMARY KEY);`
	}
]
