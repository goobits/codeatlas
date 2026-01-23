#!/usr/bin/env node

const fs = require('fs')
const path = require('path')
const { spawnSync } = require('child_process')

const allowedBumps = new Set(['patch', 'minor', 'major', 'premajor', 'preminor', 'prepatch', 'prerelease'])
const bump = process.argv[2] || 'patch'

if (!allowedBumps.has(bump)) {
	console.error(`Unknown bump type: ${bump}`)
	process.exit(1)
}

const run = (cmd, args, options = {}) => {
	const result = spawnSync(cmd, args, { stdio: 'inherit', ...options })
	if (result.error) {
		throw result.error
	}
	if (typeof result.status === 'number' && result.status !== 0) {
		process.exit(result.status)
	}
	return result
}

const runCapture = (cmd, args) => {
	const result = spawnSync(cmd, args, { encoding: 'utf8', stdio: 'pipe' })
	if (result.error) {
		throw result.error
	}
	if (typeof result.status === 'number' && result.status !== 0) {
		process.exit(result.status)
	}
	return result.stdout || ''
}

const repoDirty = () => {
	const output = runCapture('git', ['status', '--porcelain'])
	return output.trim().length > 0
}

const readVersion = () => {
	const pkgPath = path.join(process.cwd(), 'package.json')
	const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'))
	return pkg.version
}

const main = () => {
	if (repoDirty()) {
		console.error('Working tree is dirty. Commit or stash changes before releasing.')
		process.exit(1)
	}

	run('pnpm', ['version', bump, '--no-git-tag-version'])

	const version = readVersion()
	run('git', ['add', 'package.json'])
	run('git', ['commit', '-m', `v${version}`])

	run('pnpm', ['publish', '--access', 'public'])
}

try {
	main()
} catch (error) {
	console.error('Release failed:', error.message)
	process.exit(1)
}
