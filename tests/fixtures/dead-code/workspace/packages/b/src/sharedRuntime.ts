function sharedHelper(): number {
	return 4;
}

export function sharedRuntime(): number {
	return sharedHelper();
}
