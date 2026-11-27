/** Who is signed in: loaded from the server once, kept in one place. */

import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import { api, setUnauthorizedHandler, type AuthState, type UserView } from '@/lib/api'
import { AuthContext, type AuthValue } from './context'

export function AuthProvider({ children }: { children: ReactNode }) {
  const [setupRequired, setSetupRequired] = useState(false)
  const [user, setUser] = useState<UserView | null>(null)
  const [loading, setLoading] = useState(true)

  const apply = useCallback((state: AuthState) => {
    setSetupRequired(state.setup_required)
    setUser(state.user)
  }, [])

  const refresh = useCallback(async () => {
    apply(await api.get<AuthState>('/auth/state'))
  }, [apply])

  useEffect(() => {
    let live = true
    api
      .get<AuthState>('/auth/state')
      .then((state) => {
        if (live) apply(state)
      })
      .catch(() => {
        // The sign-in screen still renders; its own call reports the problem.
      })
      .finally(() => {
        if (live) setLoading(false)
      })
    return () => {
      live = false
    }
  }, [apply])

  const signedIn = useCallback((next: UserView) => {
    setSetupRequired(false)
    setUser(next)
  }, [])

  // A session that expires mid-session sends the user back to the sign-in screen.
  useEffect(() => {
    setUnauthorizedHandler(() => setUser(null))
    return () => setUnauthorizedHandler(null)
  }, [])

  const signOut = useCallback(async () => {
    try {
      await api.post('/auth/logout')
    } catch {
      // The local sign-out matters more than the reply: the session may already be
      // gone, and the next call would end it anyway.
    } finally {
      setUser(null)
    }
  }, [])

  const value = useMemo<AuthValue>(
    () => ({ setupRequired, user, loading, refresh, signedIn, signOut }),
    [setupRequired, user, loading, refresh, signedIn, signOut],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}
