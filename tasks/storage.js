const fs = require('node:fs')
const path = require('node:path')

const pathContains = (parent, candidate) => {
	const relative = path.relative(parent, candidate)
	return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative))
}

const resolveProspectivePath = candidate => {
	const suffix = []
	let existing = path.resolve(candidate)
	while (!fs.existsSync(existing)) {
		const parent = path.dirname(existing)
		if (parent === existing) {
			throw new Error(`Could not resolve an existing ancestor for ${candidate}`)
		}
		suffix.unshift(path.basename(existing))
		existing = parent
	}
	return path.join(fs.realpathSync.native(existing), ...suffix)
}

const requireExternalPath = (checkoutRoot, candidate, label) => {
	if (typeof candidate !== 'string' || !path.isAbsolute(candidate)) {
		throw new Error(`${label} must be an absolute path outside the CodeAtlas checkout`)
	}
	const checkout = fs.realpathSync.native(checkoutRoot)
	const resolved = resolveProspectivePath(candidate)
	if (pathContains(checkout, resolved) || pathContains(resolved, checkout)) {
		throw new Error(`${label} must be outside and disjoint from the CodeAtlas checkout`)
	}
	return resolved
}

const requireExternalCargoTarget = (
	checkoutRoot = process.cwd(),
	environment = process.env
) => {
	const configured = environment.CARGO_TARGET_DIR?.trim()
	if (!configured || !path.isAbsolute(configured)) {
		throw new Error('CARGO_TARGET_DIR must be an absolute path outside the CodeAtlas checkout')
	}
	return requireExternalPath(checkoutRoot, configured, 'CARGO_TARGET_DIR')
}

const writePrivateFile = (destination, contents) => {
	fs.writeFileSync(destination, contents, { mode: 0o600 })
	fs.chmodSync(destination, 0o600)
}

module.exports = { requireExternalCargoTarget, requireExternalPath, writePrivateFile }
