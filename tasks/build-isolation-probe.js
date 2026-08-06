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
	const known = new Set(['--runtime', '--socket', '--build-image', '--platform', '--network', '--out'])
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
	const out = values.get('--out')
	if (/[,\n\r\0]/.test(out)) {
		throw new Error('--out cannot contain OCI exporter separators or control characters')
	}
	return {
		runtime: values.get('--runtime'),
		socket: values.get('--socket'),
		buildImage,
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
	sourceDateEpoch
}) => createRuntimeArguments(clientRoot, socket, [
	'buildx',
	'build',
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

const verifyPinnedBuildImage = (options, clientRoot, logRoot) => {
	const result = runRuntime(
		options.runtime,
		createRuntimeArguments(clientRoot, options.socket, [
			'image',
			'inspect',
			'--format',
			'{{json .RepoDigests}}',
			options.buildImage
		]),
		clientRoot,
		logRoot,
		'build-image-inspect'
	)
	const digests = JSON.parse(result.stdout)
	if (!Array.isArray(digests) || !digests.includes(options.buildImage)) {
		throw new Error('Local build image does not expose the configured repository digest')
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
	try {
		const runtimeDataRoot = verifyRuntimeStorage(options, temporaryRoot, logRoot)
		verifyPinnedBuildImage(options, temporaryRoot, logRoot)
		runRuntime(
			options.runtime,
			createBuildArguments({
				...options,
				out,
				metadata,
				clientRoot: temporaryRoot,
				sourceDateEpoch: source.sourceDateEpoch
			}),
			temporaryRoot,
			logRoot,
			'oci-build'
		)
		const buildMetadata = JSON.parse(fs.readFileSync(metadata, 'utf8'))
		const imageDigest = buildMetadata['containerimage.digest']
		if (!digestPattern.test(imageDigest)) {
			throw new Error('Probe image builder did not return an exact OCI manifest digest')
		}
		return {
			source_commit: source.commit,
			image_digest: imageDigest,
			platform: options.platform,
			network: options.network,
			runtime_data_root: runtimeDataRoot,
			archive: out,
			metadata,
			logs: logRoot
		}
	} catch (error) {
		for (const candidate of [out, metadata]) {
			if (fs.existsSync(candidate)) fs.rmSync(candidate, { force: true })
		}
		throw error
	} finally {
		fs.rmSync(temporaryRoot, { force: true, recursive: true })
	}
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
	parseArguments,
	resolveRuntimeDataRoot
}
