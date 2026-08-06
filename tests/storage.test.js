const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')
const {
	requireExternalCargoTarget,
	requireExternalPath,
	writePrivateFile
} = require('../tasks/storage.js')

test('requires external disjoint Cargo storage', testContext => {
	const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codeatlas-storage-'))
	const checkout = path.join(root, 'checkout')
	const target = path.join(root, 'cache', 'cargo')
	fs.mkdirSync(checkout)
	testContext.after(() => fs.rmSync(root, { force: true, recursive: true }))

	assert.equal(requireExternalCargoTarget(checkout, { CARGO_TARGET_DIR: target }), target)
	assert.throws(() => requireExternalCargoTarget(checkout, {}), /must be an absolute path/)
	assert.throws(
		() => requireExternalCargoTarget(checkout, { CARGO_TARGET_DIR: 'target' }),
		/must be an absolute path/
	)
	assert.throws(
		() =>
			requireExternalCargoTarget(checkout, {
				CARGO_TARGET_DIR: path.join(checkout, 'target')
			}),
		/outside and disjoint/
	)
	assert.throws(
		() => requireExternalCargoTarget(checkout, { CARGO_TARGET_DIR: root }),
		/outside and disjoint/
	)
})

test('requires every generated path to be disjoint from the checkout', testContext => {
	const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codeatlas-external-path-'))
	const checkout = path.join(root, 'checkout')
	fs.mkdirSync(checkout)
	testContext.after(() => fs.rmSync(root, { force: true, recursive: true }))

	assert.equal(
		requireExternalPath(checkout, path.join(root, 'artifacts', 'probe.tar'), '--out'),
		path.join(root, 'artifacts', 'probe.tar')
	)
	assert.throws(
		() => requireExternalPath(checkout, path.join(checkout, 'probe.tar'), '--out'),
		/outside and disjoint/
	)
	const checkoutAlias = path.join(root, 'checkout-alias')
	fs.symlinkSync(checkout, checkoutAlias, 'dir')
	assert.throws(
		() => requireExternalPath(checkout, path.join(checkoutAlias, 'probe.tar'), '--out'),
		/outside and disjoint/
	)
})

test('private task artifacts retain owner-only permissions', testContext => {
	const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codeatlas-private-file-'))
	const destination = path.join(root, 'artifact.json')
	testContext.after(() => fs.rmSync(root, { force: true, recursive: true }))

	writePrivateFile(destination, '{"safe":true}\n')
	assert.equal(fs.readFileSync(destination, 'utf8'), '{"safe":true}\n')
	if (process.platform !== 'win32') {
		assert.equal(fs.statSync(destination).mode & 0o777, 0o600)
	}
})
