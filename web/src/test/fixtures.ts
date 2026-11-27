import type { UserView } from '@/lib/api'

export const LAS: UserView = {
  id: 1,
  username: 'las',
  display_name: 'Laszlo',
  is_admin: true,
  disabled: false,
  totp_enabled: false,
}

export const BOB: UserView = {
  id: 2,
  username: 'bob',
  display_name: 'Bob',
  is_admin: false,
  disabled: false,
  totp_enabled: true,
}

/** A `GET /auth/state` reply for a signed-in user, or for nobody. */
export function signedIn(user: UserView | null) {
  return { body: { setup_required: false, user } }
}
