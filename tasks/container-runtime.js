const fs = require('node:fs')
const path = require('node:path')
const { spawnSync } = require('node:child_process')
const { writePrivateFile } = require('./storage.js')

const digestPattern = /^sha256:[0-9a-f]{64}$/
const imagePattern = /^[a-z0-9][a-z0-9._:/-]*@sha256:[0-9a-f]{64}$/
const maxRuntimeOutputBytes = 16 * 1024 * 1024
const maxRuntimeElapsedMs = 30 * 60 * 1000
const logLabelPattern = /^[a-z0-9][a-z0-9-]*$/

const createRuntimeOptions = clientRoot => ({
	cwd: clientRoot,
	env: {
		DOCKER_CONFIG: clientRoot,
		HOME: clientRoot,
		PATH: '/usr/bin:/bin'
	},
	encoding: 'utf8',
	stdio: ['ignore', 'pipe', 'pipe'],
	maxBuffer: maxRuntimeOutputBytes,
	timeout: maxRuntimeElapsedMs,
	killSignal: 'SIGKILL'
})

const createRuntimeArguments = (clientRoot, socket, arguments_) => [
	'--config',
	clientRoot,
	'--host',
	`unix://${socket}`,
	...arguments_
]

const requireDigestImage = (value, label) => {
	if (typeof value !== 'string' || !imagePattern.test(value)) {
		throw new Error(`${label} must be an exact repository@sha256 reference`)
	}
	return value
}

const requireRuntimeLogLabel = label => {
	if (typeof label !== 'string' || !logLabelPattern.test(label)) {
		throw new Error('Runtime log label is invalid')
	}
	return label
}

const validateRuntime = (runtime, socket) => {
	if (!path.isAbsolute(runtime) || !fs.statSync(runtime).isFile()) {
		throw new Error('Container runtime must identify an absolute executable file')
	}
	fs.accessSync(runtime, fs.constants.X_OK)
	if (!path.isAbsolute(socket) || !fs.statSync(socket).isSocket()) {
		throw new Error('Container socket must identify an absolute local Unix socket')
	}
}

const runRuntime = (
	runtime,
	arguments_,
	clientRoot,
	logRoot,
	label,
	allowedStatuses = [0]
) => {
	requireRuntimeLogLabel(label)
	const result = spawnSync(runtime, arguments_, createRuntimeOptions(clientRoot))
	writePrivateFile(path.join(logRoot, `${label}.stdout.log`), result.stdout ?? '')
	writePrivateFile(path.join(logRoot, `${label}.stderr.log`), result.stderr ?? '')
	if (result.error) throw result.error
	if (result.signal) throw new Error(`${label} terminated by ${result.signal}`)
	if (!allowedStatuses.includes(result.status)) {
		throw new Error(`${label} failed: ${(result.stderr ?? '').trim().slice(-4096)}`)
	}
	return result
}

module.exports = {
	createRuntimeArguments,
	createRuntimeOptions,
	digestPattern,
	requireDigestImage,
	requireRuntimeLogLabel,
	runRuntime,
	validateRuntime
}
