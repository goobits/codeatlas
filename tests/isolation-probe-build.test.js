const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const test = require('node:test')
const {
	createBuildArguments,
	parseArguments,
	resolveRuntimeDataRoot
} = require('../tasks/build-isolation-probe.js')
const { createRuntimeOptions } = require('../tasks/container-runtime.js')

const digest = 'a'.repeat(64)

test('probe recipe verifies the musl static default without breaking procedural macros', () => {
	const recipe = fs.readFileSync(
		path.resolve(__dirname, '..', 'containers', 'isolation-conformance', 'Containerfile'),
		'utf8'
	)

	assert.match(recipe, /rustc --print cfg \| grep -Fx 'target_feature="crt-static"'/)
	assert.doesNotMatch(recipe, /target-feature=\+crt-static/)
})

test('probe build has one explicit bounded network and OCI output contract', () => {
	const options = parseArguments([
		'--runtime',
		'/usr/bin/docker',
		'--socket',
		'/run/docker.sock',
		'--build-image',
		`docker.io/library/rust@sha256:${digest}`,
		'--platform',
		'linux/arm64',
		'--network',
		'deny',
		'--out',
		'/tmp/codeatlas-probe.oci.tar'
	])
	const arguments_ = createBuildArguments({
		...options,
		clientRoot: '/tmp/client',
		metadata: '/tmp/probe.metadata.json',
		sourceDateEpoch: '1700000000'
	})

	assert.deepEqual(arguments_.slice(0, 4), [
		'--config',
		'/tmp/client',
		'--host',
		'unix:///run/docker.sock'
	])
	assert.ok(arguments_.includes('none'))
	assert.ok(arguments_.includes('type=oci,dest=/tmp/codeatlas-probe.oci.tar,tar=true,rewrite-timestamp=true'))
	assert.ok(!arguments_.includes('--push'))
	const runtimeOptions = createRuntimeOptions('/tmp/client')
	assert.equal(runtimeOptions.maxBuffer, 16 * 1024 * 1024)
	assert.equal(runtimeOptions.timeout, 30 * 60 * 1000)
	assert.equal(runtimeOptions.killSignal, 'SIGKILL')
	assert.throws(
		() => parseArguments([
			'--runtime', '/usr/bin/docker',
			'--socket', '/run/docker.sock',
			'--build-image', 'rust:latest',
			'--platform', 'linux/arm64',
			'--network', 'deny',
			'--out', '/tmp/probe.tar'
		]),
		/exact repository@sha256/
	)
	assert.throws(
		() => parseArguments([
			'--runtime', '/usr/bin/docker',
			'--socket', '/run/docker.sock',
			'--build-image', `rust@sha256:${digest}`,
			'--platform', 'linux/arm64',
			'--network', 'maybe',
			'--out', '/tmp/probe.tar'
		]),
		/exactly allow or deny/
	)
	assert.throws(
		() => parseArguments([
			'--runtime', '/usr/bin/docker',
			'--socket', '/run/docker.sock',
			'--build-image', `rust@sha256:${digest}`,
			'--platform', 'linux/arm64',
			'--network', 'deny',
			'--out', '/tmp/probe.tar,push=true'
		]),
		/exporter separators/
	)
	assert.equal(
		resolveRuntimeDataRoot(path.resolve(__dirname, '..'), JSON.stringify('/tmp/codeatlas-oci')),
		'/tmp/codeatlas-oci'
	)
	assert.throws(
		() => resolveRuntimeDataRoot(
			path.resolve(__dirname, '..'),
			JSON.stringify(path.resolve(__dirname, '..', 'runtime-data'))
		),
		/outside and disjoint/
	)
})
