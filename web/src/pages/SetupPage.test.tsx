import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeApi } from '@/test/fakeApi'
import { LAS } from '@/test/fixtures'
import { renderApp } from '@/test/renderApp'

const NO_USER = { body: { setup_required: true, user: null } }

afterEach(() => {
  vi.unstubAllGlobals()
  window.location.hash = ''
  window.history.replaceState({}, '', '/')
})

function openSetup() {
  window.location.hash = '#token=abc123'
  renderApp('/setup')
}

describe('SetupPage', () => {
  it('takes the token from the setup link and creates the first account', async () => {
    const calls = fakeApi({
      'GET /auth/state': NO_USER,
      'POST /auth/setup': { status: 201, body: { user: LAS } },
    })
    openSetup()

    expect(await screen.findByLabelText('Setup token')).toHaveValue('abc123')
    await userEvent.type(screen.getByLabelText('Username'), 'las')
    await userEvent.type(screen.getByLabelText('Display name'), 'Laszlo')
    await userEvent.type(screen.getByLabelText('Password'), 'correct horse battery')
    await userEvent.type(screen.getByLabelText('Repeat the password'), 'correct horse battery')
    await userEvent.click(screen.getByRole('button', { name: 'Create the account' }))

    expect(
      await screen.findByRole('heading', { name: 'Saffron is being built' }),
    ).toBeInTheDocument()
    expect(calls.filter((call) => call.path === '/auth/setup')[0]?.body).toEqual({
      token: 'abc123',
      username: 'las',
      display_name: 'Laszlo',
      password: 'correct horse battery',
    })
  })

  it('ignores a token in the query string', async () => {
    window.history.replaceState({}, '', '/setup?token=abc123')
    fakeApi({ 'GET /auth/state': NO_USER })
    renderApp('/setup')

    expect(await screen.findByLabelText('Setup token')).toHaveValue('')
  })

  it('refuses to submit when the passwords differ', async () => {
    const calls = fakeApi({ 'GET /auth/state': NO_USER })
    openSetup()

    await userEvent.type(await screen.findByLabelText('Username'), 'las')
    await userEvent.type(screen.getByLabelText('Display name'), 'Laszlo')
    await userEvent.type(screen.getByLabelText('Password'), 'correct horse battery')
    await userEvent.type(screen.getByLabelText('Repeat the password'), 'a different secret')
    await userEvent.click(screen.getByRole('button', { name: 'Create the account' }))

    expect(screen.getByRole('alert')).toHaveTextContent('The two passwords do not match.')
    expect(calls.some((call) => call.path === '/auth/setup')).toBe(false)
  })

  it('shows the server message for a stale setup token', async () => {
    fakeApi({
      'GET /auth/state': NO_USER,
      'POST /auth/setup': {
        status: 403,
        body: { error: 'invalid_setup_token', message: 'the setup link is invalid' },
      },
    })
    openSetup()

    await userEvent.type(await screen.findByLabelText('Username'), 'las')
    await userEvent.type(screen.getByLabelText('Display name'), 'Laszlo')
    await userEvent.type(screen.getByLabelText('Password'), 'correct horse battery')
    await userEvent.type(screen.getByLabelText('Repeat the password'), 'correct horse battery')
    await userEvent.click(screen.getByRole('button', { name: 'Create the account' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('the setup link is invalid')
  })
})
