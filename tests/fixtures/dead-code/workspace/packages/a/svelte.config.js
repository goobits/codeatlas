import { fileURLToPath } from 'node:url'

export default {
	kit: {
		alias: {
			'@i18n': 'src/i18n',
			'@fixture/b': fileURLToPath(new URL('../b/src/index.ts', import.meta.url))
		}
	}
}
