if (req.method === 'GET' && url.pathname === '/health') {
	handleHealth()
}

const documentMatch = url.pathname.match(/^\/documents\/([^/]+)$/)
if (req.method === 'DELETE' && documentMatch) {
	deleteDocument(documentMatch[1])
}

const uploadMatch = url.pathname.match(
	/^\/document-uploads\/([^/]+)(?:\/(bundle|commit))?$/
)
if (uploadMatch) {
	const action = uploadMatch[2]
	if (req.method === 'DELETE' && !action) cancelUpload(uploadMatch[1])
	if (req.method === 'PUT' && action === 'bundle') uploadBundle(uploadMatch[1])
	if (req.method === 'POST' && action === 'commit') commitUpload(uploadMatch[1])
}
