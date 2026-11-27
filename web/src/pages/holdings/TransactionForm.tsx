import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { ErrorText } from '@/components/Feedback'
import { Field, SelectField } from '@/components/Field'
import { Button } from '@/components/ui/button'
import { api } from '@/lib/api'
import {
  TRANSACTION_KINDS,
  kindAllowsFee,
  kindAllowsPrice,
  kindNeedsAmount,
  kindNeedsInstrument,
  kindNeedsPrice,
  kindNeedsQuantity,
  type Account,
  type Instrument,
  type Transaction,
  type TransactionKind,
} from '@/lib/book'
import { useErrorMessage } from '@/lib/errors'
import { InstrumentPicker, type PickedInstrument } from './InstrumentPicker'

interface TransactionFormProps {
  portfolioId: number
  /** Open accounts only; a closed account takes no new row or edit. */
  accounts: Account[]
  instruments: Map<number, Instrument>
  initial: Transaction | null
  onSaved: (message: string) => void
  onCancel: () => void
}

/** Today's date in the browser's own time zone, as YYYY-MM-DD. */
function today(): string {
  const now = new Date()
  const local = new Date(now.getTime() - now.getTimezoneOffset() * 60_000)
  return local.toISOString().slice(0, 10)
}

/** The add and edit form: the fields the chosen kind needs, and nothing more. */
export function TransactionForm({
  portfolioId,
  accounts,
  instruments,
  initial,
  onSaved,
  onCancel,
}: TransactionFormProps) {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const [kind, setKind] = useState<TransactionKind>(initial?.kind ?? 'buy')
  const [accountId, setAccountId] = useState(initial?.account_id ?? accounts[0]?.id ?? 0)
  const [date, setDate] = useState(initial?.date ?? today())
  const initialInstrument = initial?.instrument_id
    ? instruments.get(initial.instrument_id)
    : undefined
  const [instrument, setInstrument] = useState<PickedInstrument | null>(
    initialInstrument
      ? {
          id: initialInstrument.id,
          name: initialInstrument.name,
          currency: initialInstrument.currency,
        }
      : initial?.instrument_id
        ? { id: initial.instrument_id, name: `#${initial.instrument_id}`, currency: 'EUR' }
        : null,
  )
  const [quantity, setQuantity] = useState(initial?.quantity ?? '')
  const [price, setPrice] = useState(initial?.price ?? '')
  const [amount, setAmount] = useState(initial?.amount ?? '')
  const [fee, setFee] = useState(initial?.fee ?? '')
  const [currency, setCurrency] = useState(initial?.currency ?? '')
  const [fxRate, setFxRate] = useState(initial?.fx_rate ?? '')
  const [note, setNote] = useState(initial?.note ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const account = accounts.find((row) => row.id === accountId)
  const effectiveCurrency = currency.trim() === '' ? (account?.currency ?? '') : currency
  const anotherCurrency = account !== undefined && effectiveCurrency !== account.currency

  function changeAccount(id: number) {
    setAccountId(id)
    const next = accounts.find((row) => row.id === id)
    if (next && (currency.trim() === '' || currency.trim() === account?.currency)) {
      setCurrency(next.currency)
    }
    // A rate belongs to one currency pair; another account means another pair.
    setFxRate('')
  }

  async function submit(event: FormEvent) {
    event.preventDefault()
    setError(null)
    if (accountId === 0) {
      setError(t('holdings.transactions.accountNeeded'))
      return
    }
    if (kindNeedsInstrument(kind) && instrument === null) {
      setError(t('holdings.transactions.instrumentNeeded'))
      return
    }
    const body: Record<string, string | number> = {
      account_id: accountId,
      kind,
      date,
      ...(kindNeedsInstrument(kind) && instrument ? { instrument_id: instrument.id } : {}),
      ...(kindNeedsQuantity(kind) ? { quantity: quantity.trim() } : {}),
      ...(kindAllowsPrice(kind) && price.trim() !== '' ? { price: price.trim() } : {}),
      ...(kindNeedsAmount(kind) ? { amount: amount.trim() } : {}),
      ...(kindAllowsFee(kind) && fee.trim() !== '' ? { fee: fee.trim() } : {}),
      ...(anotherCurrency ? { currency: effectiveCurrency.toUpperCase() } : {}),
      ...(anotherCurrency && fxRate.trim() !== '' ? { fx_rate: fxRate.trim() } : {}),
      ...(note.trim() !== '' ? { note: note.trim() } : {}),
    }
    setBusy(true)
    try {
      const path = `/portfolios/${portfolioId}/transactions`
      if (initial) await api.patch(`${path}/${initial.id}`, body)
      else await api.post(path, body)
      onSaved(initial ? t('holdings.transactions.updated') : t('holdings.transactions.created'))
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form
      onSubmit={submit}
      aria-label={
        initial ? t('holdings.transactions.editTitle') : t('holdings.transactions.addTitle')
      }
      className="mt-4 space-y-4 rounded-xl border border-sf-border bg-sf-surface-sunken p-4"
    >
      <h3 className="font-heading text-base font-semibold">
        {initial ? t('holdings.transactions.editTitle') : t('holdings.transactions.addTitle')}
      </h3>
      <div className="grid gap-4 sm:grid-cols-2">
        <SelectField
          label={t('holdings.transactions.account')}
          name="account_id"
          value={accountId}
          // Moving a stored row to another account would reinterpret its amounts.
          disabled={initial !== null}
          onChange={(event) => changeAccount(Number(event.target.value))}
        >
          {accounts.map((row) => (
            <option key={row.id} value={row.id}>
              {row.name} ({row.currency})
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t('holdings.transactions.kind')}
          name="kind"
          value={kind}
          onChange={(event) => setKind(event.target.value as TransactionKind)}
        >
          {TRANSACTION_KINDS.map((value) => (
            <option key={value} value={value}>
              {t(`transactionKind.${value}`)}
            </option>
          ))}
        </SelectField>
        <Field
          label={t('holdings.transactions.date')}
          name="date"
          type="date"
          required
          value={date}
          onChange={(event) => setDate(event.target.value)}
        />
        {kindNeedsQuantity(kind) ? (
          <Field
            label={
              kind === 'split'
                ? t('holdings.transactions.ratio')
                : t('holdings.transactions.quantity')
            }
            name="quantity"
            inputMode="decimal"
            required
            value={quantity}
            onChange={(event) => setQuantity(event.target.value)}
          />
        ) : null}
        {kindAllowsPrice(kind) ? (
          <Field
            label={t('holdings.transactions.price')}
            name="price"
            inputMode="decimal"
            required={kindNeedsPrice(kind)}
            hint={kind === 'transfer_in' ? t('holdings.transactions.priceTransferHint') : undefined}
            value={price}
            onChange={(event) => setPrice(event.target.value)}
          />
        ) : null}
        {kindNeedsAmount(kind) ? (
          <Field
            label={t('holdings.transactions.amount')}
            name="amount"
            inputMode="decimal"
            required
            value={amount}
            onChange={(event) => setAmount(event.target.value)}
          />
        ) : null}
        {kindAllowsFee(kind) ? (
          <Field
            label={t('holdings.transactions.fee')}
            name="fee"
            inputMode="decimal"
            value={fee}
            onChange={(event) => setFee(event.target.value)}
          />
        ) : null}
        <Field
          label={t('holdings.transactions.currency')}
          name="currency"
          maxLength={3}
          hint={t('holdings.transactions.currencyHint', { currency: account?.currency ?? '' })}
          value={currency}
          onChange={(event) => setCurrency(event.target.value)}
        />
        {anotherCurrency ? (
          <Field
            label={t('holdings.transactions.fxRate')}
            name="fx_rate"
            inputMode="decimal"
            hint={t('holdings.transactions.fxRateHint', { currency: account?.currency ?? '' })}
            value={fxRate}
            onChange={(event) => setFxRate(event.target.value)}
          />
        ) : null}
      </div>
      {kindNeedsInstrument(kind) ? (
        <InstrumentPicker value={instrument} onPick={setInstrument} />
      ) : null}
      <Field
        label={t('holdings.transactions.note')}
        name="note"
        autoComplete="off"
        spellCheck={false}
        value={note}
        onChange={(event) => setNote(event.target.value)}
      />
      {error ? <ErrorText>{error}</ErrorText> : null}
      <div className="flex flex-wrap gap-2">
        <Button type="submit" className="h-10" disabled={busy}>
          {busy ? t('common.saving') : t('common.save')}
        </Button>
        <Button type="button" variant="ghost" className="h-10" onClick={onCancel}>
          {t('common.cancel')}
        </Button>
      </div>
    </form>
  )
}
