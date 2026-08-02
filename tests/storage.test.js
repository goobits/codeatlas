const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')
const { requireExternalCargoTarget } = require('../tasks/storage.js')

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
