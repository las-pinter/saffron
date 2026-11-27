import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Card } from '@/components/Card'
import { ErrorText, NoticeText } from '@/components/Feedback'
import { Field } from '@/components/Field'
import { Button } from '@/components/ui/button'
import { api } from '@/lib/api'
import { useErrorMessage } from '@/lib/errors'

/** Changes the signed-in user's password; the server ends their other sessions. */
export function PasswordSection() {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [confirm, setConfirm] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [done, setDone] = useState(false)

  async function submit(event: FormEvent) {
    event.preventDefault()
    setError(null)
    setDone(false)
    if (next !== confirm) {
      setError(t('settings.password.mismatch'))
      return
    }
    setBusy(true)
    try {
      await api.post('/auth/password', { current_password: current, new_password: next })
      setCurrent('')
      setNext('')
      setConfirm('')
      setDone(true)
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title={t('settings.password.title')} description={t('settings.password.description')}>
      <form onSubmit={submit} className="space-y-4">
        <Field
          label={t('settings.password.current')}
          name="current_password"
          type="password"
          autoComplete="current-password"
          required
          value={current}
          onChange={(event) => setCurrent(event.target.value)}
        />
        <div className="grid gap-4 sm:grid-cols-2">
          <Field
            label={t('settings.password.new')}
            name="new_password"
            type="password"
            autoComplete="new-password"
            required
            hint={t('settings.password.newHint')}
            value={next}
            onChange={(event) => setNext(event.target.value)}
          />
          <Field
            label={t('settings.password.confirm')}
            name="confirm_password"
            type="password"
            autoComplete="new-password"
            required
            value={confirm}
            onChange={(event) => setConfirm(event.target.value)}
          />
        </div>
        {error ? <ErrorText>{error}</ErrorText> : null}
        {done ? <NoticeText>{t('settings.password.done')}</NoticeText> : null}
        <Button type="submit" className="h-10" disabled={busy}>
          {busy ? t('common.saving') : t('settings.password.submit')}
        </Button>
      </form>
    </Card>
  )
}
