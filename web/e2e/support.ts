import { expect, type Page } from '@playwright/test'
import { AxeBuilder } from '@axe-core/playwright'
import type { UserView } from '../src/lib/api'

export const LAS: UserView = {
  id: 1,
  username: 'las',
  display_name: 'Laszlo',
  is_admin: true,
  disabled: false,
  totp_enabled: false,
}

const BOB: UserView = {
  id: 2,
  username: 'bob',
  display_name: 'Bob',
  is_admin: false,
  disabled: false,
  totp_enabled: true,
}

interface Reply {
  status?: number
  body?: unknown
}

/** A fixed answer, or a function called on the request so a spec can change state. */
type Stub = Reply | (() => Reply)

/**
 * Answers every `/api` request by "METHOD /path" or by path alone; an unlisted path is
 * a 404.
 */
export async function stubApi(page: Page, stubs: Record<string, Stub>) {
  await page.route('**/api/**', async (route) => {
    const path = new URL(route.request().url()).pathname.replace(/^\/api/, '')
    const method = route.request().method().toUpperCase()
    const stub = stubs[`${method} ${path}`] ?? stubs[path]
    const reply = typeof stub === 'function' ? stub() : stub
    await route.fulfill(
      reply
        ? { status: reply.status ?? 200, json: reply.body ?? null }
        : { status: 404, json: { error: 'not_found', message: 'no stub' } },
    )
  })
}

/** The API a signed-in administrator sees, with no passkeys and no portfolios yet. */
export function signedIn(overrides: Partial<UserView> = {}): Record<string, Stub> {
  const user = { ...LAS, ...overrides }
  return {
    '/auth/state': { body: { setup_required: false, user } },
    '/auth/logout': { status: 204 },
    '/auth/passkeys': { body: [] },
    '/users': { body: [user, BOB] },
    '/settings': { body: { ft_fallback: false } },
    '/portfolios': { body: [] },
  }
}

/** Fails on any axe violation of serious or critical impact. */
export async function expectNoSeriousViolations(page: Page) {
  const results = await new AxeBuilder({ page }).analyze()
  const serious = results.violations.filter(
    (violation) => violation.impact === 'serious' || violation.impact === 'critical',
  )
  expect(
    serious.map((violation) => `${violation.id}: ${violation.help}`),
    'axe violations of serious or critical impact',
  ).toEqual([])
}

/** The page never scrolls sideways. */
export async function expectNoHorizontalScroll(page: Page) {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  )
  expect(overflow, 'pixels of horizontal overflow').toBeLessThanOrEqual(0)
}
