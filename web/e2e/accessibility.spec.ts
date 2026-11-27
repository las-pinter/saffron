import { expect, test } from '@playwright/test'
import { expectNoSeriousViolations, signedIn, stubApi } from './support'

const SIGNED_IN_PAGES = [
  { path: '/', heading: 'Saffron is being built' },
  { path: '/holdings', heading: 'Saffron is being built' },
  { path: '/discover', heading: 'Saffron is being built' },
  { path: '/ai', heading: 'Saffron is being built' },
  { path: '/settings', heading: 'Settings' },
]

for (const { path, heading } of SIGNED_IN_PAGES) {
  test(`${path} has no serious accessibility violations`, async ({ page }) => {
    await stubApi(page, signedIn())
    await page.goto(path)
    await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible()
    await expect(page.getByRole('heading', { name: heading }).first()).toBeVisible()
    if (path === '/settings') {
      await expect(page.getByText('No passkeys yet.')).toBeVisible()
      await expect(page.getByLabel('Use FT markets for fund prices')).toBeVisible()
    }

    await expectNoSeriousViolations(page)
  })
}

test('the sign-in screen has no serious accessibility violations', async ({ page }) => {
  await stubApi(page, { '/auth/state': { body: { setup_required: false, user: null } } })
  await page.goto('/login')
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible()

  await expectNoSeriousViolations(page)
})

test('the setup screen has no serious accessibility violations', async ({ page }) => {
  await stubApi(page, { '/auth/state': { body: { setup_required: true, user: null } } })
  await page.goto('/setup')
  await expect(page.getByRole('heading', { name: 'Set up Saffron' })).toBeVisible()

  await expectNoSeriousViolations(page)
})
