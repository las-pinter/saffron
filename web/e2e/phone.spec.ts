import { expect, test } from '@playwright/test'
import { expectNoHorizontalScroll, expectNoSeriousViolations, signedIn, stubApi } from './support'

// 390 × 844 is the phone the design guide names.
test.use({ viewport: { width: 390, height: 844 } })

const SIGNED_IN_PAGES = ['/', '/holdings', '/discover', '/ai', '/settings'] as const

for (const path of SIGNED_IN_PAGES) {
  test(`${path} does not scroll sideways at 390 px`, async ({ page }) => {
    await stubApi(page, signedIn())
    await page.goto(path)
    await expect(page.getByRole('navigation', { name: 'Sections' })).toBeVisible()
    if (path === '/settings') {
      await expect(page.getByText('No passkeys yet.')).toBeVisible()
      await expect(page.getByLabel('Use FT markets for fund prices')).toBeVisible()
    }

    await expectNoHorizontalScroll(page)
  })
}

test('the sign-in screen does not scroll sideways at 390 px', async ({ page }) => {
  await stubApi(page, { '/auth/state': { body: { setup_required: false, user: null } } })
  await page.goto('/login')
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible()

  await expectNoHorizontalScroll(page)
  await expectNoSeriousViolations(page)
})

test('the setup screen does not scroll sideways at 390 px', async ({ page }) => {
  await stubApi(page, { '/auth/state': { body: { setup_required: true, user: null } } })
  await page.goto('/setup')
  await expect(page.getByRole('heading', { name: 'Set up Saffron' })).toBeVisible()

  await expectNoHorizontalScroll(page)
})

test('the two-factor setup step does not scroll sideways at 390 px', async ({ page }) => {
  await stubApi(page, {
    ...signedIn(),
    '/auth/totp/setup': {
      body: {
        secret: 'JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP',
        otpauth_url: 'otpauth://totp/Saffron:las?secret=JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP',
      },
    },
  })
  await page.goto('/settings')
  const section = page.getByRole('region', { name: 'Two-factor sign-in' })
  await section.getByLabel('Password').fill('correct horse battery')
  await section.getByRole('button', { name: 'Set up two-factor sign-in' }).click()
  await expect(section.getByText('JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP')).toBeVisible()

  await expectNoHorizontalScroll(page)
  await expectNoSeriousViolations(page)
})
