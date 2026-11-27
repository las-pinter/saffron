import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeApi } from '@/test/fakeApi'
import { LAS, signedIn } from '@/test/fixtures'
import { renderApp } from '@/test/renderApp'

afterEach(() => {
  vi.unstubAllGlobals()
})

function settingsApi(ftFallback = false) {
  return {
    'GET /auth/state': signedIn(LAS),
    'GET /auth/passkeys': { body: [] },
    'GET /users': { body: [LAS] },
    'GET /settings': { body: { ft_fallback: ftFallback } },
  }
}

describe('InstallationSection', () => {
  it('turns the FT fund fallback on and reports it', async () => {
    const calls = fakeApi({
      ...settingsApi(),
      'PATCH /settings': { body: { ft_fallback: true } },
    })
    renderApp('/settings')
    const section = within(await screen.findByRole('region', { name: 'Installation' }))

    const toggle = await section.findByLabelText('Use FT markets for fund prices')
    expect(toggle).not.toBeChecked()
    expect(toggle).toHaveAccessibleDescription(
      "Asks FT for a fund NAV when Yahoo has none. Keep it off until FT's terms are reviewed.",
    )
    await userEvent.click(toggle)

    expect(await section.findByText('Saved.')).toBeInTheDocument()
    expect(toggle).toBeChecked()
    expect(calls.filter((call) => call.method === 'PATCH')[0]?.body).toEqual({ ft_fallback: true })
  })
})
