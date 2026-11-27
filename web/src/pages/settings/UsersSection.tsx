import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Card } from '@/components/Card'
import { ConfirmButton } from '@/components/ConfirmButton'
import { ErrorText, NoticeText } from '@/components/Feedback'
import { CheckField, Field } from '@/components/Field'
import { Button } from '@/components/ui/button'
import { api, type UserView } from '@/lib/api'
import { useErrorMessage } from '@/lib/errors'

interface UsersSectionProps {
  currentUserId: number
}

/** Administrators list, create, disable, and reset other users. */
export function UsersSection({ currentUserId }: UsersSectionProps) {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const [users, setUsers] = useState<UserView[] | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [username, setUsername] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [password, setPassword] = useState('')
  const [isAdmin, setIsAdmin] = useState(false)
  const [resetting, setResetting] = useState<number | null>(null)
  const [newPassword, setNewPassword] = useState('')
  // A list read that started before a change must not overwrite the newer state.
  const latest = useRef(0)
  // Where focus goes when a row action unmounts the control that had it.
  const rows = useRef<HTMLUListElement>(null)

  const load = useCallback(async () => {
    const generation = ++latest.current
    const list = await api.get<UserView[]>('/users')
    if (latest.current === generation) {
      setLoadError(null)
      setUsers(list)
    }
  }, [])

  useEffect(() => {
    let live = true
    const generation = ++latest.current
    api
      .get<UserView[]>('/users')
      .then((list) => {
        if (live && latest.current === generation) {
          setLoadError(null)
          setUsers(list)
        }
      })
      .catch((failure: unknown) => {
        if (live && latest.current === generation) setLoadError(messageOf(failure))
      })
    return () => {
      live = false
    }
  }, [messageOf])

  async function act(action: () => Promise<void>, message: string): Promise<boolean> {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      await action()
      await load()
      setNotice(message)
      return true
    } catch (failure) {
      setError(messageOf(failure))
      return false
    } finally {
      setBusy(false)
    }
  }

  async function create(event: FormEvent) {
    event.preventDefault()
    await act(async () => {
      await api.post('/users', {
        username,
        display_name: displayName,
        password,
        is_admin: isAdmin,
      })
      setUsername('')
      setDisplayName('')
      setPassword('')
      setIsAdmin(false)
    }, t('settings.users.created'))
  }

  async function resetPassword(event: FormEvent, id: number) {
    event.preventDefault()
    const done = await act(async () => {
      await api.post(`/users/${id}/password`, { password: newPassword })
      setResetting(null)
      setNewPassword('')
    }, t('settings.users.passwordReset'))
    // The form that had focus unmounts; keep a keyboard user in the section.
    if (done) rows.current?.focus()
  }

  async function resetTotp(user: UserView) {
    const done = await act(
      () => api.post(`/users/${user.id}/totp/reset`),
      t('settings.users.totpReset'),
    )
    if (done) rows.current?.focus()
  }

  return (
    <Card title={t('settings.users.title')} description={t('settings.users.description')}>
      {loadError ? <ErrorText>{loadError}</ErrorText> : null}
      {users === null && !loadError ? (
        <p role="status" className="text-sm text-sf-text-muted">
          {t('common.loading')}
        </p>
      ) : null}
      {users ? (
        <ul ref={rows} tabIndex={-1} className="divide-y divide-sf-divider focus:outline-none">
          {users.map((user) => (
            <li key={user.id} className="py-3">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="flex flex-wrap items-center gap-2 font-medium break-words">
                    {user.display_name}
                    {user.is_admin ? (
                      <Badge tone="accent">{t('settings.users.admin')}</Badge>
                    ) : null}
                    {user.disabled ? (
                      <Badge tone="loss">{t('settings.users.disabled')}</Badge>
                    ) : null}
                    {user.totp_enabled ? (
                      <Badge tone="gain">{t('settings.users.totp')}</Badge>
                    ) : null}
                  </p>
                  <p className="text-xs break-all text-sf-text-muted">@{user.username}</p>
                </div>
                {user.id === currentUserId ? (
                  <span className="text-xs text-sf-text-muted">{t('settings.users.you')}</span>
                ) : (
                  <div className="flex flex-wrap gap-1">
                    <Button
                      type="button"
                      variant="ghost"
                      disabled={busy}
                      aria-label={
                        user.disabled
                          ? t('settings.users.enableNamed', { name: user.display_name })
                          : t('settings.users.disableNamed', { name: user.display_name })
                      }
                      onClick={() =>
                        act(
                          () =>
                            api.post(`/users/${user.id}/${user.disabled ? 'enable' : 'disable'}`),
                          t('settings.users.updated'),
                        )
                      }
                    >
                      {user.disabled ? t('settings.users.enable') : t('settings.users.disable')}
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      disabled={busy}
                      aria-label={t('settings.users.resetPasswordNamed', {
                        name: user.display_name,
                      })}
                      onClick={() => {
                        setResetting(resetting === user.id ? null : user.id)
                        setNewPassword('')
                      }}
                    >
                      {t('settings.users.resetPassword')}
                    </Button>
                    {user.totp_enabled ? (
                      <ConfirmButton
                        label={t('settings.users.resetTotp')}
                        confirmLabel={t('settings.users.confirmResetTotp')}
                        labelFor={t('settings.users.resetTotpNamed', { name: user.display_name })}
                        confirmLabelFor={t('settings.users.confirmResetTotpNamed', {
                          name: user.display_name,
                        })}
                        busy={busy}
                        onConfirm={() => resetTotp(user)}
                      />
                    ) : null}
                  </div>
                )}
              </div>
              {resetting === user.id ? (
                <form
                  onSubmit={(event) => resetPassword(event, user.id)}
                  className="mt-3 flex flex-wrap items-end gap-3 rounded-lg bg-sf-surface-sunken p-3"
                >
                  <div className="min-w-0 flex-1">
                    <Field
                      label={t('settings.users.newPassword')}
                      name="new_password"
                      type="password"
                      autoComplete="new-password"
                      required
                      value={newPassword}
                      onChange={(event) => setNewPassword(event.target.value)}
                    />
                  </div>
                  <div className="flex gap-1">
                    <Button type="submit" disabled={busy}>
                      {t('settings.users.setPassword')}
                    </Button>
                    <Button type="button" variant="ghost" onClick={() => setResetting(null)}>
                      {t('common.cancel')}
                    </Button>
                  </div>
                </form>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}

      <form onSubmit={create} className="mt-5 space-y-4 border-t border-sf-divider pt-5">
        <div className="grid gap-4 sm:grid-cols-2">
          <Field
            label={t('settings.users.username')}
            name="username"
            spellCheck={false}
            autoComplete="off"
            required
            value={username}
            onChange={(event) => setUsername(event.target.value)}
          />
          <Field
            label={t('settings.users.displayName')}
            name="display_name"
            autoComplete="off"
            required
            value={displayName}
            onChange={(event) => setDisplayName(event.target.value)}
          />
        </div>
        <Field
          label={t('settings.users.password')}
          name="password"
          type="password"
          autoComplete="new-password"
          required
          hint={t('settings.users.passwordHint')}
          value={password}
          onChange={(event) => setPassword(event.target.value)}
        />
        <CheckField
          label={t('settings.users.admin')}
          name="is_admin"
          checked={isAdmin}
          onChange={(event) => setIsAdmin(event.target.checked)}
        />
        {error ? <ErrorText>{error}</ErrorText> : null}
        {notice ? <NoticeText>{notice}</NoticeText> : null}
        <Button type="submit" className="h-10" disabled={busy}>
          {busy ? t('common.saving') : t('settings.users.create')}
        </Button>
      </form>
    </Card>
  )
}

function Badge({ tone, children }: { tone: 'accent' | 'loss' | 'gain'; children: string }) {
  const tones = {
    accent: 'bg-sf-accent-soft text-sf-accent-on-soft',
    loss: 'bg-sf-loss-soft text-sf-loss',
    gain: 'bg-sf-gain-soft text-sf-gain',
  } as const

  return (
    <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${tones[tone]}`}>
      {children}
    </span>
  )
}
