/** Route guards: one for the signed-in application, one for the public screens. */

import { useTranslation } from 'react-i18next'
import { Navigate, Outlet } from 'react-router'
import { useAuth } from './context'

function Waiting() {
  const { t } = useTranslation()
  return (
    <p role="status" className="p-8 text-sf-text-muted">
      {t('common.loading')}
    </p>
  )
}

/** Sends a visitor to setup or sign-in before the application renders. */
export function RequireAuth() {
  const { loading, setupRequired, user } = useAuth()
  if (loading) return <Waiting />
  if (setupRequired) return <Navigate to="/setup" replace />
  if (!user) return <Navigate to="/login" replace />
  return <Outlet />
}

/** Keeps a signed-in user away from the sign-in screen. */
export function RequireGuest() {
  const { loading, setupRequired, user } = useAuth()
  if (loading) return <Waiting />
  if (setupRequired) return <Navigate to="/setup" replace />
  if (user) return <Navigate to="/" replace />
  return <Outlet />
}

/** The setup screen is reachable only while no user exists. */
export function RequireSetup() {
  const { loading, setupRequired, user } = useAuth()
  if (loading) return <Waiting />
  if (!setupRequired) return <Navigate to={user ? '/' : '/login'} replace />
  return <Outlet />
}
