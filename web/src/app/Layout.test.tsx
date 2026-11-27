import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeApi } from '@/test/fakeApi'
import { LAS, signedIn } from '@/test/fixtures'
import { renderApp } from '@/test/renderApp'

afterEach(() => {
  vi.unstubAllGlobals()
})

async function mainNavigation() {
  return within(await screen.findByRole('navigation', { name: 'Main' }))
}

describe('Layout', () => {
  it('marks the overview link as current on the root path', async () => {
    fakeApi({ 'GET /auth/state': signedIn(LAS) })
    renderApp('/')

    expect((await mainNavigation()).getByRole('link', { name: 'Overview' })).toHaveAttribute(
      'aria-current',
      'page',
    )
  })

  it('moves the current marker when a navigation link is clicked', async () => {
    fakeApi({ 'GET /auth/state': signedIn(LAS) })
    const router = renderApp('/')

    await userEvent.click((await mainNavigation()).getByRole('link', { name: 'Holdings' }))

    expect(router.state.location.pathname).toBe('/holdings')
    expect(
      within(screen.getByRole('navigation', { name: 'Main' })).getByRole('link', {
        name: 'Holdings',
      }),
    ).toHaveAttribute('aria-current', 'page')
  })

  it('offers the same sections as tabs for a phone', async () => {
    fakeApi({ 'GET /auth/state': signedIn(LAS) })
    const router = renderApp('/')

    const tabs = within(await screen.findByRole('navigation', { name: 'Sections' }))
    expect(tabs.getAllByRole('link')).toHaveLength(4)
    await userEvent.click(tabs.getByRole('link', { name: 'AI desk' }))

    expect(router.state.location.pathname).toBe('/ai')
  })

  it('offers a skip link to the main content', async () => {
    fakeApi({ 'GET /auth/state': signedIn(LAS) })
    renderApp('/')

    const skip = await screen.findByRole('link', { name: 'Skip to content' })
    expect(skip).toHaveAttribute('href', '#main')
    expect(document.querySelector('main')).toHaveAttribute('id', 'main')
  })

  it('renders translated text instead of raw keys', async () => {
    fakeApi({ 'GET /auth/state': signedIn(LAS) })
    renderApp('/')

    expect(
      await screen.findByRole('heading', { name: 'Saffron is being built' }),
    ).toBeInTheDocument()
  })

  it('names the signed-in user in the account menu', async () => {
    fakeApi({ 'GET /auth/state': signedIn(LAS) })
    renderApp('/')

    await userEvent.click(await screen.findByText('Laszlo'))

    expect(screen.getByRole('link', { name: 'Settings' })).toHaveAttribute('href', '/settings')
  })

  it('sends a signed-out visitor to the sign-in screen', async () => {
    fakeApi({ 'GET /auth/state': signedIn(null) })
    renderApp('/')

    expect(await screen.findByRole('heading', { name: 'Sign in' })).toBeInTheDocument()
  })

  it('returns to sign-in when a call reports the session is gone', async () => {
    fakeApi({
      'GET /auth/state': signedIn(LAS),
      'GET /auth/passkeys': { status: 401, body: { error: 'unauthenticated' } },
      'GET /users': { body: [LAS] },
      'GET /settings': { body: { ft_fallback: false } },
    })
    renderApp('/settings')

    expect(await screen.findByRole('heading', { name: 'Sign in' })).toBeInTheDocument()
  })

  it('ends the session and returns to sign-in when Sign out is clicked', async () => {
    const calls = fakeApi({
      'GET /auth/state': signedIn(LAS),
      'POST /auth/logout': { status: 204 },
    })
    renderApp('/')

    await userEvent.click(await screen.findByText('Laszlo'))
    await userEvent.click(screen.getByRole('button', { name: 'Sign out' }))

    expect(await screen.findByRole('heading', { name: 'Sign in' })).toBeInTheDocument()
    expect(calls.filter((call) => call.path === '/auth/logout')).toHaveLength(1)
  })

  it('still signs out locally when the server call fails', async () => {
    fakeApi({
      'GET /auth/state': signedIn(LAS),
      'POST /auth/logout': { status: 500, body: { error: 'internal', message: 'failed' } },
    })
    renderApp('/')

    await userEvent.click(await screen.findByText('Laszlo'))
    await userEvent.click(screen.getByRole('button', { name: 'Sign out' }))

    expect(await screen.findByRole('heading', { name: 'Sign in' })).toBeInTheDocument()
  })
})
