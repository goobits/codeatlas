#!/usr/bin/env node

const fs = require('node:fs')
const path = require('node:path')
const { spawnSync } = require('node:child_process')
const {
	requireExternalCargoTarget,
	requireExternalPath
} = require('./storage.js')
const {
	createRuntimeArguments,
	digestPattern,
	requireDigestImage,
	runRuntime,
	validateRuntime
} = require('./container-runtime.js')

const repositoryRoot = path.resolve(__dirname, '..')
const probeRoot = path.join(repositoryRoot, 'crates', 'isolation-conformance')
const containerfile = path.join(
	repositoryRoot,
	'containers',
	'isolation-conformance',
	'Containerfile'
)
const maxArchiveBytes = 64 * 1024 * 1024
const maxMetadataBytes = 1024 * 1024
const parseArguments = arguments_ => {
	const values = new Map()
	for (let index = 0; index < arguments_.length; index += 2) {
		const name = arguments_[index]
		const value = arguments_[index + 1]
		if (!name?.startsWith('--') || value === undefined || value.startsWith('--')) {
			throw new Error('Probe build options must be exact --name value pairs')
		}
		if (values.has(name)) throw new Error(`Probe build option ${name} was repeated`)
		values.set(name, value)
	}
	const known = new Set([
		'--runtime',
		'--socket',
		'--build-image',
		'--buildkit-image',
		'--platform',
		'--network',
		'--out'
	])
	for (const name of values.keys()) {
		if (!known.has(name)) throw new Error(`Unknown probe build option ${name}`)
	}
	for (const name of known) {
		if (!values.has(name)) throw new Error(`Missing required probe build option ${name}`)
	}
	const network = values.get('--network')
	if (network !== 'allow' && network !== 'deny') {
		throw new Error('--network must be exactly allow or deny')
	}
	const platform = values.get('--platform')
	if (!/^linux\/(amd64|arm64)$/.test(platform)) {
		throw new Error('--platform must be exactly linux/amd64 or linux/arm64')
	}
	const buildImage = requireDigestImage(values.get('--build-image'), '--build-image')
	const buildkitImage = requireDigestImage(
		values.get('--buildkit-image'),
		'--buildkit-image'
	)
	const out = values.get('--out')
	if (/[,\n\r\0]/.test(out)) {
		throw new Error('--out cannot contain OCI exporter separators or control characters')
	}
	return {
		runtime: values.get('--runtime'),
		socket: values.get('--socket'),
		buildImage,
		buildkitImage,
		platform,
		network,
		out
	}
}

const createBuildArguments = ({
	socket,
	clientRoot,
	buildImage,
	platform,
	network,
	out,
	metadata,
	sourceDateEpoch,
	builder
}) => createRuntimeArguments(clientRoot, socket, [
	'buildx',
	'build',
	'--builder',
	builder,
	'--file',
	containerfile,
	'--platform',
	platform,
	'--network',
	network === 'allow' ? 'default' : 'none',
	'--pull=false',
	'--no-cache',
	'--provenance=false',
	'--sbom=false',
	'--build-arg',
	`BUILD_IMAGE=${buildImage}`,
	'--build-arg',
	`SOURCE_DATE_EPOCH=${sourceDateEpoch}`,
	'--metadata-file',
	metadata,
	'--output',
	`type=oci,dest=${out},tar=true,rewrite-timestamp=true`,
	probeRoot
])

const runGit = arguments_ => {
	const result = spawnSync('git', arguments_, {
		cwd: repositoryRoot,
		encoding: 'utf8',
		maxBuffer: 1024 * 1024
	})
	if (result.error) throw result.error
	if (result.status !== 0) throw new Error(result.stderr.trim() || 'Git identity query failed')
	return result.stdout.trim()
}

const resolveSourceIdentity = () => {
	if (runGit(['status', '--porcelain=v1', '--untracked-files=normal']) !== '') {
		throw new Error('Isolation probe images must be built from a clean committed CodeAtlas HEAD')
	}
	const commit = runGit(['rev-parse', '--verify', 'HEAD'])
	const sourceDateEpoch = runGit(['show', '-s', '--format=%ct', 'HEAD'])
	if (!/^[0-9a-f]{40}$/.test(commit) || !/^[1-9][0-9]*$/.test(sourceDateEpoch)) {
		throw new Error('Git returned an invalid source identity')
	}
	return { commit, sourceDateEpoch }
}

const verifyPinnedImage = (options, image, clientRoot, logRoot, label) => {
	const result = runRuntime(
		options.runtime,
		createRuntimeArguments(clientRoot, options.socket, [
			'image',
			'inspect',
			'--format',
			'{{json .RepoDigests}}',
			image
		]),
		clientRoot,
		logRoot,
		label
	)
	const digests = JSON.parse(result.stdout)
	if (!Array.isArray(digests) || !digests.includes(image)) {
		throw new Error(`${label} does not expose the configured repository digest`)
	}
}

const createBuilderArguments = (builder, buildkitImage) => [
	'buildx',
	'create',
	'--name',
	builder,
	'--driver',
	'docker-container',
	'--driver-opt',
	`image=${buildkitImage}`,
	'--bootstrap'
]

const createBuilder = (options, clientRoot, logRoot, builder) => {
	const result = runRuntime(
		options.runtime,
		createRuntimeArguments(
			clientRoot,
			options.socket,
			createBuilderArguments(builder, options.buildkitImage)
		),
		clientRoot,
		logRoot,
		'builder-create'
	)
	if (result.stdout.trim() !== builder) {
		throw new Error('BuildKit builder did not return its exact planned name')
	}
}

const createBuilderRemovalArguments = builder => ['buildx', 'rm', builder]

