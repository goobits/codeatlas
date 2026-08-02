const path = require('node:path')

const pathContains = (parent, candidate) => {
	const relative = path.relative(parent, candidate)
	return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative))
}

const requireExternalCargoTarget = (
	checkoutRoot = process.cwd(),
	environment = process.env
) => {
	const configured = environment.CARGO_TARGET_DIR?.trim()
	if (!configured || !path.isAbsolute(configured)) {
		throw new Error('CARGO_TARGET_DIR must be an absolute path outside the CodeAtlas checkout')
	}
	const checkout = path.resolve(checkoutRoot)
	const target = path.resolve(configured)
	if (pathContains(checkout, target) || pathContains(target, checkout)) {
		throw new Error('CARGO_TARGET_DIR must be outside and disjoint from the CodeAtlas checkout')
	}
	return target
}

module.exports = { requireExternalCargoTarget }
