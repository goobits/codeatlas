import { createRoute } from '@hono/zod-openapi'

export const updateWidget = createRoute({
	method: 'post',
	path: '/widgets/{id}',
})

export const getWidget = createRoute({
	method: 'get',
	path: '/widgets/{id}',
})

app.get('/health', () => new Response(null, { status: 204 }))
