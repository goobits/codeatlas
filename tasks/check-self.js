#!/usr/bin/env node

const fs = require('node:fs')
const path = require('node:path')
const { spawnSync } = require('node:child_process')
const { requireExternalCargoTarget } = require('./storage.js')

const rootDir = path.resolve(__dirname, '..')

try {
	const targetDir = requireExternalCargoTarget(rootDir)
	fs.mkdirSync(targetDir, { recursive: true })
	const result = spawnSync(
		'cargo',
		[
			'run',
			'--quiet',
			'--locked',
			'--',
			'--root',
			'.',
			'check',
			'code',
			'--format',
			'json',
			'--out',
			path.join(targetDir, 'codeatlas-self-check.json')
		],
		{ cwd: rootDir, stdio: 'inherit' }
	)
	if (result.error) throw result.error
	if (result.signal) throw new Error(`CodeAtlas self-check terminated by ${result.signal}`)
	process.exitCode = result.status ?? 1
} catch (error) {
	console.error(error instanceof Error ? error.message : String(error))
	process.exitCode = 1
}
