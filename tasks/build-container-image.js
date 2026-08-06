const fs = require('node:fs')
const path = require('node:path')
const { spawnSync } = require('node:child_process')
const { requireExternalPath } = require('./storage.js')
const {
	createRuntimeArguments,
	digestPattern,
	requireDigestImage,
	runRuntime,
	validateRuntime
} = require('./container-runtime.js')

const repositoryRoot = path.resolve(__dirname, '..')
const maxMetadataBytes = 1024 * 1024

const validateExporterPath = (value, label) => {
	if (typeof value !== 'string' || /[,\n\r\0]/.test(value)) {
		throw new Error(`${label} cannot contain exporter separators or control characters`)
	}
	return value
}

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
		throw new Error('Container images must be built from a clean committed CodeAtlas HEAD')
	}
	const commit = runGit(['rev-parse', '--verify', 'HEAD'])
	const sourceDateEpoch = runGit(['show', '-s', '--format=%ct', 'HEAD'])
	if (!/^[0-9a-f]{40}$/.test(commit) || !/^[1-9][0-9]*$/.test(sourceDateEpoch)) {
		throw new Error('Git returned an invalid source identity')
	}
	return { commit, sourceDateEpoch }
}

const requireRepositorySource = (candidate, label, type) => {
	const checkout = fs.realpathSync.native(repositoryRoot)
	const resolved = fs.realpathSync.native(candidate)
	const relative = path.relative(checkout, resolved)
	if (relative.startsWith('..') || path.isAbsolute(relative)) {
		throw new Error(`${label} must resolve inside the CodeAtlas checkout`)
	}
	const stats = fs.statSync(resolved)
	if ((type === 'file' && !stats.isFile()) || (type === 'directory' && !stats.isDirectory())) {
		throw new Error(`${label} is not a ${type}`)
	}
	return resolved
}

const validateSpecification = specification => {
	if (
		typeof specification?.name !== 'string' ||
		!specification.name ||
		typeof specification.slug !== 'string' ||
		!/^[a-z][a-z0-9-]*$/.test(specification.slug)
	) {
		throw new Error('Container image specification has no valid name and slug')
	}
	if (
		!Number.isSafeInteger(specification.maxArchiveBytes) ||
		specification.maxArchiveBytes <= 0
	) {
		throw new Error(`${specification.name} archive ceiling is invalid`)
	}
	const buildArguments = specification.buildArguments ?? {}
	if (
		Object.entries(buildArguments).some(
			([name, value]) =>
				!/^[_A-Z][_A-Z0-9]*$/.test(name) ||
				name === 'SOURCE_DATE_EPOCH' ||
				typeof value !== 'string' ||
				/[\n\r\0]/.test(value)
		)
	) {
		throw new Error(`${specification.name} build arguments are invalid`)
	}
	if (!Array.isArray(specification.pinnedImages) || specification.pinnedImages.length === 0) {
		throw new Error(`${specification.name} has no pinned input image`)
	}
	const pinnedImages = specification.pinnedImages.map(({ image, label }) => ({
		image: requireDigestImage(image, `${specification.name} ${label}`),
		label: `${specification.slug}-${label}`
	}))
	return {
		...specification,
		containerfile: requireRepositorySource(
			specification.containerfile,
			`${specification.name} Containerfile`,
			'file'
		),
		context: requireRepositorySource(
			specification.context,
			`${specification.name} build context`,
			'directory'
		),
		buildArguments,
		pinnedImages
	}
}

const createBuildArguments = ({
	socket,
	clientRoot,
	buildkitImage,
	platform,
	network,
	out,
	metadata,
	loadOut,
	sourceDateEpoch,
	builder,
	specification
}) => createRuntimeArguments(clientRoot, socket, [
	'buildx',
	'build',
	'--builder',
	builder,
	'--file',
	specification.containerfile,
	'--platform',
	platform,
	'--network',
	network === 'allow' ? 'default' : 'none',
	'--pull=false',
	'--no-cache',
	'--provenance=false',
	'--sbom=false',
	...Object.entries(specification.buildArguments)
		.sort(([left], [right]) => left.localeCompare(right))
		.flatMap(([name, value]) => ['--build-arg', `${name}=${value}`]),
	'--build-arg',
	`SOURCE_DATE_EPOCH=${sourceDateEpoch}`,
	'--metadata-file',
	metadata,
	'--output',
	`type=oci,dest=${out},tar=true,rewrite-timestamp=true`,
	...(loadOut === undefined ? [] : ['--output', `type=docker,dest=${loadOut}`]),
	specification.context
])

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

const validateBuildArtifacts = (
	archive,
	metadata,
	loadArchive,
	maxArchiveBytes,
	label = 'Container image'
) => {
	const archiveBytes = fs.statSync(archive).size
	const metadataBytes = fs.statSync(metadata).size
	if (archiveBytes === 0 || archiveBytes > maxArchiveBytes) {
		throw new Error(`${label} OCI archive must contain 1 through ${maxArchiveBytes} bytes`)
	}
	if (metadataBytes === 0 || metadataBytes > maxMetadataBytes) {
		throw new Error(`${label} build metadata must contain 1 through ${maxMetadataBytes} bytes`)
	}
	const sizes = { archiveBytes, metadataBytes }
	if (loadArchive !== undefined) {
		const loadArchiveBytes = fs.statSync(loadArchive).size
		if (loadArchiveBytes === 0 || loadArchiveBytes > maxArchiveBytes) {
			throw new Error(
				`${label} Docker import archive must contain 1 through ${maxArchiveBytes} bytes`
			)
		}
		sizes.loadArchiveBytes = loadArchiveBytes
	}
	return sizes
}

