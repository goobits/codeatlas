export default {
	root: new URL('.', import.meta.url).pathname,
	test: {
		setupFiles: ['test/setup.ts']
	},
	resolve: {
		alias: [
			{ find: 'fixture', replacement: './src/test/mock.ts' }
		]
	}
}
