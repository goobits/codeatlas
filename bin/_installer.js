const crypto = require('crypto')
const fs = require('fs')
const https = require('https')
const os = require('os')
const path = require('path')
const { Transform } = require('stream')
const { pipeline } = require('stream/promises')
const { spawnSync } = require('child_process')

const rootDir = path.resolve(__dirname, '..')
const pkg = require(path.join(rootDir, 'package.json'))

const MAX_ARCHIVE_BYTES = 128 * 1024 * 1024
const MAX_CHECKSUM_BYTES = 1024 * 1024
const MAX_REDIRECTS = 5
const LOCK_TIMEOUT_MS = 60_000
const LOCK_STALE_MS = 15 * 60_000
const LOCK_POLL_MS = 50

const createLogger = (environment = process.env) => (...args) => {
	if (environment.CODEATLAS_DEBUG === '1') {
		console.error('[codeatlas]', ...args)
	}
}

const getBinaryName = (platform = process.platform) =>
	platform === 'win32' ? 'codeatlas.exe' : 'codeatlas'

const getCacheBase = (
	environment = process.env,
	platform = process.platform,
	homeDirectory = os.homedir()
) => {
	if (environment.CODEATLAS_CACHE_DIR) {
		return environment.CODEATLAS_CACHE_DIR
	}

	if (platform === 'win32') {
		return environment.LOCALAPPDATA || environment.APPDATA || path.join(homeDirectory, 'AppData', 'Local')
	}

	if (platform === 'darwin') {
		return path.join(homeDirectory, 'Library', 'Caches')
	}

	return environment.XDG_CACHE_HOME || path.join(homeDirectory, '.cache')
}

const getTarget = (platform = process.platform, platformArch = process.arch) => {
	let arch
	switch (platformArch) {
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
			throw new Error(`Unsupported architecture: ${platformArch}`)
	}

	switch (platform) {
		case 'darwin':
			return `${arch}-apple-darwin`
		case 'linux':
			return `${arch}-unknown-linux-gnu`
		case 'win32':
			return `${arch}-pc-windows-msvc`
		default:
			throw new Error(`Unsupported platform: ${platform}`)
	}
}

const fileExists = filePath => {
	try {
		return fs.existsSync(filePath)
	} catch {
		return false
	}
}

const ensureExecutable = (filePath, platform = process.platform) => {
	if (platform !== 'win32') {
		fs.chmodSync(filePath, 0o755)
	}
}

const requestResponse = (url, get) => new Promise((resolve, reject) => {
	let request
	try {
		request = get(url, resolve)
	} catch (error) {
		reject(error)
		return
	}
	request.once('error', reject)
})

const downloadFile = async (url, destination, options = {}) => {
	const {
		get = https.get,
		maxBytes = MAX_ARCHIVE_BYTES,
		maxRedirects = MAX_REDIRECTS,
		redirects = 0,
		log = createLogger()
	} = options

	const parsedUrl = new URL(url)
	if (parsedUrl.protocol !== 'https:') {
		throw new Error(`Download refused non-HTTPS URL: ${parsedUrl.protocol}`)
	}
	if (redirects > maxRedirects) {
		throw new Error(`Download failed after ${maxRedirects} redirects`)
	}

	log('downloading', parsedUrl.toString())
	const response = await requestResponse(parsedUrl.toString(), get)
	const status = response.statusCode ?? 0
	if (status >= 300 && status < 400 && response.headers.location) {
		const nextUrl = new URL(response.headers.location, parsedUrl)
		response.resume()
		return downloadFile(nextUrl.toString(), destination, {
			...options,
			get,
			maxBytes,
			maxRedirects,
			redirects: redirects + 1,
			log
		})
	}
	if (status !== 200) {
		response.resume()
		throw new Error(`Download failed (${status})`)
	}

	const contentLength = Number(response.headers['content-length'])
	if (Number.isFinite(contentLength) && contentLength > maxBytes) {
		response.destroy()
		throw new Error(`Download exceeds ${maxBytes} bytes`)
	}

	let received = 0
	const limiter = new Transform({
		transform(chunk, _encoding, callback) {
			received += chunk.length
			if (received > maxBytes) {
				callback(new Error(`Download exceeds ${maxBytes} bytes`))
				return
			}
			callback(null, chunk)
		}
	})
	const output = fs.createWriteStream(destination, { flags: 'wx' })
	let opened = false
	output.once('open', () => {
		opened = true
	})

	try {
		await pipeline(response, limiter, output)
	} catch (error) {
		if (opened) {
			fs.rmSync(destination, { force: true })
		}
		throw error
	}
}

