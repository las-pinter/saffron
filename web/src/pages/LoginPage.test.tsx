import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeApi } from '@/test/fakeApi'
import { LAS, signedIn } from '@/test/fixtures'
import { renderApp } from '@/test/renderApp'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('LoginPage', () => {
  it('signs in with a password and shows the application', async () => {
    const calls = fakeApi({
      'GET /auth/state': signedIn(null),
      'POST /auth/login': { body: { user: LAS } },
    })
    renderApp('/login')

    await userEvent.type(await screen.findByLabelText('Username'), 'las')
    await userEvent.type(screen.getByLabelText('Password'), 'correct horse battery')
    await userEvent.click(screen.getByRole('button', { name: 'Sign in' }))

    expect(
      await screen.findByRole('heading', { name: 'Saffron is being built' }),
    ).toBeInTheDocument()
    expect(calls.at(-1)?.body).toEqual({ username: 'las', password: 'correct horse battery' })
  })

  it('asks for the authenticator code when the account has two-factor sign-in', async () => {
    fakeApi({
      'GET /auth/state': signedIn(null),
      'POST /auth/login': {
        status: 401,
        body: { error: 'totp_required', message: 'enter the code from your authenticator app' },
      },
    })
    renderApp('/login')

    await userEvent.type(await screen.findByLabelText('Username'), 'las')
    await userEvent.type(screen.getByLabelText('Password'), 'correct horse battery')
    await userEvent.click(screen.getByRole('button', { name: 'Sign in' }))

    expect(await screen.findByLabelText('Authenticator code')).toBeInTheDocument()
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Enter the code from your authenticator app.',
    )
  })

  it('sends the authenticator code once the form asks for it', async () => {
    let attempts = 0
    const calls = fakeApi({
      'GET /auth/state': signedIn(null),
      'POST /auth/login': () => {
        attempts += 1
        return attempts === 1
          ? { status: 401, body: { error: 'totp_required', message: 'code needed' } }
          : { body: { user: LAS } }
      },
    })
    renderApp('/login')

    await userEvent.type(await screen.findByLabelText('Username'), 'las')
    await userEvent.type(screen.getByLabelText('Password'), 'correct horse battery')
    await userEvent.click(screen.getByRole('button', { name: 'Sign in' }))
    await userEvent.type(await screen.findByLabelText('Authenticator code'), '123456')
    await userEvent.click(screen.getByRole('button', { name: 'Sign in' }))

    expect(await screen.findByText('Laszlo')).toBeInTheDocument()
    expect(calls.filter((call) => call.path === '/auth/login').at(-1)?.body).toEqual({
      username: 'las',
      password: 'correct horse battery',
      totp_code: '123456',
    })
  })

  it('shows the server message for wrong credentials', async () => {
    fakeApi({
      'GET /auth/state': signedIn(null),
      'POST /auth/login': {
        status: 401,
        body: { error: 'invalid_credentials', message: 'the username or password is incorrect' },
      },
    })
    renderApp('/login')

    await userEvent.type(await screen.findByLabelText('Username'), 'las')
    await userEvent.type(screen.getByLabelText('Password'), 'wrong horse battery')
    await userEvent.click(screen.getByRole('button', { name: 'Sign in' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'the username or password is incorrect',
    )
  })

  it('explains that passkeys are unavailable in a browser without them', async () => {
    fakeApi({ 'GET /auth/state': signedIn(null) })
    renderApp('/login')

    await userEvent.click(await screen.findByRole('button', { name: 'Sign in with a passkey' }))

    expect(screen.getByRole('alert')).toHaveTextContent('Passkeys are unavailable in this browser.')
  })
})
