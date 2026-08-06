const assert = require('node:assert/strict')
const test = require('node:test')
const {
	createRegistryArguments,
	parseArguments,
	parseLoadedImage,
	parseRegistryAddress,
	selectPublishedReference,
	verifyLoadedImage,
	waitForRegistry
} = require('../tasks/check-isolation-live.js')

const digest = 'a'.repeat(64)
const registryDigest = 'b'.repeat(64)

test('live isolation accepts one exact bounded runner contract', () => {
	const options = parseArguments([
		'--runtime', '/usr/bin/docker',
		'--socket', '/var/run/docker.sock',
		'--build-image', `docker.io/library/rust@sha256:${digest}`,
		'--registry-image', `docker.io/library/registry@sha256:${registryDigest}`,
		'--platform', 'linux/amd64',
		'--network', 'allow',
		'--out-dir', '/tmp/codeatlas-live'
	])
	assert.equal(options.platform, 'linux/amd64')
	assert.equal(options.network, 'allow')
	assert.throws(
		() => parseArguments([
			'--runtime', '/usr/bin/docker',
			'--socket', '/var/run/docker.sock',
			'--build-image', 'rust:latest',
			'--registry-image', `registry@sha256:${registryDigest}`,
			'--platform', 'linux/amd64',
			'--network', 'allow',
			'--out-dir', '/tmp/codeatlas-live'
		]),
		/exact repository@sha256/
	)
	assert.throws(
		() => parseArguments([
			'--runtime', '/usr/bin/docker',
			'--socket', '/var/run/docker.sock',
			'--build-image', `rust@sha256:${digest}`,
			'--registry-image', `registry@sha256:${registryDigest}`,
			'--platform', 'linux/amd64',
			'--network', 'allow',
			'--out-dir', '/tmp/codeatlas-live',
			'--command', 'anything'
		]),
		/Unknown live isolation option/
	)
})

test('temporary registry is loopback-only, bounded, and unprivileged', () => {
	const arguments_ = createRegistryArguments(
		'codeatlas-live-registry-1',
		`registry@sha256:${registryDigest}`
	)
	assert.ok(arguments_.includes('127.0.0.1::5000'))
	assert.ok(arguments_.includes('--read-only'))
	assert.ok(arguments_.includes('--cap-drop'))
	assert.ok(arguments_.includes('no-new-privileges=true'))
	assert.ok(arguments_.includes('--memory'))
	assert.ok(arguments_.includes('--pids-limit'))
	assert.ok(!arguments_.includes('--privileged'))
	assert.ok(!arguments_.includes('/var/run/docker.sock'))
})

test('image import and publication preserve their distinct exact digests', () => {
	const imageId = `sha256:${digest}`
	const manifestDigest = `sha256:${registryDigest}`
	assert.equal(parseLoadedImage(`Loaded image ID: ${imageId}\n`), imageId)
	assert.throws(() => parseLoadedImage('Loaded image: latest\n'), /exact image ID/)
	assert.equal(verifyLoadedImage(JSON.stringify(imageId), imageId), imageId)
	assert.throws(
		() => verifyLoadedImage(JSON.stringify(manifestDigest), imageId),
		/differs from the loaded image ID/
	)
	assert.equal(parseRegistryAddress('127.0.0.1:49153\n'), '127.0.0.1:49153')
	assert.throws(() => parseRegistryAddress('0.0.0.0:5000\n'), /loopback IPv4/)
	const repository = '127.0.0.1:49153/codeatlas-isolation-probe'
	const reference = `${repository}@${manifestDigest}`
	assert.equal(
		selectPublishedReference(JSON.stringify([reference]), repository, manifestDigest),
		reference
	)
	assert.throws(
		() => selectPublishedReference(JSON.stringify([]), repository, manifestDigest),
		/differs from the built OCI manifest/
	)
})

test('registry readiness is bounded and requires an HTTP 200 response', async () => {
	let calls = 0
	await waitForRegistry(
		'127.0.0.1:5000',
		async () => {
			calls += 1
			return { status: calls === 2 ? 200 : 503 }
		},
		async () => {}
	)
	assert.equal(calls, 2)
})
