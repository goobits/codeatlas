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
		containerfile: path.join(repositoryRoot, 'containers', 'http-fuzz', 'Containerfile'),
		images: Object.freeze(['python'])
	}),
	'code-python': Object.freeze({
		name: 'Python code-fuzz workload',
		slug: 'code-fuzz-python-workload',
		containerfile: path.join(
			repositoryRoot,
			'containers',
			'code-fuzz-python',
			'Containerfile'
		),
		images: Object.freeze(['python'])
	}),
	'code-rust': Object.freeze({
		name: 'Rust code-fuzz workload',
		slug: 'code-fuzz-rust-workload',
		containerfile: path.join(
			repositoryRoot,
			'containers',
			'code-fuzz-rust',
			'Containerfile'
		),
		images: Object.freeze(['python', 'rust'])
	})
})

const requireWorkload = value => {
	if (!Object.hasOwn(definitions, value)) {
		throw new Error('--workload must be exactly http, code-python, or code-rust')
	}
	return value
}

const parseArguments = arguments_ => {
	const values = new Map()
	for (let index = 0; index < arguments_.length; index += 2) {
		const name = arguments_[index]
		const value = arguments_[index + 1]
		if (!name?.startsWith('--') || value === undefined || value.startsWith('--')) {
			throw new Error('Workload build options must be exact --name value pairs')
		}
		if (values.has(name)) throw new Error(`Workload build option ${name} was repeated`)
		values.set(name, value)
	}
	const known = new Set([
		'--workload',
		'--runtime',
		'--socket',
		'--python-image',
		'--rust-image',
		'--buildkit-image',
		'--platform',
		'--network',
		'--out'
	])
	for (const name of values.keys()) {
		if (!known.has(name)) throw new Error(`Unknown workload build option ${name}`)
	}
	for (const name of [
		'--workload',
		'--runtime',
		'--socket',
		'--buildkit-image',
		'--platform',
		'--network',
		'--out'
	]) {
		if (!values.has(name)) throw new Error(`Missing required workload build option ${name}`)
	}
	const workload = requireWorkload(values.get('--workload'))
	const definition = definitions[workload]
	for (const image of definition.images) {
		const name = `--${image}-image`
		if (!values.has(name)) throw new Error(`Missing required workload build option ${name}`)
	}
	for (const image of ['python', 'rust']) {
		const name = `--${image}-image`
		if (!definition.images.includes(image) && values.has(name)) {
			throw new Error(`${name} is not used by workload ${workload}`)
		}
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
		workload,
		runtime: values.get('--runtime'),
		socket: values.get('--socket'),
		pythonImage: values.has('--python-image')
			? requireDigestImage(values.get('--python-image'), '--python-image')
			: undefined,
		rustImage: values.has('--rust-image')
			? requireDigestImage(values.get('--rust-image'), '--rust-image')
			: undefined,
		buildkitImage: requireDigestImage(
			values.get('--buildkit-image'),
			'--buildkit-image'
		),
		platform,
		network,
		out: validateExporterPath(values.get('--out'), '--out')
	}
}

const workloadSpecification = ({ workload, pythonImage, rustImage, out, loadOut }) => {
	const definition = definitions[requireWorkload(workload)]
	const images = {
		python: pythonImage,
		rust: rustImage
	}
	const resolvedImages = definition.images.map(name => ({
		name,
		image: requireDigestImage(images[name], `--${name}-image`)
	}))
	return {
		...definition,
		context: repositoryRoot,
		buildArguments: Object.fromEntries(
			resolvedImages.map(({ name, image }) => [`${name.toUpperCase()}_IMAGE`, image])
		),
		pinnedImages: resolvedImages.map(({ name, image }) => ({
			image,
			label: `${name}-image`
		})),
		maxArchiveBytes,
		out,
		loadOut
	}
}

const createBuildArguments = options =>
	createContainerBuildArguments({
		...options,
		specification: workloadSpecification(options)
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

const buildWorkload = options => {
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
		workloadSpecification(options)
	)
}

if (require.main === module) {
	try {
		process.stdout.write(
			`${JSON.stringify(buildWorkload(parseArguments(process.argv.slice(2))))}\n`
		)
	} catch (error) {
		process.stderr.write(`${error.message}\n`)
		process.exitCode = 1
	}
}

module.exports = {
	buildWorkload,
	createBuildArguments,
	parseArguments,
	workloadSpecification,
	validateBuildArtifacts
}
