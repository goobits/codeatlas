/** @codeatlas-fuzz deny: publishes to the real artifact registry */
export function publish(bundle) {
	return Boolean(bundle)
}

/** @codeatlas-fuzz allow: stale comments may not grant authority */
export function staleAllow(bundle) {
	return Boolean(bundle)
}
