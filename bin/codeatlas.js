#!/usr/bin/env node

const { ensureBinary, runBinary } = require('./_installer.js')

const configureBundledTools = () => {
	if (process.env.CODEATLAS_SQUAWK_PATH) return
	try {
		process.env.CODEATLAS_SQUAWK_PATH = require.resolve('squawk-cli/js/bin/squawk')
	} catch (error) {
		if (error.code !== 'MODULE_NOT_FOUND') throw error
	}
}

const main = async () => {
	try {
		configureBundledTools()
		const binaryPath = await ensureBinary()
		process.exitCode = runBinary(binaryPath)
	} catch (error) {
		console.error('codeatlas failed:', error.message)
		process.exitCode = 1
	}
}

if (require.main === module) {
	void main()
}
