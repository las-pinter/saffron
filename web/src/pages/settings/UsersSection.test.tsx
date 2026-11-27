import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeApi } from '@/test/fakeApi'
import { BOB, LAS, signedIn } from '@/test/fixtures'
import { renderApp } from '@/test/renderApp'

afterEach(() => {
  vi.unstubAllGlobals()
})

function usersApi(users = [LAS, BOB]) {
  return {
    'GET /auth/state': signedIn(LAS),
    'GET /auth/passkeys': { body: [] },
    'GET /users': { body: users },
    'GET /settings': { body: { ft_fallback: false } },
  }
}

async function usersSection() {
  renderApp('/settings')
  return within(await screen.findByRole('region', { name: 'Users' }))
}

describe('UsersSection', () => {
  it('marks the signed-in administrator and offers actions only for others', async () => {
    fakeApi(usersApi())
    const section = await usersSection()

    expect(await section.findByText('Bob')).toBeInTheDocument()
    expect(section.getByText('This is you')).toBeInTheDocument()
    expect(section.getAllByRole('button', { name: 'Disable Bob' })).toHaveLength(1)
    expect(section.getByText('2FA')).toBeInTheDocument()
  })

  it('creates a user with the form values', async () => {
    const calls = fakeApi({ ...usersApi(), 'POST /users': { status: 201, body: BOB } })
    const section = await usersSection()

    await userEvent.type(await section.findByLabelText('Username'), 'bob')
    await userEvent.type(section.getByLabelText('Display name'), 'Bob')
    await userEvent.type(section.getByLabelText('Password'), 'correct horse battery')
    await userEvent.click(section.getByRole('button', { name: 'Create user' }))

    expect(await section.findByText('User created.')).toBeInTheDocument()
    expect(
      calls.filter((call) => call.path === '/users' && call.method === 'POST')[0]?.body,
    ).toEqual({
      username: 'bob',
      display_name: 'Bob',
      password: 'correct horse battery',
      is_admin: false,
    })
  })

  it('clears a failed list read once a later read succeeds', async () => {
    let offline = true
    fakeApi({
      ...usersApi(),
      'GET /users': () =>
        offline
          ? { status: 500, body: { error: 'internal', message: 'the server is not answering' } }
          : { body: [LAS, BOB] },
      'POST /users': () => {
        offline = false
        return { status: 201, body: BOB }
      },
    })
    const section = await usersSection()

    expect(await section.findByText('the server is not answering')).toBeInTheDocument()
    await userEvent.type(await section.findByLabelText('Username'), 'bob')
    await userEvent.type(section.getByLabelText('Display name'), 'Bob')
    await userEvent.type(section.getByLabelText('Password'), 'correct horse battery')
    await userEvent.click(section.getByRole('button', { name: 'Create user' }))

    expect(await section.findByText('User created.')).toBeInTheDocument()
    expect(section.queryByText('the server is not answering')).not.toBeInTheDocument()
  })

  it('disables another user after confirming nothing is needed', async () => {
    const calls = fakeApi({ ...usersApi(), 'POST /users/2/disable': { status: 204 } })
    const section = await usersSection()

    const bob = (await section.findByText('Bob')).closest('li')
    if (!bob) throw new Error('the user row is missing')
    await userEvent.click(within(bob).getByRole('button', { name: 'Disable Bob' }))

    expect(await section.findByText('User updated.')).toBeInTheDocument()
    expect(calls.some((call) => call.path === '/users/2/disable')).toBe(true)
  })

  it('enables a disabled user', async () => {
    const disabled = { ...BOB, id: 3, username: 'cy', display_name: 'Cy', disabled: true }
    const calls = fakeApi({ ...usersApi([LAS, disabled]), 'POST /users/3/enable': { status: 204 } })
    const section = await usersSection()

    const cy = (await section.findByText('Cy')).closest('li')
    if (!cy) throw new Error('the user row is missing')
    await userEvent.click(within(cy).getByRole('button', { name: 'Enable Cy' }))

    expect(await section.findByText('User updated.')).toBeInTheDocument()
    expect(calls.some((call) => call.path === '/users/3/enable')).toBe(true)
  })

  it('resets another user password', async () => {
    const calls = fakeApi({ ...usersApi(), 'POST /users/2/password': { status: 204 } })
    const section = await usersSection()

    const bob = (await section.findByText('Bob')).closest('li')
    if (!bob) throw new Error('the user row is missing')
    await userEvent.click(within(bob).getByRole('button', { name: 'Reset password for Bob' }))
    await userEvent.type(within(bob).getByLabelText('New password'), 'correct horse battery')
    await userEvent.click(within(bob).getByRole('button', { name: 'Set password' }))

    expect(await section.findByText(/Password reset/)).toBeInTheDocument()
    expect(section.getByRole('list')).toHaveFocus()
    expect(calls.filter((call) => call.path === '/users/2/password')[0]?.body).toEqual({
      password: 'correct horse battery',
    })
  })

  it('resets another user two-factor sign-in after confirming', async () => {
    const calls = fakeApi({ ...usersApi(), 'POST /users/2/totp/reset': { status: 204 } })
    const section = await usersSection()

    const bob = (await section.findByText('Bob')).closest('li')
    if (!bob) throw new Error('the user row is missing')
    await userEvent.click(within(bob).getByRole('button', { name: 'Reset 2FA for Bob' }))
    await userEvent.click(within(bob).getByRole('button', { name: 'Reset 2FA now for Bob' }))

    expect(await section.findByText(/Two-factor sign-in reset/)).toBeInTheDocument()
    expect(section.getByRole('list')).toHaveFocus()
    expect(calls.some((call) => call.path === '/users/2/totp/reset')).toBe(true)
  })
})