const removeBuilder = (options, clientRoot, logRoot, builder) => {
	const result = runRuntime(
		options.runtime,
		createRuntimeArguments(
			clientRoot,
			options.socket,
			createBuilderRemovalArguments(builder)
		),
		clientRoot,
		logRoot,
		'builder-remove',
		[0, 1]
	)
	if (result.status !== 0 && !/no builder|not found/i.test(result.stderr ?? '')) {
		throw new Error(`BuildKit builder cleanup failed: ${(result.stderr ?? '').trim().slice(-4096)}`)
	}
}

const resolveRuntimeDataRoot = (checkoutRoot, output) => {
	const dataRoot = JSON.parse(output)
	if (typeof dataRoot !== 'string' || !path.isAbsolute(dataRoot)) {
		throw new Error('Container runtime did not report an absolute local data root')
	}
	return requireExternalPath(checkoutRoot, dataRoot, 'Container runtime data root')
}

const verifyRuntimeStorage = (options, clientRoot, logRoot) =>
	resolveRuntimeDataRoot(
		repositoryRoot,
		runRuntime(
			options.runtime,
			createRuntimeArguments(clientRoot, options.socket, [
				'info',
				'--format',
				'{{json .DockerRootDir}}'
			]),
			clientRoot,
			logRoot,
			'runtime-storage-inspect'
		).stdout
	)

const validateBuildArtifacts = (archive, metadata) => {
	const archiveBytes = fs.statSync(archive).size
	const metadataBytes = fs.statSync(metadata).size
	if (archiveBytes === 0 || archiveBytes > maxArchiveBytes) {
		throw new Error(`Probe OCI archive must contain 1 through ${maxArchiveBytes} bytes`)
	}
	if (metadataBytes === 0 || metadataBytes > maxMetadataBytes) {
		throw new Error(`Probe build metadata must contain 1 through ${maxMetadataBytes} bytes`)
	}
	return { archiveBytes, metadataBytes }
}

const buildProbe = options => {
	requireExternalCargoTarget(repositoryRoot)
	const out = requireExternalPath(repositoryRoot, options.out, '--out')
	const metadata = `${out}.metadata.json`
	const logRoot = `${out}.logs`
	if (fs.existsSync(out) || fs.existsSync(metadata) || fs.existsSync(logRoot)) {
		throw new Error('Probe build refuses to overwrite an existing archive, metadata, or log path')
	}
	validateRuntime(options.runtime, options.socket)
	const source = resolveSourceIdentity()
	fs.mkdirSync(path.dirname(out), { recursive: true, mode: 0o700 })
	const temporaryRoot = fs.mkdtempSync(
		path.join(path.dirname(out), '.codeatlas-probe-build-')
	)
	fs.mkdirSync(logRoot, { mode: 0o700 })
	const builder = `codeatlas-probe-${process.pid}`
	let builderAttempted = false
	let failure
	let cleanupFailure
	let summary
	try {
		const runtimeDataRoot = verifyRuntimeStorage(options, temporaryRoot, logRoot)
		verifyPinnedImage(
			options,
			options.buildImage,
			temporaryRoot,
			logRoot,
			'build-image-inspect'
		)
		verifyPinnedImage(
			options,
			options.buildkitImage,
			temporaryRoot,
			logRoot,
			'buildkit-image-inspect'
		)
		builderAttempted = true
		createBuilder(options, temporaryRoot, logRoot, builder)
		runRuntime(
			options.runtime,
			createBuildArguments({
				...options,
				out,
				metadata,
				clientRoot: temporaryRoot,
				sourceDateEpoch: source.sourceDateEpoch,
				builder
			}),
			temporaryRoot,
			logRoot,
			'oci-build'
		)
		const artifactSizes = validateBuildArtifacts(out, metadata)
		const buildMetadata = JSON.parse(fs.readFileSync(metadata, 'utf8'))
		const imageDigest = buildMetadata['containerimage.digest']
		if (!digestPattern.test(imageDigest)) {
			throw new Error('Probe image builder did not return an exact OCI manifest digest')
		}
		summary = {
			source_commit: source.commit,
			image_digest: imageDigest,
			buildkit_image: options.buildkitImage,
			platform: options.platform,
			network: options.network,
			runtime_data_root: runtimeDataRoot,
			archive_bytes: artifactSizes.archiveBytes,
			metadata_bytes: artifactSizes.metadataBytes,
			archive: out,
			metadata,
			logs: logRoot
		}
	} catch (error) {
		failure = error
	} finally {
		if (builderAttempted) {
			try {
				removeBuilder(options, temporaryRoot, logRoot, builder)
			} catch (error) {
				cleanupFailure = error
			}
		}
		try {
			fs.rmSync(temporaryRoot, { force: true, recursive: true })
		} catch (error) {
			cleanupFailure ??= error
		}
	}
	if (failure || cleanupFailure) {
		for (const candidate of [out, metadata]) {
			if (fs.existsSync(candidate)) fs.rmSync(candidate, { force: true })
		}
		if (failure && cleanupFailure) {
			throw new Error(`${failure.message}; cleanup failed: ${cleanupFailure.message}`)
		}
		throw failure ?? cleanupFailure
	}
	summary.builder_cleanup_verified = true
	return summary
}

if (require.main === module) {
	try {
		process.stdout.write(`${JSON.stringify(buildProbe(parseArguments(process.argv.slice(2))))}\n`)
	} catch (error) {
		process.stderr.write(`${error.message}\n`)
		process.exitCode = 1
	}
}

module.exports = {
	buildProbe,
	createBuildArguments,
	createBuilderArguments,
	createBuilderRemovalArguments,
	parseArguments,
	resolveRuntimeDataRoot,
	validateBuildArtifacts
}
