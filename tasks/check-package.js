#!/usr/bin/env node

const fs = require('fs')
const os = require('os')
const path = require('path')
const { spawnSync } = require('child_process')

const rootDir = path.resolve(__dirname, '..')
const pkg = require(path.join(rootDir, 'package.json'))
const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'codeatlas-pack-'))

const requiredFiles = [
	'bin/codeatlas.js',
	'Cargo.lock',
	'Cargo.toml',
	'LICENSE',
	'package.json',
	'README.md',
	'src/http/schemathesis-requirements.txt',
	'src/http/schemathesis_hooks.py',
	'src/main.rs'
]
const forbiddenPrefixes = ['.github/', 'node_modules/', 'target/', 'tasks/']
const isForbiddenFile = file =>
	forbiddenPrefixes.some(prefix => file.startsWith(prefix)) ||
	file.split('/').includes('__pycache__') ||
	/\.py[co]$/.test(file)

try {
	const result = spawnSync('pnpm', [
		'pack',
		'--json',
		'--pack-destination',
		tempDir
	], {
		cwd: rootDir,
		encoding: 'utf8'
	})

	if (result.error) {
		throw result.error
	}
	if (result.status !== 0) {
		process.stderr.write(result.stderr || result.stdout)
		process.exit(result.status || 1)
	}

	const manifest = JSON.parse(result.stdout)
	const files = new Set(manifest.files.map(file => file.path))
	const missing = requiredFiles.filter(file => !files.has(file))
	const forbidden = [...files].filter(isForbiddenFile)

	if (manifest.name !== pkg.name || manifest.version !== pkg.version) {
		throw new Error(`Packed identity ${manifest.name}@${manifest.version} does not match package.json`)
	}
	if (missing.length > 0) {
		throw new Error(`Packed artifact is missing: ${missing.join(', ')}`)
	}
	if (forbidden.length > 0) {
		throw new Error(`Packed artifact contains internal files: ${forbidden.join(', ')}`)
	}

	console.log(`Packed ${manifest.name}@${manifest.version}: ${files.size} files`)
} finally {
	fs.rmSync(tempDir, { force: true, recursive: true })
}
