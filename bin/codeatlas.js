#!/usr/bin/env node

const { ensureBinary, runBinary } = require('./_installer.js')

const main = async () => {
	try {
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
