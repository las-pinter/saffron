import { describe, expect, it } from 'vitest'
import { ApiError } from './api'
import { errorKey } from './errors'

describe('errorKey', () => {
  it('names a message this client wrote', () => {
    expect(errorKey(new ApiError(0, 'network_error', 'x'))).toBe('errors.network')
    expect(errorKey(new ApiError(0, 'passkeys_unavailable', 'x'))).toBe(
      'errors.passkeysUnavailable',
    )
    expect(errorKey(new ApiError(0, 'passkey_cancelled', 'x'))).toBe('errors.passkeyCancelled')
  })

  it('leaves a server message to the server', () => {
    expect(
      errorKey(new ApiError(503, 'passkeys_unavailable', 'passkeys need SAFFRON_PUBLIC_URL')),
    ).toBeNull()
    expect(errorKey(new ApiError(400, 'invalid_password', 'too short'))).toBeNull()
  })

  it('names an expired session', () => {
    expect(errorKey(new ApiError(401, 'unauthenticated', 'sign in'))).toBe('errors.sessionExpired')
  })
})
