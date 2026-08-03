export const docsMetadata = import.meta.glob(
	['/packages/*/docs/meta/*.ts', '!/packages/b/docs/meta/excluded.ts'],
	{ eager: true }
)
read('b/src/absolute.ts')
