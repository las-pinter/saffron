import { useEffect, useRef, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useAuth } from '@/auth/context'
import { registerPasskey } from '@/auth/passkeys'
import { Card } from '@/components/Card'
import { ConfirmButton } from '@/components/ConfirmButton'
import { ErrorText, NoticeText } from '@/components/Feedback'
import { Field } from '@/components/Field'
import { Button } from '@/components/ui/button'
import { api, type PasskeyView } from '@/lib/api'
import { useErrorMessage } from '@/lib/errors'
import { formatDateTime } from '@/lib/format'

/** Lists the signed-in user's passkeys and registers or removes one. */
export function PasskeysSection() {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const { user } = useAuth()
  const [passkeys, setPasskeys] = useState<PasskeyView[] | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [name, setName] = useState('')
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  // A list read that started before a change must not overwrite the newer state.
  const latest = useRef(0)
  // Where focus goes after the removed row disappears.
  const list = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let live = true
    const generation = ++latest.current
    api
      .get<PasskeyView[]>('/auth/passkeys')
      .then((value) => {
        if (live && latest.current === generation) {
          setLoadError(null)
          setPasskeys(value)
        }
      })
      .catch((failure: unknown) => {
        if (live && latest.current === generation) setLoadError(messageOf(failure))
      })
    return () => {
      live = false
    }
  }, [messageOf])

  /** Re-reads the list after a change, so the server stays the one source of it. */
  async function load() {
    const generation = ++latest.current
    const value = await api.get<PasskeyView[]>('/auth/passkeys')
    if (latest.current === generation) {
      setLoadError(null)
      setPasskeys(value)
    }
  }

  async function register(event: FormEvent) {
    event.preventDefault()
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      await registerPasskey(name, password, code || undefined)
      await load()
      setName('')
      setPassword('')
      setCode('')
      setNotice(t('settings.passkeys.registered'))
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  async function remove(id: number) {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      await api.delete(`/auth/passkeys/${id}`)
      await load()
      setNotice(t('settings.passkeys.removed'))
      list.current?.focus()
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title={t('settings.passkeys.title')} description={t('settings.passkeys.description')}>
      <div
        ref={list}
        role="group"
        aria-label={t('settings.passkeys.list')}
        tabIndex={-1}
        className="focus:outline-none"
      >
        {loadError ? <ErrorText>{loadError}</ErrorText> : null}
        {passkeys === null && !loadError ? (
          <p role="status" className="text-sm text-sf-text-muted">
            {t('common.loading')}
          </p>
        ) : null}
        {passkeys?.length === 0 ? (
          <p className="text-sm text-sf-text-muted">{t('settings.passkeys.empty')}</p>
        ) : null}
        {passkeys && passkeys.length > 0 ? (
          <ul className="divide-y divide-sf-divider">
            {passkeys.map((passkey) => (
              <li
                key={passkey.id}
                className="flex flex-wrap items-center justify-between gap-3 py-3"
              >
                <div className="min-w-0">
                  <p className="font-medium break-words">{passkey.name}</p>
                  <p className="text-xs text-sf-text-muted">
                    {t('settings.passkeys.created', {
                      when: formatDateTime(passkey.created_at),
                    })}{' '}
                    ·{' '}
                    {passkey.last_used_at
                      ? t('settings.passkeys.lastUsed', {
                          when: formatDateTime(passkey.last_used_at),
                        })
                      : t('settings.passkeys.never')}
                  </p>
                </div>
                <ConfirmButton
                  label={t('common.remove')}
                  confirmLabel={t('settings.passkeys.confirmRemove')}
                  labelFor={t('settings.passkeys.removeNamed', { name: passkey.name })}
                  confirmLabelFor={t('settings.passkeys.confirmRemoveNamed', {
                    name: passkey.name,
                  })}
                  busy={busy}
                  onConfirm={() => remove(passkey.id)}
                />
              </li>
            ))}
          </ul>
        ) : null}
      </div>

      <form onSubmit={register} className="mt-5 space-y-4 border-t border-sf-divider pt-5">
        <div className="grid gap-4 sm:grid-cols-2">
          <Field
            label={t('settings.passkeys.name')}
            name="name"
            required
            autoComplete="off"
            hint={t('settings.passkeys.nameHint')}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <Field
            label={t('settings.passkeys.password')}
            name="password"
            type="password"
            autoComplete="current-password"
            required
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
        </div>
        {user?.totp_enabled ? (
          <Field
            label={t('settings.passkeys.code')}
            name="code"
            spellCheck={false}
            autoComplete="one-time-code"
            inputMode="numeric"
            pattern="[0-9]*"
            required
            value={code}
            onChange={(event) => setCode(event.target.value)}
          />
        ) : null}
        {error ? <ErrorText>{error}</ErrorText> : null}
        {notice ? <NoticeText>{notice}</NoticeText> : null}
        <Button type="submit" className="h-10" disabled={busy}>
          {busy ? t('common.saving') : t('settings.passkeys.register')}
        </Button>
      </form>
    </Card>
  )
}
