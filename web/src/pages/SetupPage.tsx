import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { useAuth } from '@/auth/context'
import { ErrorText } from '@/components/Feedback'
import { Field } from '@/components/Field'
import { PublicHeader } from '@/components/Logo'
import { Button } from '@/components/ui/button'
import { api, type UserView } from '@/lib/api'
import { useErrorMessage } from '@/lib/errors'

/** The token from the link the server prints; the fragment keeps it out of request logs. */
function tokenFromLocation(): string {
  const hash = new URLSearchParams(window.location.hash.replace(/^#/, ''))
  return hash.get('token') ?? ''
}

/** First-run setup: creates the first administrator from the one-time link. */
export function SetupPage() {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const { signedIn } = useAuth()
  const navigate = useNavigate()
  const [token, setToken] = useState(tokenFromLocation)
  const [username, setUsername] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function submit(event: FormEvent) {
    event.preventDefault()
    setError(null)
    if (password !== confirm) {
      setError(t('setup.passwordMismatch'))
      return
    }
    setBusy(true)
    try {
      const reply = await api.post<{ user: UserView }>('/auth/setup', {
        token,
        username,
        display_name: displayName,
        password,
      })
      signedIn(reply.user)
      navigate('/', { replace: true })
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  return (
    <main className="flex min-h-screen justify-center px-5 py-12">
      <div className="w-full max-w-sm">
        <PublicHeader title={t('setup.title')} subtitle={t('setup.subtitle')} />
        <form
          onSubmit={submit}
          className="mt-8 space-y-4 rounded-2xl border border-border bg-sf-surface p-6"
        >
          <Field
            label={t('setup.token')}
            name="token"
            spellCheck={false}
            required
            autoComplete="off"
            hint={t('setup.tokenHint')}
            value={token}
            onChange={(event) => setToken(event.target.value)}
          />
          <Field
            label={t('setup.username')}
            name="username"
            spellCheck={false}
            autoComplete="username"
            required
            value={username}
            onChange={(event) => setUsername(event.target.value)}
          />
          <Field
            label={t('setup.displayName')}
            name="display_name"
            autoComplete="name"
            required
            value={displayName}
            onChange={(event) => setDisplayName(event.target.value)}
          />
          <Field
            label={t('setup.password')}
            name="password"
            type="password"
            autoComplete="new-password"
            required
            hint={t('setup.passwordHint')}
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
          <Field
            label={t('setup.confirm')}
            name="confirm"
            type="password"
            autoComplete="new-password"
            required
            value={confirm}
            onChange={(event) => setConfirm(event.target.value)}
          />
          {error ? <ErrorText>{error}</ErrorText> : null}
          <Button type="submit" className="h-10 w-full" disabled={busy}>
            {busy ? t('setup.submitting') : t('setup.submit')}
          </Button>
        </form>
      </div>
    </main>
  )
}
