/** Options accepted by {@link createThing}. */
export interface ThingOptions {
	label: string
}

/** Stable identifier for one thing. */
export type ThingId = string

/** Label used when a caller does not provide one. */
export const DEFAULT_LABEL: string = 'thing'

/**
 * Create a thing.
 *
 * The result is deterministic.
 * @param options - Thing options.
 * @returns The created label.
 * @example createThing({ label: 'demo' })
 * @since 1.0.0
 */
export function createThing(options: ThingOptions): string {
	return options.label
}

/** Create a thing through the arrow-function export path. */
export const createThingArrow = (options: ThingOptions): string => options.label

/** Store and inspect things. */
export class ThingStore {
	/** Human-readable store name. */
	readonly name: string

	/** Create a store. */
	constructor(name: string, public readonly category: string) {
		this.name = name
	}

	/** Number of stored things. */
	get size(): number {
		return 0
	}

	/** Find one stored thing. */
	find(id: ThingId): string | undefined {
		return id
	}

	#reset(): void {}
}

/** Not part of the package export map. */
export function internalOnly(): void {}
