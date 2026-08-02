#!/usr/bin/env node

const fs = require('fs')
const path = require('path')
const { spawnSync } = require('child_process')
const { requireExternalCargoTarget } = require('./storage.js')

const allowedBumps = new Set(['patch', 'minor', 'major', 'premajor', 'preminor', 'prepatch', 'prerelease'])
const bump = process.argv[2] || 'patch'
const nodeModulesPath = path.join(process.cwd(), 'node_modules')
const packageJsonPath = path.join(process.cwd(), 'package.json')
const cargoTomlPath = path.join(process.cwd(), 'Cargo.toml')

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

const readPackageJson = () => JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'))

const syncCargoVersion = (version) => {
	const cargoToml = fs.readFileSync(cargoTomlPath, 'utf8')
	const nextCargoToml = cargoToml.replace(
		/(\[package\][\s\S]*?\nversion\s*=\s*")[^"]+("\s*\n)/,
		`$1${version}$2`
	)
	if (nextCargoToml === cargoToml) {
		throw new Error('Could not update [package].version in Cargo.toml')
	}
	fs.writeFileSync(cargoTomlPath, nextCargoToml)
}

const repoDirty = () => {
	const output = runCapture('git', ['status', '--porcelain'])
	return output.trim().length > 0
}

const readVersion = () => readPackageJson().version

const parseVersion = (version) => {
	const match = /^(\d+)\.(\d+)\.(\d+)(-.+)?$/.exec(version)
	if (!match) return null
	return {
		major: Number.parseInt(match[1], 10),
		minor: Number.parseInt(match[2], 10),
		patch: Number.parseInt(match[3], 10),
		prerelease: match[4] || ''
	}
}

const compareVersions = (a, b) => {
	if (a.major !== b.major) return a.major - b.major
	if (a.minor !== b.minor) return a.minor - b.minor
	if (a.patch !== b.patch) return a.patch - b.patch
	if (a.prerelease === b.prerelease) return 0
	if (!a.prerelease) return 1
	if (!b.prerelease) return -1
	return a.prerelease.localeCompare(b.prerelease)
}

const bumpVersion = (base, bumpType) => {
	const parsed = parseVersion(base)
	if (!parsed) return null
	const next = { ...parsed, prerelease: '' }

	switch (bumpType) {
		case 'major':
			next.major += 1
			next.minor = 0
			next.patch = 0
			break
		case 'minor':
			next.minor += 1
			next.patch = 0
			break
		case 'patch':
			next.patch += 1
			break
		default:
			return null
	}

	return `${next.major}.${next.minor}.${next.patch}`
}

const getPublishedVersion = (packageName) => {
	const result = spawnSync('npm', ['view', packageName, 'version', '--json'], { encoding: 'utf8' })
	if (result.error || result.status !== 0) {
		return null
	}
	const raw = (result.stdout || '').trim()
	if (!raw) return null
	try {
		const parsed = JSON.parse(raw)
		return typeof parsed === 'string' ? parsed : null
	} catch {
		return raw
	}
}

const resolveNextVersion = () => {
	if (!['major', 'minor', 'patch'].includes(bump)) {
		return null
	}

	const pkg = readPackageJson()
	const current = pkg.version
	const published = getPublishedVersion(pkg.name)
	if (!published) {
		return bumpVersion(current, bump)
	}

	const currentParsed = parseVersion(current)
	const publishedParsed = parseVersion(published)
	if (!currentParsed || !publishedParsed) {
		return bumpVersion(current, bump)
	}

	const base = compareVersions(currentParsed, publishedParsed) <= 0 ? published : current
	return bumpVersion(base, bump)
}

const main = () => {
	requireExternalCargoTarget()
	if (!fs.existsSync(nodeModulesPath)) {
		console.error('Dependencies are missing. Install them before releasing CodeAtlas.')
		process.exit(1)
	}

	if (repoDirty()) {
		console.error('Working tree is dirty. Commit or stash changes before releasing.')
		process.exit(1)
	}

	const nextVersion = resolveNextVersion()
	if (nextVersion) {
		run('pnpm', ['version', nextVersion, '--no-git-tag-version'])
	} else {
		run('pnpm', ['version', bump, '--no-git-tag-version'])
	}

	const version = readVersion()
	syncCargoVersion(version)
	run('cargo', ['check', '--jobs', '1'])
	run('cargo', ['test', '--locked', '--jobs', '1'])
	run('node', ['tasks/check-package.js'])
	run('git', ['add', 'package.json', 'pnpm-lock.yaml', 'Cargo.toml', 'Cargo.lock'])
	run('git', ['commit', '-m', `v${version}`])
	run('git', ['tag', `v${version}`])
	console.log(`Created v${version}. Push the commit and tag to run the release workflow.`)
}

try {
	main()
} catch (error) {
	console.error('Release failed:', error.message)
	process.exit(1)
}
