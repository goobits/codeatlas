export interface ArtifactPublisher {
	/** @codeatlas-fuzz deny: publishes to the real artifact registry */
	publish(bundle: string): boolean

	/** @codeatlas-fuzz allow: stale comments may not grant authority */
	staleAllow(bundle: string): boolean
}
