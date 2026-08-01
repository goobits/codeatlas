declare const db: { query(sql: string): Promise<void> }
declare const ownerId: string

void db.query('SELECT id FROM inline_users WHERE id = $1')
void db.query(`DELETE FROM inline_users WHERE id = ${ownerId}`)
