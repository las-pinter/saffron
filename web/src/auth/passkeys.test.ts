import { afterEach, describe, expect, it, vi } from 'vitest'
import { fakeApi } from '@/test/fakeApi'
import { LAS } from '@/test/fixtures'
import { fakeAssertion, fakeAttestation, restoreBrowser, stubBrowser } from '@/test/browser'
import { registerPasskey, signInWithPasskey } from './passkeys'

/** The server sends the browser options wrapped in a `publicKey` field. */
function challenge(options: Record<string, unknown>) {
  return { body: { ceremony_id: 'ceremony-1', options: { publicKey: options } } }
}

afterEach(() => {
  restoreBrowser()
})

describe('passkeys', () => {
  it('signs in with the challenge from the publicKey envelope', async () => {
    const get = vi.fn<(options?: unknown) => Promise<PublicKeyCredential | null>>(async () =>
      fakeAssertion(),
    )
    stubBrowser({ get })
    const calls = fakeApi({
      'POST /auth/passkey/login/start': challenge({
        challenge: 'Y2hhbGxlbmdl',
        rpId: 'localhost',
        allowCredentials: [{ id: 'Y3JlZGVudGlhbC0x', type: 'public-key' }],
      }),
      'POST /auth/passkey/login/finish': { body: { user: LAS } },
    })

    const user = await signInWithPasskey()

    expect(user).toEqual(LAS)
    const request = get.mock.calls[0]?.[0] as {
      publicKey: { challenge: ArrayBuffer; rpId: string; allowCredentials: { id: ArrayBuffer }[] }
    }
    expect(new TextDecoder().decode(request.publicKey.challenge)).toBe('challenge')
    expect(request.publicKey.rpId).toBe('localhost')
    expect(request.publicKey.allowCredentials[0]?.id.byteLength).toBeGreaterThan(0)
    expect(calls.at(-1)?.body).toMatchObject({
      ceremony_id: 'ceremony-1',
      credential: { id: 'credential-1', response: { signature: 'c2lnbmF0dXJl' } },
    })
  })

  it('registers a passkey with the user and relying party decoded', async () => {
    const create = vi.fn<(options?: unknown) => Promise<PublicKeyCredential | null>>(async () =>
      fakeAttestation(),
    )
    stubBrowser({ create })
    const calls = fakeApi({
      'POST /auth/passkeys/register/start': challenge({
        challenge: 'Y2hhbGxlbmdl',
        rp: { name: 'Saffron', id: 'localhost' },
        user: { id: 'dXNlci0x', name: 'las', displayName: 'Laszlo' },
        pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
      }),
      'POST /auth/passkeys/register/finish': {
        body: { id: 7, name: 'Laptop', created_at: '2026-11-01T10:00:00Z', last_used_at: null },
      },
    })

    const added = await registerPasskey('Laptop', 'correct horse battery', undefined)

    expect(added.id).toBe(7)
    const options = create.mock.calls[0]?.[0] as {
      publicKey: { user: { id: ArrayBuffer }; rp: { name: string } }
    }
    expect(new TextDecoder().decode(options.publicKey.user.id)).toBe('user-1')
    expect(options.publicKey.rp.name).toBe('Saffron')
    expect(calls.at(-1)?.body).toMatchObject({
      ceremony_id: 'ceremony-1',
      name: 'Laptop',
      credential: { response: { attestationObject: 'YXR0ZXN0YXRpb24', transports: ['internal'] } },
    })
  })

  it('reports a cancelled prompt instead of a missing browser', async () => {
    const get = vi.fn<(options?: unknown) => Promise<PublicKeyCredential | null>>(async () => {
      throw new DOMException('the user cancelled', 'NotAllowedError')
    })
    stubBrowser({ get })
    fakeApi({
      'POST /auth/passkey/login/start': challenge({ challenge: 'Y2hhbGxlbmdl' }),
    })

    await expect(signInWithPasskey()).rejects.toMatchObject({ code: 'passkey_cancelled' })
  })

  it('reports an empty answer as a cancelled prompt', async () => {
    const get = vi.fn<(options?: unknown) => Promise<PublicKeyCredential | null>>(async () => null)
    stubBrowser({ get })
    fakeApi({
      'POST /auth/passkey/login/start': challenge({ challenge: 'Y2hhbGxlbmdl' }),
    })

    await expect(signInWithPasskey()).rejects.toMatchObject({ code: 'passkey_cancelled' })
  })
})
