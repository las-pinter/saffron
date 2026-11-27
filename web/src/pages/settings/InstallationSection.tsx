import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Card } from '@/components/Card'
import { ErrorText, NoticeText } from '@/components/Feedback'
import { CheckField } from '@/components/Field'
import { api, type SettingsView } from '@/lib/api'
import { useErrorMessage } from '@/lib/errors'

/** Installation-wide settings that administrators change. */
export function InstallationSection() {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const [settings, setSettings] = useState<SettingsView | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [done, setDone] = useState(false)

  useEffect(() => {
    let live = true
    api
      .get<SettingsView>('/settings')
      .then((value) => {
        if (live) setSettings(value)
      })
      .catch((failure: unknown) => {
        if (live) setError(messageOf(failure))
      })
    return () => {
      live = false
    }
  }, [messageOf])

  async function toggle(on: boolean) {
    setBusy(true)
    setError(null)
    setDone(false)
    try {
      setSettings(await api.patch<SettingsView>('/settings', { ft_fallback: on }))
      setDone(true)
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card
      title={t('settings.installation.title')}
      description={t('settings.installation.description')}
    >
      {settings === null && !error ? (
        <p role="status" className="text-sm text-sf-text-muted">
          {t('common.loading')}
        </p>
      ) : null}
      {settings ? (
        <div className="space-y-2">
          <CheckField
            label={t('settings.installation.ftFallback')}
            name="ft_fallback"
            aria-describedby="ft-fallback-hint"
            checked={settings.ft_fallback}
            disabled={busy}
            onChange={(event) => toggle(event.target.checked)}
          />
          <p id="ft-fallback-hint" className="text-xs text-sf-text-muted">
            {t('settings.installation.ftFallbackHint')}
          </p>
        </div>
      ) : null}
      {error ? <ErrorText>{error}</ErrorText> : null}
      {done ? (
        <div className="mt-3">
          <NoticeText>{t('settings.installation.saved')}</NoticeText>
        </div>
      ) : null}
    </Card>
  )
}
