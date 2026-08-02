declare const db: { query(sql: string): Promise<void> }
declare const ownerId: string
declare const prisma: {
	$queryRaw(strings: TemplateStringsArray, ...values: unknown[]): Promise<unknown>
	$executeRaw(strings: TemplateStringsArray, ...values: unknown[]): Promise<unknown>
	$queryRawUnsafe(sql: string): Promise<unknown>
}
declare const Prisma: {
	sql(strings: TemplateStringsArray, ...values: unknown[]): unknown
}

void db.query('SELECT id FROM inline_users WHERE id = $1')
void db.query(`DELETE FROM inline_users WHERE id = ${ownerId}`)
void prisma.$queryRaw`SELECT id FROM prisma_users WHERE id = ${ownerId}`
void prisma.$executeRaw`UPDATE prisma_users SET active = true WHERE id = ${ownerId}`
void prisma.$queryRawUnsafe('SELECT count(*) FROM prisma_users')
void prisma.$queryRaw`SELECT id FROM prisma_users WHERE ${Prisma.sql`active = true`}`
