const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')
const {
	createBuildArguments,
	parseArguments,
	validateBuildArtifacts
} = require('../tasks/build-isolation-probe.js')
const {
	createBuilderArguments,
	createBuilderRemovalArguments,
	resolveRuntimeDataRoot,
	validateSpecification
} = require('../tasks/build-container-image.js')
const {
	createBuildArguments: createHttpBuildArguments,
	parseArguments: parseHttpArguments,
	validateBuildArtifacts: validateHttpBuildArtifacts
} = require('../tasks/build-http-workload.js')
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

test('HTTP workload recipe is hash-locked, version-checked, and clears inherited image metadata', () => {
	const recipe = fs.readFileSync(
		path.resolve(__dirname, '..', 'containers', 'http-fuzz', 'Containerfile'),
		'utf8'
	)

	assert.match(recipe, /--require-hashes/)
	assert.match(recipe, /-q\s+\\\n\s+-f\s+\\\n\s+--invalidation-mode checked-hash/)
	assert.match(recipe, /--invalidation-mode checked-hash/)
	assert.doesNotMatch(recipe, /--quiet|--force/)
	assert.match(recipe, /4\\\.24\\\.3/)
	assert.match(recipe, /FROM scratch\nCOPY --from=runtime \/ \/\n$/)
	assert.doesNotMatch(recipe.slice(recipe.lastIndexOf('FROM scratch')), /^ENV /m)
	assert.equal(
		fs.readFileSync(
			path.resolve(
				__dirname,
				'..',
				'containers',
				'http-fuzz',
				'Containerfile.dockerignore'
			),
			'utf8'
		),
		'**\n!src/\n!src/http/\n!src/http/schemathesis/\n!src/http/schemathesis/requirements.txt\n'
	)
})

test('container image specifications use the runtime log-label contract', () => {
	const buildImage = `docker.io/library/rust@sha256:${digest}`
	const specification = validateSpecification({
		name: 'Probe',
		slug: 'isolation-probe',
		containerfile: path.resolve(
			__dirname,
			'..',
			'containers',
			'isolation-conformance',
			'Containerfile'
		),
		context: path.resolve(__dirname, '..', 'crates', 'isolation-conformance'),
		buildArguments: { BUILD_IMAGE: buildImage },
		pinnedImages: [{ image: buildImage, label: 'build-image' }],
		maxArchiveBytes: 1024,
		out: '/tmp/probe.oci.tar'
	})
	assert.equal(specification.pinnedImages[0].label, 'isolation-probe-build-image')
	assert.throws(
		() => validateSpecification({
			...specification,
			pinnedImages: [{ image: buildImage, label: 'build image' }]
		}),
		/Runtime log label is invalid/
	)
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

test('HTTP workload build reuses the bounded image transaction with an exact Python input', () => {
	const pythonDigest = 'd'.repeat(64)
	const options = parseHttpArguments([
		'--runtime', '/usr/bin/docker',
		'--socket', '/run/docker.sock',
		'--python-image', `python@sha256:${pythonDigest}`,
		'--buildkit-image', `moby/buildkit@sha256:${buildkitDigest}`,
		'--platform', 'linux/amd64',
		'--network', 'allow',
		'--out', '/tmp/codeatlas-http.oci.tar'
	])
	const arguments_ = createHttpBuildArguments({
		...options,
		clientRoot: '/tmp/client',
		metadata: '/tmp/http.metadata.json',
		loadOut: '/tmp/codeatlas-http.docker.tar',
		sourceDateEpoch: '1700000000',
		builder: 'codeatlas-images-1'
	})

	assert.ok(arguments_.includes(`PYTHON_IMAGE=python@sha256:${pythonDigest}`))
	assert.ok(arguments_.includes('default'))
	assert.equal(arguments_.at(-1), path.resolve(__dirname, '..'))
	assert.equal(arguments_.filter(argument => argument === '--output').length, 2)
	assert.throws(
		() => parseHttpArguments([
			'--runtime', '/usr/bin/docker',
			'--socket', '/run/docker.sock',
			'--python-image', 'python:3.10',
			'--buildkit-image', `moby/buildkit@sha256:${buildkitDigest}`,
			'--platform', 'linux/amd64',
			'--network', 'allow',
			'--out', '/tmp/http.tar'
		]),
		/exact repository@sha256/
	)
	assert.throws(
		() => parseHttpArguments([
			'--runtime', '/usr/bin/docker',
			'--socket', '/run/docker.sock',
			'--python-image', `python@sha256:${pythonDigest}`,
			'--buildkit-image', `moby/buildkit@sha256:${buildkitDigest}`,
			'--platform', 'linux/amd64',
			'--network', 'deny',
			'--out', '/tmp/http.tar'
		]),
		/exactly allow/
	)
})

test('container image build owns one pinned disposable BuildKit builder', () => {
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
		fs.truncateSync(loadArchive, 65 * 1024 * 1024)
		assert.deepEqual(validateHttpBuildArtifacts(archive, metadata, loadArchive), {
			archiveBytes: 7,
			loadArchiveBytes: 65 * 1024 * 1024,
			metadataBytes: 2
		})
	} finally {
		fs.rmSync(root, { force: true, recursive: true })
	}
})
