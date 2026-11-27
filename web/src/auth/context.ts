/** The auth context and its hook, apart from the provider so fast refresh works. */

import { createContext, useContext } from 'react'
import type { UserView } from '@/lib/api'

export interface AuthValue {
  /** No user exists yet, so the setup screen is the only way in. */
  setupRequired: boolean
  /** The signed-in user, or null. */
  user: UserView | null
  /** Whether the first state read has finished. */
  loading: boolean
  /** Re-reads the server's view of the session. */
  refresh: () => Promise<void>
  /** Records the user a sign-in or setup call returned. */
  signedIn: (user: UserView) => void
  /** Ends the session and forgets the user. */
  signOut: () => Promise<void>
}

export const AuthContext = createContext<AuthValue | null>(null)

export function useAuth(): AuthValue {
  const value = useContext(AuthContext)
  if (!value) throw new Error('useAuth is used outside AuthProvider')
  return value
}
