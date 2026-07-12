#!/usr/bin/env node

const fs = require('fs')
const os = require('os')
const path = require('path')
const https = require('https')
const { spawnSync } = require('child_process')

const rootDir = path.resolve(__dirname, '..')
const pkg = require(path.join(rootDir, 'package.json'))
const binName = process.platform === 'win32' ? 'codeatlas.exe' : 'codeatlas'
const debug = process.env.CODEATLAS_DEBUG === '1'

const log = (...args) => {
	if (debug) {
		console.error('[codeatlas]', ...args)
	}
}

const getCacheBase = () => {
	if (process.env.CODEATLAS_CACHE_DIR) {
		return process.env.CODEATLAS_CACHE_DIR
	}

	if (process.platform === 'win32') {
		return process.env.LOCALAPPDATA || process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Local')
	}

	if (process.platform === 'darwin') {
		return path.join(os.homedir(), 'Library', 'Caches')
	}

	return process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache')
}

const getTarget = () => {
	let arch
	switch (process.arch) {
		case 'x64':
			arch = 'x86_64'
			break
		case 'arm64':
			arch = 'aarch64'
			break
		case 'arm':
			arch = 'armv7'
			break
		default:
			throw new Error(`Unsupported architecture: ${process.arch}`)
	}

	switch (process.platform) {
		case 'darwin':
			return `${arch}-apple-darwin`
		case 'linux':
			return `${arch}-unknown-linux-gnu`
		case 'win32':
			return `${arch}-pc-windows-msvc`
		default:
			throw new Error(`Unsupported platform: ${process.platform}`)
	}
}

const fileExists = (filePath) => {
	try {
		return fs.existsSync(filePath)
	} catch {
		return false
	}
}

const ensureExecutable = (filePath) => {
	if (process.platform === 'win32') {
		return
	}

	try {
		fs.chmodSync(filePath, 0o755)
	} catch (error) {
		log('chmod failed', error.message)
	}
}

const downloadFile = (url, destPath, redirects = 0) => new Promise((resolve, reject) => {
	log('downloading', url)
	if (redirects > 5) {
		reject(new Error('Download failed (too many redirects)'))
		return
	}

	const request = https.get(url, (response) => {
		if (response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
			const nextUrl = new URL(response.headers.location, url).toString()
			response.resume()
			resolve(downloadFile(nextUrl, destPath, redirects + 1))
			return
		}

		if (response.statusCode !== 200) {
			reject(new Error(`Download failed (${response.statusCode})`))
			response.resume()
			return
		}

		const file = fs.createWriteStream(destPath)
		response.pipe(file)
		file.on('finish', () => file.close(resolve))
	})

	request.on('error', reject)
})

const extractTar = async (archivePath, destDir) => {
	log('extracting', archivePath)
	const tar = require('tar')
	await tar.x({
		file: archivePath,
		cwd: destDir,
		strip: 1
	})
}

const buildFromSource = (binaryPath) => {
	log('building from source')
	const cargoCheck = spawnSync('cargo', ['--version'], { stdio: 'ignore' })
	if (cargoCheck.error) {
		throw new Error('Rust toolchain not found. Install Rust or provide CODEATLAS_BINARY_PATH.')
	}

	const build = spawnSync('cargo', ['build', '--release'], {
		cwd: rootDir,
		stdio: 'inherit'
	})

	if (build.status !== 0) {
		throw new Error('cargo build failed')
	}

	const builtBinary = path.join(rootDir, 'target', 'release', binName)
	if (!fileExists(builtBinary)) {
		throw new Error(`Built binary missing at ${builtBinary}`)
	}

	fs.copyFileSync(builtBinary, binaryPath)
	ensureExecutable(binaryPath)
}

const ensureBinary = async () => {
	if (process.env.CODEATLAS_BINARY_PATH) {
		const customPath = process.env.CODEATLAS_BINARY_PATH
		if (fileExists(customPath)) {
			return customPath
		}
		throw new Error(`CODEATLAS_BINARY_PATH not found: ${customPath}`)
	}

	const target = getTarget()
	const cacheDir = path.join(getCacheBase(), 'codeatlas', pkg.version, target)
	const binaryPath = path.join(cacheDir, binName)

	if (fileExists(binaryPath)) {
		return binaryPath
	}

	fs.mkdirSync(cacheDir, { recursive: true })

	const repo = process.env.CODEATLAS_REPO || 'goobits/codeatlas'
	const assetName = `codeatlas-${target}.tar.gz`
	const url = `https://github.com/${repo}/releases/download/v${pkg.version}/${assetName}`
	const archivePath = path.join(cacheDir, assetName)

	try {
		await downloadFile(url, archivePath)
		await extractTar(archivePath, cacheDir)
		if (!fileExists(binaryPath)) {
			throw new Error('Binary missing after extraction')
		}
		ensureExecutable(binaryPath)
		fs.rmSync(archivePath, { force: true })
		return binaryPath
	} catch (error) {
		log('download failed', error.message)
		buildFromSource(binaryPath)
		return binaryPath
	}
}

const run = (binaryPath) => {
	const result = spawnSync(binaryPath, process.argv.slice(2), {
		stdio: 'inherit'
	})

	if (result.error) {
		throw result.error
	}

	process.exit(result.status ?? 0)
}

;(async () => {
	try {
		const binaryPath = await ensureBinary()
		run(binaryPath)
	} catch (error) {
		console.error('codeatlas failed:', error.message)
		process.exit(1)
	}
})()