const buildContainerImages = (options, inputSpecifications) => {
	if (!Array.isArray(inputSpecifications) || inputSpecifications.length === 0) {
		throw new Error('Container image build requires at least one specification')
	}
	if (!/^linux\/(amd64|arm64)$/.test(options.platform)) {
		throw new Error('Container image platform must be exactly linux/amd64 or linux/arm64')
	}
	if (options.network !== 'allow' && options.network !== 'deny') {
		throw new Error('Container image network must be exactly allow or deny')
	}
	const buildkitImage = requireDigestImage(options.buildkitImage, 'BuildKit image')
	validateRuntime(options.runtime, options.socket)
	const source = resolveSourceIdentity()
	const specifications = inputSpecifications.map(validateSpecification).map(specification => {
		const out = requireExternalPath(
			repositoryRoot,
			validateExporterPath(specification.out, `${specification.name} output`),
			`${specification.name} output`
		)
		const loadOut = specification.loadOut === undefined
			? undefined
			: requireExternalPath(
				repositoryRoot,
				validateExporterPath(
					specification.loadOut,
					`${specification.name} Docker import output`
				),
				`${specification.name} Docker import output`
			)
		if (loadOut === out) {
			throw new Error(`${specification.name} canonical OCI and Docker outputs must be distinct`)
		}
		return { ...specification, out, loadOut, metadata: `${out}.metadata.json` }
	})
	const artifactPaths = specifications.flatMap(specification =>
		[specification.out, specification.loadOut, specification.metadata].filter(Boolean)
	)
	if (new Set(artifactPaths).size !== artifactPaths.length) {
		throw new Error('Container image output paths must be unique')
	}
	const logRoot = requireExternalPath(
		repositoryRoot,
		options.logRoot ?? `${specifications[0].out}.logs`,
		'Container image build logs'
	)
	if ([...artifactPaths, logRoot].some(candidate => fs.existsSync(candidate))) {
		throw new Error('Container image build refuses to overwrite an archive, metadata, or log path')
	}
	for (const specification of specifications) {
		fs.mkdirSync(path.dirname(specification.out), { recursive: true, mode: 0o700 })
	}
	const temporaryRoot = fs.mkdtempSync(
		path.join(path.dirname(specifications[0].out), '.codeatlas-image-build-')
	)
	fs.mkdirSync(logRoot, { recursive: true, mode: 0o700 })
	const builder = `codeatlas-images-${process.pid}`
	let builderAttempted = false
	let failure
	let cleanupFailure
	let summaries
	try {
		const runtimeDataRoot = verifyRuntimeStorage(options, temporaryRoot, logRoot)
		const pinnedImages = new Map([[buildkitImage, 'buildkit-image-inspect']])
		for (const specification of specifications) {
			for (const input of specification.pinnedImages) {
				if (!pinnedImages.has(input.image)) pinnedImages.set(input.image, `${input.label}-inspect`)
			}
		}
		for (const [image, label] of pinnedImages) {
			verifyPinnedImage(options, image, temporaryRoot, logRoot, label)
		}
		builderAttempted = true
		createBuilder({ ...options, buildkitImage }, temporaryRoot, logRoot, builder)
		summaries = specifications.map(specification => {
			runRuntime(
				options.runtime,
				createBuildArguments({
					...options,
					buildkitImage,
					out: specification.out,
					metadata: specification.metadata,
					loadOut: specification.loadOut,
					clientRoot: temporaryRoot,
					sourceDateEpoch: source.sourceDateEpoch,
					builder,
					specification
				}),
				temporaryRoot,
				logRoot,
				`${specification.slug}-oci-build`
			)
			const artifactSizes = validateBuildArtifacts(
				specification.out,
				specification.metadata,
				specification.loadOut,
				specification.maxArchiveBytes,
				specification.name
			)
			const buildMetadata = JSON.parse(fs.readFileSync(specification.metadata, 'utf8'))
			const imageDigest = buildMetadata['containerimage.digest']
			if (!digestPattern.test(imageDigest)) {
				throw new Error(`${specification.name} builder did not return an exact manifest digest`)
			}
			return {
				name: specification.name,
				slug: specification.slug,
				source_commit: source.commit,
				image_digest: imageDigest,
				buildkit_image: buildkitImage,
				platform: options.platform,
				network: options.network,
				runtime_data_root: runtimeDataRoot,
				archive_bytes: artifactSizes.archiveBytes,
				load_archive: specification.loadOut,
				load_archive_bytes: artifactSizes.loadArchiveBytes,
				metadata_bytes: artifactSizes.metadataBytes,
				archive: specification.out,
				metadata: specification.metadata,
				logs: logRoot
			}
		})
	} catch (error) {
		failure = error
	} finally {
		if (builderAttempted) {
			try {
				removeBuilder({ ...options, buildkitImage }, temporaryRoot, logRoot, builder)
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
		for (const candidate of artifactPaths) {
			if (fs.existsSync(candidate)) fs.rmSync(candidate, { force: true })
		}
		if (failure && cleanupFailure) {
			throw new Error(`${failure.message}; cleanup failed: ${cleanupFailure.message}`)
		}
		throw failure ?? cleanupFailure
	}
	for (const summary of summaries) summary.builder_cleanup_verified = true
	return summaries
}

const buildContainerImage = (options, specification) =>
	buildContainerImages(options, [specification])[0]

module.exports = {
	buildContainerImage,
	buildContainerImages,
	createBuildArguments,
	createBuilderArguments,
	createBuilderRemovalArguments,
	resolveRuntimeDataRoot,
	validateBuildArtifacts,
	validateExporterPath
}
