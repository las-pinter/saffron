/** Turning a failure into a line the interface can show. */

import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from './api'

/** Failures this client names itself; a server message stays as the server wrote it. */
const CLIENT_ERROR_KEYS: Record<string, string> = {
  network_error: 'errors.network',
  passkeys_unavailable: 'errors.passkeysUnavailable',
  passkey_cancelled: 'errors.passkeyCancelled',
}

/** The translation key for a failure this client names, or null for a server message. */
export function errorKey(error: unknown): string | null {
  if (!(error instanceof ApiError)) return null
  if (error.status === 401 && error.code === 'unauthenticated') return 'errors.sessionExpired'
  if (error.status !== 0) return null
  return CLIENT_ERROR_KEYS[error.code] ?? null
}

/** The server's own message when there is one; the caller supplies the fallback. */
function describeError(error: unknown, fallback: string): string {
  if (error instanceof ApiError) return error.message
  if (error instanceof Error && error.message) return error.message
  return fallback
}

/** The message for a failed call, translated where this client wrote it. */
export function useErrorMessage(): (error: unknown) => string {
  const { t } = useTranslation()

  return useCallback(
    (error: unknown) => {
      const key = errorKey(error)
      return key ? t(key) : describeError(error, t('common.error'))
    },
    [t],
  )
}
