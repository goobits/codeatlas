#!/usr/bin/env node

const crypto = require('node:crypto')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const { spawnSync } = require('node:child_process')
const { buildProbe } = require('./build-isolation-probe.js')
const {
	createRuntimeArguments,
	digestPattern,
	requireDigestImage,
	runRuntime,
	validateRuntime
} = require('./container-runtime.js')
const {
	requireExternalCargoTarget,
	requireExternalPath,
	writePrivateFile
} = require('./storage.js')

const repositoryRoot = path.resolve(__dirname, '..')
const ownerLabel = 'dev.codeatlas.owner=execution-kernel'
const registryOwnerLabel = 'dev.codeatlas.owner=live-conformance'
const maxCommandOutputBytes = 16 * 1024 * 1024
const maxCommandElapsedMs = 30 * 60 * 1000
const registryReadyAttempts = 40
const registryReadyDelayMs = 250
const receiptFilename = 'execution-receipt.json'

const parseArguments = arguments_ => {
	const values = new Map()
	for (let index = 0; index < arguments_.length; index += 2) {
		const name = arguments_[index]
		const value = arguments_[index + 1]
		if (!name?.startsWith('--') || value === undefined || value.startsWith('--')) {
			throw new Error('Live isolation options must be exact --name value pairs')
		}
		if (values.has(name)) throw new Error(`Live isolation option ${name} was repeated`)
		values.set(name, value)
	}
	const known = new Set([
		'--runtime',
		'--socket',
		'--build-image',
		'--registry-image',
		'--platform',
		'--network',
		'--out-dir'
	])
	for (const name of values.keys()) {
		if (!known.has(name)) throw new Error(`Unknown live isolation option ${name}`)
	}
	for (const name of known) {
		if (!values.has(name)) throw new Error(`Missing required live isolation option ${name}`)
	}
	const platform = values.get('--platform')
	if (!/^linux\/(amd64|arm64)$/.test(platform)) {
		throw new Error('--platform must be exactly linux/amd64 or linux/arm64')
	}
	const network = values.get('--network')
	if (network !== 'allow' && network !== 'deny') {
		throw new Error('--network must be exactly allow or deny')
	}
	return {
		runtime: values.get('--runtime'),
		socket: values.get('--socket'),
		buildImage: requireDigestImage(values.get('--build-image'), '--build-image'),
		registryImage: requireDigestImage(values.get('--registry-image'), '--registry-image'),
		platform,
		network,
		outDir: values.get('--out-dir')
	}
}

const createRegistryArguments = (name, image) => [
	'container',
	'run',
	'--detach',
	'--name',
	name,
	'--label',
	registryOwnerLabel,
	'--publish',
	'127.0.0.1::5000',
	'--read-only',
	'--tmpfs',
	'/var/lib/registry:rw,noexec,nosuid,nodev,size=268435456',
	'--cap-drop',
	'ALL',
	'--security-opt',
	'no-new-privileges=true',
	'--memory',
	'268435456',
	'--memory-swap',
	'268435456',
	'--pids-limit',
	'64',
	'--cpus',
	'1',
	'--log-driver',
	'none',
	image
]

const parseLoadedImage = output => {
	const matches = output
		.split(/\r?\n/)
		.map(line => line.match(/^Loaded image ID: (sha256:[0-9a-f]{64})$/)?.[1])
		.filter(Boolean)
	if (matches.length !== 1) throw new Error('OCI import did not return one exact image ID')
	return matches[0]
}

const verifyLoadedImage = (output, expectedImageId) => {
	const imageId = JSON.parse(output)
	if (!digestPattern.test(imageId) || imageId !== expectedImageId) {
		throw new Error('Imported probe image inspection differs from the loaded image ID')
	}
	return imageId
}

const parseRegistryAddress = output => {
	const match = output.trim().match(/^127\.0\.0\.1:([1-9][0-9]{0,4})$/)
	if (!match) throw new Error('Local registry did not expose one loopback IPv4 port')
	const port = Number.parseInt(match[1], 10)
	if (port > 65_535) throw new Error('Local registry returned an invalid TCP port')
	return `127.0.0.1:${port}`
}

