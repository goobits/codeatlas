const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const test = require('node:test')

const repositoryRoot = path.resolve(__dirname, '..')

test('workspace names product crates while keeping the isolation probe standalone', () => {
	const manifest = fs.readFileSync(path.join(repositoryRoot, 'Cargo.toml'), 'utf8')
	const probeManifest = fs.readFileSync(
		path.join(repositoryRoot, 'crates', 'isolation-conformance', 'Cargo.toml'),
		'utf8'
	)

	assert.match(
		manifest,
		/^members = \["crates\/domain", "crates\/source", "crates\/languages"\]$/m
	)
	assert.doesNotMatch(manifest, /^members = .*crates\/\*/m)
	assert.match(manifest, /^exclude = \["crates\/isolation-conformance"\]$/m)
	assert.match(probeManifest, /^\[workspace\]$/m)
	assert.equal(
		fs.existsSync(path.join(repositoryRoot, 'crates', 'isolation-conformance', 'Cargo.lock')),
		true
	)
})
