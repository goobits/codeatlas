const assert = require('node:assert/strict')
const test = require('node:test')
const { getCargoBuildArgs } = require('../bin/codeatlas.js')

test('source fallback uses a locked single-job build by default', () => {
	assert.deepEqual(getCargoBuildArgs({}), [
		'build',
		'--release',
		'--locked',
		'--jobs',
		'1'
	])
})

test('source fallback accepts an explicit CodeAtlas job limit', () => {
	assert.deepEqual(getCargoBuildArgs({ CODEATLAS_CARGO_JOBS: '4' }), [
		'build',
		'--release',
		'--locked',
		'--jobs',
		'4'
	])
})

test('source fallback accepts the standard Cargo job limit', () => {
	assert.deepEqual(getCargoBuildArgs({ CARGO_BUILD_JOBS: '2' }), [
		'build',
		'--release',
		'--locked',
		'--jobs',
		'2'
	])
})

test('source fallback rejects invalid job limits', () => {
	assert.throws(
		() => getCargoBuildArgs({ CODEATLAS_CARGO_JOBS: 'all' }),
		/Invalid Cargo job count: all/
	)
})
