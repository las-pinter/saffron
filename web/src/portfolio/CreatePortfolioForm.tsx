import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { ErrorText } from '@/components/Feedback'
import { Field } from '@/components/Field'
import { Button } from '@/components/ui/button'
import { useErrorMessage } from '@/lib/errors'
import { usePortfolio } from './context'

/** Creates a portfolio and selects it. */
export function CreatePortfolioForm({ onDone }: { onDone?: () => void }) {
  const { t } = useTranslation()
  const { create } = usePortfolio()
  const messageOf = useErrorMessage()
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function submit(event: FormEvent) {
    event.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await create(name.trim())
      setName('')
      onDone?.()
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form onSubmit={submit} className="space-y-3">
      <Field
        label={t('portfolio.name')}
        name="name"
        autoComplete="off"
        required
        value={name}
        onChange={(event) => setName(event.target.value)}
      />
      {error ? <ErrorText>{error}</ErrorText> : null}
      <Button type="submit" className="h-10" disabled={busy}>
        {busy ? t('common.saving') : t('portfolio.create')}
      </Button>
    </form>
  )
}
