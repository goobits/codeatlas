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
const maxArchiveBytes = 512 * 1024 * 1024
const definitions = Object.freeze({
	http: Object.freeze({
		name: 'HTTP workload',
		slug: 'http-workload',
		containerfile: path.join(repositoryRoot, 'containers', 'http-fuzz', 'Containerfile')
	}),
	code: Object.freeze({
		name: 'Python code-fuzz workload',
		slug: 'code-fuzz-python-workload',
		containerfile: path.join(
			repositoryRoot,
			'containers',
			'code-fuzz-python',
			'Containerfile'
		)
	})
})

const requireWorkload = value => {
	if (!Object.hasOwn(definitions, value)) {
		throw new Error('--workload must be exactly http or code')
	}
	return value
}

const parseArguments = arguments_ => {
	const values = new Map()
	for (let index = 0; index < arguments_.length; index += 2) {
		const name = arguments_[index]
		const value = arguments_[index + 1]
		if (!name?.startsWith('--') || value === undefined || value.startsWith('--')) {
			throw new Error('Python workload build options must be exact --name value pairs')
		}
		if (values.has(name)) throw new Error(`Python workload build option ${name} was repeated`)
		values.set(name, value)
	}
	const known = new Set([
		'--workload',
		'--runtime',
		'--socket',
		'--python-image',
		'--buildkit-image',
		'--platform',
		'--network',
		'--out'
	])
	for (const name of values.keys()) {
		if (!known.has(name)) throw new Error(`Unknown Python workload build option ${name}`)
	}
	for (const name of known) {
		if (!values.has(name)) throw new Error(`Missing required Python workload build option ${name}`)
	}
	const network = values.get('--network')
	if (network !== 'allow') {
		throw new Error('--network must be exactly allow for hash-locked dependency installation')
	}
	const platform = values.get('--platform')
	if (!/^linux\/(amd64|arm64)$/.test(platform)) {
		throw new Error('--platform must be exactly linux/amd64 or linux/arm64')
	}
	return {
		workload: requireWorkload(values.get('--workload')),
		runtime: values.get('--runtime'),
		socket: values.get('--socket'),
		pythonImage: requireDigestImage(values.get('--python-image'), '--python-image'),
		buildkitImage: requireDigestImage(
			values.get('--buildkit-image'),
			'--buildkit-image'
		),
		platform,
		network,
		out: validateExporterPath(values.get('--out'), '--out')
	}
}

const pythonWorkloadSpecification = ({ workload, pythonImage, out, loadOut }) => {
	const definition = definitions[requireWorkload(workload)]
	return {
		...definition,
		context: repositoryRoot,
		buildArguments: { PYTHON_IMAGE: pythonImage },
		pinnedImages: [{ image: pythonImage, label: 'python-image' }],
		maxArchiveBytes,
		out,
		loadOut
	}
}

const createBuildArguments = options =>
	createContainerBuildArguments({
		...options,
		specification: pythonWorkloadSpecification(options)
	})

const validateBuildArtifacts = (workload, archive, metadata, loadArchive) => {
	const definition = definitions[requireWorkload(workload)]
	return validateContainerBuildArtifacts(
		archive,
		metadata,
		loadArchive,
		maxArchiveBytes,
		definition.name
	)
}

const buildPythonWorkload = options => {
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
		pythonWorkloadSpecification(options)
	)
}

if (require.main === module) {
	try {
		process.stdout.write(
			`${JSON.stringify(buildPythonWorkload(parseArguments(process.argv.slice(2))))}\n`
		)
	} catch (error) {
		process.stderr.write(`${error.message}\n`)
		process.exitCode = 1
	}
}

module.exports = {
	buildPythonWorkload,
	createBuildArguments,
	parseArguments,
	pythonWorkloadSpecification,
	validateBuildArtifacts
}
