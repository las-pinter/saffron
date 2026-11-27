import { useTranslation } from 'react-i18next'
import { useAuth } from '@/auth/context'
import { InstallationSection } from './settings/InstallationSection'
import { PasskeysSection } from './settings/PasskeysSection'
import { PasswordSection } from './settings/PasswordSection'
import { TotpSection } from './settings/TotpSection'
import { UsersSection } from './settings/UsersSection'

/** Account security for everyone, plus user and installation administration. */
export function SettingsPage() {
  const { t } = useTranslation()
  const { user } = useAuth()
  if (!user) return null

  return (
    <div className="space-y-6">
      <header>
        <h1 className="font-heading text-3xl font-semibold tracking-tight">
          {t('settings.title')}
        </h1>
        <p className="mt-1 text-sm text-sf-text-muted">
          {t('settings.subtitle', { name: user.display_name })}
        </p>
      </header>
      <PasswordSection />
      <TotpSection />
      <PasskeysSection />
      {user.is_admin ? <UsersSection currentUserId={user.id} /> : null}
      {user.is_admin ? <InstallationSection /> : null}
    </div>
  )
}
