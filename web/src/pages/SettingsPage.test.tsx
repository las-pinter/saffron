import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeApi } from '@/test/fakeApi'
import { BOB, LAS, signedIn } from '@/test/fixtures'
import { fakeAttestation, restoreBrowser, stubBrowser } from '@/test/browser'
import { renderApp } from '@/test/renderApp'

afterEach(() => {
  restoreBrowser()
})

function settings(user = LAS) {
  return {
    'GET /auth/state': signedIn(user),
    'GET /auth/passkeys': { body: [] },
    'GET /users': { body: [LAS, BOB] },
    'GET /settings': { body: { ft_fallback: false } },
  }
}

describe('SettingsPage', () => {
  it('shows every section to an administrator', async () => {
    fakeApi(settings())
    renderApp('/settings')

    for (const name of ['Password', 'Two-factor sign-in', 'Passkeys', 'Users', 'Installation']) {
      expect(await screen.findByRole('region', { name })).toBeInTheDocument()
    }
    expect(screen.getByText('Signed in as Laszlo.')).toBeInTheDocument()
  })

  it('hides administration from a member who is not an administrator', async () => {
    fakeApi(settings(BOB))
    renderApp('/settings')

    expect(await screen.findByRole('region', { name: 'Passkeys' })).toBeInTheDocument()
    expect(screen.queryByRole('region', { name: 'Users' })).not.toBeInTheDocument()
    expect(screen.queryByRole('region', { name: 'Installation' })).not.toBeInTheDocument()
  })

  it('changes the password and reports it', async () => {
    const calls = fakeApi({ ...settings(), 'POST /auth/password': { status: 204 } })
    renderApp('/settings')
    const section = within(await screen.findByRole('region', { name: 'Password' }))

    await userEvent.type(section.getByLabelText('Current password'), 'old horse battery')
    await userEvent.type(section.getByLabelText('New password'), 'new horse battery')
    await userEvent.type(section.getByLabelText('Repeat the new password'), 'new horse battery')
    await userEvent.click(section.getByRole('button', { name: 'Change password' }))

    expect(await screen.findByText(/Password changed/)).toBeInTheDocument()
    expect(calls.filter((call) => call.path === '/auth/password')[0]?.body).toEqual({
      current_password: 'old horse battery',
      new_password: 'new horse battery',
    })
  })

  it('adds a passkey through the form', async () => {
    const create = vi.fn<(options?: unknown) => Promise<PublicKeyCredential | null>>(async () =>
      fakeAttestation(),
    )
    stubBrowser({ create })
    let passkeys: unknown[] = []
    const calls = fakeApi({
      ...settings(),
      'GET /auth/passkeys': () => ({ body: passkeys }),
      'POST /auth/passkeys/register/start': {
        body: {
          ceremony_id: 'ceremony-1',
          options: {
            publicKey: {
              challenge: 'Y2hhbGxlbmdl',
              rp: { name: 'Saffron', id: 'localhost' },
              user: { id: 'dXNlci0x', name: 'las', displayName: 'Laszlo' },
              pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
            },
          },
        },
      },
      'POST /auth/passkeys/register/finish': () => {
        passkeys = [
          { id: 7, name: 'Laptop', created_at: '2026-11-01T10:00:00Z', last_used_at: null },
          { id: 8, name: 'Phone', created_at: '2026-11-02T10:00:00Z', last_used_at: null },
        ]
        return { body: passkeys[0] }
      },
    })
    renderApp('/settings')
    const section = within(await screen.findByRole('region', { name: 'Passkeys' }))

    await userEvent.type(await section.findByLabelText('Name'), 'Laptop')
    await userEvent.type(section.getByLabelText('Password'), 'correct horse battery')
    await userEvent.click(section.getByRole('button', { name: 'Add a passkey' }))

    expect(await section.findByText('Passkey added.')).toBeInTheDocument()
    expect(await section.findByText('Laptop')).toBeInTheDocument()
    // The list is read back, so a passkey registered elsewhere shows too.
    expect(await section.findByText('Phone')).toBeInTheDocument()
    expect(calls.filter((call) => call.path === '/auth/passkeys/register/finish')[0]?.body).toEqual(
      expect.objectContaining({ ceremony_id: 'ceremony-1', name: 'Laptop' }),
    )
  })

  it('clears a failed passkey read once a later read succeeds', async () => {
    const create = vi.fn<(options?: unknown) => Promise<PublicKeyCredential | null>>(async () =>
      fakeAttestation(),
    )
    stubBrowser({ create })
    let offline = true
    let passkeys: unknown[] = []
    fakeApi({
      ...settings(),
      'GET /auth/passkeys': () =>
        offline
          ? { status: 500, body: { error: 'internal', message: 'the server is not answering' } }
          : { body: passkeys },
      'POST /auth/passkeys/register/start': {
        body: {
          ceremony_id: 'ceremony-1',
          options: {
            publicKey: {
              challenge: 'Y2hhbGxlbmdl',
              rp: { name: 'Saffron', id: 'localhost' },
              user: { id: 'dXNlci0x', name: 'las', displayName: 'Laszlo' },
              pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
            },
          },
        },
      },
      'POST /auth/passkeys/register/finish': () => {
        offline = false
        passkeys = [
          { id: 7, name: 'Laptop', created_at: '2026-11-01T10:00:00Z', last_used_at: null },
        ]
        return { body: passkeys[0] }
      },
    })
    renderApp('/settings')
    const section = within(await screen.findByRole('region', { name: 'Passkeys' }))

    expect(await section.findByText('the server is not answering')).toBeInTheDocument()
    await userEvent.type(await section.findByLabelText('Name'), 'Laptop')
    await userEvent.type(section.getByLabelText('Password'), 'correct horse battery')
    await userEvent.click(section.getByRole('button', { name: 'Add a passkey' }))

    expect(await section.findByText('Passkey added.')).toBeInTheDocument()
    expect(section.queryByText('the server is not answering')).not.toBeInTheDocument()
    expect(await section.findByText('Laptop')).toBeInTheDocument()
  })

  it('lists passkeys and removes one after confirming', async () => {
    let passkeys = [
      { id: 7, name: 'Laptop', created_at: '2026-11-01T10:00:00Z', last_used_at: null },
    ]
    const calls = fakeApi({
      ...settings(),
      'GET /auth/passkeys': () => ({ body: passkeys }),
      'DELETE /auth/passkeys/7': () => {
        passkeys = []
        return { status: 204 }
      },
    })
    renderApp('/settings')
    const section = within(await screen.findByRole('region', { name: 'Passkeys' }))

    expect(await section.findByText('Laptop')).toBeInTheDocument()
    expect(section.getByText(/never used/)).toBeInTheDocument()
    await userEvent.click(section.getByRole('button', { name: 'Remove passkey Laptop' }))
    expect(section.getByRole('button', { name: 'Remove passkey Laptop' })).toHaveFocus()
    await userEvent.click(section.getByRole('button', { name: 'Remove passkey Laptop' }))

    expect(await section.findByText('Passkey removed.')).toBeInTheDocument()
    expect(section.getByRole('group', { name: 'Your passkeys' })).toHaveFocus()
    expect(calls.some((call) => call.method === 'DELETE' && call.path === '/auth/passkeys/7')).toBe(
      true,
    )
  })
})
