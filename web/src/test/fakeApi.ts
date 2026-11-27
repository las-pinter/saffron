import { vi } from 'vitest'

/** One request the app made, as the mock saw it. */
export interface Call {
  method: string
  path: string
  body: unknown
}

export interface Reply {
  status?: number
  body?: unknown
}

export type Handler = (call: Call) => Reply | Promise<Reply>
export type Routes = Record<string, Reply | Handler>

/**
 * Replaces `fetch` with a router keyed by "METHOD /path"; an unmatched call answers
 * 404. The returned array records every call, so a test can look at what was sent.
 */
export function fakeApi(routes: Routes): Call[] {
  const calls: Call[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: string | URL, init?: RequestInit) => {
      const url = new URL(String(input), 'http://saffron.test')
      const call: Call = {
        method: (init?.method ?? 'GET').toUpperCase(),
        path: url.pathname.replace(/^\/api/, ''),
        body: init?.body ? JSON.parse(String(init.body)) : undefined,
      }
      calls.push(call)
      const route = routes[`${call.method} ${call.path}`]
      const reply =
        typeof route === 'function'
          ? await route(call)
          : (route ?? { status: 404, body: { error: 'not_found', message: 'no route' } })
      const status = reply.status ?? 200
      return {
        ok: status >= 200 && status < 300,
        status,
        statusText: '',
        json: async () => reply.body ?? null,
      }
    }),
  )
  return calls
}
