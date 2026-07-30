import { createRoute } from '@hono/zod-openapi'

export const updateWidget = createRoute({
	method: 'post',
	path: '/widgets/{id}',
})

app.get('/health', () => new Response(null, { status: 204 }))
