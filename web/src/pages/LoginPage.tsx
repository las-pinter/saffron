import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { useAuth } from '@/auth/context'
import { signInWithPasskey } from '@/auth/passkeys'
import { ErrorText } from '@/components/Feedback'
import { Field } from '@/components/Field'
import { PublicHeader } from '@/components/Logo'
import { Button } from '@/components/ui/button'
import { ApiError, api, type UserView } from '@/lib/api'
import { useErrorMessage } from '@/lib/errors'

/** Sign in with a password, a TOTP code when the account asks for one, or a passkey. */
export function LoginPage() {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const { signedIn } = useAuth()
  const navigate = useNavigate()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [needsCode, setNeedsCode] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function submit(event: FormEvent) {
    event.preventDefault()
    setBusy(true)
    setError(null)
    try {
      const reply = await api.post<{ user: UserView }>('/auth/login', {
        username,
        password,
        ...(code ? { totp_code: code } : {}),
      })
      signedIn(reply.user)
      navigate('/', { replace: true })
    } catch (failure) {
      if (failure instanceof ApiError && failure.code === 'totp_required') {
        setNeedsCode(true)
        setError(t('login.codeNeeded'))
      } else {
        setError(messageOf(failure))
      }
    } finally {
      setBusy(false)
    }
  }

  async function passkey() {
    setBusy(true)
    setError(null)
    try {
      const user = await signInWithPasskey()
      signedIn(user)
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
        <PublicHeader title={t('login.title')} subtitle={t('login.subtitle')} />
        <form
          onSubmit={submit}
          className="mt-8 space-y-4 rounded-2xl border border-border bg-sf-surface p-6"
        >
          <Field
            label={t('login.username')}
            name="username"
            spellCheck={false}
            autoComplete="username"
            required
            value={username}
            onChange={(event) => setUsername(event.target.value)}
          />
          <Field
            label={t('login.password')}
            name="password"
            type="password"
            autoComplete="current-password"
            required
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
          {needsCode ? (
            <Field
              label={t('login.code')}
              name="totp_code"
              spellCheck={false}
              autoComplete="one-time-code"
              inputMode="numeric"
              pattern="[0-9]*"
              hint={t('login.codeHint')}
              required
              value={code}
              onChange={(event) => setCode(event.target.value)}
            />
          ) : null}
          {error ? <ErrorText>{error}</ErrorText> : null}
          <Button type="submit" className="h-10 w-full" disabled={busy}>
            {busy ? t('login.submitting') : t('login.submit')}
          </Button>
        </form>
        <div className="mt-6 flex items-center gap-3 text-xs text-sf-text-muted">
          <span className="h-px flex-1 bg-sf-divider" />
          {t('login.or')}
          <span className="h-px flex-1 bg-sf-divider" />
        </div>
        <Button
          type="button"
          variant="outline"
          className="mt-4 h-10 w-full"
          disabled={busy}
          onClick={passkey}
        >
          {t('login.passkey')}
        </Button>
      </div>
    </main>
  )
}
