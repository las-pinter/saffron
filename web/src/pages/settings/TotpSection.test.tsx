import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeApi } from '@/test/fakeApi'
import { LAS, signedIn } from '@/test/fixtures'
import { renderApp } from '@/test/renderApp'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('TotpSection', () => {
  it('offers a secret, confirms a code, and shows the second factor as on', async () => {
    let enabled = false
    const calls = fakeApi({
      'GET /auth/state': () => signedIn({ ...LAS, totp_enabled: enabled }),
      'GET /auth/passkeys': { body: [] },
      'GET /users': { body: [LAS] },
      'GET /settings': { body: { ft_fallback: false } },
      'POST /auth/totp/setup': {
        body: {
          secret: 'JBSWY3DPEHPK3PXP',
          otpauth_url: 'otpauth://totp/Saffron:las?secret=JBSWY3DPEHPK3PXP',
        },
      },
      'POST /auth/totp/confirm': () => {
        enabled = true
        return { status: 204 }
      },
    })
    renderApp('/settings')
    const section = within(await screen.findByRole('region', { name: 'Two-factor sign-in' }))

    await userEvent.type(section.getByLabelText('Password'), 'correct horse battery')
    await userEvent.click(section.getByRole('button', { name: 'Set up two-factor sign-in' }))

    expect(await section.findByText('JBSWY3DPEHPK3PXP')).toBeInTheDocument()
    await userEvent.type(section.getByLabelText('Authenticator code'), '123456')
    await userEvent.click(section.getByRole('button', { name: 'Turn on' }))

    expect(await section.findByText('Two-factor sign-in is now on.')).toBeInTheDocument()
    expect(calls.filter((call) => call.path === '/auth/totp/confirm')[0]?.body).toEqual({
      code: '123456',
    })
  })

  it('turns the second factor off with the password and a code', async () => {
    let enabled = true
    const calls = fakeApi({
      'GET /auth/state': () => signedIn({ ...LAS, totp_enabled: enabled }),
      'GET /auth/passkeys': { body: [] },
      'GET /users': { body: [LAS] },
      'GET /settings': { body: { ft_fallback: false } },
      'POST /auth/totp/disable': () => {
        enabled = false
        return { status: 204 }
      },
    })
    renderApp('/settings')
    const section = within(await screen.findByRole('region', { name: 'Two-factor sign-in' }))

    await userEvent.type(section.getByLabelText('Password'), 'correct horse battery')
    await userEvent.type(section.getByLabelText('Authenticator code'), '123456')
    await userEvent.click(section.getByRole('button', { name: 'Turn off two-factor sign-in' }))

    expect(await section.findByText('Two-factor sign-in is now off.')).toBeInTheDocument()
    expect(calls.filter((call) => call.path === '/auth/totp/disable')[0]?.body).toEqual({
      password: 'correct horse battery',
      code: '123456',
    })
  })
})
