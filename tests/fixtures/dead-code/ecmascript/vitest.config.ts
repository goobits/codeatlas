import path from 'node:path'

const mockModule = path.resolve(__dirname, './src/test/mock.ts')

export default {
	root: new URL('.', import.meta.url).pathname,
	test: {
		setupFiles: ['test/setup.ts']
	},
	resolve: {
		alias: [
			{ find: 'fixture', replacement: mockModule }
		]
	}
}
