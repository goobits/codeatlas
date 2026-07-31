import { resolve } from 'node:path'

export function createCanvasShimAlias(workspaceRoot: string) {
	return {
		find: 'canvas',
		replacement: resolve(workspaceRoot, 'packages/b/src/canvasBrowserShim.js')
	}
}
