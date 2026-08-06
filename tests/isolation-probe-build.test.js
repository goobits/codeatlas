const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')
const {
	createBuildArguments,
	createBuilderArguments,
	createBuilderRemovalArguments,
	parseArguments,
	resolveRuntimeDataRoot,
	validateBuildArtifacts
} = require('../tasks/build-isolation-probe.js')
const { createRuntimeOptions } = require('../tasks/container-runtime.js')

const digest = 'a'.repeat(64)
const buildkitDigest = 'b'.repeat(64)

test('probe recipe verifies the musl static default without breaking procedural macros', () => {
	const recipe = fs.readFileSync(
		path.resolve(__dirname, '..', 'containers', 'isolation-conformance', 'Containerfile'),
		'utf8'
	)

	assert.match(recipe, /rustc --print cfg \| grep -Fx 'target_feature="crt-static"'/)
	assert.doesNotMatch(recipe, /target-feature=\+crt-static/)
})

test('probe build has one bounded solve with canonical OCI and optional Docker import outputs', () => {
	const options = parseArguments([
		'--runtime',
		'/usr/bin/docker',
		'--socket',
		'/run/docker.sock',
		'--build-image',
		`docker.io/library/rust@sha256:${digest}`,
		'--buildkit-image',
		`moby/buildkit@sha256:${buildkitDigest}`,
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
		loadOut: '/tmp/codeatlas-probe.docker.tar',
		sourceDateEpoch: '1700000000',
		builder: 'codeatlas-probe-1'
	})

	assert.deepEqual(arguments_.slice(0, 4), [
		'--config',
		'/tmp/client',
		'--host',
		'unix:///run/docker.sock'
	])
	assert.ok(arguments_.includes('none'))
	assert.ok(arguments_.includes('codeatlas-probe-1'))
	assert.ok(arguments_.includes('type=oci,dest=/tmp/codeatlas-probe.oci.tar,tar=true,rewrite-timestamp=true'))
	assert.ok(arguments_.includes('type=docker,dest=/tmp/codeatlas-probe.docker.tar'))
	assert.equal(arguments_.filter(argument => argument === '--output').length, 2)
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
			'--buildkit-image', `moby/buildkit@sha256:${buildkitDigest}`,
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
			'--buildkit-image', `moby/buildkit@sha256:${buildkitDigest}`,
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
			'--buildkit-image', `moby/buildkit@sha256:${buildkitDigest}`,
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

test('probe build owns one pinned disposable BuildKit builder', () => {
	const image = `moby/buildkit@sha256:${buildkitDigest}`
	assert.deepEqual(createBuilderArguments('codeatlas-probe-1', image), [
		'buildx',
		'create',
		'--name',
		'codeatlas-probe-1',
		'--driver',
		'docker-container',
		'--driver-opt',
		`image=${image}`,
		'--bootstrap'
	])
	assert.deepEqual(createBuilderRemovalArguments('codeatlas-probe-1'), [
		'buildx',
		'rm',
		'codeatlas-probe-1'
	])
})

test('probe build artifacts have finite nonzero byte ceilings', () => {
	const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codeatlas-probe-artifacts-'))
	const archive = path.join(root, 'probe.oci.tar')
	const loadArchive = path.join(root, 'probe.docker.tar')
	const metadata = path.join(root, 'probe.metadata.json')
	try {
		fs.writeFileSync(archive, 'archive')
		fs.writeFileSync(loadArchive, 'load archive')
		fs.writeFileSync(metadata, '{}')
		assert.deepEqual(validateBuildArtifacts(archive, metadata, loadArchive), {
			archiveBytes: 7,
			loadArchiveBytes: 12,
			metadataBytes: 2
		})
		fs.truncateSync(archive, 64 * 1024 * 1024 + 1)
		assert.throws(
			() => validateBuildArtifacts(archive, metadata),
			/1 through 67108864 bytes/
		)
		fs.truncateSync(archive, 7)
		fs.truncateSync(loadArchive, 64 * 1024 * 1024 + 1)
		assert.throws(
			() => validateBuildArtifacts(archive, metadata, loadArchive),
			/Docker import archive must contain 1 through 67108864 bytes/
		)
	} finally {
		fs.rmSync(root, { force: true, recursive: true })
	}
})
