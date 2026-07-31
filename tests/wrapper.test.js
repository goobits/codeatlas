const assert = require('node:assert/strict')
const crypto = require('node:crypto')
const { EventEmitter } = require('node:events')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const { Readable } = require('node:stream')
const test = require('node:test')
const tar = require('tar')
const {
	acquireInstallLock,
	downloadFile,
	ensureBinary,
	extractTar,
	getCacheBase,
	getCargoBuildArgs,
	getTarget,
	installRelease,
	verifyChecksum
} = require('../bin/_installer.js')

const temporaryDirectory = testContext => {
	const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'codeatlas-wrapper-'))
	testContext.after(() => fs.rmSync(directory, { force: true, recursive: true }))
	return directory
}

const response = (statusCode, headers = {}, body = '') => {
	const stream = Readable.from(body ? [Buffer.from(body)] : [])
	stream.statusCode = statusCode
	stream.headers = headers
	return stream
}

const fakeGet = handler => (url, callback) => {
	const request = new EventEmitter()
	queueMicrotask(() => callback(handler(url)))
	return request
}

test('source fallback uses a locked single-job build by default', () => {
	assert.deepEqual(getCargoBuildArgs({}), [
		'build',
		'--release',
		'--locked',
		'--jobs',
		'1'
	])
})

test('source fallback accepts explicit CodeAtlas and Cargo job limits', () => {
	assert.deepEqual(getCargoBuildArgs({ CODEATLAS_CARGO_JOBS: '4' }).slice(-2), ['--jobs', '4'])
	assert.deepEqual(getCargoBuildArgs({ CARGO_BUILD_JOBS: '2' }).slice(-2), ['--jobs', '2'])
})

test('source fallback rejects invalid job limits', () => {
	assert.throws(
		() => getCargoBuildArgs({ CODEATLAS_CARGO_JOBS: 'all' }),
		/Invalid Cargo job count: all/
	)
})

test('platform and cache mappings are deterministic', () => {
	assert.equal(getTarget('linux', 'x64'), 'x86_64-unknown-linux-gnu')
	assert.equal(getTarget('darwin', 'arm64'), 'aarch64-apple-darwin')
	assert.equal(getTarget('win32', 'x64'), 'x86_64-pc-windows-msvc')
	assert.throws(() => getTarget('freebsd', 'x64'), /Unsupported platform/)
	assert.throws(() => getTarget('linux', 'mips'), /Unsupported architecture/)

	assert.equal(getCacheBase({ CODEATLAS_CACHE_DIR: '/custom' }, 'linux', '/home'), '/custom')
	assert.equal(getCacheBase({ XDG_CACHE_HOME: '/xdg' }, 'linux', '/home'), '/xdg')
	assert.equal(getCacheBase({}, 'darwin', '/home'), path.join('/home', 'Library', 'Caches'))
	assert.equal(getCacheBase({ LOCALAPPDATA: 'C:\\cache' }, 'win32', 'C:\\home'), 'C:\\cache')
})

test('downloads follow bounded HTTPS redirects', async testContext => {
	const directory = temporaryDirectory(testContext)
	const destination = path.join(directory, 'asset')
	let requests = 0
	const get = fakeGet(url => {
		requests += 1
		return url.endsWith('/start')
			? response(302, { location: '/asset' })
			: response(200, {}, 'release')
	})

	await downloadFile('https://example.test/start', destination, { get })
	assert.equal(fs.readFileSync(destination, 'utf8'), 'release')
	assert.equal(requests, 2)

	await assert.rejects(
		downloadFile('https://example.test/loop', path.join(directory, 'loop'), {
			get: fakeGet(() => response(302, { location: '/loop' })),
			maxRedirects: 1
		}),
		/after 1 redirects/
	)
	await assert.rejects(
		downloadFile('http://example.test/asset', path.join(directory, 'http')),
		/refused non-HTTPS/
	)
})

test('downloads reject oversized streams and remove partial files', async testContext => {
	const directory = temporaryDirectory(testContext)
	const destination = path.join(directory, 'asset')

	await assert.rejects(
		downloadFile('https://example.test/asset', destination, {
			get: fakeGet(() => response(200, {}, 'oversized')),
			maxBytes: 4
		}),
		/exceeds 4 bytes/
	)
	assert.equal(fs.existsSync(destination), false)
})

test('checksums require one exact asset match and reject corruption', testContext => {
	const directory = temporaryDirectory(testContext)
	const archivePath = path.join(directory, 'asset.tar.gz')
	const checksumPath = path.join(directory, 'SHA256SUMS')
	const digest = crypto.createHash('sha256').update('release').digest('hex')
	fs.writeFileSync(archivePath, 'release')

	fs.writeFileSync(checksumPath, `${digest}  asset.tar.gz\n`)
	assert.doesNotThrow(() => verifyChecksum(archivePath, checksumPath, 'asset.tar.gz'))

	fs.writeFileSync(checksumPath, `${'0'.repeat(64)}  asset.tar.gz\n`)
	assert.throws(() => verifyChecksum(archivePath, checksumPath, 'asset.tar.gz'), /Checksum mismatch/)

	fs.writeFileSync(checksumPath, `${digest}  another.tar.gz\n`)
	assert.throws(() => verifyChecksum(archivePath, checksumPath, 'asset.tar.gz'), /missing or ambiguous/)
})

