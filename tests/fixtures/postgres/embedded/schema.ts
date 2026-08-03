export const IMPORTED_SCHEMA_SQL = `
CREATE TABLE imported_users (
	id BIGINT PRIMARY KEY
);
CREATE TABLE prisma_users (
	id BIGINT PRIMARY KEY,
	active BOOLEAN NOT NULL DEFAULT false
);
`

export const COMPOSED_EXTENSION_SQL = `
CREATE TABLE composed_extension (
	id BIGINT PRIMARY KEY
);
`
