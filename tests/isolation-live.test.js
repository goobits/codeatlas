const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
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
const buildkitDigest = 'c'.repeat(64)
const workflow = fs.readFileSync(
	path.resolve(__dirname, '..', '.github', 'workflows', 'live-oci-isolation.yml'),
	'utf8'
)

test('hosted isolation is manual, finite, least-authority, and immutable', () => {
	assert.match(workflow, /^on:\n  workflow_dispatch:\n/m)
	assert.doesNotMatch(workflow, /^  (push|pull_request|pull_request_target|schedule):/m)
	assert.match(workflow, /^permissions:\n  contents: read$/m)
	assert.match(workflow, /if: github\.ref_name == github\.event\.repository\.default_branch/)
	assert.match(workflow, /timeout-minutes: 90/)
	assert.doesNotMatch(workflow, /\bsecrets\./)
	assert.doesNotMatch(workflow, /--privileged|\/var\/run\/docker\.sock:/)
	for (const pin of [
		'actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
		'actions/setup-node@820762786026740c76f36085b0efc47a31fe5020',
		'actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9',
		'actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9',
		'actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a'
	]) {
		assert.ok(workflow.includes(pin), `missing immutable action pin ${pin}`)
	}
	for (const imageVariable of [
		'BUILD_IMAGE',
		'PYTHON_BASE_IMAGE',
		'RUST_BASE_IMAGE',
		'BUILDKIT_IMAGE',
		'REGISTRY_IMAGE'
	]) {
		assert.match(
			workflow,
			new RegExp(`^      ${imageVariable}: [a-z0-9][a-z0-9._/-]*@sha256:[0-9a-f]{64}$`, 'm')
		)
	}
})

test('hosted Cargo cache has bounded compatible generations and an external owner', () => {
	for (const identity of [
		'runner.os',
		'runner.arch',
		'github.sha',
		'codeatlas-cargo-v2-${CACHE_OS}-${CACHE_ARCH}-${rust_fingerprint}-${dependency_fingerprint}-',
		'cache_key="${compatibility_prefix}${SOURCE_REVISION}"',
		'steps.rust-identity.outputs.cache_key',
		'steps.rust-identity.outputs.compatibility_prefix',
		'steps.cargo-cache.outputs.cache-matched-key',
		'Cargo.toml',
		'Cargo.lock',
		'crates/isolation-conformance/Cargo.toml',
		'crates/isolation-conformance/Cargo.lock',
		"-name 'execution_isolation-*'"
	]) {
		assert.ok(workflow.includes(identity), `missing cache identity ${identity}`)
	}
	assert.match(workflow, /CARGO_TARGET_DIR: \/tmp\/codeatlas-cargo-target/)
	assert.match(workflow, /CODEATLAS_CACHE_LIMIT_BYTES: 6000000000/)
	assert.match(workflow, /steps\.final-cache\.outputs\.eligible == 'true'/)
	assert.match(workflow, /steps\.final-cache\.outputs\.useful == 'true'/)
	assert.match(
		workflow,
		/restore-keys: \|\n\s+\$\{\{ steps\.rust-identity\.outputs\.compatibility_prefix \}\}/
	)
	assert.equal(workflow.match(/restore-keys:/g)?.length, 1)
	assert.doesNotMatch(workflow, /codeatlas-cargo-v1-/)
	assert.doesNotMatch(workflow, /cache-from|cache-to/)
})

test('live isolation accepts one exact bounded runner contract', () => {
	const options = parseArguments([
		'--runtime', '/usr/bin/docker',
		'--socket', '/var/run/docker.sock',
		'--build-image', `docker.io/library/rust@sha256:${digest}`,
		'--python-base-image', `docker.io/library/python@sha256:${digest}`,
		'--rust-base-image', `docker.io/library/rust@sha256:${digest}`,
		'--buildkit-image', `moby/buildkit@sha256:${buildkitDigest}`,
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
			'--python-base-image', `python@sha256:${digest}`,
			'--rust-base-image', `rust@sha256:${digest}`,
			'--buildkit-image', `moby/buildkit@sha256:${buildkitDigest}`,
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
			'--python-base-image', `python@sha256:${digest}`,
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
		selectPublishedReference(JSON.stringify([reference]), repository),
		reference
	)
	assert.throws(
		() => selectPublishedReference(JSON.stringify([]), repository),
		/one exact repository digest/
	)
})

test('live orchestration builds all images through one owner and one import each', () => {
	const source = fs.readFileSync(
		path.resolve(__dirname, '..', 'tasks', 'check-isolation-live.js'),
		'utf8'
	)
	assert.match(source, /isolation-probe\.oci\.tar/)
	assert.match(source, /isolation-probe\.docker\.tar/)
	assert.match(source, /http-workload\.oci\.tar/)
	assert.match(source, /http-workload\.docker\.tar/)
	assert.match(source, /code-fuzz-python-workload\.oci\.tar/)
	assert.match(source, /code-fuzz-python-workload\.docker\.tar/)
	assert.match(source, /code-fuzz-rust-workload\.oci\.tar/)
	assert.match(source, /code-fuzz-rust-workload\.docker\.tar/)
	assert.equal(source.match(/buildContainerImages\(/g)?.length, 1)
	assert.match(source, /probeSpecification\(/)
	assert.equal(source.match(/workloadSpecification\(/g)?.length, 3)
	assert.match(source, /probe_import_archive_cleanup_verified/)
	assert.match(source, /http_workload_import_archive_cleanup_verified/)
	assert.match(source, /python_code_workload_import_archive_cleanup_verified/)
	assert.match(source, /rust_code_workload_import_archive_cleanup_verified/)
	assert.doesNotMatch(source, /'image',\s*'load',[\s\S]{0,100}\barchive\b/)
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
