/** The JSON API client: one fetch wrapper, one error type, no hidden state. */

export class ApiError extends Error {
  readonly status: number
  readonly code: string

  constructor(status: number, code: string, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
  }
}

export interface UserView {
  id: number
  username: string
  display_name: string
  is_admin: boolean
  disabled: boolean
  totp_enabled: boolean
}

export interface AuthState {
  setup_required: boolean
  user: UserView | null
}

export interface PasskeyView {
  id: number
  name: string
  created_at: string
  last_used_at: string | null
}

export interface TotpOffer {
  secret: string
  otpauth_url: string
}

export interface SettingsView {
  ft_fallback: boolean
}

export interface Challenge<T> {
  ceremony_id: string
  options: T
}

type Method = 'GET' | 'POST' | 'PATCH' | 'PUT' | 'DELETE'

interface ErrorBody {
  error?: string
  message?: string
}

type UnauthorizedHandler = () => void
let onUnauthorized: UnauthorizedHandler | null = null

/**
 * Registers what happens when the server says the session is gone, so one place can
 * return the user to the sign-in screen. Passing null removes the handler.
 */
export function setUnauthorizedHandler(handler: UnauthorizedHandler | null) {
  onUnauthorized = handler
}

async function request<T>(method: Method, path: string, body?: unknown): Promise<T> {
  let response: Response
  try {
    response = await fetch(`/api${path}`, {
      method,
      credentials: 'same-origin',
      headers: body === undefined ? undefined : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
  } catch {
    throw new ApiError(0, 'network_error', 'The server cannot be reached.')
  }
  if (response.status === 204) {
    return undefined as T
  }
  const data = (await response.json().catch(() => null)) as ErrorBody | T | null
  if (!response.ok) {
    const error = data as ErrorBody | null
    const failure = new ApiError(
      response.status,
      error?.error ?? 'error',
      error?.message ?? response.statusText,
    )
    if (failure.status === 401 && failure.code === 'unauthenticated') onUnauthorized?.()
    throw failure
  }
  return data as T
}

export const api = {
  get: <T>(path: string) => request<T>('GET', path),
  post: <T>(path: string, body?: unknown) => request<T>('POST', path, body),
  patch: <T>(path: string, body: unknown) => request<T>('PATCH', path, body),
  put: <T>(path: string, body: unknown) => request<T>('PUT', path, body),
  delete: <T = void>(path: string) => request<T>('DELETE', path),
}