const selectPublishedReference = (output, repository, expectedDigest) => {
	const digests = JSON.parse(output)
	if (!Array.isArray(digests)) throw new Error('Published probe image omitted repository digests')
	const expected = `${repository}@${expectedDigest}`
	if (!digests.includes(expected)) {
		throw new Error('Published probe digest differs from the built OCI manifest')
	}
	return expected
}

const waitForRegistry = async (
	address,
	fetch_ = globalThis.fetch,
	delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds))
) => {
	for (let attempt = 0; attempt < registryReadyAttempts; attempt += 1) {
		try {
			const response = await fetch_(`http://${address}/v2/`, {
				signal: AbortSignal.timeout(1_000)
			})
			if (response.status === 200) return
		} catch {}
		if (attempt + 1 < registryReadyAttempts) await delay(registryReadyDelayMs)
	}
	throw new Error('Local registry did not become ready within 10 seconds')
}

const digestFile = filename => {
	const digest = crypto.createHash('sha256')
	const descriptor = fs.openSync(filename, 'r')
	const buffer = Buffer.allocUnsafe(1024 * 1024)
	try {
		for (;;) {
			const bytes = fs.readSync(descriptor, buffer, 0, buffer.length, null)
			if (bytes === 0) break
			digest.update(buffer.subarray(0, bytes))
		}
	} finally {
		fs.closeSync(descriptor)
	}
	return `sha256:${digest.digest('hex')}`
}

const runCommand = (command, arguments_, environment, logRoot, label) => {
	const result = spawnSync(command, arguments_, {
		cwd: repositoryRoot,
		env: environment,
		encoding: 'utf8',
		stdio: ['ignore', 'pipe', 'pipe'],
		maxBuffer: maxCommandOutputBytes,
		timeout: maxCommandElapsedMs,
		killSignal: 'SIGKILL'
	})
	writePrivateFile(path.join(logRoot, `${label}.stdout.log`), result.stdout ?? '')
	writePrivateFile(path.join(logRoot, `${label}.stderr.log`), result.stderr ?? '')
	if (result.error) throw result.error
	if (result.signal) throw new Error(`${label} terminated by ${result.signal}`)
	if (result.status !== 0) {
		throw new Error(`${label} failed: ${(result.stderr ?? '').trim().slice(-4096)}`)
	}
}

const listOwnedContainers = (options, clientRoot, logRoot, owner, label) => {
	const result = runRuntime(
		options.runtime,
		createRuntimeArguments(clientRoot, options.socket, [
			'container',
			'ls',
			'--all',
			'--quiet',
			'--filter',
			`label=${owner}`
		]),
		clientRoot,
		logRoot,
		label
	)
	return result.stdout.trim().split(/\s+/).filter(Boolean)
}

const inspectRuntimeEvidence = (options, clientRoot, logRoot) => {
	const version = runRuntime(
		options.runtime,
		createRuntimeArguments(clientRoot, options.socket, [
			'version',
			'--format',
			'{{json .}}'
		]),
		clientRoot,
		logRoot,
		'runtime-version'
	).stdout
	const info = runRuntime(
		options.runtime,
		createRuntimeArguments(clientRoot, options.socket, [
			'info',
			'--format',
			'{{json .}}'
		]),
		clientRoot,
		logRoot,
		'runtime-info'
	).stdout
	return {
		version_digest: `sha256:${crypto.createHash('sha256').update(version).digest('hex')}`,
		info_digest: `sha256:${crypto.createHash('sha256').update(info).digest('hex')}`
	}
}

const removeRuntimeObject = (
	options,
	clientRoot,
	logRoot,
	label,
	arguments_,
	missingPattern
) => {
	try {
		const result = runRuntime(
			options.runtime,
			createRuntimeArguments(clientRoot, options.socket, arguments_),
			clientRoot,
			logRoot,
			label,
			[0, 1]
		)
		if (result.status !== 0 && !missingPattern?.test(result.stderr ?? '')) {
			return new Error(`${label} failed: ${(result.stderr ?? '').trim().slice(-4096)}`)
		}
	} catch (error) {
		return error
	}
	return null
}

