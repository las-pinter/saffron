import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Card } from '@/components/Card'
import { ConfirmButton } from '@/components/ConfirmButton'
import { ErrorText, NoticeText } from '@/components/Feedback'
import { Button } from '@/components/ui/button'
import { api } from '@/lib/api'
import {
  accountName,
  formatMoney,
  formatQuantity,
  type Account,
  type Instrument,
  type Transaction,
} from '@/lib/book'
import { useErrorMessage } from '@/lib/errors'
import { formatDate } from '@/lib/format'
import { TransactionForm } from './TransactionForm'

interface TransactionsSectionProps {
  portfolioId: number
  accounts: Account[] | null
  transactions: Transaction[] | null
  instruments: Map<number, Instrument>
  readOnly: boolean
  reload: () => Promise<void>
}

/** The transaction list and the add/edit form above it. */
export function TransactionsSection({
  portfolioId,
  accounts,
  transactions,
  instruments,
  readOnly,
  reload,
}: TransactionsSectionProps) {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const [editing, setEditing] = useState<Transaction | 'new' | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  const openAccounts = (accounts ?? []).filter((account) => account.closed_at === null)
  const rows = [...(transactions ?? [])].reverse()

  // Two rows can share a day, so the action names say what the row is.
  function actionLabel(key: string, row: Transaction) {
    const names = {
      kind: t(`transactionKind.${row.kind}`).toLowerCase(),
      instrument:
        row.instrument_id === null ? null : (instruments.get(row.instrument_id)?.name ?? null),
      date: formatDate(row.date),
    }
    return names.instrument
      ? t(`holdings.transactions.${key}Named`, names)
      : t(`holdings.transactions.${key}NamedPlain`, names)
  }

  async function remove(id: number) {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      await api.delete(`/portfolios/${portfolioId}/transactions/${id}`)
      await reload()
      setNotice(t('holdings.transactions.deleted'))
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  async function saved(message: string) {
    setEditing(null)
    await reload()
    setNotice(message)
    setError(null)
  }

  return (
    <Card
      title={t('holdings.transactions.title')}
      description={t('holdings.transactions.description')}
    >
      {error ? <ErrorText>{error}</ErrorText> : null}
      {notice ? <NoticeText>{notice}</NoticeText> : null}
      {transactions === null ? (
        <p role="status" className="text-sm text-sf-text-muted">
          {t('common.loading')}
        </p>
      ) : null}
      {transactions?.length === 0 && editing === null ? (
        <p className="text-sm text-sf-text-muted">{t('holdings.transactions.empty')}</p>
      ) : null}

      {editing !== null ? (
        <TransactionForm
          key={editing === 'new' ? 'new' : editing.id}
          portfolioId={portfolioId}
          accounts={openAccounts}
          instruments={instruments}
          initial={editing === 'new' ? null : editing}
          onSaved={saved}
          onCancel={() => setEditing(null)}
        />
      ) : readOnly ? null : (
        <div className="mb-4">
          <Button
            type="button"
            className="h-10"
            disabled={openAccounts.length === 0}
            onClick={() => setEditing('new')}
          >
            {t('holdings.transactions.add')}
          </Button>
        </div>
      )}

      {transactions && transactions.length > 0 ? (
        <div className="overflow-x-auto">
          <table className="w-full border-collapse text-sm">
            <thead>
              <tr className="text-left text-xs text-sf-text-muted">
                <th scope="col" className="py-2 pr-3 font-medium">
                  {t('holdings.transactions.date')}
                </th>
                <th scope="col" className="py-2 pr-3 font-medium">
                  {t('holdings.transactions.account')}
                </th>
                <th scope="col" className="py-2 pr-3 font-medium">
                  {t('holdings.transactions.kind')}
                </th>
                <th scope="col" className="py-2 pr-3 font-medium">
                  {t('holdings.transactions.instrument')}
                </th>
                <th scope="col" className="py-2 pr-3 text-right font-medium">
                  {t('holdings.transactions.quantity')}
                </th>
                <th scope="col" className="py-2 pr-3 text-right font-medium">
                  {t('holdings.transactions.price')}
                </th>
                <th scope="col" className="py-2 pr-3 text-right font-medium">
                  {t('holdings.transactions.amount')}
                </th>
                <th scope="col" className="py-2 pr-3 text-right font-medium">
                  {t('holdings.transactions.fee')}
                </th>
                <th scope="col" className="py-2 pr-3 font-medium">
                  {t('holdings.transactions.note')}
                </th>
                {readOnly ? null : (
                  <th scope="col" className="py-2 font-medium">
                    <span className="sr-only">{t('holdings.transactions.actions')}</span>
                  </th>
                )}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const accountClosed =
                  accounts?.find((account) => account.id === row.account_id)?.closed_at != null
                return (
                  <tr key={row.id} className="border-t border-sf-divider align-top">
                    <td className="py-2 pr-3 whitespace-nowrap">{formatDate(row.date)}</td>
                    <td className="max-w-40 py-2 pr-3 break-words">
                      {accountName(accounts, row.account_id)}
                    </td>
                    <td className="py-2 pr-3 whitespace-nowrap">
                      {t(`transactionKind.${row.kind}`)}
                    </td>
                    <td className="max-w-48 py-2 pr-3 break-words">
                      {row.instrument_id === null
                        ? '—'
                        : (instruments.get(row.instrument_id)?.name ?? `#${row.instrument_id}`)}
                    </td>
                    <td className="py-2 pr-3 text-right whitespace-nowrap">
                      {row.quantity === null ? '—' : formatQuantity(row.quantity)}
                    </td>
                    <td className="py-2 pr-3 text-right whitespace-nowrap">
                      {row.price === null ? '—' : formatMoney(row.price, row.currency)}
                    </td>
                    <td className="py-2 pr-3 text-right whitespace-nowrap">
                      {row.amount === null ? '—' : formatMoney(row.amount, row.currency)}
                    </td>
                    <td className="py-2 pr-3 text-right whitespace-nowrap">
                      {formatMoney(row.fee, row.currency)}
                    </td>
                    <td className="max-w-48 py-2 pr-3 break-words">{row.note ?? ''}</td>
                    {readOnly ? null : (
                      <td className="py-2">
                        <div className="flex flex-wrap gap-1">
                          {accountClosed ? null : (
                            <Button
                              type="button"
                              variant="ghost"
                              disabled={busy}
                              aria-label={actionLabel('edit', row)}
                              onClick={() => setEditing(row)}
                            >
                              {t('holdings.transactions.edit')}
                            </Button>
                          )}
                          {accountClosed ? null : (
                            <ConfirmButton
                              label={t('holdings.transactions.delete')}
                              confirmLabel={t('holdings.transactions.confirmDelete')}
                              labelFor={actionLabel('delete', row)}
                              confirmLabelFor={actionLabel('confirmDelete', row)}
                              busy={busy}
                              onConfirm={() => remove(row.id)}
                            />
                          )}
                        </div>
                      </td>
                    )}
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      ) : null}
    </Card>
  )
}
