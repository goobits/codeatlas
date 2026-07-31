if (req.method === 'GET' && url.pathname === '/health') {
	handleHealth()
}

const documentMatch = url.pathname.match(/^\/documents\/([^/]+)$/)
if (req.method === 'DELETE' && documentMatch) {
	deleteDocument(documentMatch[1])
}
