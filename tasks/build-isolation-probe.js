#!/usr/bin/env node

const path = require('node:path')
const {
	buildContainerImage,
	createBuildArguments: createContainerBuildArguments,
	validateBuildArtifacts: validateContainerBuildArtifacts,
	validateExporterPath
} = require('./build-container-image.js')
const { requireDigestImage } = require('./container-runtime.js')
const { requireExternalCargoTarget } = require('./storage.js')

const repositoryRoot = path.resolve(__dirname, '..')
const probeRoot = path.join(repositoryRoot, 'crates', 'isolation-conformance')
const containerfile = path.join(
	repositoryRoot,
	'containers',
	'isolation-conformance',
	'Containerfile'
)
const maxArchiveBytes = 64 * 1024 * 1024

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
	return {
		runtime: values.get('--runtime'),
		socket: values.get('--socket'),
		buildImage: requireDigestImage(values.get('--build-image'), '--build-image'),
		buildkitImage: requireDigestImage(
			values.get('--buildkit-image'),
			'--buildkit-image'
		),
		platform,
		network,
		out: validateExporterPath(values.get('--out'), '--out')
	}
}

const probeSpecification = ({ buildImage, out, loadOut }) => ({
	name: 'Probe',
	slug: 'isolation-probe',
	containerfile,
	context: probeRoot,
	buildArguments: { BUILD_IMAGE: buildImage },
	pinnedImages: [{ image: buildImage, label: 'build-image' }],
	maxArchiveBytes,
	out,
	loadOut
})

const createBuildArguments = options =>
	createContainerBuildArguments({
		...options,
		specification: probeSpecification({
			buildImage: options.buildImage,
			out: options.out,
			loadOut: options.loadOut
		})
	})

const validateBuildArtifacts = (archive, metadata, loadArchive) =>
	validateContainerBuildArtifacts(
		archive,
		metadata,
		loadArchive,
		maxArchiveBytes,
		'Probe'
	)

const buildProbe = options => {
	requireExternalCargoTarget(repositoryRoot)
	return buildContainerImage(
		{
			runtime: options.runtime,
			socket: options.socket,
			buildkitImage: options.buildkitImage,
			platform: options.platform,
			network: options.network,
			logRoot: options.logRoot
		},
		probeSpecification(options)
	)
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
	probeSpecification,
	validateBuildArtifacts
}