test('release archives extract the root binary and nothing else', async testContext => {
	const directory = temporaryDirectory(testContext)
	const source = path.join(directory, 'source')
	const destination = path.join(directory, 'destination')
	const archivePath = path.join(directory, 'release.tar.gz')
	fs.mkdirSync(source)
	fs.mkdirSync(destination)
	fs.writeFileSync(path.join(source, 'codeatlas'), 'binary')
	fs.writeFileSync(path.join(source, 'extra.txt'), 'extra')
	await tar.c({ cwd: source, file: archivePath, gzip: true }, ['codeatlas', 'extra.txt'])

	await extractTar(archivePath, destination, 'codeatlas')
	assert.equal(fs.readFileSync(path.join(destination, 'codeatlas'), 'utf8'), 'binary')
	assert.equal(fs.existsSync(path.join(destination, 'extra.txt')), false)
})

test('release installs publish atomically and clean temporary files', async testContext => {
	const cacheDir = temporaryDirectory(testContext)
	const binaryPath = path.join(cacheDir, 'codeatlas')
	const archive = Buffer.from('release archive')
	const digest = crypto.createHash('sha256').update(archive).digest('hex')
	const download = async (url, destination) => {
		fs.writeFileSync(
			destination,
			url.endsWith('/SHA256SUMS') ? `${digest}  codeatlas-target.tar.gz\n` : archive
		)
	}
	const extract = async (_archivePath, destination) => {
		assert.equal(fs.existsSync(binaryPath), false)
		fs.writeFileSync(path.join(destination, 'codeatlas'), 'binary')
	}

	await installRelease({
		assetName: 'codeatlas-target.tar.gz',
		binaryName: 'codeatlas',
		binaryPath,
		cacheDir,
		download,
		extract,
		platform: 'linux',
		repository: 'goobits/codeatlas',
		version: '1.2.3'
	})

	assert.equal(fs.readFileSync(binaryPath, 'utf8'), 'binary')
	assert.deepEqual(fs.readdirSync(cacheDir), ['codeatlas'])
})

test('failed release installs leave no partial artifacts', async testContext => {
	const cacheDir = temporaryDirectory(testContext)
	const download = async (url, destination) => {
		fs.writeFileSync(
			destination,
			url.endsWith('/SHA256SUMS')
				? `${'0'.repeat(64)}  codeatlas-target.tar.gz\n`
				: 'release archive'
		)
	}

	await assert.rejects(
		installRelease({
			assetName: 'codeatlas-target.tar.gz',
			binaryName: 'codeatlas',
			binaryPath: path.join(cacheDir, 'codeatlas'),
			cacheDir,
			download,
			extract: () => assert.fail('corrupt archives must not be extracted'),
			platform: 'linux',
			repository: 'goobits/codeatlas',
			version: '1.2.3'
		}),
		/Checksum mismatch/
	)
	assert.deepEqual(fs.readdirSync(cacheDir), [])
})

const ensureOptions = (cacheBase, overrides = {}) => ({
	environment: { CODEATLAS_CACHE_DIR: cacheBase },
	lock: { pollMs: 2, timeoutMs: 1_000 },
	packageVersion: '1.2.3',
	platform: 'linux',
	platformArch: 'x64',
	...overrides
})

const cachedBinaryPath = cacheBase => path.join(
	cacheBase,
	'codeatlas',
	'1.2.3',
	'x86_64-unknown-linux-gnu',
	'codeatlas'
)

test('existing cached binaries bypass installation', async testContext => {
	const cacheBase = temporaryDirectory(testContext)
	const binaryPath = cachedBinaryPath(cacheBase)
	fs.mkdirSync(path.dirname(binaryPath), { recursive: true })
	fs.writeFileSync(binaryPath, 'cached')

	const actual = await ensureBinary(ensureOptions(cacheBase, {
		build: () => assert.fail('cached binaries must not build'),
		install: () => assert.fail('cached binaries must not download')
	}))
	assert.equal(actual, binaryPath)
})

test('concurrent callers share one installation', async testContext => {
	const cacheBase = temporaryDirectory(testContext)
	let installs = 0
	const install = async ({ binaryPath }) => {
		installs += 1
		await new Promise(resolve => setTimeout(resolve, 20))
		fs.writeFileSync(binaryPath, 'installed')
	}
	const options = ensureOptions(cacheBase, {
		build: () => assert.fail('successful installs must not build'),
		install
	})

	const [first, second] = await Promise.all([ensureBinary(options), ensureBinary(options)])
	assert.equal(first, cachedBinaryPath(cacheBase))
	assert.equal(second, first)
	assert.equal(installs, 1)
	assert.equal(fs.existsSync(path.join(path.dirname(first), '.install.lock')), false)
})

test('stale lock owners cannot remove a replacement lock', async testContext => {
	const directory = temporaryDirectory(testContext)
	const lockPath = path.join(directory, '.install.lock')
	const release = await acquireInstallLock(lockPath, path.join(directory, 'codeatlas'))
	assert.ok(release)
	fs.writeFileSync(lockPath, 'replacement-owner\n')

	release()
	assert.equal(fs.readFileSync(lockPath, 'utf8'), 'replacement-owner\n')
})

test('failed release installation falls back to one source build', async testContext => {
	const cacheBase = temporaryDirectory(testContext)
	let builds = 0
	const binaryPath = await ensureBinary(ensureOptions(cacheBase, {
		install: async () => {
			throw new Error('release unavailable')
		},
		build: async destination => {
			builds += 1
			fs.writeFileSync(destination, 'source build')
		}
	}))

	assert.equal(binaryPath, cachedBinaryPath(cacheBase))
	assert.equal(fs.readFileSync(binaryPath, 'utf8'), 'source build')
	assert.equal(builds, 1)
	assert.equal(fs.existsSync(path.join(path.dirname(binaryPath), '.install.lock')), false)
})
