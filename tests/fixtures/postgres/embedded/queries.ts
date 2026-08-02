declare const db: { query(sql: string): Promise<void> }
declare const sql: (strings: TemplateStringsArray, ...values: unknown[]) => Promise<void>
declare const ownerId: string
declare const includeOrdering: boolean
declare const prisma: {
	$queryRaw(strings: TemplateStringsArray, ...values: unknown[]): Promise<unknown>
	$executeRaw(strings: TemplateStringsArray, ...values: unknown[]): Promise<unknown>
	$queryRawUnsafe(sql: string): Promise<unknown>
}
declare const Prisma: {
	sql(strings: TemplateStringsArray, ...values: unknown[]): unknown
}
const selectedColumns = sql('id')

void db.query('SELECT id FROM inline_users WHERE id = $1')
void db.query(`DELETE FROM inline_users WHERE id = ${ownerId}`)
void prisma.$queryRaw`SELECT id FROM prisma_users WHERE id = ${ownerId}`
void prisma.$executeRaw`UPDATE prisma_users SET active = true WHERE id = ${ownerId}`
void prisma.$queryRawUnsafe('SELECT count(*) FROM prisma_users')
void prisma.$queryRaw`SELECT id FROM prisma_users WHERE ${Prisma.sql`active = true`}`
void sql`SELECT id FROM inline_users WHERE id = ${ownerId}`
void sql<{ id: string }[]>`SELECT id FROM ${sql('inline_users')} WHERE id = ${ownerId}`
void sql`SELECT $3::bigint, id FROM inline_users WHERE id = ${ownerId}`
void sql`SELECT ${selectedColumns} FROM inline_users`
void sql`SELECT id FROM inline_users WHERE true ${includeOrdering ? sql`ORDER BY id` : sql``}`
void db.query(sql`SELECT id AS wrapped_id FROM inline_users WHERE id = ${ownerId}`)
