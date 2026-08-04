#!/usr/bin/env node

const { spawnSync } = require('node:child_process')
const path = require('node:path')
const { requireExternalCargoTarget } = require('./storage.js')

const repositoryRoot = path.resolve(__dirname, '..')
requireExternalCargoTarget(repositoryRoot)

const result = spawnSync(
	'cargo',
	[
		'test',
		'--locked',
		'--bin',
		'codeatlas',
		'published_schemas::tests::update_published_schemas',
		'--',
		'--ignored',
		'--exact',
	],
	{
		cwd: repositoryRoot,
		encoding: 'utf8',
		stdio: 'inherit',
	}
)

if (result.error) {
	throw result.error
}
process.exitCode = result.status ?? 1