const extractTar = async (archivePath, destination, binaryName, log = createLogger()) => {
	log('extracting', archivePath)
	const tar = require('tar')
	await tar.x({
		file: archivePath,
		cwd: destination,
		filter: entryPath => entryPath.replace(/^\.\//, '') === binaryName,
		preservePaths: false
	})
}

const verifyChecksum = (archivePath, checksumPath, assetName) => {
	const matches = fs.readFileSync(checksumPath, 'utf8')
		.split(/\r?\n/)
		.map(line => line.match(/^([a-fA-F0-9]{64})\s+\*?(.+)$/))
		.filter(match => match && match[2] === assetName)
	if (matches.length !== 1) {
		throw new Error(`Checksum missing or ambiguous for ${assetName}`)
	}

	const expected = Buffer.from(matches[0][1], 'hex')
	const actual = crypto.createHash('sha256').update(fs.readFileSync(archivePath)).digest()
	if (!crypto.timingSafeEqual(actual, expected)) {
		throw new Error(`Checksum mismatch for ${assetName}`)
	}
}

const getCargoBuildArgs = (environment = process.env) => {
	const jobs = environment.CODEATLAS_CARGO_JOBS || environment.CARGO_BUILD_JOBS || '1'
	if (!/^[1-9]\d*$/.test(jobs)) {
		throw new Error(`Invalid Cargo job count: ${jobs}`)
	}
	return ['build', '--release', '--locked', '--jobs', jobs]
}

const uniqueTemporaryPath = (directory, label) =>
	path.join(directory, `.${label}-${process.pid}-${crypto.randomUUID()}`)

const publishBinary = (source, binaryPath, platform = process.platform) => {
	const metadata = fs.lstatSync(source)
	if (!metadata.isFile()) {
		throw new Error('Installed binary is not a regular file')
	}
	ensureExecutable(source, platform)
	fs.renameSync(source, binaryPath)
}

const installRelease = async ({
	assetName,
	binaryName,
	binaryPath,
	cacheDir,
	download = downloadFile,
	extract = extractTar,
	platform = process.platform,
	repository,
	version,
	log = createLogger()
}) => {
	if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
		throw new Error(`Invalid CodeAtlas repository: ${repository}`)
	}

	const releaseUrl = `https://github.com/${repository}/releases/download/v${version}`
	const archivePath = uniqueTemporaryPath(cacheDir, assetName)
	const checksumPath = uniqueTemporaryPath(cacheDir, 'SHA256SUMS')
	const extractDir = uniqueTemporaryPath(cacheDir, 'install')
	fs.mkdirSync(extractDir)

	try {
		await download(`${releaseUrl}/${assetName}`, archivePath, {
			maxBytes: MAX_ARCHIVE_BYTES,
			log
		})
		await download(`${releaseUrl}/SHA256SUMS`, checksumPath, {
			maxBytes: MAX_CHECKSUM_BYTES,
			log
		})
		verifyChecksum(archivePath, checksumPath, assetName)
		await extract(archivePath, extractDir, binaryName, log)
		const stagedBinary = path.join(extractDir, binaryName)
		if (!fileExists(stagedBinary)) {
			throw new Error('Binary missing after extraction')
		}
		publishBinary(stagedBinary, binaryPath, platform)
	} finally {
		fs.rmSync(archivePath, { force: true })
		fs.rmSync(checksumPath, { force: true })
		fs.rmSync(extractDir, { force: true, recursive: true })
	}
}

const buildFromSource = (binaryPath, options = {}) => {
	const {
		binaryName = getBinaryName(),
		environment = process.env,
		platform = process.platform,
		rootDirectory = rootDir,
		spawn = spawnSync,
		log = createLogger(environment)
	} = options
	log('building from source')

	const cargoCheck = spawn('cargo', ['--version'], { stdio: 'ignore' })
	if (cargoCheck.error || cargoCheck.status !== 0) {
		throw new Error('Rust toolchain not found. Install Rust or provide CODEATLAS_BINARY_PATH.')
	}

	const build = spawn('cargo', getCargoBuildArgs(environment), {
		cwd: rootDirectory,
		stdio: 'inherit'
	})
	if (build.error || build.status !== 0) {
		throw new Error('cargo build failed')
	}

	const builtBinary = path.join(rootDirectory, 'target', 'release', binaryName)
	if (!fileExists(builtBinary)) {
		throw new Error(`Built binary missing at ${builtBinary}`)
	}

	const stagedBinary = uniqueTemporaryPath(path.dirname(binaryPath), binaryName)
	try {
		fs.copyFileSync(builtBinary, stagedBinary, fs.constants.COPYFILE_EXCL)
		publishBinary(stagedBinary, binaryPath, platform)
	} finally {
		fs.rmSync(stagedBinary, { force: true })
	}
}

