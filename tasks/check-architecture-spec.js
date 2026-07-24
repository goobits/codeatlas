#!/usr/bin/env node

const crypto = require('crypto')
const fs = require('fs')
const path = require('path')

const rootDir = path.resolve(__dirname, '..')
const specDir = path.join(rootDir, 'spec', 'architecture', 'v0.1')
const manifestPath = path.join(specDir, 'MANIFEST.sha256')
const write = process.argv.includes('--write')

function collectFiles(directory, files = []) {
	for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
		const filePath = path.join(directory, entry.name)
		const relativePath = path.relative(specDir, filePath).split(path.sep).join('/')

		if (relativePath === 'MANIFEST.sha256') {
			continue
		}
		if (entry.isSymbolicLink()) {
			throw new Error(`Architecture specification cannot contain a symlink: ${relativePath}`)
		}
		if (entry.isDirectory()) {
			collectFiles(filePath, files)
		} else if (entry.isFile()) {
			files.push(relativePath)
		}
	}
	return files
}

function buildManifest() {
	const lines = [
		'# generated: true',
		'# generator: codeatlas.task.check-architecture-spec/1',
		'# command: pnpm run spec:write',
		'# manual-editing: prohibited',
		'# excludes: MANIFEST.sha256'
	]

	for (const relativePath of collectFiles(specDir).sort()) {
		const bytes = fs.readFileSync(path.join(specDir, relativePath))
		const digest = crypto.createHash('sha256').update(bytes).digest('hex')
		lines.push(`${digest}  ${relativePath}`)
	}
	return `${lines.join('\n')}\n`
}

const expected = buildManifest()
if (write) {
	const temporaryPath = `${manifestPath}.${process.pid}.tmp`
	fs.writeFileSync(temporaryPath, expected)
	try {
		fs.renameSync(temporaryPath, manifestPath)
	} catch (error) {
		if (!fs.existsSync(manifestPath)) {
			fs.rmSync(temporaryPath, { force: true })
			throw error
		}
		fs.rmSync(manifestPath)
		fs.renameSync(temporaryPath, manifestPath)
	}
	console.log('Updated spec/architecture/v0.1/MANIFEST.sha256')
	process.exit(0)
}

const actual = fs.readFileSync(manifestPath, 'utf8')
if (actual !== expected) {
	console.error('Architecture specification manifest is stale. Run pnpm run spec:write.')
	process.exit(1)
}

console.log('Architecture specification manifest is current')
