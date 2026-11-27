import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Card } from '@/components/Card'
import { ErrorText, NoticeText } from '@/components/Feedback'
import { CheckField, Field, SelectField } from '@/components/Field'
import { Button } from '@/components/ui/button'
import { api } from '@/lib/api'
import { ACCOUNT_KINDS, type Account } from '@/lib/book'
import { useErrorMessage } from '@/lib/errors'

interface AccountsSectionProps {
  portfolioId: number
  baseCurrency: string
  accounts: Account[] | null
  readOnly: boolean
  reload: () => Promise<void>
}

/** The portfolio's accounts: create, rename, close, and reopen them. */
export function AccountsSection({
  portfolioId,
  baseCurrency,
  accounts,
  readOnly,
  reload,
}: AccountsSectionProps) {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const [showClosed, setShowClosed] = useState(false)
  const [adding, setAdding] = useState(false)
  const [name, setName] = useState('')
  const [kind, setKind] = useState<string>('book_entry')
  const [currency, setCurrency] = useState('')
  const [institution, setInstitution] = useState('')
  const [renaming, setRenaming] = useState<number | null>(null)
  const [renameTo, setRenameTo] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  async function act(action: () => Promise<void>, message: string) {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      await action()
      await reload()
      setNotice(message)
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  async function create(event: FormEvent) {
    event.preventDefault()
    await act(async () => {
      await api.post(`/portfolios/${portfolioId}/accounts`, {
        name,
        kind,
        ...(currency.trim() ? { currency: currency.trim().toUpperCase() } : {}),
        ...(institution.trim() ? { institution: institution.trim() } : {}),
      })
      setName('')
      setCurrency('')
      setInstitution('')
      setAdding(false)
    }, t('holdings.accounts.created'))
  }

  async function rename(event: FormEvent, account: Account) {
    event.preventDefault()
    await act(
      () =>
        api.patch(`/portfolios/${portfolioId}/accounts/${account.id}`, {
          name: renameTo,
        }),
      t('holdings.accounts.renamed'),
    )
    setRenaming(null)
  }

  const open = (accounts ?? []).filter((account) => account.closed_at === null)
  const closed = (accounts ?? []).filter((account) => account.closed_at !== null)

  function row(account: Account) {
    return (
      <li key={account.id} className="py-3">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <p className="flex flex-wrap items-center gap-2 font-medium break-words">
              {account.name}
              {account.closed_at !== null ? (
                <span className="rounded-full bg-sf-loss-soft px-2 py-0.5 text-xs font-medium text-sf-loss">
                  {t('holdings.accounts.closed')}
                </span>
              ) : null}
            </p>
            <p className="text-xs text-sf-text-muted">
              {t(`accountKind.${account.kind}`)} · {account.currency}
              {account.institution ? ` · ${account.institution}` : ''}
            </p>
          </div>
          {readOnly ? null : (
            <div className="flex flex-wrap gap-1">
              <Button
                type="button"
                variant="ghost"
                disabled={busy}
                aria-label={t('holdings.accounts.renameNamed', { name: account.name })}
                onClick={() => {
                  setRenaming(renaming === account.id ? null : account.id)
                  setRenameTo(account.name)
                }}
              >
                {t('holdings.accounts.rename')}
              </Button>
              <Button
                type="button"
                variant="ghost"
                disabled={busy}
                aria-label={t(
                  account.closed_at === null
                    ? 'holdings.accounts.closeNamed'
                    : 'holdings.accounts.reopenNamed',
                  { name: account.name },
                )}
                onClick={() =>
                  act(
                    () =>
                      api.post(
                        `/portfolios/${portfolioId}/accounts/${account.id}/${
                          account.closed_at === null ? 'close' : 'reopen'
                        }`,
                      ),
                    t(
                      account.closed_at === null
                        ? 'holdings.accounts.closeDone'
                        : 'holdings.accounts.reopenDone',
                    ),
                  )
                }
              >
                {account.closed_at === null
                  ? t('holdings.accounts.close')
                  : t('holdings.accounts.reopen')}
              </Button>
            </div>
          )}
        </div>
        {renaming === account.id ? (
          <form
            onSubmit={(event) => rename(event, account)}
            className="mt-3 flex flex-wrap items-end gap-3 rounded-lg bg-sf-surface-sunken p-3"
          >
            <div className="min-w-0 flex-1">
              <Field
                label={t('holdings.accounts.newName')}
                name="name"
                required
                value={renameTo}
                onChange={(event) => setRenameTo(event.target.value)}
              />
            </div>
            <div className="flex gap-1">
              <Button type="submit" disabled={busy}>
                {t('common.save')}
              </Button>
              <Button type="button" variant="ghost" onClick={() => setRenaming(null)}>
                {t('common.cancel')}
              </Button>
            </div>
          </form>
        ) : null}
      </li>
    )
  }

  return (
    <Card title={t('holdings.accounts.title')} description={t('holdings.accounts.description')}>
      {error ? <ErrorText>{error}</ErrorText> : null}
      {notice ? <NoticeText>{notice}</NoticeText> : null}
      {accounts === null ? (
        <p role="status" className="text-sm text-sf-text-muted">
          {t('common.loading')}
        </p>
      ) : null}
      {accounts?.length === 0 ? (
        <p className="text-sm text-sf-text-muted">{t('holdings.accounts.empty')}</p>
      ) : null}
      {open.length > 0 ? <ul className="divide-y divide-sf-divider">{open.map(row)}</ul> : null}
      {closed.length > 0 ? (
        <div className="mt-3 border-t border-sf-divider pt-3">
          <CheckField
            label={t('holdings.accounts.showClosed', { count: closed.length })}
            name="show_closed"
            checked={showClosed}
            onChange={(event) => setShowClosed(event.target.checked)}
          />
          {showClosed ? <ul className="divide-y divide-sf-divider">{closed.map(row)}</ul> : null}
        </div>
      ) : null}

      {readOnly ? null : adding ? (
        <form onSubmit={create} className="mt-4 space-y-4 border-t border-sf-divider pt-4">
          <div className="grid gap-4 sm:grid-cols-2">
            <Field
              label={t('holdings.accounts.name')}
              name="name"
              autoComplete="off"
              required
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
            <SelectField
              label={t('holdings.accounts.kind')}
              name="kind"
              value={kind}
              onChange={(event) => setKind(event.target.value)}
            >
              {ACCOUNT_KINDS.map((value) => (
                <option key={value} value={value}>
                  {t(`accountKind.${value}`)}
                </option>
              ))}
            </SelectField>
            <Field
              label={t('holdings.accounts.currency')}
              name="currency"
              autoComplete="off"
              hint={t('holdings.accounts.currencyHint', { currency: baseCurrency })}
              value={currency}
              onChange={(event) => setCurrency(event.target.value)}
            />
            <Field
              label={t('holdings.accounts.institution')}
              name="institution"
              autoComplete="off"
              spellCheck={false}
              value={institution}
              onChange={(event) => setInstitution(event.target.value)}
            />
          </div>
          <div className="flex gap-2">
            <Button type="submit" className="h-10" disabled={busy}>
              {busy ? t('common.saving') : t('holdings.accounts.create')}
            </Button>
            <Button type="button" variant="ghost" onClick={() => setAdding(false)}>
              {t('common.cancel')}
            </Button>
          </div>
        </form>
      ) : (
        <div className="mt-4 border-t border-sf-divider pt-4">
          <Button type="button" onClick={() => setAdding(true)}>
            {t('holdings.accounts.add')}
          </Button>
        </div>
      )}
    </Card>
  )
}
