/** Signing in and registering with a passkey, on top of the JSON API. */

import { ApiError, api, type Challenge, type PasskeyView, type UserView } from '@/lib/api'
import {
  assertionJson,
  attestationJson,
  creationOptions,
  requestOptions,
  type CeremonyResponse,
} from './webauthn'

function unsupported(): ApiError {
  return new ApiError(0, 'passkeys_unavailable', 'Passkeys are unavailable in this browser.')
}

function cancelled(): ApiError {
  return new ApiError(0, 'passkey_cancelled', 'The passkey prompt was cancelled.')
}

/** Runs a browser ceremony; a cancelled prompt or an empty answer is not a failure. */
async function ceremony<T>(call: () => Promise<T | null>): Promise<T> {
  try {
    const result = await call()
    if (!result) throw cancelled()
    return result
  } catch (error) {
    if (error instanceof DOMException && error.name === 'NotAllowedError') throw cancelled()
    throw error
  }
}

/** Signs in with any passkey this site holds; the browser names the user. */
export async function signInWithPasskey(): Promise<UserView> {
  if (!window.PublicKeyCredential) throw unsupported()
  const challenge = await api.post<Challenge<CeremonyResponse>>('/auth/passkey/login/start')
  const credential = await ceremony(() =>
    navigator.credentials.get({ publicKey: requestOptions(challenge.options.publicKey) }),
  )
  const reply = await api.post<{ user: UserView }>('/auth/passkey/login/finish', {
    ceremony_id: challenge.ceremony_id,
    credential: assertionJson(credential as PublicKeyCredential),
  })
  return reply.user
}

/** Registers a new passkey; the password and, while TOTP is on, a code are needed. */
export async function registerPasskey(
  name: string,
  password: string,
  code: string | undefined,
): Promise<PasskeyView> {
  if (!window.PublicKeyCredential) throw unsupported()
  const challenge = await api.post<Challenge<CeremonyResponse>>('/auth/passkeys/register/start', {
    password,
    ...(code ? { code } : {}),
  })
  const credential = await ceremony(() =>
    navigator.credentials.create({ publicKey: creationOptions(challenge.options.publicKey) }),
  )
  return api.post<PasskeyView>('/auth/passkeys/register/finish', {
    ceremony_id: challenge.ceremony_id,
    name,
    credential: attestationJson(credential as PublicKeyCredential),
  })
}