const wait = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds))

const acquireInstallLock = async (lockPath, binaryPath, options = {}) => {
	const {
		pollMs = LOCK_POLL_MS,
		staleMs = LOCK_STALE_MS,
		timeoutMs = LOCK_TIMEOUT_MS
	} = options
	const deadline = Date.now() + timeoutMs

	while (true) {
		if (fileExists(binaryPath)) {
			return null
		}

		try {
			const descriptor = fs.openSync(lockPath, 'wx')
			const owner = `${process.pid}-${crypto.randomUUID()}\n`
			try {
				fs.writeFileSync(descriptor, owner)
			} finally {
				fs.closeSync(descriptor)
			}
			return () => {
				try {
					if (fs.readFileSync(lockPath, 'utf8') === owner) {
						fs.rmSync(lockPath)
					}
				} catch (error) {
					if (error.code !== 'ENOENT') {
						throw error
					}
				}
			}
		} catch (error) {
			if (error.code !== 'EEXIST') {
				throw error
			}
		}

		try {
			if (Date.now() - fs.statSync(lockPath).mtimeMs > staleMs) {
				fs.rmSync(lockPath, { force: true })
				continue
			}
		} catch (error) {
			if (error.code === 'ENOENT') {
				continue
			}
			throw error
		}

		if (Date.now() >= deadline) {
			throw new Error(`Timed out waiting for CodeAtlas install lock: ${lockPath}`)
		}
		await wait(pollMs)
	}
}

const ensureBinary = async (options = {}) => {
	const {
		build = buildFromSource,
		environment = process.env,
		homeDirectory = os.homedir(),
		install = installRelease,
		packageVersion = pkg.version,
		platform = process.platform,
		platformArch = process.arch,
		rootDirectory = rootDir
	} = options
	const log = options.log || createLogger(environment)

	if (environment.CODEATLAS_BINARY_PATH) {
		const customPath = environment.CODEATLAS_BINARY_PATH
		if (fileExists(customPath)) {
			return customPath
		}
		throw new Error(`CODEATLAS_BINARY_PATH not found: ${customPath}`)
	}

	const target = getTarget(platform, platformArch)
	const binaryName = getBinaryName(platform)
	const cacheDir = path.join(
		getCacheBase(environment, platform, homeDirectory),
		'codeatlas',
		packageVersion,
		target
	)
	const binaryPath = path.join(cacheDir, binaryName)
	if (fileExists(binaryPath)) {
		return binaryPath
	}

	fs.mkdirSync(cacheDir, { recursive: true })
	const releaseLock = await acquireInstallLock(
		path.join(cacheDir, '.install.lock'),
		binaryPath,
		options.lock
	)
	if (!releaseLock) {
		return binaryPath
	}

	try {
		if (fileExists(binaryPath)) {
			return binaryPath
		}

		const repository = environment.CODEATLAS_REPO || 'goobits/codeatlas'
		const assetName = `codeatlas-${target}.tar.gz`
		try {
			await install({
				assetName,
				binaryName,
				binaryPath,
				cacheDir,
				platform,
				repository,
				version: packageVersion,
				log
			})
		} catch (error) {
			log('release install failed', error.message)
			await build(binaryPath, {
				binaryName,
				environment,
				platform,
				rootDirectory,
				log
			})
		}

		if (!fileExists(binaryPath)) {
			throw new Error(`Installed binary missing at ${binaryPath}`)
		}
		return binaryPath
	} finally {
		releaseLock()
	}
}

const runBinary = (binaryPath, args = process.argv.slice(2)) => {
	const result = spawnSync(binaryPath, args, { stdio: 'inherit' })
	if (result.error) {
		throw result.error
	}
	return result.status ?? 1
}

module.exports = {
	acquireInstallLock,
	buildFromSource,
	downloadFile,
	ensureBinary,
	extractTar,
	getCacheBase,
	getCargoBuildArgs,
	getTarget,
	installRelease,
	runBinary,
	verifyChecksum
}
