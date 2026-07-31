import path from 'node:path'

export default {
	resolve: {
		alias: [
			{
				find: 'excluded-component',
				replacement: path.resolve(__dirname, 'src/excluded.svelte')
			}
		]
	}
}
