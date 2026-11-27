import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useAuth } from '@/auth/context'
import { Card } from '@/components/Card'
import { ErrorText, NoticeText } from '@/components/Feedback'
import { Field } from '@/components/Field'
import { Button } from '@/components/ui/button'
import { api, type TotpOffer } from '@/lib/api'
import { useErrorMessage } from '@/lib/errors'

/** Enrols, confirms, and turns off the second factor of password sign-in. */
export function TotpSection() {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const { user, refresh } = useAuth()
  const enabled = user?.totp_enabled ?? false
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [offer, setOffer] = useState<TotpOffer | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<'enabled' | 'disabled' | null>(null)

  async function run(action: () => Promise<void>) {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      await action()
      await refresh()
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  async function start(event: FormEvent) {
    event.preventDefault()
    await run(async () => {
      setOffer(await api.post<TotpOffer>('/auth/totp/setup', { password }))
      setPassword('')
    })
  }

  async function confirm(event: FormEvent) {
    event.preventDefault()
    await run(async () => {
      await api.post('/auth/totp/confirm', { code })
      setOffer(null)
      setCode('')
      setNotice('enabled')
    })
  }

  async function disable(event: FormEvent) {
    event.preventDefault()
    await run(async () => {
      await api.post('/auth/totp/disable', { password, ...(code ? { code } : {}) })
      setPassword('')
      setCode('')
      setNotice('disabled')
    })
  }

  return (
    <Card title={t('settings.totp.title')} description={t('settings.totp.description')}>
      {notice ? (
        <div className="mb-4">
          <NoticeText>
            {t(notice === 'enabled' ? 'settings.totp.enabled' : 'settings.totp.disabled')}
          </NoticeText>
        </div>
      ) : null}
      {enabled ? (
        <form onSubmit={disable} className="space-y-4">
          <p className="text-sm text-sf-gain">● {t('settings.totp.on')}</p>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field
              label={t('settings.totp.password')}
              name="password"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(event) => setPassword(event.target.value)}
            />
            <Field
              label={t('settings.totp.code')}
              name="code"
              spellCheck={false}
              autoComplete="one-time-code"
              inputMode="numeric"
              pattern="[0-9]*"
              required
              value={code}
              onChange={(event) => setCode(event.target.value)}
            />
          </div>
          {error ? <ErrorText>{error}</ErrorText> : null}
          <Button type="submit" variant="destructive" className="h-10" disabled={busy}>
            {t('settings.totp.turnOff')}
          </Button>
        </form>
      ) : offer ? (
        <form onSubmit={confirm} className="space-y-4">
          <p className="text-sm text-sf-text-muted">{t('settings.totp.secretHint')}</p>
          <code className="block rounded-lg border border-sf-border bg-sf-surface-sunken px-3 py-2 font-mono text-sm break-all">
            {offer.secret}
          </code>
          <a href={offer.otpauth_url} className="inline-flex min-h-11 items-center text-sm">
            {t('settings.totp.openApp')}
          </a>
          <Field
            label={t('settings.totp.code')}
            name="code"
            spellCheck={false}
            autoComplete="one-time-code"
            inputMode="numeric"
            pattern="[0-9]*"
            required
            hint={t('settings.totp.codeHint')}
            value={code}
            onChange={(event) => setCode(event.target.value)}
          />
          {error ? <ErrorText>{error}</ErrorText> : null}
          <div className="flex flex-wrap gap-2">
            <Button type="submit" className="h-10" disabled={busy}>
              {t('settings.totp.confirm')}
            </Button>
            <Button
              type="button"
              variant="ghost"
              className="h-10"
              disabled={busy}
              onClick={() => {
                setOffer(null)
                setCode('')
                setError(null)
              }}
            >
              {t('common.cancel')}
            </Button>
          </div>
        </form>
      ) : (
        <form onSubmit={start} className="space-y-4">
          <p className="text-sm text-sf-text-muted">{t('settings.totp.off')}</p>
          <Field
            label={t('settings.totp.password')}
            name="password"
            type="password"
            autoComplete="current-password"
            required
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
          {error ? <ErrorText>{error}</ErrorText> : null}
          <Button type="submit" className="h-10" disabled={busy}>
            {t('settings.totp.start')}
          </Button>
        </form>
      )}
    </Card>
  )
}