const checkIsolationLive = async options => {
	requireExternalCargoTarget(repositoryRoot)
	validateRuntime(options.runtime, options.socket)
	const outDir = requireExternalPath(repositoryRoot, options.outDir, '--out-dir')
	if (fs.existsSync(outDir)) throw new Error('Live isolation output directory must not exist')
	fs.mkdirSync(outDir, { recursive: true, mode: 0o700 })
	const logRoot = path.join(outDir, 'logs')
	const clientRoot = path.join(outDir, 'runtime-client')
	const temporaryRoot = path.join(outDir, 'tmp')
	for (const directory of [logRoot, clientRoot, temporaryRoot]) {
		fs.mkdirSync(directory, { mode: 0o700 })
	}
	const registryName = `codeatlas-live-registry-${process.pid}`
	const archive = path.join(outDir, 'isolation-probe.oci.tar')
	const receipt = path.join(outDir, receiptFilename)
	let registryCreated = false
	let loadedImage
	let localTag
	let publishedReference
	let failure
	let summary
	try {
		for (const [owner, label] of [
			[ownerLabel, 'execution-containers-before'],
			[registryOwnerLabel, 'registry-containers-before']
		]) {
			const existing = listOwnedContainers(options, clientRoot, logRoot, owner, label)
			if (existing.length > 0) {
				throw new Error(`Live isolation refuses pre-existing ${owner} containers`)
			}
		}
		for (const [label, image] of [
			['build-image-pull', options.buildImage],
			['registry-image-pull', options.registryImage]
		]) {
			runRuntime(
				options.runtime,
				createRuntimeArguments(clientRoot, options.socket, ['image', 'pull', image]),
				clientRoot,
				logRoot,
				label
			)
		}
		const build = buildProbe({
			runtime: options.runtime,
			socket: options.socket,
			buildImage: options.buildImage,
			platform: options.platform,
			network: options.network,
			out: archive
		})
		const registry = runRuntime(
			options.runtime,
			createRuntimeArguments(
				clientRoot,
				options.socket,
				createRegistryArguments(registryName, options.registryImage)
			),
			clientRoot,
			logRoot,
			'registry-start'
		)
		registryCreated = true
		if (!/^[0-9a-f]{64}\s*$/.test(registry.stdout)) {
			throw new Error('Local registry did not return one exact container ID')
		}
		const address = parseRegistryAddress(
			runRuntime(
				options.runtime,
				createRuntimeArguments(clientRoot, options.socket, [
					'container',
					'port',
					registryName,
					'5000/tcp'
				]),
				clientRoot,
				logRoot,
				'registry-port'
			).stdout
		)
		await waitForRegistry(address)
		loadedImage = parseLoadedImage(
			runRuntime(
				options.runtime,
				createRuntimeArguments(clientRoot, options.socket, [
					'image',
					'load',
					'--input',
					archive
				]),
				clientRoot,
				logRoot,
				'probe-image-load'
			).stdout
		)
		verifyLoadedImage(
			runRuntime(
				options.runtime,
				createRuntimeArguments(clientRoot, options.socket, [
					'image',
					'inspect',
					'--format',
					'{{json .Id}}',
					loadedImage
				]),
				clientRoot,
				logRoot,
				'probe-image-id-inspect'
			).stdout,
			loadedImage
		)
		const repository = `${address}/codeatlas-isolation-probe`
		localTag = `${repository}:${build.source_commit.slice(0, 12)}`
		runRuntime(
			options.runtime,
			createRuntimeArguments(clientRoot, options.socket, [
				'image',
				'tag',
				loadedImage,
				localTag
			]),
			clientRoot,
			logRoot,
			'probe-image-tag'
		)
		runRuntime(
			options.runtime,
			createRuntimeArguments(clientRoot, options.socket, ['image', 'push', localTag]),
			clientRoot,
			logRoot,
			'probe-image-push'
		)
		publishedReference = selectPublishedReference(
			runRuntime(
				options.runtime,
				createRuntimeArguments(clientRoot, options.socket, [
					'image',
					'inspect',
					'--format',
					'{{json .RepoDigests}}',
					localTag
				]),
				clientRoot,
				logRoot,
				'probe-image-inspect'
			).stdout,
			repository,
			build.image_digest
		)
		const testEnvironment = {
			...process.env,
			CODEATLAS_TEST_OCI_RUNTIME: options.runtime,
			CODEATLAS_TEST_OCI_SOCKET: options.socket,
			CODEATLAS_TEST_OCI_PROBE_IMAGE: publishedReference,
			CODEATLAS_TEST_OCI_RECEIPT_OUT: receipt,
			TMPDIR: temporaryRoot
		}
		runCommand(
			'cargo',
			[
				'test',
				'--locked',
				'--jobs',
				'1',
				'--test',
				'execution_isolation',
				'live_oci_backend_passes_target_observed_conformance',
				'--',
				'--ignored',
				'--exact',
				'--nocapture'
			],
			testEnvironment,
			logRoot,
			'live-baseline'
		)
		runCommand(
			'cargo',
			[
				'test',
				'--locked',
				'--jobs',
				'1',
				'--bin',
				'codeatlas',
				'execution::sandbox::container::live_tests::live_oci_destructive_matrix',
				'--',
				'--ignored',
				'--exact',
				'--nocapture'
			],
			testEnvironment,
			logRoot,
			'live-destructive-matrix'
		)
		if (!fs.statSync(receipt).isFile()) {
			throw new Error('Live baseline did not persist its canonical execution receipt')
		}
		const receiptValue = JSON.parse(fs.readFileSync(receipt, 'utf8'))
		const runtimeEvidence = inspectRuntimeEvidence(options, clientRoot, logRoot)
		const cgroup = fs.existsSync('/proc/self/cgroup')
			? fs.readFileSync('/proc/self/cgroup')
			: Buffer.from('unavailable')
		summary = {
			source_commit: build.source_commit,
			platform: build.platform,
			kernel_release: os.release(),
			cgroup_digest: `sha256:${crypto.createHash('sha256').update(cgroup).digest('hex')}`,
			...runtimeEvidence,
			probe_image: publishedReference,
			probe_image_digest: build.image_digest,
			probe_archive_digest: digestFile(archive),
			probe_metadata_digest: digestFile(build.metadata),
			receipt_id: receiptValue.id,
			receipt_digest: digestFile(receipt),
			capabilities: receiptValue.runtime?.capabilities ?? [],
			rootless: receiptValue.runtime?.rootless,
			nested: receiptValue.runtime?.nested,
			cleanup_verified: receiptValue.cleanup?.every(
				entry => entry.released === true && entry.verified === true
			)
		}
		writePrivateFile(
			path.join(outDir, 'live-oci-evidence.json'),
			`${JSON.stringify(summary, null, 2)}\n`
		)
	} catch (error) {
		failure = error
	}

	const cleanupErrors = []
	if (registryCreated) {
		for (const [label, arguments_] of [
			['registry-stop', ['container', 'stop', '--time', '5', registryName]],
			['registry-remove', ['container', 'rm', registryName]]
		]) {
			const error = removeRuntimeObject(
				options,
				clientRoot,
				logRoot,
				label,
				arguments_,
				/No such container/
			)
			if (error) cleanupErrors.push(error)
		}
	}
	for (const [label, image] of [
		['probe-digest-remove', publishedReference],
		['probe-tag-remove', localTag],
		['probe-image-remove', loadedImage]
	]) {
		if (!image) continue
		const error = removeRuntimeObject(
			options,
			clientRoot,
			logRoot,
			label,
			['image', 'rm', image],
			/No such image/
		)
		if (error) cleanupErrors.push(error)
	}
	for (const [owner, label] of [
		[ownerLabel, 'execution-containers-after'],
		[registryOwnerLabel, 'registry-containers-after']
	]) {
		try {
			const leaked = listOwnedContainers(options, clientRoot, logRoot, owner, label)
			if (leaked.length > 0) {
				cleanupErrors.push(new Error(`${owner} containers remain after live tests: ${leaked.join(',')}`))
			}
		} catch (error) {
			cleanupErrors.push(error)
		}
	}
	if (cleanupErrors.length > 0) {
		const cleanup = cleanupErrors.map(error => error.message).join('; ')
		throw new Error(failure ? `${failure.message}; cleanup failed: ${cleanup}` : cleanup)
	}
	if (failure) throw failure
	return summary
}

if (require.main === module) {
	checkIsolationLive(parseArguments(process.argv.slice(2)))
		.then(summary => process.stdout.write(`${JSON.stringify(summary)}\n`))
		.catch(error => {
			process.stderr.write(`${error.message}\n`)
			process.exitCode = 1
		})
}

module.exports = {
	checkIsolationLive,
	createRegistryArguments,
	parseArguments,
	parseLoadedImage,
	parseRegistryAddress,
	selectPublishedReference,
	verifyLoadedImage,
	waitForRegistry
}
